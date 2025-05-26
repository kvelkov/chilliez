// src/arbitrage/engine.rs
use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::VolatilityTracker, // Added import
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
use log::{error, info, warn, debug}; // Added debug
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
use tokio::time::{timeout};
use tokio::time::error::Elapsed as TimeoutError; 


pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
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
    pub(crate) dex_providers: Vec<Arc<dyn DexClient>>,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        dex_api_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        let initial_min_profit_pct = config.min_profit_pct * 100.0; 
        // Ensure these config values exist or provide sensible defaults if they are Option types.
        // Assuming they are present in Config as per settings.rs structure.
        let min_profit_usd = config.degradation_profit_factor.unwrap_or(0.1); 
        let sol_price = config.sol_price_usd.unwrap_or(100.0); 
        let priority_fee = config.default_priority_fee_lamports;

        let internal_detector = Arc::new(Mutex::new(ArbitrageDetector::new(
            initial_min_profit_pct,
            min_profit_usd, 
            sol_price,      
            priority_fee    
        )));

        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);

        Self {
            pools,
            ws_manager: ws_manager_instance,
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
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector: internal_detector, 
            dex_providers: dex_api_clients, // <-- FIXED
        }
    }

    pub async fn start_services(&self) {
        info!("ArbitrageEngine services starting...");
        if let Some(ws_manager_arc) = &self.ws_manager {
            let ws_manager_guard = ws_manager_arc.lock().await; 
            if let Err(e) = ws_manager_guard.start().await {
                error!("Failed to start WebSocket Manager: {}", e);
            } else {
                info!("WebSocket Manager started successfully.");
                let pools_guard = self.pools.read().await;
                if !pools_guard.is_empty() {
                    info!("Attempting to subscribe to {} existing pools via WebSocket.", pools_guard.len());
                    for pool_address in pools_guard.keys() {
                        if let Err(e) = ws_manager_guard.subscribe_to_account(*pool_address).await {
                            error!("Failed to subscribe to pool {}: {}", pool_address, e);
                        }
                    }
                } else {
                    info!("No initial pools to subscribe to via WebSocket.");
                }
            }
        } else {
            warn!("WebSocket Manager not available, not starting WS services.");
        }
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
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError>
    where
        F: FnOnce( 
            Arc<Mutex<ArbitrageDetector>>, 
            HashMap<Pubkey, Arc<PoolInfo>>, 
            Arc<Mutex<Metrics>>, 
        ) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<MultiHopArbOpportunity>, ArbError>>,
    {
        self.maybe_check_health().await?;

        let pools_map_clone = { 
            let guard = timeout(
                Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
                self.pools.read(),
            )
            .await
            .map_err(|_elapsed: TimeoutError| { 
                warn!("{}: Timeout waiting for pools read lock", operation_name);
                ArbError::TimeoutError(format!("Timeout for pools read lock in {}", operation_name))
            })?; // Single '?' here after map_err transforms TimeoutError to ArbError
            guard.deref().clone() 
        };
        
        detector_call(
            Arc::clone(&self.detector),
            pools_map_clone, 
            Arc::clone(&self.metrics)
        ).await
    }

    pub async fn _discover_direct_opportunities( 
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal(
            "discover_direct_opportunities",
            |detector_arc, pools_map, metrics_arc| async move { 
                let detector_guard = detector_arc.lock().await; // No 'mut' needed if method takes &self
                let mut metrics_guard = metrics_arc.lock().await; // 'mut' if method takes &mut self
                detector_guard
                    .find_all_opportunities(&pools_map, &mut *metrics_guard) 
                    .await
            },
        )
        .await
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal(
            "discover_multihop_opportunities",
            |detector_arc, pools_map, metrics_arc| async move {
                let detector_guard = detector_arc.lock().await; 
                let mut metrics_guard = metrics_arc.lock().await;
                detector_guard
                    .find_all_multihop_opportunities(&pools_map, &mut *metrics_guard)
                    .await
            },
        )
        .await
    }

    pub async fn _discover_multihop_opportunities_with_risk( 
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
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
                        &mut *metrics_guard,
                        max_slippage_pct,
                        tx_fee_lamports_for_acceptance,
                    )
                    .await
            },
        )
        .await
    }
    
    pub async fn _resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        self.with_pool_guard_async(
            "_resolve_pools_for_opportunity",
            false, 
            |pools_arc_local| async move { 
                let pools_guard = pools_arc_local.read().await;
                let mut resolved_pools = Vec::new();
                for hop_pool_address in &opportunity.pool_path {
                    match pools_guard.get(hop_pool_address) {
                        Some(pool_info_arc) => resolved_pools.push(Arc::clone(pool_info_arc)),
                        None => {
                            let hop_details = opportunity
                                .hops
                                .iter()
                                .find(|h| &h.pool == hop_pool_address);
                            let (input_token_symbol, output_token_symbol) = hop_details 
                                .map_or(("N/A".to_string(), "N/A".to_string()), |h| {
                                    (h.input_token.clone(), h.output_token.clone())
                                });
                            warn!(
                                "Pool {} for hop {}->{} not found in local cache.",
                                hop_pool_address, input_token_symbol, output_token_symbol
                            );
                            return Err(ArbError::PoolNotFound(hop_pool_address.to_string()));
                        }
                    }
                }
                Ok(resolved_pools)
            },
        )
        .await
    }

    async fn with_pool_guard_async<Fut, T>(
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

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() > self.health_check_interval {
            drop(last_check_guard); 
            info!("Performing scheduled health checks due to interval.");
            self.run_health_checks().await; 
        }
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        info!("Health check task running periodical checks...");
        let mut overall_healthy = true;
        if let Some(rpc) = &self.rpc_client {
            if !rpc.is_healthy().await {
                warn!("RPC client reported as unhealthy.");
                overall_healthy = false;
            } else {
                info!("RPC client reported as healthy.");
            }
        } else {
            warn!("RPC client not configured; skipping RPC health check.");
        }

        if let Some(ws_manager_arc) = &self.ws_manager {
            let manager = ws_manager_arc.lock().await; 
            if !manager.is_connected().await {
                warn!("WebSocket manager reported as disconnected.");
                overall_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1; 
                if attempts <= self.max_ws_reconnect_attempts { 
                    warn!(
                        "Attempting to reconnect WebSocket (attempt {}/{})",
                        attempts, self.max_ws_reconnect_attempts
                    );
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
        
        self.metrics.lock().await.set_system_health(overall_healthy);
        *self.last_health_check.write().await = Instant::now(); 
        info!("Health checks completed. System healthy: {}. Last check updated.", overall_healthy);
    }

    pub async fn update_pools(
        &self,
        new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        if new_pools_data.is_empty() {
            debug!("update_pools called with no new pool data.");
            return Ok(());
        }

        let mut pools_guard = self.pools.write().await;
        let mut new_count = 0;
        let mut updated_count = 0;

        let ws_manager_opt = self.ws_manager.as_ref().map(Arc::clone);

        for (key, pool_info_arc) in new_pools_data {
            if pools_guard.contains_key(&key) {
                updated_count += 1;
            } else {
                new_count += 1;
                if let Some(ws_arc) = &ws_manager_opt {
                    let ws_guard = ws_arc.lock().await; // Lock here
                    if ws_guard.is_connected().await { // Check connection before subscribing
                        if let Err(e) = ws_guard.subscribe_to_account(key).await {
                            error!("Failed to subscribe to new pool {} via WebSocket: {}", key, e);
                        } else {
                            info!("Subscribed to new pool {} via WebSocket.", key);
                        }
                    } else {
                        warn!("WebSocket manager not connected, cannot subscribe to new pool {}.", key);
                    }
                }
            }
            pools_guard.insert(key, pool_info_arc);
        }
        let total_count = pools_guard.len();
        drop(pools_guard); 

        info!(
            "Pools updated: {} new, {} updated. Total pools in engine: {}",
            new_count, updated_count, total_count
        );
        self.metrics
            .lock()
            .await
            .log_pools_updated(new_count, updated_count, total_count);
        Ok(())
    }

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

    pub async fn run_dynamic_threshold_updates(&self) {
        info!("Dynamic threshold update service starting within ArbitrageEngine.");
        let mut vol_tracker = VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));
        let update_interval_duration = Duration::from_secs(self.config.dynamic_threshold_update_interval_secs.unwrap_or(300));
        let base_threshold_fractional = self.config.min_profit_pct; 
        let volatility_factor_val = self.config.volatility_threshold_factor.unwrap_or(0.1); 

        loop {
            tokio::time::sleep(update_interval_duration).await;
            
            let current_price_of_major_asset = match &self.price_provider {
                Some(provider) => {
                    let symbol_to_track = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL/USDC"); 
                    match provider.get_price(symbol_to_track).await {
                        Some(price) => {
                            debug!("DynamicThreshold: Fetched price for {}: {}", symbol_to_track, price); // Changed to debug
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
            let historical_volatility = vol_tracker.volatility();

            let new_threshold_fractional = crate::arbitrage::dynamic_threshold::recommend_min_profit_threshold(
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

    pub async fn get_current_status_string(&self) -> String { 
        let min_profit = self.get_min_profit_threshold_pct().await; 
        let max_slip_pct = self.config.max_slippage_pct * 100.0; 
        let degradation = self.degradation_mode.load(Ordering::Relaxed); 

        format!(
            "ArbitrageEngine Status: Min Profit Pct = {:.4}%, Max Slippage Pct = {:.4}%, Degradation Mode = {}",
            min_profit,
            max_slip_pct,
            degradation
        )
    }
}