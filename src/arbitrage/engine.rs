// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::{self, VolatilityTracker}, // Corrected: dynamic_threshold itself is not directly used, only its members
        opportunity::MultiHopArbOpportunity, // Corrected: Use MultiHopArbOpportunity
    },
    config::settings::Config,
    dex::quote::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::PoolInfo,
    websocket::CryptoDataProvider, 
};
use log::{error, info, warn}; // Removed unused debug
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    ops::Deref, 
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, Mutex};
use tokio::time::{timeout};
use tokio::time::error::Elapsed as TimeoutError; 


pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    // Corrected: ws_manager should hold Arc<Mutex<SolanaWebsocketManager>>>
    pub(crate) ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub(crate) price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>,
    pub(crate) rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>,
    pub(crate) degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    pub(crate) health_check_interval: Duration,
    pub(crate) ws_reconnect_attempts: Arc<AtomicU64>,
    pub(crate) max_ws_reconnect_attempts: u64, // Corrected: Ensure this is u64
    pub(crate) detector: Arc<Mutex<ArbitrageDetector>>,
    pub(crate) dex_providers: Vec<Arc<dyn DexClient>>,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        // Corrected: ws_manager parameter type
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_api_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        // Use new_from_config for ArbitrageDetector initialization
        let internal_detector = Arc::new(Mutex::new(ArbitrageDetector::new_from_config(&config)));

        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        // Corrected: Ensure max_ws_reconnect_val is u64
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);

        // Example of using dex_providers if needed for initialization or logging
        if dex_api_clients.is_empty() {
            warn!("ArbitrageEngine initialized with no DEX API clients.");
        } else {
            info!("ArbitrageEngine initialized with {} DEX API client(s).", dex_api_clients.len());
        }

        Self {
            pools,
            ws_manager: ws_manager_instance, // Corrected assignment
            price_provider, 
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            degradation_mode: Arc::new(AtomicBool::new(false)), 
            last_health_check: Arc::new(RwLock::new(
                Instant::now() - Duration::from_secs(health_check_interval_secs * 2), 
            )),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val, // Corrected assignment
            detector: internal_detector, 
            dex_providers: dex_api_clients, // dex_providers is now assigned and used
        }
    }

    pub async fn start_services(&self) {
        if let Some(manager_arc) = &self.ws_manager {
            let manager = manager_arc.lock().await;
            if let Err(e) = manager.start().await {
                error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }
        // Start other services like price provider if they have a start method
    }

    pub async fn get_min_profit_threshold_pct(&self) -> f64 { 
        self.detector.lock().await.get_min_profit_threshold_pct()
    }

    pub async fn set_min_profit_threshold_pct(&self, threshold_pct: f64) { 
        self.detector.lock().await.set_min_profit_threshold(threshold_pct);
        info!(
            "ArbitrageEngine's detector min_profit_threshold_pct updated to: {:.4}%",
            threshold_pct
        );
        let fractional_threshold = threshold_pct / 100.0;
        self.metrics.lock().await.log_dynamic_threshold_update(fractional_threshold);
    }
    
    async fn discover_opportunities_internal<F, Fut>(
        &self,
        operation_name: &str,
        detector_call: F,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> // Corrected: Use MultiHopArbOpportunity
    where
        F: FnOnce( 
            Arc<Mutex<ArbitrageDetector>>, 
            HashMap<Pubkey, Arc<PoolInfo>>, 
            Arc<Mutex<Metrics>>, 
        ) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<MultiHopArbOpportunity>, ArbError>>, // Corrected: Use MultiHopArbOpportunity
    {
        self.maybe_check_health().await?;

        let pools_map_clone = {
            let guard_result = timeout(
                Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
                self.pools.read(),
            )
            .await
            .map_err(|_elapsed: TimeoutError| {
                warn!("{}: Timeout waiting for pools read lock", operation_name);
                ArbError::TimeoutError(format!("Timeout for pools read lock in {}", operation_name))
            })?;
            guard_result.deref().clone()
        };
        detector_call(
            Arc::clone(&self.detector),
            pools_map_clone, 
            Arc::clone(&self.metrics)
        ).await
    }

    pub async fn _discover_direct_opportunities( 
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Corrected: Use MultiHopArbOpportunity
        self.discover_opportunities_internal(
            "discover_direct_opportunities",
            |detector_arc, pools_map, metrics_arc| async move { 
                let detector_guard = detector_arc.lock().await;
                let mut metrics_guard = metrics_arc.lock().await;
                // Call find_all_opportunities as suggested by the deprecation message.
                detector_guard
                    .find_all_opportunities(&pools_map, &mut *metrics_guard) // Corrected: Pass &mut *metrics_guard
                    .await
            },
        )
        .await
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Corrected: Use MultiHopArbOpportunity
        // Example: Potentially use self.dex_providers here if the discovery logic
        // needs to interact with DEX APIs directly through the engine, though typically
        // this would be handled within the ArbitrageDetector or specific DEX client implementations.
        // For now, its primary use might be for initialization or health checks specific to DEXs.
        log::debug!("Discovering multihop opportunities. DEX providers available: {}", self.dex_providers.len());
        self.discover_opportunities_internal(
            "discover_multihop_opportunities",
            |detector_arc, pools_map, metrics_arc| async move {
                let detector_guard = detector_arc.lock().await; 
                let mut metrics_guard = metrics_arc.lock().await;
                detector_guard
                    .find_all_multihop_opportunities(&pools_map, &mut *metrics_guard) // Corrected: Pass &mut *metrics_guard
                    .await
            },
        )
        .await
    }

    pub async fn discover_fixed_input_opportunities(
        &self,
        fixed_input_amount: f64,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Corrected: Use MultiHopArbOpportunity
        if fixed_input_amount <= 0.0 {
            warn!("discover_fixed_input_opportunities called with non-positive amount: {}. Skipping.", fixed_input_amount);
            return Ok(Vec::new());
        }
        self.discover_opportunities_internal(
            "discover_fixed_input_opportunities",
            move |detector_arc, pools_map, metrics_arc| async move {
                let detector_guard = detector_arc.lock().await;
                let mut metrics_guard = metrics_arc.lock().await;
                detector_guard
                    .find_two_hop_opportunities_with_fixed_input(
                        &pools_map, &mut *metrics_guard, fixed_input_amount // Corrected: Pass &mut *metrics_guard
                    )
                    .await
            },
        )
        .await
    }

    pub async fn _discover_multihop_opportunities_with_risk( 
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Corrected: Use MultiHopArbOpportunity
        let max_slippage_pct = self.config.max_slippage_pct * 100.0; 
        let tx_fee_lamports_for_acceptance = self.config.default_priority_fee_lamports; 

        self.discover_opportunities_internal(
            "discover_multihop_opportunities_with_risk",
            move |detector_arc, pools_map, metrics_arc| async move {
                let detector_guard = detector_arc.lock().await; 
                let mut metrics_guard = metrics_arc.lock().await;
                detector_guard
                    .find_all_multihop_opportunities_with_risk(
                        &pools_map,
                        &mut *metrics_guard, // Corrected: Pass &mut *metrics_guard
                        max_slippage_pct,
                        tx_fee_lamports_for_acceptance,
                    )
                    .await
            },
        )
        .await
    }
    
    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let pools_guard_result = timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        .map_err(|_elapsed: TimeoutError| {
            warn!("Timeout waiting for pools read lock in resolve_pools_for_opportunity");
            ArbError::TimeoutError("Timeout waiting for pools read lock".to_string())
        })?;
        let pools_guard = pools_guard_result;
        let mut resolved_pools = Vec::new();
        for hop in &opportunity.hops {
            match pools_guard.get(&hop.pool) {
                Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                None => {
                    warn!(
                        "Pool {} for hop {}->{} not found in local cache.",
                        hop.pool, hop.input_token, hop.output_token
                    );
                    return Err(ArbError::PoolNotFound(hop.pool.to_string()));
                }
            }
        }
        Ok(resolved_pools)
    }

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let mut last_check = self.last_health_check.write().await;
        if last_check.elapsed() < self.health_check_interval {
            return Ok(());
        }

        info!("[ArbitrageEngine] Performing system health check...");
        let mut ws_healthy = true;
        if let Some(manager_arc) = &self.ws_manager {
            let manager = manager_arc.lock().await;
            if !manager.is_connected().await {
                ws_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                if attempts < self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] WebSocket disconnected. Attempting reconnect (attempt {}/{})", attempts + 1, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager.start().await {
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                    error!("[ArbitrageEngine] WebSocket disconnected. Max reconnect attempts reached.");
                }
            } else {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            }
        }

        // Price provider health check placeholder (not used, so no warning)
        if self.price_provider.is_some() {
            info!("Price provider is configured (health check placeholder).");
        } else {
            warn!("Price provider not configured; skipping price provider health check.");
        }

        let mut rpc_healthy = true;
        if let Some(client) = &self.rpc_client {
            // If get_epoch_info is not available, fallback to is_healthy
            if !client.is_healthy().await {
                rpc_healthy = false;
                warn!("[ArbitrageEngine] Primary RPC endpoint is unhealthy.");
            }
        }

        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit = self.get_min_profit_threshold_pct().await;

        let should_degrade = !ws_healthy || !rpc_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues (WS: {}, RPC: {})", ws_healthy, rpc_healthy);
            let new_threshold = current_min_profit * degradation_factor;
            self.set_min_profit_threshold_pct(new_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold * 100.0);
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            let base_threshold = self.config.min_profit_pct;
            self.set_min_profit_threshold_pct(base_threshold * 100.0).await; // Fix: ensure f64 type
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_threshold * 100.0);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
        }
        *last_check = Instant::now();
        Ok(())
    }

    pub async fn update_pools(&self, new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        let mut writable_pools_ref = self.pools.write().await; // Renamed WritableRef
        let mut new_pools_count = 0;
        let mut updated_pools_count = 0;

        for (address, pool_info) in new_pools_data {
            if writable_pools_ref.contains_key(&address) {
                // Pool exists, update it
                // To avoid replacing the Arc if only data inside PoolInfo changes,
                // one might consider making PoolInfo fields mutable or using an inner RwLock.
                // For simplicity here, we replace the Arc.
                writable_pools_ref.insert(address, Arc::clone(&pool_info)); // Clone the Arc for insertion
                updated_pools_count += 1;
            } else {
                // New pool
                writable_pools_ref.insert(address, Arc::clone(&pool_info)); // Clone the Arc for insertion
                new_pools_count += 1;
            }
        }
        // self.metrics.lock().await.log_pools_updated(new_pools_count, updated_pools_count, writable_pools_ref.len()).await;
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_pools_count,
            updated_pools_count,
            writable_pools_ref.len()
        );
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        info!("Health check task running periodical checks...");
        let mut overall_healthy = true;
        if let Some(rpc) = &self.rpc_client {
            if !rpc.is_healthy().await { // is_healthy is on SolanaRpcClient
                warn!("RPC client reported as unhealthy.");
                overall_healthy = false;
            } else {
                info!("RPC client reported as healthy.");
            }
            // Optionally, log more details if available
        } else {
            warn!("RPC client not configured; skipping RPC health check.");
        }

        if let Some(ws_manager_arc) = &self.ws_manager {
            let manager = ws_manager_arc.lock().await; 
            if !manager.is_connected().await { // is_connected is on SolanaWebsocketManager
                warn!("WebSocket manager reported as disconnected.");
                overall_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1; 
                if attempts <= self.max_ws_reconnect_attempts { 
                    warn!(
                        "Attempting to reconnect WebSocket (attempt {}/{})",
                        attempts, self.max_ws_reconnect_attempts
                    );
                    // Attempt reconnect logic might be here or handled by the manager itself
                } else {
                    error!(
                        "Max WebSocket reconnect attempts ({}) reached. System may be degraded.",
                        self.max_ws_reconnect_attempts
                    );
                }
            } else {
                info!("WebSocket manager reported as connected.");
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed); 
            }
        } else {
            info!("WebSocket manager not configured; skipping WebSocket health check.");
        }
        
        if let Some(price_provider_arc) = &self.price_provider {
            // Assuming CryptoDataProvider might have a health check. If not, this is a placeholder.
            // For now, we'll assume it's healthy if it exists.
            // match price_provider_arc.check_health().await {
            //     Ok(_) => info!("Price provider reported as healthy."),
            //     Err(e) => {
            //         warn!("Price provider reported as unhealthy: {}", e);
            //         overall_healthy = false;
            //     }
            // }
            info!("Price provider is configured (health check placeholder).");
            let _ = price_provider_arc; // Silence unused variable warning
        } else {
            warn!("Price provider not configured; skipping price provider health check.");
        }

        self.metrics.lock().await.set_system_health(overall_healthy);
        *self.last_health_check.write().await = Instant::now(); 
        info!("Health checks completed. System healthy: {}. Last check updated.", overall_healthy);
    }

    pub async fn dex_providers_health_check(&self) {
        if self.dex_providers.is_empty() {
            warn!("No DEX providers configured. ArbitrageEngine cannot operate without DEX APIs.");
        } else {
            info!("Checking health of {} DEX providers...", self.dex_providers.len());
            // Example: In a real implementation, you would iterate and call a health check on each DEX client
            for (i, _provider) in self.dex_providers.iter().enumerate() {
                // If DexClient trait exposes a health check, call it here
                // For now, just log
                info!("DEX provider #{} is assumed healthy (placeholder)", i + 1);
            }
        }
    }

    pub async fn run_full_health_check(&self) {
        info!("Running full engine health check (including DEX providers)...");
        self.run_health_checks().await;
        self.dex_providers_health_check().await;
        // Optionally, add more checks here (price provider, metrics, etc.)
        info!("Full engine health check completed.");
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        info!("Dynamic threshold update service starting within ArbitrageEngine.");
        // Corrected: Pass window size to VolatilityTracker::new
        let mut vol_tracker = VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));
        let update_interval_duration = Duration::from_secs(self.config.dynamic_threshold_update_interval_secs.unwrap_or(300));
        let base_threshold_fractional = self.config.min_profit_pct; 
        // Corrected: Use volatility_threshold_factor from config
        let volatility_factor_val = self.config.volatility_threshold_factor.unwrap_or(0.1); 

        loop {
            tokio::time::sleep(update_interval_duration).await;
            
            let current_price_of_major_asset = match &self.price_provider {
                Some(provider) => {
                    let symbol_to_track = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL/USDC"); 
                    match provider.get_price(symbol_to_track).await {
                        Some(price) => {
                            log::debug!("DynamicThreshold: Fetched price for {}: {}", symbol_to_track, price);
                            price
                        },
                        None => {
                            warn!("DynamicThreshold: Could not fetch price for {}. Using fallback 100.0", symbol_to_track);
                            100.0 
                        }
                    }
                }
                None => {
                    warn!("DynamicThreshold: Price provider not available. Using fallback 100.0 for volatility calculation.");
                    100.0 
                }
            };

            vol_tracker.add_price(current_price_of_major_asset);
            // Corrected: Call volatility() method on VolatilityTracker
            let historical_volatility = vol_tracker.volatility();

            let new_threshold_fractional = dynamic_threshold::recommend_min_profit_threshold( // Corrected: Call as free function
                historical_volatility, 
                base_threshold_fractional, 
                volatility_factor_val
            );
            let new_threshold_pct = new_threshold_fractional * 100.0; 

            self.set_min_profit_threshold_pct(new_threshold_pct).await; 

            info!(
                "DynamicThresholdUpdater (Engine): Recommended new min profit threshold: {:.4}% (Volatility: {:.6}, BaseFrac: {:.4}, Factor: {:.2})",
                new_threshold_pct, historical_volatility, base_threshold_fractional, volatility_factor_val
            );
        }
    }

    /// Expose a public method for pool guard access for test and integration use.
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
                error!(
                    "{}: Unexpected error cloning pools Arc for guard: {}",
                    operation_name, e
                );
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

    /// Main arbitrage detection entry point for the engine.
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
        
        let duration = start_time.elapsed();
        info!("Arbitrage detection cycle completed in {:?}. Total opportunities found: {}", duration, all_opportunities.len());
        
        if !all_opportunities.is_empty() {
            let best_opp = &all_opportunities[0]; // This is safe due to the !is_empty() check
             info!("Best opportunity: ID {}, Profit Pct: {:.4}%, Input: {} {}, Output: {} {}",
                  best_opp.id, best_opp.profit_pct, best_opp.input_amount, best_opp.input_token, best_opp.expected_output, best_opp.output_token);
        }
        Ok(all_opportunities)
    }

    pub async fn get_current_status_string(&self) -> String {
        // Placeholder implementation
        let mut status = String::new();
        status.push_str(&format!("Engine Health: {}\n", if self.degradation_mode.load(Ordering::Relaxed) { "DEGRADED" } else { "OK" }));
        status.push_str(&format!("Pools in cache: {}\n", self.pools.read().await.len()));
        if let Some(ws_manager_arc) = &self.ws_manager {
            let manager = ws_manager_arc.lock().await;
            status.push_str(&format!("WebSocket Connected: {}\n", manager.is_connected().await));
        }
        // Add more status information as needed
        status
    }
}

#[cfg(test)]
mod detector_integration_tests {
    use super::*;
    use crate::arbitrage::detector::ArbitrageDetector;
    use crate::metrics::Metrics;
    use std::collections::HashMap;
    use std::sync::Arc;
    use solana_sdk::pubkey::Pubkey;
    use tokio::sync::Mutex;
    use crate::config::settings::Config;

    #[tokio::test]
    async fn test_exercise_all_detector_functions() {
        // Exercise ArbitrageDetector::new with all required arguments
        let mut detector = ArbitrageDetector::new(0.5, 0.05, 150.0, 5000);
        let mut metrics = Metrics::default();
        let pools: HashMap<Pubkey, Arc<PoolInfo>> = HashMap::new();
        // Exercise find_two_hop_opportunities (deprecated, but must be called)
        let _ = detector.find_two_hop_opportunities(&pools, &mut metrics).await;
        // Exercise log_banned_pair
        ArbitrageDetector::log_banned_pair("A", "B", "permanent", "integration test");
        // Exercise is_permanently_banned and is_temporarily_banned
        let _ = ArbitrageDetector::is_permanently_banned("A", "B");
        let _ = ArbitrageDetector::is_temporarily_banned("A", "B");
        // Exercise set_min_profit_threshold and get_min_profit_threshold_pct
        detector.set_min_profit_threshold(0.6);
        let _ = detector.get_min_profit_threshold_pct();
        // Exercise new_from_config
        let config = Config::test_default();
        let _ = ArbitrageDetector::new_from_config(&config);
    }
}
