// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::{self, recommend_min_profit_threshold, VolatilityTracker},
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::quote::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::SolanaWebsocketManager,
    },
    // Removed unused DexType, TokenAmount from utils direct import here
    utils::{PoolInfo}, // Keep PoolInfo
    websocket::CryptoDataProvider,
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    ops::Deref, // To use &*pools_guard for HashMap
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
// Removed unused import: use tokio::time::error::Elapsed;


pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: Arc<RwLock<f64>>,
    max_slippage: f64,
    tx_fee_lamports: u64,
    ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>,
    rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>,
    degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    health_check_interval: Duration,
    ws_reconnect_attempts: Arc<AtomicU64>,
    max_ws_reconnect_attempts: u64,
    detector: Arc<Mutex<ArbitrageDetector>>,
    dex_providers: Vec<Arc<dyn DexClient>>,
    // Removed redundant websocket_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
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
        let max_slippage_fraction = config.max_slippage_pct;
        let tx_fee_lamports_val = config.default_priority_fee_lamports;
        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);

        let detector = Arc::new(Mutex::new(ArbitrageDetector::new(initial_min_profit_pct)));

        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(initial_min_profit_pct)),
            max_slippage: max_slippage_fraction,
            tx_fee_lamports: tx_fee_lamports_val,
            ws_manager: ws_manager_instance, // Use the passed instance
            price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now() - Duration::from_secs(health_check_interval_secs * 2))),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,
            dex_providers: dex_api_clients,
            // websocket_manager field removed
        }
    }

    pub async fn start_services(&self) {
        if let Some(manager_arc) = &self.ws_manager {
            let mut manager_guard = manager_arc.lock().await;
            if let Err(e) = manager_guard.start().await {
                error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }
    }

    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn set_min_profit_threshold(&self, threshold_pct: f64) {
        let mut current_threshold_guard = self.min_profit_threshold.write().await;
        *current_threshold_guard = threshold_pct;
        self.metrics.lock().await.log_dynamic_threshold_update(threshold_pct / 100.0);
    }

    pub fn should_execute_trade(&self, calculated_slippage_fraction: f64, estimated_fee_lamports: u64) -> bool {
        calculated_slippage_fraction <= self.max_slippage
            && estimated_fee_lamports <= self.tx_fee_lamports * 2
    }

    async fn get_pools_map_ref<F, R>(&self, operation_name: &str, critical: bool, f: F) -> Result<R, ArbError>
    where
        F: FnOnce(&HashMap<Pubkey, Arc<PoolInfo>>) -> Result<R, ArbError>,
    {
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        ).await {
            Ok(Ok(guard)) => f(guard.deref()), // Pass &HashMap directly
            Ok(Err(poison_error)) => {
                error!("{}: Failed to acquire read lock on pools (poisoned): {}", operation_name, poison_error);
                Err(ArbError::Unknown(format!("Pools lock poisoned during {}: {}", operation_name, poison_error)))
            }
            Err(_timeout_error) => { // tokio::time::error::Elapsed
                if critical {
                    error!("{}: Timeout waiting for pools read lock. This is critical.", operation_name);
                    Err(ArbError::TimeoutError(format!("Critical timeout for pools read lock in {}", operation_name)))
                } else {
                    warn!("{}: Timeout waiting for pools read lock.", operation_name);
                    Err(ArbError::TimeoutError(format!("Timeout for pools read lock in {}", operation_name)))
                }
            }
        }
    }


    pub async fn discover_direct_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        self.get_pools_map_ref("discover_direct_opportunities", true, |pools_map| {
            let detector_guard = self.detector.blocking_lock(); // Use blocking_lock if in async context for a sync Mutex
            let mut metrics_guard = self.metrics.blocking_lock();
            // find_all_opportunities is async, so this structure needs adjustment if detector methods are truly async
            // For now, assuming detector methods can be called like this if they are adapted for Option<&mut Metrics>
            // Or pass the guards directly if detector methods are async and take them.
            // This shows a potential issue if detector's methods are async and need to lock metrics themselves.
            // The provided detector.rs methods are async and take &Metrics or &mut Metrics
            // So we need to call them in an async block if this outer function remains sync, or make this async.
            // The function is already async.

            // Re-evaluate locking strategy for detector and metrics if detector methods are async.
            // Simplest if detector methods are sync, or they handle their own async locking.
            // Assuming detector methods are async as per previous code:
            let rt = tokio::runtime::Handle::current(); // Get current runtime handle
            let detector = Arc::clone(&self.detector);
            let metrics_arc = Arc::clone(&self.metrics);
            let pools_map_clone = pools_map.clone(); // Clone data for the async block

            rt.block_on(async move { // block_on within an async fn is an anti-pattern. This must be refactored.
                                     // The get_pools_map_ref should pass the guard to an async closure.
                let mut metrics_guard = metrics_arc.lock().await;
                detector.lock().await.find_all_opportunities(&pools_map_clone, &mut metrics_guard).await
            })

        }).await // get_pools_map_ref is now async
    }


    // Adjusted structure for async operations with pool guard
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
            async { Ok(Arc::clone(&self.pools)) } // Just pass the Arc for the closure to handle locking
        ).await {
            Ok(Ok(pools_arc)) => closure(pools_arc).await,
            Ok(Err(e)) => { // This branch should ideally not be hit with current async block
                error!("{}: Error in pool guard handling (unexpected): {}", operation_name, e);
                Err(ArbError::Unknown(format!("Unexpected pool guard error in {}: {}", operation_name, e)))
            }
            Err(_timeout_error) => {
                 if critical {
                    error!("{}: Timeout in pool guard handling. This is critical.", operation_name);
                    Err(ArbError::TimeoutError(format!("Critical timeout for pool guard in {}", operation_name)))
                } else {
                    warn!("{}: Timeout in pool guard handling.", operation_name);
                    Err(ArbError::TimeoutError(format!("Timeout for pool guard in {}", operation_name)))
                }
            }
        }
    }
    
    // Refactored discovery methods using with_pool_guard_async
    pub async fn discover_direct_opportunities_refactored(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        self.with_pool_guard_async("discover_direct_opportunities", true, |pools_arc| async {
            let pools_guard = pools_arc.read().await;
            let mut metrics_guard = self.metrics.lock().await;
            self.detector.lock().await.find_all_opportunities(&pools_guard, &mut metrics_guard).await
        }).await
    }

    pub async fn discover_multihop_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        self.with_pool_guard_async("discover_multihop_opportunities", true, |pools_arc| async {
            let pools_guard = pools_arc.read().await;
            let mut metrics_guard = self.metrics.lock().await;
            self.detector.lock().await.find_all_multihop_opportunities(&pools_guard, &mut metrics_guard).await
        }).await
    }

    pub async fn discover_multihop_opportunities_with_risk(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
         self.with_pool_guard_async("discover_multihop_opportunities_with_risk", true, |pools_arc| async {
            let pools_guard = pools_arc.read().await;
            let mut metrics_guard = self.metrics.lock().await;
            self.detector.lock().await.find_all_multihop_opportunities_with_risk(
                &pools_guard, &mut metrics_guard, self.max_slippage, self.tx_fee_lamports
            ).await
        }).await
    }


    pub async fn resolve_pools_for_opportunity(&self, opportunity: &MultiHopArbOpportunity) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        self.with_pool_guard_async("resolve_pools_for_opportunity", false, |pools_arc| async {
            let pools_guard = pools_arc.read().await;
            let mut resolved_pools = Vec::new();
            for hop_pool_address in &opportunity.pool_path {
                match pools_guard.get(hop_pool_address) {
                    Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                    None => {
                        let hop_details = opportunity.hops.iter().find(|h| &h.pool == hop_pool_address);
                        let (input_token, output_token) = hop_details.map_or(("N/A".to_string(), "N/A".to_string()), |h| (h.input_token.clone(), h.output_token.clone()));
                        warn!("Pool {} for hop {}->{} not found in local cache.", hop_pool_address, input_token, output_token);
                        return Err(ArbError::PoolNotFound(hop_pool_address.to_string())); // Use String for PoolNotFound
                    }
                }
            }
            Ok(resolved_pools)
        }).await
    }

    // ... rest of ArbitrageEngine methods (maybe_check_health, run_health_checks, etc.)
    // Ensure they are updated to use self.ws_manager consistently.
    // The run_health_checks method was already mostly correct in the previous version.

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() < self.health_check_interval {
            return Ok(());
        }
        // Drop the write lock before calling run_health_checks to avoid holding it too long
        drop(last_check_guard);
        
        self.run_health_checks().await; // Call the actual health check logic

        // Re-acquire write lock to update timestamp
        *self.last_health_check.write().await = Instant::now();
        Ok(())
    }
    
    pub async fn run_health_checks(&self) {
        info!("[ArbitrageEngine] Performing system health check...");
        let mut overall_healthy = true;

        // WebSocket Health
        let mut ws_healthy = true;
        if let Some(manager_arc) = &self.ws_manager { // Using self.ws_manager
            let mut manager_guard = manager_arc.lock().await;
            if manager_guard.is_connected().await {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            } else {
                ws_healthy = false;
                warn!("[ArbitrageEngine] WebSocket disconnected.");
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] Attempting WebSocket reconnect (attempt {}/{})", attempts, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager_guard.start().await {
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                    error!("[ArbitrageEngine] WebSocket max reconnect attempts ({}) reached.", self.max_ws_reconnect_attempts);
                }
            }
        } else {
            info!("[ArbitrageEngine] WebSocket manager not configured, skipping check.");
        }
        if !ws_healthy { overall_healthy = false; }

        // Price Provider Health
        let mut price_healthy = true;
        if let Some(provider_arc) = &self.price_provider {
            let health_token_symbol = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL");
            match provider_arc.get_price(health_token_symbol).await {
                Some(price) => info!("[ArbitrageEngine] Price provider healthy, price for {}: {}", health_token_symbol, price),
                None => {
                     price_healthy = false;
                     warn!("[ArbitrageEngine] Price provider failed to get price for {}.", health_token_symbol);
                }
            }
        } else {
            info!("[ArbitrageEngine] Price provider not configured, skipping check.");
        }
        if !price_healthy { overall_healthy = false; }

        // RPC Health
        let mut rpc_healthy = true;
        if let Some(client_arc) = &self.rpc_client {
            if client_arc.is_healthy().await {
                 match client_arc.primary_client.get_epoch_info().await {
                    Ok(epoch_info) => info!("[ArbitrageEngine] RPC (HA primary) healthy. Current epoch: {}", epoch_info.epoch),
                    Err(e) => {
                        rpc_healthy = false;
                        error!("[ArbitrageEngine] RPC health check (HA primary): Failed to get epoch info: {}", e);
                    }
                }
            } else {
                rpc_healthy = false;
                warn!("[ArbitrageEngine] Primary RPC endpoint (via HA client) is unhealthy.");
            }
        } else {
            info!("[ArbitrageEngine] HA RPC client not configured, skipping check.");
        }
        if !rpc_healthy { overall_healthy = false; }

        for dex_client in &self.dex_providers {
            debug!("[ArbitrageEngine] Conceptual health check for DEX Client: {}", dex_client.get_name());
        }

        let degradation_profit_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let base_min_profit_pct = self.config.min_profit_pct * 100.0; 

        let should_degrade = !overall_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode. Overall Health: {}. Details - WS: {}, PriceProvider: {}, RPC: {}", overall_healthy, ws_healthy, price_healthy, rpc_healthy);
            let new_threshold_pct = base_min_profit_pct * degradation_profit_factor;
            self.set_min_profit_threshold(new_threshold_pct).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold_pct);
            self.metrics.lock().await.log_degradation_mode_change(true, Some(new_threshold_pct / 100.0));
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            self.set_min_profit_threshold(base_min_profit_pct).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_min_profit_pct);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            self.metrics.lock().await.log_degradation_mode_change(false, Some(base_min_profit_pct / 100.0));
        }
        self.metrics.lock().await.set_system_health(overall_healthy);
        // No need to update last_health_check here, it's done in maybe_check_health
    }


    pub async fn update_pools(&self, new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        let mut writable_pools_ref = self.pools.write().await;
        let mut new_pools_count = 0;
        let mut updated_pools_count = 0;

        for (address, pool_info_arc) in new_pools_data {
            if writable_pools_ref.contains_key(&address) {
                writable_pools_ref.insert(address, pool_info_arc);
                updated_pools_count += 1;
            } else {
                writable_pools_ref.insert(address, pool_info_arc);
                new_pools_count += 1;
            }
        }
        self.metrics.lock().await.log_pools_updated(new_pools_count, updated_pools_count, writable_pools_ref.len());
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_pools_count, updated_pools_count, writable_pools_ref.len()
        );
        Ok(())
    }
    
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // This could be a wrapper that tries different types of detection
        // For now, defaulting to general multi-hop discovery
        self.discover_multihop_opportunities_refactored().await
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        let update_interval_secs = self.config.dynamic_threshold_update_interval_secs.unwrap_or(300);
        if update_interval_secs == 0 {
            info!("Dynamic threshold updates disabled (interval is 0).");
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(update_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut volatility_tracker = VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));
        info!("Dynamic threshold update task started. Interval: {}s", update_interval_secs);

        loop {
            interval.tick().await;
            
            let sol_symbol = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL");
            let current_price_of_major_asset = if let Some(provider) = &self.price_provider {
                provider.get_price(sol_symbol).await.unwrap_or_else(|| {
                    warn!("Could not fetch price for {} for volatility. Using 1.0.", sol_symbol);
                    1.0 
                })
            } else {
                warn!("Price provider not configured for volatility. Using 1.0.");
                1.0
            };

            volatility_tracker.add_price(current_price_of_major_asset);
            let current_volatility = volatility_tracker.volatility();
            
            let base_threshold_fractional = self.config.min_profit_pct;
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.5);

            let new_threshold_fractional = recommend_min_profit_threshold(
                current_volatility, base_threshold_fractional, volatility_factor,
            );
            let new_threshold_pct = new_threshold_fractional * 100.0;

            self.set_min_profit_threshold(new_threshold_pct).await;
            self.detector.lock().await.set_min_profit_threshold(new_threshold_pct);

            info!(
                "Dynamic minimum profit threshold updated to: {:.4}% (Volatility: {:.6}, Base (frac): {:.5}, Factor: {:.2})",
                new_threshold_pct, current_volatility, base_threshold_fractional, volatility_factor
            );
            self.metrics.lock().await.log_dynamic_threshold_update(new_threshold_fractional);
        }
    }

    pub async fn get_current_status(&self) -> String {
        let current_threshold_pct = self.get_min_profit_threshold().await;
        format!(
            "ArbitrageEngine status: OK. Current min profit threshold: {:.4}%",
            current_threshold_pct
        )
    }
}