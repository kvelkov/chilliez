use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::DynamicThresholdUpdater,
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::DexClient,
    error::{ArbError, CircuitBreaker, RetryPolicy},
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::PoolInfo,
};


use crate::solana::WebsocketUpdate;
use crate::websocket::CryptoDataProvider;
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
    time::Instant,
};
use tokio::{
    sync::{Mutex, RwLock},
    time::{sleep, Duration},
};

/// The ArbitrageEngine orchestrates the entire arbitrage workflow:
/// - It monitors and updates pool state via WebSocket feeds,
/// - It triggers dynamic threshold updates through the DynamicThresholdUpdater,
/// - It calls the detector to search for arbitrage opportunities,
/// - And it schedules execution via the Executor (through mod.rs or directly).
pub struct ArbitrageEngine {
    // Shared pool state (keyed by pool address)
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    // Optional websocket manager for pool & market updates.
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    // Price provider for dynamic threshold updates.
    price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    // Metrics system for external recording of events and states.
    metrics: Arc<Mutex<Metrics>>,
    // Optional RPC client (e.g., Solana cluster interface)
    rpc_client: Option<Arc<SolanaRpcClient>>,
    pub config: Arc<Config>, // Made public for direct test access, consider getter for encapsulation
    // A flag to indicate if the engine is in degradation mode.
    pub degradation_mode: Arc<AtomicBool>,
    // Timestamp for the last health check – used for periodic system status updates.
    last_health_check: Arc<RwLock<Instant>>,
    // Interval at which health checks are run.
    pub health_check_interval: Duration,
    // Websocket reconnect attempts counter.
    pub ws_reconnect_attempts: Arc<AtomicU64>,
    // Maximum permitted websocket reconnect attempts.
    pub max_ws_reconnect_attempts: u64,
    // Detector instance used for scanning arbitrage opportunities.
    detector: Arc<Mutex<ArbitrageDetector>>,
    // Optionally provided DEX clients (if any).
    _dex_providers: Vec<Arc<dyn DexClient>>,
    // Dynamic threshold updater instance (for adjusting min profit thresholds in real time).
    dynamic_threshold_updater: Option<Arc<Mutex<DynamicThresholdUpdater>>>,
    // Circuit breaker to disable trading in extreme conditions.
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    // Policy for retrying operations in case of transient errors.
    retry_policy: RetryPolicy,
    // Minimum profit threshold (in percentage) in use (shared with both the engine and detector).
    min_profit_threshold_pct: Arc<RwLock<f64>>,
}

impl ArbitrageEngine {
    /// Constructs a new ArbitrageEngine.
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_api_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        // Initialize the detector from the configuration.
        let internal_detector = Arc::new(Mutex::new(ArbitrageDetector::new_from_config(&config)));
        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);
        let initial_profit_threshold = config.min_profit_pct * 100.0;

        // Initialize the dynamic threshold updater.
        let dynamic_threshold_updater = Some(Arc::new(Mutex::new(
            DynamicThresholdUpdater::new(Arc::clone(&config)),
        )));

        if dex_api_clients.is_empty() {
            warn!("ArbitrageEngine initialized with no DEX API clients.");
        } else {
            info!(
                "ArbitrageEngine initialized with {} DEX API client(s).",
                dex_api_clients.len()
            );
        }

        Self {
            pools,
            ws_manager: ws_manager_instance,
            price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now() - Duration::from_secs(health_check_interval_secs * 2))),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector: internal_detector,
            _dex_providers: dex_api_clients,
            dynamic_threshold_updater,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            retry_policy: RetryPolicy::default(),
            min_profit_threshold_pct: Arc::new(RwLock::new(initial_profit_threshold)),
        }
    }

    /// Handles incoming updates from the WebSocket feed.
    pub async fn handle_websocket_update(&self, update: WebsocketUpdate) -> Result<(), ArbError> {
        match update {
            WebsocketUpdate::PoolUpdate(pool_info) => {
                let pubkey = pool_info.address;
                debug!("[Engine] Received PoolUpdate for {} ({})", pubkey, pool_info.name);
                let pool_info_arc = Arc::new(pool_info);
                let mut new_pool = HashMap::new();
                new_pool.insert(pubkey, pool_info_arc.clone());

                if let Err(e) = self.update_pools(new_pool).await {
                    error!("[Engine] Failed to update pool {}: {}", pubkey, e);
                } else {
                    info!("[Engine] Pool {} updated via WS.", pubkey);
                    let total_pools = self.pools.read().await.len();
                    self.metrics.lock().await.log_pools_updated(0, 1, total_pools);
                }
            }
            WebsocketUpdate::GenericUpdate(msg) => {
                warn!("[Engine] Received GenericUpdate: {}", msg);
            }
        }
        Ok(())
    }

    /// Parses raw pool data using the pool parser registry.
    pub async fn try_parse_pool_data(&self, pubkey: Pubkey, data: &[u8]) -> Result<PoolInfo, ArbError> {
        use crate::dex::pool::POOL_PARSER_REGISTRY;
        for (program_id, parser_fn) in POOL_PARSER_REGISTRY.iter() {
            if let Ok(pool_info) = parser_fn(pubkey, data) {
                debug!("Pool {} parsed with program {}.", pubkey, program_id);
                return Ok(pool_info);
            }
        }
        Err(ArbError::ParseError(format!("Could not parse data for pool {}", pubkey)))
    }

    /// Updates the shared pools cache with new pool data.
    pub async fn update_pools(
        &self,
        new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        let mut pools_guard = self.pools.write().await;
        let mut new_count = 0;
        let mut update_count = 0;
        for (key, pool_info) in new_pools_data {
            if pools_guard.contains_key(&key) {
                pools_guard.insert(key, pool_info);
                update_count += 1;
            } else {
                pools_guard.insert(key, pool_info);
                new_count += 1;
            }
        }
        info!(
            "[Engine] Pools updated: {} new, {} updated. Total: {}",
            new_count,
            update_count,
            pools_guard.len()
        );
        Ok(())
    }

    /// Resolves pool references for a given arbitrage opportunity.
    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let mut resolved = Vec::new();
        let pools_guard = self.pools.read().await;
        for pool_address in &opportunity.pool_path {
            if let Some(pool_arc) = pools_guard.get(pool_address) {
                resolved.push(Arc::clone(pool_arc));
            } else {
                let err_msg = format!("Pool not found: {}", pool_address);
                error!("{}", err_msg);
                return Err(ArbError::PoolNotFound(pool_address.to_string()));
            }
        }
        Ok(resolved)
    }

    /// Sets the minimum profit threshold (as percent) and updates the detector.
    pub async fn set_min_profit_threshold_pct(&self, threshold: f64) {
        {
            let mut guard = self.min_profit_threshold_pct.write().await;
            *guard = threshold;
        }
        let mut detector_guard = self.detector.lock().await;
        detector_guard.set_min_profit_threshold(threshold);
    }

    /// Retrieves the current min profit threshold.
    pub async fn get_min_profit_threshold_pct(&self) -> f64 {
        *self.min_profit_threshold_pct.read().await
    }

    /// Returns a clone of the engine's circuit breaker.
    pub fn get_circuit_breaker(&self) -> Arc<RwLock<CircuitBreaker>> {
        Arc::clone(&self.circuit_breaker)
    }

    /// Returns a clone of the engine's retry policy.
    pub fn get_retry_policy(&self) -> RetryPolicy {
        self.retry_policy.clone()
    }

    /// Returns a clone of the engine's configuration.
    pub fn get_config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }

    /// Returns a clone of the Arc-wrapped last health check timestamp.
    pub fn get_last_health_check(&self) -> Arc<RwLock<Instant>> {
        Arc::clone(&self.last_health_check)
    }

    /// Executes a closure with a read guard to the pools map.
    /// This allows safe, read-only access to the shared pool data.
    pub async fn with_pool_guard_async<F, R>(&self, f: F) -> R
    where
        F: FnOnce(tokio::sync::RwLockReadGuard<'_, HashMap<Pubkey, Arc<PoolInfo>>>) -> R,
    {
        let pools_guard = self.pools.read().await;
        f(pools_guard)
    }

    /// Discovers arbitrage opportunities by invoking the detector (both direct and multi-hop).
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let pools_guard = self.pools.read().await;
        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        let mut opps = detector_guard.find_all_opportunities(&*pools_guard, &mut *metrics_guard).await?;
        let mut multihop_opps = detector_guard.find_all_multihop_opportunities(&*pools_guard, &mut *metrics_guard).await?;
        opps.append(&mut multihop_opps);
        opps.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        Ok(opps)
    }

    /// Discovers fixed input arbitrage opportunities.
    pub async fn discover_fixed_input_opportunities(&self, fixed_input_amount: f64) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let pools_guard = self.pools.read().await;
        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        detector_guard.find_two_hop_opportunities(&*pools_guard, &mut *metrics_guard, fixed_input_amount).await
    }

    /// Discovers multi-hop arbitrage opportunities.
    pub async fn discover_multihop_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let pools_guard = self.pools.read().await;
        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        detector_guard.find_all_multihop_opportunities(&*pools_guard, &mut *metrics_guard).await
    }

    /// Checks if an opportunity is still valid based on current market conditions.
    pub async fn is_opportunity_still_valid(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        if opportunity.hops.is_empty() {
            return false;
        }
        // This is a simplified check. A more robust check would re-simulate the trade
        // with current pool reserves and compare against dynamic profit thresholds.
        // For now, we'll assume it's valid if the profit percentage still meets the engine's current threshold.
        let current_min_profit_pct = self.get_min_profit_threshold_pct().await;
        opportunity.profit_pct >= current_min_profit_pct
    }

    /// Starts background services like WebSocket manager.
    pub async fn start_services(&self, cache: Option<Arc<crate::cache::Cache>>) {
        if let Some(ws_manager_arc) = &self.ws_manager {
            info!("[Engine] Starting WebSocket Manager service...");
            if let Err(e) = ws_manager_arc.lock().await.start(cache).await {
                error!("[Engine] Failed to start WebSocket Manager: {}", e);
            }
        } else {
            warn!("[Engine] WebSocket Manager not configured, skipping start_services for it.");
        }
        // Potentially start other services here
    }

    /// Health check: verifies that the websocket connection is active.
    pub async fn run_health_checks(&self) {
        if let Some(ws_manager_arc) = &self.ws_manager {
            let ws_manager = ws_manager_arc.lock().await;
            if !ws_manager.is_connected().await {
                error!("[Engine] Websocket manager is NOT connected!");
            } else {
                debug!("[Engine] Websocket manager connection healthy.");
            }
        } else {
            warn!("[Engine] No websocket manager configured.");
        }
    }

    /// Processes incoming websocket updates in a non-blocking loop.
    pub async fn process_websocket_updates_loop(&self) {
        if let Some(ws_manager_arc) = &self.ws_manager {
            let mut ws_manager = ws_manager_arc.lock().await;
            loop {
                match ws_manager.try_recv_update().await {
                    Ok(Some(update)) => {
                        if let Err(e) = self.handle_websocket_update(update).await {
                            error!("[Engine] Error processing WS update: {}", e);
                        }
                    }
                    Ok(None) => {
                        tokio::task::yield_now().await;
                    }
                    Err(e) => {
                        warn!("[Engine] WebSocket receive error: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        } else {
            warn!("[Engine] Skipping websocket update loop: not configured.");
        }
    }

    /// Returns a status string summarizing the engine’s current state.
    pub async fn get_current_status_string(&self) -> String {
        let num_pools = self.pools.read().await.len();
        let min_profit = self.get_min_profit_threshold_pct().await;
        let detector_guard = self.detector.lock().await;
        let detector_min_profit = detector_guard.get_min_profit_threshold_pct();
        format!(
            "ArbitrageEngine Status: Pools: {}, MinProfitThreshold (Engine): {:.4}%, DetectorMinProfit: {:.4}%",
            num_pools, min_profit, detector_min_profit
        )
    }

    /// Runs the dynamic threshold update loop.
    /// This method is intended to be spawned as a separate task.
    pub async fn run_dynamic_threshold_updates(&self) {
        if let Some(dynamic_updater_arc) = &self.dynamic_threshold_updater {
            let updater = Arc::clone(dynamic_updater_arc);
            let metrics = Arc::clone(&self.metrics);
            let detector = Arc::clone(&self.detector);
            let price_provider = self.price_provider.clone(); // Clone the Option<Arc<...>>

            DynamicThresholdUpdater::start_monitoring_task(
                updater,
                metrics,
                detector,
                price_provider,
            ).await;
        } else {
            warn!("[Engine] Dynamic threshold updater not configured. Threshold updates will not run.");
        }
    }
    /// Spawns and runs the key background services concurrently:
    ///  - WebSocket update processing,
    ///  - Dynamic threshold updates,
    ///  - Periodic health checks,
    ///  - And arbitrage detection scans.
    pub async fn run_main_loop(&self) {
        let ws_updates_task = {
            let engine_clone = self.clone();
            tokio::spawn(async move {
                engine_clone.process_websocket_updates_loop().await;
            })
        };

        let health_checks_task = {
            let engine_clone = self.clone();
            tokio::spawn(async move {
                loop {
                    engine_clone.run_health_checks().await;
                    sleep(engine_clone.health_check_interval).await;
                }
            })
        };

        let dynamic_threshold_task = {
            if let Some(dynamic_updater) = &self.dynamic_threshold_updater {
                let updater = Arc::clone(dynamic_updater);
                let metrics = self.metrics.clone();
                let detector = self.detector.clone();
                let price_provider = self.price_provider.clone();
                tokio::spawn(async move {
                    crate::arbitrage::dynamic_threshold::DynamicThresholdUpdater::start_monitoring_task(
                        updater,
                        metrics,
                        detector,
                        price_provider,
                    ).await;
                })
            } else {
                // If not configured, spawn a dummy future that never ends.
                tokio::spawn(async move { loop { sleep(Duration::from_secs(3600)).await } })
            }
        };

        let detection_task = {
            let engine_clone = self.clone();
            tokio::spawn(async move {
                loop {
                    match engine_clone.detect_arbitrage().await {
                        Ok(opportunities) => {
                            if opportunities.is_empty() {
                                debug!("[Engine] No arbitrage opportunities found this cycle.");
                            } else {
                                for opp in opportunities.iter() {
                                    info!("[Engine] Detected Opportunity: {}", opp.id);
                                    opp.log_summary();
                                    // Execution instruction is pushed via mod.rs (coordinator task) or directly to executor.
                                }
                            }
                        }
                        Err(e) => {
                            error!("[Engine] Error during arbitrage detection: {}", e);
                        }
                    }
                    sleep(Duration::from_secs(engine_clone.config.cycle_interval_seconds.unwrap_or(5))).await;
                }
            })
        };

        // Wait for all background tasks (which never end in a long-running service)
        tokio::select! {
            _ = ws_updates_task => {},
            _ = health_checks_task => {},
            _ = dynamic_threshold_task => {},
            _ = detection_task => {},
        }
    }

    /// Gracefully shuts down the arbitrage engine and its components.
    pub async fn shutdown(&self) -> Result<(), ArbError> {
        info!("[Engine] Initiating shutdown sequence...");
        // Example: If ws_manager was started by the engine, it should be stopped here.
        // However, in the current main.rs, ws_manager.stop() is called directly.
        // This shutdown method can be expanded for other engine-specific cleanup.
        info!("[Engine] Shutdown sequence complete.");
        Ok(())
    }
}

// Implement Clone manually for ArbitrageEngine so we can pass it to spawned tasks.
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
            _dex_providers: self._dex_providers.clone(),
            dynamic_threshold_updater: self.dynamic_threshold_updater.as_ref().map(Arc::clone),
            circuit_breaker: Arc::clone(&self.circuit_breaker),
            retry_policy: self.retry_policy.clone(),
            min_profit_threshold_pct: Arc::clone(&self.min_profit_threshold_pct),
        }
    }
}
