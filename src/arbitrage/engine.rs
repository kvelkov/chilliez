// src/arbitrage/engine.rs

// src/arbitrage/engine.rs
// Use this specific import to avoid conflicts
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
// If there's a conflict, you can alias it:
// use crate::error::{RetryPolicy as ErrorRetryPolicy};
use crate::utils::PoolParser; // Import the PoolParser trait
use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::DynamicThresholdUpdater, // Removed VolatilityTracker and self
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::quote::DexClient,
    dex::whirlpool_parser::{WhirlpoolPoolParser, ORCA_WHIRLPOOL_PROGRAM_ID}, // For pool discovery example
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager}, // Added PoolParser
    utils::PoolInfo,
    websocket::CryptoDataProvider,
};
use log::{error, info, warn}; // Removed unused debug
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::error::Elapsed as TimeoutError;
use tokio::time::timeout; // For Pubkey::from_str
use crate::WebsocketUpdate;
use log::debug;
use tokio::sync::mpsc::error::TryRecvError;

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
    pub(crate) max_ws_reconnect_attempts: u64,
    pub(crate) detector: Arc<Mutex<ArbitrageDetector>>,
    pub(crate) _dex_providers: Vec<Arc<dyn DexClient>>, // Prefixed
    pub(crate) dynamic_threshold_updater: Option<Arc<Mutex<DynamicThresholdUpdater>>>,
    // Error handling components
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    retry_policy: RetryPolicy,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
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
            info!(
                "ArbitrageEngine initialized with {} DEX API client(s).",
                dex_api_clients.len()
            );
        }

        // Initialize the dynamic threshold updater
        let dynamic_threshold_updater = Some(Arc::new(Mutex::new(DynamicThresholdUpdater::new(
            Arc::clone(&config),
        )))); // Wrapped in Mutex

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
            _dex_providers: dex_api_clients, // Prefixed
            dynamic_threshold_updater,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            retry_policy: RetryPolicy::default(),
        }
    }

    async fn load_initial_pools(&self) -> Result<(), ArbError> {
        info!("Attempting to load initial pools...");
        if let Some(rpc_client) = &self.rpc_client {
            // Example: Discovering Orca Whirlpools
            let whirlpool_program_id =
                Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).map_err(|e| {
                    ArbError::ConfigError(format!("Invalid Whirlpool Program ID: {}", e))
                })?;

            match rpc_client
                .get_program_accounts(&whirlpool_program_id, vec![])
                .await
            {
                Ok(accounts_data) => {
                    let mut discovered_pools = HashMap::new();
                    for (pubkey, data) in accounts_data {
                        match WhirlpoolPoolParser::parse_pool_data(pubkey, &data) {
                            Ok(pool_info) => {
                                discovered_pools.insert(pubkey, Arc::new(pool_info));
                            }
                            Err(e) => {
                                warn!("Failed to parse pool data for account {}: {}", pubkey, e);
                            }
                        }
                    }
                    self.update_pools(discovered_pools).await?;
                    info!(
                        "Successfully loaded {} initial Whirlpool pools.",
                        self.pools.read().await.len()
                    );
                }
                Err(e) => {
                    error!("Failed to get program accounts for Whirlpools: {}", e);
                }
            }
        } else {
            warn!("RPC client not available for initial pool loading.");
        }
        Ok(())
    }

    pub async fn start_services(&self, cache: Option<Arc<crate::cache::Cache>>) {
        self.load_initial_pools()
            .await
            .unwrap_or_else(|e| error!("Failed to load initial pools: {}", e));

        if let Some(manager_arc) = &self.ws_manager {
            let manager = manager_arc.lock().await;
            if let Err(e) = manager.start(cache.clone()).await {
                error!(
                    "[ArbitrageEngine] Failed to start WebSocket manager: {}",
                    e
                );
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }

        // Start dynamic threshold updater if available
        if let Some(updater_arc) = &self.dynamic_threshold_updater {
            let updater_clone = Arc::clone(updater_arc);
            let metrics_clone = Arc::clone(&self.metrics);
            let detector_clone = Arc::clone(&self.detector); // Pass detector for setting threshold

            // Spawn the task so it doesn't block start_services
            tokio::spawn(async move {
                DynamicThresholdUpdater::start_monitoring_task(
                    updater_clone,
                    metrics_clone,
                    detector_clone,
                )
                .await;
            });
            info!("[ArbitrageEngine] Dynamic threshold updater task spawned successfully.");
        }

        // Start other services like price provider if they have a start method
        // Make run_full_health_check live by calling it once on startup
        // TODO: Consider moving to a periodic task in the main application loop for continuous health monitoring.
        info!("[ArbitrageEngine] Performing initial full health check...");
        self.run_full_health_check(cache).await;

        // Make get_cached_pool_count (and thus with_pool_guard_async) live
        match self.get_cached_pool_count().await {
            Ok(count) => info!("[ArbitrageEngine] Initial cached pool count: {}", count),
            Err(e) => warn!(
                "[ArbitrageEngine] Could not retrieve initial cached pool count: {}",
                e
            ),
        }

        info!("[ArbitrageEngine] All core services started or initialized.");
    }

    pub async fn get_min_profit_threshold_pct(&self) -> f64 {
        self.detector.lock().await.get_min_profit_threshold_pct()
    }

    pub async fn set_min_profit_threshold_pct(&self, threshold_pct: f64) {
        self.detector
            .lock()
            .await
            .set_min_profit_threshold(threshold_pct);
        info!(
            "ArbitrageEngine's detector min_profit_threshold_pct updated to: {:.4}%",
            threshold_pct
        );
        let fractional_threshold = threshold_pct / 100.0;
        self.metrics
            .lock()
            .await
            .log_dynamic_threshold_update(fractional_threshold);
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
                ArbError::TimeoutError(format!(
                    "Timeout for pools read lock in {}",
                    operation_name
                ))
            })?;
            guard_result.deref().clone()
        };

        detector_call(
            Arc::clone(&self.detector),
            pools_map_clone,
            Arc::clone(&self.metrics),
        )
        .await
    }

    pub async fn _discover_direct_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // Corrected: Use MultiHopArbOpportunity
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

    pub async fn discover_fixed_input_opportunities(
        &self,
        fixed_input_amount: f64,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // Corrected: Use MultiHopArbOpportunity
        if fixed_input_amount <= 0.0 {
            warn!(
                "discover_fixed_input_opportunities called with non-positive amount: {}. Skipping.",
                fixed_input_amount
            );
            return Ok(Vec::new());
        }

        self.discover_opportunities_internal(
            "discover_fixed_input_opportunities",
            move |detector_arc, pools_map, metrics_arc| async move {
                let detector_guard = detector_arc.lock().await;
                let mut metrics_guard = metrics_arc.lock().await;
                detector_guard
                    .find_two_hop_opportunities_with_fixed_input(
                        &pools_map,
                        &mut *metrics_guard,
                        fixed_input_amount, // Corrected: Pass &mut *metrics_guard
                    )
                    .await
            },
        )
        .await
    }

    pub async fn _discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // Corrected: Use MultiHopArbOpportunity
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

    // Added back discover_multihop_opportunities
    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // This is a simplified version. The primary multihop logic is in _discover_multihop_opportunities_with_risk.
        // This function can delegate or implement a simpler form of multihop detection if needed.
        // For now, let's make it delegate to the risk-adjusted version or a simpler internal call.
        // This is a placeholder implementation.
        info!("discover_multihop_opportunities called (placeholder). Consider using _discover_multihop_opportunities_with_risk for full features.");
        self._discover_multihop_opportunities_with_risk().await
    }

    // Added back resolve_pools_for_opportunity
    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let mut resolved_pools = Vec::new();
        let pools_guard = self.pools.read().await;

        for pool_address in &opportunity.pool_path {
            match pools_guard.get(pool_address) {
                Some(pool_arc) => resolved_pools.push(Arc::clone(pool_arc)),
                None => {
                    let err_msg = format!("Pool not found in cache: {}", pool_address);
                    error!("{}", err_msg);
                    return Err(ArbError::PoolNotFound(pool_address.to_string()));
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
            // The .start() method now requires a cache argument (Option<Arc<Cache>>)
            // Always provide an argument, e.g., None if you don't have a cache instance here.
            if !manager.is_connected().await {
                ws_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                if attempts < self.max_ws_reconnect_attempts {
                    warn!(
                        "[ArbitrageEngine] WebSocket disconnected. Attempting reconnect (attempt {}/{})",
                        attempts + 1,
                        self.max_ws_reconnect_attempts
                    );
                    if let Err(e) = manager.start(None).await {
                        // <-- FIX: pass None for cache
                        error!(
                            "[ArbitrageEngine] WebSocket reconnect attempt failed: {}",
                            e
                        );
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
        let was_degraded = self
            .degradation_mode
            .swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!(
                "[ArbitrageEngine] Entering degradation mode due to system health issues (WS: {}, RPC: {})",
                ws_healthy, rpc_healthy
            );
            let new_threshold = current_min_profit * degradation_factor;
            self.set_min_profit_threshold_pct(new_threshold).await;
            info!(
                "[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.",
                new_threshold * 100.0
            );
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            let base_threshold = self.config.min_profit_pct;
            self.set_min_profit_threshold_pct(base_threshold * 100.0)
                .await; // Fix: ensure f64 type
            info!(
                "[ArbitrageEngine] Min profit threshold reset to {:.4}%.",
                base_threshold * 100.0
            );
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
        }

        *last_check = Instant::now();
        Ok(())
    }

    // Added back update_pools
    pub async fn update_pools(
        &self,
        new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        let mut writable_pools_ref = self.pools.write().await;
        let mut new_pools_count = 0;
        let mut updated_pools_count = 0;

        for (key, pool_info) in new_pools_data {
            if writable_pools_ref.contains_key(&key) {
                writable_pools_ref.insert(key, pool_info);
                updated_pools_count += 1;
            } else {
                writable_pools_ref.insert(key, pool_info);
                new_pools_count += 1;
            }
        }

        if new_pools_count > 0 || updated_pools_count > 0 {
            info!(
                "Pools updated: {} new, {} updated. Total pools: {}",
                new_pools_count,
                updated_pools_count,
                writable_pools_ref.len()
            );
        }

        Ok(())
    }

    pub async fn run_health_checks(&self) {
        info!("Health check task running periodical checks...");
        let mut overall_healthy = true;

        if let Some(rpc) = &self.rpc_client {
            if !rpc.is_healthy().await {
                // is_healthy is on SolanaRpcClient
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
            if !manager.is_connected().await {
                // is_connected is on SolanaWebsocketManager
                warn!("WebSocket manager reported as disconnected.");
                overall_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!(
                        "Attempting to reconnect WebSocket (attempt {}/{})",
                        attempts, self.max_ws_reconnect_attempts