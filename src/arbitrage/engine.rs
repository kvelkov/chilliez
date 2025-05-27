// /Users/kiril/Desktop/chilliez/src/arbitrage/engine.rs
// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::ArbitrageDetector, dynamic_threshold::DynamicThresholdUpdater,
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::{
        quote::DexClient,
        whirlpool_parser::{WhirlpoolPoolParser, ORCA_WHIRLPOOL_PROGRAM_ID},
    },
    error::{ArbError, CircuitBreaker, RetryPolicy},
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::{PoolInfo, PoolParser},
    websocket::CryptoDataProvider,
    WebsocketUpdate,
};

use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    ops::Deref,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::{
    sync::{mpsc::error::TryRecvError, Mutex, RwLock},
    time::{error::Elapsed as TimeoutError, timeout, Duration},
};

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
                error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }

        // Start dynamic threshold updater if available
        if let Some(updater_arc) = &self.dynamic_threshold_updater {
            let updater_clone = Arc::clone(updater_arc);
            let metrics_clone = Arc::clone(&self.metrics);
            let detector_clone = Arc::clone(&self.detector); // Pass detector for setting threshold
            let price_provider_clone = self.price_provider.clone(); // Clone price_provider for the task

            // Spawn the task so it doesn't block start_services
            tokio::spawn(async move {
                DynamicThresholdUpdater::start_monitoring_task(
                    updater_clone,
                    metrics_clone,
                    detector_clone,
                    price_provider_clone, // Pass it here
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

    // Added to expose the detector's min_profit_threshold_usd
    pub async fn get_min_profit_threshold_usd(&self) -> f64 {
        self.detector.lock().await.get_min_profit_threshold_usd()
    }

    async fn discover_opportunities_internal<F, Fut>(
        &self,
        operation_name: &str,
        detector_call: F,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError>
    // Corrected: Use MultiHopArbOpportunity
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
                    error!(
                        "[ArbitrageEngine] WebSocket disconnected. Max reconnect attempts reached."
                    );
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
                    );
                    if let Err(e) = manager.start(None).await {
                        error!("WebSocket reconnection failed: {}", e);
                    } else {
                        info!("WebSocket reconnection successful.");
                        self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
                    }
                } else {
                    error!("Max WebSocket reconnection attempts reached. Manual intervention may be required.");
                }
            } else {
                debug!("WebSocket manager reported as connected.");
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            }
        } else {
            warn!("WebSocket manager not configured; skipping WebSocket health check.");
        }

        if overall_healthy {
            info!("Overall system health: HEALTHY");
        } else {
            warn!("Overall system health: DEGRADED");
        }
    }

    pub async fn run_full_health_check(&self, _cache: Option<Arc<crate::cache::Cache>>) {
        info!("Running full health check...");

        // Check pool count
        let pool_count = self.pools.read().await.len();
        info!("Current pool count: {}", pool_count);

        // Check degradation mode
        let is_degraded = self.degradation_mode.load(Ordering::Relaxed);
        info!(
            "Degradation mode: {}",
            if is_degraded { "ACTIVE" } else { "INACTIVE" }
        );

        // Check current profit threshold
        let current_threshold = self.get_min_profit_threshold_pct().await;
        info!("Current min profit threshold: {:.4}%", current_threshold);

        // Run standard health checks
        self.run_health_checks().await;

        // Update last health check timestamp
        *self.last_health_check.write().await = Instant::now();

        info!("Full health check completed.");
    }

    pub async fn get_cached_pool_count(&self) -> Result<usize, ArbError> {
        self.with_pool_guard_async(|pools| pools.len()).await
    }

    pub async fn with_pool_guard_async<F, R>(&self, operation: F) -> Result<R, ArbError>
    where
        F: FnOnce(&HashMap<Pubkey, Arc<PoolInfo>>) -> R,
    {
        let timeout_duration =
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000));

        match timeout(timeout_duration, self.pools.read()).await {
            Ok(guard) => Ok(operation(&*guard)),
            Err(_) => {
                warn!("Timeout waiting for pools read lock in with_pool_guard_async");
                Err(ArbError::TimeoutError("Pool read lock timeout".to_string()))
            }
        }
    }

    pub async fn process_websocket_updates(&self) -> Result<(), ArbError> {
        if let Some(ws_manager_arc) = &self.ws_manager {
            let mut manager = ws_manager_arc.lock().await;

            loop {
                match manager.try_recv_update().await {
                    Ok(Some(update)) => {
                        debug!("Processing WebSocket update: {:?}", update);
                        self.handle_websocket_update(update).await?;
                    }
                    Ok(None) => {
                        // No more updates available
                        break;
                    }
                    Err(TryRecvError::Empty) => {
                        // No updates available right now
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        warn!("WebSocket update channel disconnected");
                        return Err(ArbError::WebSocketError(
                            "Update channel disconnected".to_string(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_websocket_update(&self, update: WebsocketUpdate) -> Result<(), ArbError> {
        // Use a more generic approach since we don't know the exact variant names
        debug!("Received WebSocket update: {:?}", update);

        // For now, we'll implement a placeholder that can be extended
        // once we know the exact structure of WebsocketUpdate
        match update {
            // Add specific handling once we know the correct variant names
            _ => {
                debug!("WebSocket update received but not specifically handled yet");
            }
        }
        Ok(())
    }

    async fn try_parse_pool_data(&self, pubkey: Pubkey, data: &[u8]) -> Result<PoolInfo, ArbError> {
        // This would use the pool parser registry to determine the correct parser
        // For now, we'll use a simple approach
        use crate::dex::pool::POOL_PARSER_REGISTRY;

        // Try to determine the program owner and use appropriate parser
        // This is a simplified version - in practice you'd need to get the account owner
        for (program_id, parser_fn) in POOL_PARSER_REGISTRY.iter() {
            if let Ok(pool_info) = parser_fn(pubkey, data) {
                debug!(
                    "Successfully parsed pool {} with parser for program {}",
                    pubkey, program_id
                );
                return Ok(pool_info);
            }
        }

        Err(ArbError::ParseError(format!(
            "Could not parse pool data for {}",
            pubkey
        )))
    }

    pub async fn get_current_status_string(&self) -> String {
        let mut status = String::new();
        status.push_str(&format!(
            "Engine Health: {}\n",
            if self.degradation_mode.load(Ordering::Relaxed) {
                "DEGRADED"
            } else {
                "OK"
            }
        ));
        status.push_str(&format!(
            "Pools in cache: {}\n",
            self.pools.read().await.len()
        ));
        if let Some(ws_manager_arc) = &self.ws_manager {
            let manager = ws_manager_arc.lock().await;
            status.push_str(&format!(
                "WebSocket Connected: {}\n",
                manager.is_connected().await
            ));
        }
        if let Some(rpc_client) = &self.rpc_client {
            status.push_str(&format!("RPC Healthy: {}\n", rpc_client.is_healthy().await));
        }
        status.push_str(&format!(
            "Min Profit Threshold: {:.4}%\n",
            self.get_min_profit_threshold_pct().await
        ));
        status.push_str(&format!(
            "Min Profit USD Threshold: ${:.2}\n", // Added USD threshold display
            self.get_min_profit_threshold_usd().await
        ));
        status.push_str(&format!(
            "WS Reconnect Attempts: {}/{}\n",
            self.ws_reconnect_attempts.load(Ordering::Relaxed),
            self.max_ws_reconnect_attempts
        ));

        status
    }

    pub async fn shutdown(&self) -> Result<(), ArbError> {
        info!("Shutting down ArbitrageEngine...");

        // Stop WebSocket manager
        if let Some(ws_manager_arc) = &self.ws_manager {
            let _manager = ws_manager_arc.lock().await;
            // If there's no stop method, just drop the manager
            info!("WebSocket manager will be stopped when dropped.");
        }

        // Stop dynamic threshold updater if needed
        if let Some(_updater_arc) = &self.dynamic_threshold_updater {
            info!("Dynamic threshold updater will be stopped when dropped.");
        }

        // Clear pools cache
        self.pools.write().await.clear();
        info!("Pools cache cleared.");

        info!("ArbitrageEngine shutdown completed.");
        Ok(())
    }

    pub async fn run_dynamic_threshold_updates(&self) -> Result<(), ArbError> {
        info!("Starting dynamic threshold updates...");

        let mut interval = tokio::time::interval(Duration::from_secs(60)); // Update every minute

        loop {
            interval.tick().await;

            // Check if we should stop
            if self.degradation_mode.load(Ordering::Relaxed) {
                warn!("Skipping threshold update due to degradation mode");
                continue;
            }

            // Update threshold based on market conditions
            let current_threshold = self.get_min_profit_threshold_pct().await;
            let pool_count = self.pools.read().await.len();

            // Simple dynamic adjustment logic
            let new_threshold = if pool_count < 10 {
                // Fewer pools, require higher profit
                current_threshold * 1.1
            } else if pool_count > 100 {
                // More pools, can accept lower profit
                current_threshold * 0.95
            } else {
                current_threshold
            };

            // Clamp to reasonable bounds
            let clamped_threshold = new_threshold.max(0.1).min(5.0);

            if (clamped_threshold - current_threshold).abs() > 0.01 {
                info!(
                    "Updating profit threshold from {:.4}% to {:.4}%",
                    current_threshold, clamped_threshold
                );
                self.set_min_profit_threshold_pct(clamped_threshold).await;
            }

            debug!(
                "Dynamic threshold update completed. Current: {:.4}%",
                clamped_threshold
            );
        }
    }

    pub async fn process_websocket_updates_loop(&self) -> Result<(), ArbError> {
        info!("Starting WebSocket updates processing loop...");

        let mut interval = tokio::time::interval(Duration::from_millis(100)); // Process every 100ms

        loop {
            interval.tick().await;

            // Process any pending WebSocket updates
            if let Err(e) = self.process_websocket_updates().await {
                warn!("Error processing WebSocket updates: {}", e);

                // If we're in degradation mode, sleep longer
                if self.degradation_mode.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }

            // Check if we should continue
            if self.degradation_mode.load(Ordering::Relaxed) {
                // In degradation mode, process less frequently
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        debug!("Starting arbitrage detection...");

        // Get current pools
        let pools = self.pools.read().await.clone();
        if pools.is_empty() {
            debug!("No pools available for arbitrage detection");
            return Ok(vec![]);
        }

        debug!(
            "Detecting arbitrage opportunities across {} pools",
            pools.len()
        );

        // Use the ArbitrageDetector with proper constructor
        let detector = ArbitrageDetector::new_from_config(&self.config);

        // Convert pools to the format expected by the detector
        let pools_hashmap: HashMap<Pubkey, Arc<PoolInfo>> =
            pools.iter().map(|(k, v)| (*k, Arc::clone(v))).collect();

        // Use the correct method name from detector.rs
        let mut metrics = Metrics::default();
        let opportunities = detector
            .find_all_opportunities(&pools_hashmap, &mut metrics)
            .await?;

        info!("Found {} arbitrage opportunities", opportunities.len());

        // Log summary of opportunities
        for (i, opp) in opportunities.iter().enumerate() {
            debug!(
                "Opportunity {}: profit={:.4}% amount={:.6}",
                i, opp.profit_pct, opp.total_profit
            );
        }

        Ok(opportunities)
    }

    /// Returns a reference to the price provider if configured.
    pub fn get_price_provider(&self) -> Option<&Arc<dyn CryptoDataProvider + Send + Sync>> {
        self.price_provider.as_ref()
    }

    /// Returns the health check interval duration.
    pub fn get_health_check_interval(&self) -> Duration {
        self.health_check_interval
    }

    /// Returns a clone of the circuit breaker Arc.
    pub fn get_circuit_breaker(&self) -> Arc<RwLock<CircuitBreaker>> {
        Arc::clone(&self.circuit_breaker)
    }

    /// Returns a reference to the retry policy.
    pub fn get_retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    // Placeholder for the actual implementation.
    // This method is crucial for the retry logic in main.rs.
    pub async fn is_opportunity_still_valid(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        info!("[ArbitrageEngine] Checking if opportunity ID {} is still valid.", opportunity.id);
        // In a real implementation, you would:
        // 1. Re-fetch or use cached current prices for the input token.
        // 2. Re-calculate the opportunity's profit percentage and USD profit based on current pool states (if feasible without full re-detection).
        //    Alternatively, check against slightly tighter thresholds than initial detection to account for market movement.
        // 3. Compare against the detector's current min_profit_threshold_pct and min_profit_threshold_usd.
        
        // For now, let's assume it's valid if its original profit meets current thresholds.
        // This is a simplification.
        let detector_guard = self.detector.lock().await;
        let current_min_pct = detector_guard.get_min_profit_threshold_pct();
        let current_min_usd = detector_guard.get_min_profit_threshold_usd();
        
        let meets_pct = opportunity.profit_pct >= current_min_pct;
        let meets_usd = opportunity.estimated_profit_usd.unwrap_or(-1.0) >= current_min_usd;

        meets_pct && meets_usd
    }
}
