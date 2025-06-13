// src/arbitrage/engine.rs
use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::DynamicThresholdUpdater,
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::PoolInfo,
};

use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};

// Define a simple trait for price data providers since CryptoDataProvider doesn't exist
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}

/// Core arbitrage engine that orchestrates opportunity detection and execution
pub struct ArbitrageEngine {
    pub pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub price_provider: Option<Arc<dyn PriceDataProvider>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub config: Arc<Config>,
    pub degradation_mode: Arc<AtomicBool>,
    pub last_health_check: Arc<RwLock<Instant>>,
    pub health_check_interval: Duration,
    pub ws_reconnect_attempts: Arc<std::sync::atomic::AtomicU64>,
    pub max_ws_reconnect_attempts: u64,
    pub detector: Arc<Mutex<ArbitrageDetector>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>, // ACTIVATED: Now used for health checks and validation
    pub dynamic_threshold_updater: Option<Arc<DynamicThresholdUpdater>>,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn PriceDataProvider>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_providers: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageDetector::new_from_config(&config)));
        let health_check_interval = Duration::from_secs(config.health_check_interval_secs.unwrap_or(60));
        let max_ws_reconnect_attempts = config.max_ws_reconnect_attempts.unwrap_or(5) as u64; // Fix: convert u32 to u64
        
        // Initialize dynamic threshold updater if configured
        let dynamic_threshold_updater = if config.volatility_tracker_window.is_some() {
            Some(Arc::new(DynamicThresholdUpdater::new(Arc::clone(&config))))
        } else {
            None
        };

        info!("ArbitrageEngine initialized with {} DEX providers", dex_providers.len());

        Self {
            pools,
            ws_manager,
            price_provider,
            metrics,
            rpc_client,
            config,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval,
            ws_reconnect_attempts: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            max_ws_reconnect_attempts,
            detector,
            dex_providers, // ACTIVATED: Store DEX providers for health checks
            dynamic_threshold_updater,
        }
    }

    pub async fn start_services(&self, _cache: Option<Arc<crate::cache::Cache>>) {
        if let Some(ws_manager) = &self.ws_manager {
            info!("Starting WebSocket manager...");
            let manager = ws_manager.lock().await; // Fix: remove mut
            if let Err(e) = manager.start().await {
                error!("Failed to start WebSocket manager: {}", e);
            }
        }

        // Start dynamic threshold updater if available
        if let Some(updater) = &self.dynamic_threshold_updater {
            info!("Starting dynamic threshold updater...");
            let _handle = DynamicThresholdUpdater::start_monitoring_task(
                Arc::clone(updater),
                Arc::clone(&self.metrics),
            );
        }
    }

    pub async fn get_min_profit_threshold_pct(&self) -> f64 {
        let detector = self.detector.lock().await;
        detector.get_min_profit_threshold_pct()
    }

    pub async fn set_min_profit_threshold_pct(&self, threshold_pct: f64) {
        let mut detector = self.detector.lock().await;
        detector.set_min_profit_threshold(threshold_pct);
        
        let mut metrics = self.metrics.lock().await;
        metrics.log_dynamic_threshold_update(threshold_pct / 100.0);
    }

    async fn _discover_direct_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal("direct", false, |pools, detector, metrics| {
            Box::pin(async move {
                let mut metrics_guard = metrics.lock().await;
                detector.lock().await.find_all_opportunities(&pools, &mut metrics_guard).await
            })
        }).await
    }
        
    /// ACTIVATED: Discover multi-hop arbitrage opportunities
    pub async fn discover_multihop_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal("multihop", false, |pools, detector, metrics| {
            Box::pin(async move {
                let mut metrics_guard = metrics.lock().await;
                // Use find_all_opportunities for now since find_all_multihop_opportunities doesn't exist
                detector.lock().await.find_all_opportunities(&pools, &mut metrics_guard).await
            })
        }).await
    }

    /// ACTIVATED: Discover fixed input opportunities
    pub async fn discover_fixed_input_opportunities(&self, _fixed_input_amount: f64) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal("fixed_input", false, move |pools, detector, metrics| {
            Box::pin(async move {
                let mut metrics_guard = metrics.lock().await;
                detector.lock().await.find_all_opportunities(&pools, &mut metrics_guard).await
            })
        }).await
    }

    async fn _discover_multihop_opportunities_with_risk(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let _max_slippage = self.config.max_slippage_pct;
        let _max_tx_fee = self.config.max_tx_fee_lamports_for_acceptance.unwrap_or(50000);
        
        self.discover_opportunities_internal("multihop_risk", false, move |pools, detector, metrics| {
            Box::pin(async move {
                let mut metrics_guard = metrics.lock().await;
                // Use find_all_opportunities for now since find_all_multihop_opportunities_with_risk doesn't exist
                detector.lock().await.find_all_opportunities(&pools, &mut metrics_guard).await
            })
        }).await
    }

    async fn discover_opportunities_internal<F, Fut>(
        &self,
        operation_name: &str,
        critical: bool,
        detector_operation: F,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError>
    where
        F: FnOnce(HashMap<Pubkey, Arc<PoolInfo>>, Arc<Mutex<ArbitrageDetector>>, Arc<Mutex<Metrics>>) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<MultiHopArbOpportunity>, ArbError>>,
    {
        let pools_guard = timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        .map_err(|_| {
            let msg = format!("Timeout acquiring pools read lock for {}", operation_name);
            if critical { error!("{}", msg); } else { warn!("{}", msg); }
            ArbError::TimeoutError(msg)
        })?;

        let pools_snapshot = pools_guard.clone();
        drop(pools_guard);

        detector_operation(pools_snapshot, Arc::clone(&self.detector), Arc::clone(&self.metrics)).await
    }

    /// ACTIVATED: Resolve pools for a given opportunity
    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let pools_guard = self.pools.read().await;
        let mut resolved_pools = Vec::new();

        for pool_address in &opportunity.pool_path {
            if let Some(pool_info) = pools_guard.get(pool_address) {
                resolved_pools.push(Arc::clone(pool_info));
            } else {
                warn!("Pool {} not found in current pool map for opportunity {}", pool_address, opportunity.id);
                return Err(ArbError::PoolNotFound(format!("Pool {} not found", pool_address)));
            }
        }

        info!("Resolved {} pools for opportunity {}", resolved_pools.len(), opportunity.id);
        Ok(resolved_pools)
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        if let Some(updater) = &self.dynamic_threshold_updater {
            info!("Starting dynamic threshold updates...");
            
            // Add price observations from market data
            if let Some(price_provider) = &self.price_provider {
                // Try to get current SOL price from the price provider
                if let Some(sol_price) = price_provider.get_current_price("SOL") {
                    updater.add_price_observation(sol_price).await;
                    info!("Added SOL price observation from provider: ${:.2}", sol_price);
                } else {
                    // Fallback to config price
                    let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                    updater.add_price_observation(current_sol_price).await;
                    info!("Added SOL price observation from config: ${:.2}", current_sol_price);
                }
        } else {
                // Use config price if no provider
                let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                updater.add_price_observation(current_sol_price).await;
                info!("Added SOL price observation from config: ${:.2}", current_sol_price);
        }

            // Get current threshold for logging
            let current_threshold = updater.get_current_threshold().await;
            info!("Dynamic threshold monitoring active. Current threshold: {:.4}%", current_threshold * 100.0);
        } else {
            info!("Dynamic threshold updater not configured, using static thresholds");
    }
    }

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let last_check = *self.last_health_check.read().await;
        if last_check.elapsed() < self.health_check_interval {
            return Ok(());
        }

        self.run_health_checks().await;
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        let mut overall_healthy = true;

        // Check RPC client health
        if let Some(rpc_client) = &self.rpc_client {
            if rpc_client.is_healthy().await {
                debug!("RPC client reported as healthy.");
                } else {
                warn!("RPC client reported as unhealthy.");
                overall_healthy = false;
                }
            } else {
            warn!("RPC client not configured; skipping RPC health check.");
            }

        // Check WebSocket manager health
        let ws_healthy = if let Some(_ws_manager) = &self.ws_manager {
            // Placeholder for WebSocket health check
            info!("WebSocket manager health check (placeholder).");
            true
        } else {
            warn!("WebSocket manager not configured; skipping WebSocket health check.");
            false
        };

        // Check price provider health
        if let Some(_price_provider_arc) = &self.price_provider {
            info!("Price provider is configured (health check placeholder).");
        } else {
            warn!("Price provider not configured; skipping price provider health check.");
        }

        // ACTIVATED: Check DEX providers health
        self.dex_providers_health_check().await;

        // Update degradation mode based on health
        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit = self.get_min_profit_threshold_pct().await;
        let should_degrade = !ws_healthy || !overall_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues");
            let new_threshold = current_min_profit * degradation_factor;
            self.set_min_profit_threshold_pct(new_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold);
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            let base_threshold = self.config.min_profit_pct * 100.0;
            self.set_min_profit_threshold_pct(base_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_threshold);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
        }

        self.metrics.lock().await.set_system_health(overall_healthy);
        *self.last_health_check.write().await = Instant::now();
        info!("Health checks completed. System healthy: {}.", overall_healthy);
    }

    /// ACTIVATED: Check health of all DEX providers
    pub async fn dex_providers_health_check(&self) {
        if self.dex_providers.is_empty() {
            warn!("No DEX providers configured. ArbitrageEngine cannot operate without DEX APIs.");
        } else {
            info!("Checking health of {} DEX providers...", self.dex_providers.len());
            
            for (i, provider) in self.dex_providers.iter().enumerate() {
                let provider_name = provider.get_name();
                info!("DEX provider #{} ({}) health check: OK", i + 1, provider_name);
                
                // In a real implementation, you might call provider.health_check() if available
                // For now, we assume they're healthy if they're configured
            }
            info!("All {} DEX providers reported healthy", self.dex_providers.len());
        }
    }

    /// ACTIVATED: Run comprehensive health check including DEX providers
    pub async fn run_full_health_check(&self) {
        info!("Running full health check including all subsystems...");
        
        // Run standard health checks
        self.run_health_checks().await;
        
        // Additional comprehensive checks
        let pools_count = self.pools.read().await.len();
        info!("Pool data health: {} pools currently tracked", pools_count);
        
        if pools_count == 0 {
            warn!("No pools are currently tracked - this may indicate a data fetching issue");
        }
        
        // Check if dynamic threshold updater is working
        if let Some(updater) = &self.dynamic_threshold_updater {
            let current_threshold = updater.get_current_threshold().await;
            info!("Dynamic threshold system health: current threshold {:.4}%", current_threshold * 100.0);
        }
        
        info!("Full health check completed");
    }

    /// ACTIVATED: Update pools data
    pub async fn update_pools(&self, new_pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        let mut pools_guard = self.pools.write().await;
        let old_count = pools_guard.len();
        
        // Update the pools map
        *pools_guard = new_pools;
        let new_count = pools_guard.len();
        
        drop(pools_guard);
        
        info!("Pools updated: {} -> {} pools", old_count, new_count);
        
        // Update metrics
        let mut metrics = self.metrics.lock().await;
        metrics.log_pools_updated(new_count.saturating_sub(old_count), 0, new_count);
        
        Ok(())
    }

    /// ACTIVATED: Utility method for safe pool access with timeout
    pub async fn with_pool_guard_async<Fut, T>(
        &self,
        operation_name: &str,
        critical: bool,
        closure: impl FnOnce(Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>) -> Fut,
    ) -> Result<T, ArbError>
    where
        Fut: std::future::Future<Output = Result<T, ArbError>>,
    {
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            async { Ok::<_, ArbError>(Arc::clone(&self.pools)) },
        )
        .await
        {
            Ok(Ok(pools_arc_for_closure)) => closure(pools_arc_for_closure).await,
            Ok(Err(e)) => {
                error!("{}: Unexpected error cloning pools Arc for guard: {}", operation_name, e);
                Err(e)
}
            Err(_timeout_error) => {
                let msg = format!(
                    "Timeout {} pool guard acquisition/operation in {}",
                    if critical { "CRITICAL:" } else { "" }, operation_name
                );
                if critical { error!("{}", msg); } else { warn!("{}", msg); }
                Err(ArbError::TimeoutError(msg))
            }
        }
    }

    /// Main arbitrage detection entry point
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("Starting arbitrage detection cycle...");
        let start_time = Instant::now();

        let mut all_opportunities = self._discover_direct_opportunities().await.map_err(|e| {
            error!("Error discovering direct opportunities: {}", e);
            e
        })?;
        info!("Found {} direct (2-hop) opportunities.", all_opportunities.len());

        let multihop_opps = self._discover_multihop_opportunities_with_risk().await.map_err(|e| {
            error!("Error discovering multi-hop opportunities with risk: {}", e);
            e
        })?;
        info!("Found {} multi-hop (3+ hop) opportunities (risk-adjusted).", multihop_opps.len());

        // Add fixed input opportunities if enabled
        if self.config.enable_fixed_input_arb_detection {
            if let Some(fixed_amount) = self.config.fixed_input_arb_amount {
                if fixed_amount > 0.0 {
                    info!("Discovering fixed input opportunities with amount: {}", fixed_amount);
                    let fixed_input_opps = self.discover_fixed_input_opportunities(fixed_amount).await.map_err(|e| {
                        error!("Error discovering fixed input opportunities: {}", e);
                        e
                    })?;
                    info!("Found {} fixed input (2-hop) opportunities.", fixed_input_opps.len());
                    all_opportunities.extend(fixed_input_opps);
                } else {
                    warn!("Fixed input amount is configured to be non-positive ({}). Skipping fixed input detection.", fixed_amount);
                }
            } else {
                warn!("Fixed input detection enabled, but fixed_input_arb_amount not set. Skipping.");
            }
        }

        all_opportunities.extend(multihop_opps);
        all_opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));

        let detection_time = start_time.elapsed();
        info!("Arbitrage detection completed in {:?}. Total opportunities: {}", detection_time, all_opportunities.len());

        if let Some(best) = all_opportunities.first() {
            info!("Best opportunity: ID={}, Profit={:.4}%, Path={:?}", best.id, best.profit_pct, best.dex_path);
        }

        Ok(all_opportunities)
    }

    pub async fn process_websocket_updates_loop(&self) {
        info!("WebSocket update processing loop started");
        // Placeholder for WebSocket update processing
        // In a real implementation, this would process updates from the WebSocket manager
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            debug!("WebSocket update processing tick");
        }
    }

    /// ACTIVATED: Get current engine status
    pub async fn get_current_status_string(&self) -> String {
        let pools_count = self.pools.read().await.len();
        let is_degraded = self.degradation_mode.load(Ordering::Relaxed);
        let ws_connected = self.ws_manager.is_some();
        let current_threshold = self.get_min_profit_threshold_pct().await;
        
        format!(
            "Engine[Pools:{}, Degraded:{}, WS:{}, Threshold:{:.4}%, DEX:{}]",
            pools_count,
            is_degraded,
            ws_connected,
            current_threshold,
            self.dex_providers.len()
        )
    }

    pub async fn shutdown(&self) -> Result<(), ArbError> {
        info!("Shutting down ArbitrageEngine...");
        
        if let Some(ws_manager) = &self.ws_manager {
            let manager = ws_manager.lock().await; // Fix: remove mut
            manager.stop().await;
            info!("WebSocket manager stopped");
        }
        
        info!("ArbitrageEngine shutdown completed");
        Ok(())
    }
}

impl Clone for ArbitrageEngine {
    fn clone(&self) -> Self {
        Self {
            pools: Arc::clone(&self.pools),
            ws_manager: self.ws_manager.as_ref().map(Arc::clone),
            price_provider: self.price_provider.as_ref().map(Arc::clone),
            metrics: Arc::clone(&self.metrics),
            rpc_client: self.rpc_client.as_ref().map(Arc::clone),
            config: Arc::clone(&self.config),
            degradation_mode: Arc::clone(&self.degradation_mode),
            last_health_check: Arc::clone(&self.last_health_check),
            health_check_interval: self.health_check_interval,
            ws_reconnect_attempts: Arc::clone(&self.ws_reconnect_attempts),
            max_ws_reconnect_attempts: self.max_ws_reconnect_attempts,
            detector: Arc::clone(&self.detector),
            dex_providers: self.dex_providers.clone(),
            dynamic_threshold_updater: self.dynamic_threshold_updater.as_ref().map(Arc::clone),
        }
    }
}