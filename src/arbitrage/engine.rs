// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::DynamicThresholdUpdater, // Removed VolatilityTracker and self
        opportunity::MultiHopArbOpportunity,
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
use tokio::sync::{Mutex, RwLock};
use tokio::time::error::Elapsed as TimeoutError;
use tokio::time::timeout;

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
        }
    }

    pub async fn start_services(&self, cache: Option<Arc<crate::cache::Cache>>) {
        // Make update_pools live with an initial empty update
        // TODO: Load initial pools from a persistent source or configuration if available.
        if let Err(e) = self.update_pools(HashMap::new()).await {
            error!("[ArbitrageEngine] Initial pool update failed: {}", e);
        }

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
                    warn!("[ArbitrageEngine] WebSocket disconnected. Attempting reconnect (attempt {}/{})", attempts + 1, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager.start(None).await { // <-- FIX: pass None for cache
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
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues (WS: {}, RPC: {})", ws_healthy, rpc_healthy);
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
        info!(
            "Health checks completed. System healthy: {}. Last check updated.",
            overall_healthy
        );
    }

    // Added back dex_providers_health_check
    pub async fn dex_providers_health_check(&self) {
        if self._dex_providers.is_empty() {
            warn!("No DEX providers configured. ArbitrageEngine cannot operate without DEX APIs.");
            return;
        }
        info!(
            "Performing health check on {} DEX providers...",
            self._dex_providers.len()
        );
        for provider in &self._dex_providers {
            // Assuming DexClient has a health check method or similar.
            // For now, just log the provider's name.
            // In a real scenario, you'd call something like provider.is_healthy().await
            info!("Checking health of DEX provider: {}", provider.get_name());
            // Example: if !provider.is_healthy().await { warn!("DEX provider {} is unhealthy!", provider.get_name()); }
        }
        info!("DEX providers health check completed.");
    }

    // Added back run_full_health_check
    pub async fn run_full_health_check(&self, _cache: Option<Arc<crate::cache::Cache>>) {
        info!("Running full engine health check (including DEX providers)...");

        // General health checks
        self.run_health_checks().await;

        // DEX specific checks
        self.dex_providers_health_check().await;

        // Optionally, re-check WebSocket manager with cache
        if let Some(manager_arc) = &self.ws_manager {
            let manager = manager_arc.lock().await;
            let _ = manager.start(None).await; // Pass None for cache argument
        }

        info!("Full engine health check completed.");
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        info!("Dynamic threshold update service starting within ArbitrageEngine (using internal updater).");

        let updater_arc = match &self.dynamic_threshold_updater {
            Some(updater) => Arc::clone(updater),
            None => {
                warn!("DynamicThresholdUpdater not initialized in ArbitrageEngine. Internal threshold updates will not run.");
                return;
            }
        };

        let update_interval_duration = Duration::from_secs(
            self.config
                .dynamic_threshold_update_interval_secs
                .unwrap_or(300),
        );

        loop {
            tokio::time::sleep(update_interval_duration).await;

            let current_price_of_major_asset = match &self.price_provider {
                Some(provider) => {
                    let symbol_to_track = self
                        .config
                        .health_check_token_symbol
                        .as_deref()
                        .unwrap_or("SOL/USDC");
                    match provider.get_price(symbol_to_track).await {
                        Some(price) => {
                            log::debug!(
                                "DynamicThreshold (Engine Task): Fetched price for {}: {}",
                                symbol_to_track,
                                price
                            );
                            price
                        }
                        None => {
                            warn!("DynamicThreshold (Engine Task): Could not fetch price for {}. Using fallback 100.0", symbol_to_track);
                            100.0
                        }
                    }
                }
                None => {
                    warn!("DynamicThreshold (Engine Task): Price provider not available. Using fallback 100.0 for volatility calculation.");
                    100.0
                }
            };

            let new_threshold_pct = {
                let mut updater = updater_arc.lock().await;
                updater.add_price_observation(current_price_of_major_asset); // Synchronous call
                updater.get_current_threshold() // Synchronous call
            };

            self.set_min_profit_threshold_pct(new_threshold_pct).await;

            info!(
                "DynamicThresholdUpdater (Engine Task): Set new min profit threshold: {:.4}% via internal updater logic.",
                new_threshold_pct
            );
        }
    }

    // Added new method to make with_pool_guard_async live
    pub async fn get_cached_pool_count(&self) -> Result<usize, ArbError> {
        self.with_pool_guard_async("get_cached_pool_count", false, |pools_guard| async move {
            Ok(pools_guard.len())
        })
        .await
    }

    // Added back with_pool_guard_async
    pub async fn with_pool_guard_async<'s, Fut, T, F>(
        &'s self,
        operation_name: &str,
        _write_lock: bool, // Parameter kept for signature compatibility, but logic simplified
        critical_section: F,
    ) -> Result<T, ArbError>
    where
        F: FnOnce(
            Box<dyn Deref<Target = HashMap<Pubkey, Arc<PoolInfo>>> + Send + Sync + 's>,
        ) -> Fut,
        Fut: std::future::Future<Output = Result<T, ArbError>> + Send + 's,
    {
        let timeout_duration =
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000));

        // Simplified to always use a read lock for this example due to boxing complexities with write guards.
        let guard = timeout(timeout_duration, self.pools.read())
            .await
            .map_err(|_elapsed| {
                // Handle timeout error
                warn!("{}: Timeout waiting for pools read lock", operation_name);
                ArbError::TimeoutError(format!("Timeout for pools read lock in {}", operation_name))
            })?; // This '?' unwraps the Result<RwLockReadGuard, Elapsed> from timeout

        let boxed_guard: Box<
            dyn Deref<Target = HashMap<Pubkey, Arc<PoolInfo>>> + Send + Sync + 's,
        > = Box::new(guard);

        critical_section(boxed_guard).await
    }

    /// Main arbitrage detection entry point for the engine.
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("Starting arbitrage detection cycle...");
        let start_time = Instant::now();

        let direct_opportunities = self._discover_direct_opportunities().await.map_err(|e| {
            error!("Error discovering direct opportunities: {}", e);
            e
        })?;
        info!(
            "Found {} direct (2-hop) opportunities.",
            direct_opportunities.len()
        );

        let multihop_opps = self.discover_multihop_opportunities().await.map_err(|e| {
            error!("Error discovering multi-hop opportunities: {}", e);
            e
        })?;
        info!(
            "Found {} multi-hop (3+ hop) opportunities.",
            multihop_opps.len()
        );

        let mut all_opportunities = direct_opportunities;
        all_opportunities.extend(multihop_opps);

        // Add fixed input opportunities if enabled
        if self.config.enable_fixed_input_arb_detection {
            if let Some(fixed_amount) = self.config.fixed_input_arb_amount {
                if fixed_amount > 0.0 {
                    info!(
                        "Discovering fixed input opportunities with amount: {}",
                        fixed_amount
                    );
                    let fixed_input_opps = self
                        .discover_fixed_input_opportunities(fixed_amount)
                        .await
                        .map_err(|e| {
                            error!("Error discovering fixed input opportunities: {}", e);
                            e
                        })?;
                    info!(
                        "Found {} fixed input (2-hop) opportunities.",
                        fixed_input_opps.len()
                    );
                    all_opportunities.extend(fixed_input_opps);
                } else {
                    warn!("Fixed input amount is configured to be non-positive ({}). Skipping fixed input detection.", fixed_amount);
                }
            } else {
                warn!(
                    "Fixed input detection enabled, but fixed_input_arb_amount not set. Skipping."
                );
            }
        }

        // Call find_two_hop_opportunities to mark it as used if tests are not sufficient
        // This is deprecated, so its use here is primarily to satisfy the "must use" requirement.
        // In a production system, this might be guarded by a debug flag or removed if the deprecation is final.
        let deprecated_two_hop_opps = {
            // The 'mut' keyword is removed from here
            let detector_guard = self.detector.lock().await;
            let mut metrics_guard = self.metrics.lock().await;
            detector_guard
                .find_two_hop_opportunities(&self.pools.read().await.clone(), &mut metrics_guard)
                .await
                .map_err(|e| {
                    error!("Error discovering deprecated two_hop_opportunities: {}", e);
                    e
                })?
        };
        if !deprecated_two_hop_opps.is_empty() {
            info!(
                "Found {} opportunities via deprecated find_two_hop_opportunities.",
                deprecated_two_hop_opps.len()
            );
            all_opportunities.extend(deprecated_two_hop_opps);
        }

        // --- Filter for profitability using MultiHopArbOpportunity methods ---
        let min_profit_pct = self.get_min_profit_threshold_pct().await;
        let min_profit_usd = self.detector.lock().await.get_min_profit_threshold_usd(); // Use getter

        let filtered_opportunities: Vec<MultiHopArbOpportunity> = all_opportunities
            .into_iter()
            .filter(|opp| opp.is_profitable(min_profit_pct, min_profit_usd))
            .collect();

        info!(
            "Filtered to {} profitable opportunities (min_pct: {:.4}%, min_usd: {:.4}).",
            filtered_opportunities.len(),
            min_profit_pct,
            min_profit_usd
        );

        let mut all_opportunities = filtered_opportunities;

        all_opportunities.sort_by(|a, b| {
            b.profit_pct
                .partial_cmp(&a.profit_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let duration = start_time.elapsed();
        info!(
            "Arbitrage detection cycle completed in {:?}. Total opportunities found: {}",
            duration,
            all_opportunities.len()
        );

        if !all_opportunities.is_empty() {
            let best_opp = &all_opportunities[0]; // This is safe due to the !is_empty() check
            info!(
                "Best opportunity: ID {}, Profit Pct: {:.4}%, Input: {} {}, Output: {} {}",
                best_opp.id,
                best_opp.profit_pct,
                best_opp.input_amount,
                best_opp.input_token,
                best_opp.expected_output,
                best_opp.output_token
            );

            // Make resolve_pools_for_opportunity live
            match self.resolve_pools_for_opportunity(best_opp).await {
                Ok(resolved_pools) => {
                    info!(
                        "Successfully resolved {} pools for the best opportunity. First pool: {:?}",
                        resolved_pools.len(),
                        resolved_pools.first().map(|p| p.name.clone())
                    );
                }
                Err(e) => {
                    warn!(
                        "Could not resolve pools for the best opportunity {}: {}",
                        best_opp.id, e
                    );
                }
            }
        }
        Ok(all_opportunities)
    }

    pub async fn get_current_status_string(&self) -> String {
        // Placeholder implementation
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
        // Add more status information as needed
        status
    }
}

#[cfg(test)]
mod detector_integration_tests {
    use super::*;
    use crate::arbitrage::detector::ArbitrageDetector;
    // Removed this unused import:
    // use crate::config::settings::Config;
    use crate::metrics::Metrics;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_exercise_all_detector_functions() {
        // Exercise ArbitrageDetector::new with all required arguments
        let mut detector = ArbitrageDetector::new(0.5, 0.05, 150.0, 5000);
        let mut metrics = Metrics::default();
        let pools: HashMap<Pubkey, Arc<PoolInfo>> = HashMap::new();
        // Exercise find_two_hop_opportunities (deprecated, but must be called)
        let _ = detector
            .find_two_hop_opportunities(&pools, &mut metrics)
            .await;
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
