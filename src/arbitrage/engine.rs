// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::{self, recommend_min_profit_threshold, VolatilityTracker},
        opportunity::MultiHopArbOpportunity, // Using the unified struct
    },
    config::settings::Config,
    dex::quote::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::SolanaWebsocketManager, // Corrected trait name if it's a struct
    },
    utils::{DexType, PoolInfo, TokenAmount},
    websocket::CryptoDataProvider, // Trait for price provider
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: Arc<RwLock<f64>>, // This will be dynamically updated
    max_slippage: f64,
    tx_fee_lamports: u64,
    ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>, // For managing WebSocket connection
    price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>,
    rpc_client: Option<Arc<SolanaRpcClient>>, // HA RPC Client
    config: Arc<Config>,
    degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    health_check_interval: Duration,
    ws_reconnect_attempts: Arc<AtomicU64>,
    max_ws_reconnect_attempts: u64,
    detector: Arc<Mutex<ArbitrageDetector>>,
    dex_providers: Vec<Arc<dyn DexClient>>, // For DEX-specific operations if needed
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>, // Expecting the HA client
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>, // Passed from main
        dex_api_clients: Vec<Arc<dyn DexClient>>,                      // Passed from main
    ) -> Self {
        let initial_min_profit = config.min_profit_pct; // Use min_profit_pct from config
        let max_slippage_val = config.max_slippage_pct; // Use max_slippage_pct from config
        let tx_fee_lamports_val = config.default_priority_fee_lamports;
        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);

        let detector = Arc::new(Mutex::new(ArbitrageDetector::new(initial_min_profit)));

        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(initial_min_profit)),
            max_slippage: max_slippage_val,
            tx_fee_lamports: tx_fee_lamports_val,
            ws_manager: ws_manager_instance,
            price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config), // Clone Arc for local storage
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,
            dex_providers: dex_api_clients,
        }
    }

    pub async fn start_services(&self) {
        if let Some(manager_arc) = &self.ws_manager {
            // Assuming SolanaWebsocketManager::start might not be async or might not exist.
            // This depends on the actual implementation of SolanaWebsocketManager.
            // For now, let's assume a conceptual start.
            // let mut manager_guard = manager_arc.lock().await;
            // if let Err(e) = manager_guard.start().await { // If start is async
            //     error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            // } else {
            //     info!("[ArbitrageEngine] WebSocket manager conceptual start initiated.");
            // }
            info!("[ArbitrageEngine] WebSocket manager setup (conceptual start). Ensure its internal tasks are spawned if needed.");
        }
        // Start other services like price provider if they have a start method
    }

    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn set_min_profit_threshold(&self, threshold: f64) {
        let mut current_threshold_guard = self.min_profit_threshold.write().await;
        *current_threshold_guard = threshold;
        // Ensure log_dynamic_threshold_update is adapted if Metrics methods become async
        self.metrics.lock().await.log_dynamic_threshold_update(threshold);
    }

    pub fn should_execute_trade(
        &self,
        calculated_slippage: f64,
        estimated_fee_lamports: u64,
    ) -> bool {
        calculated_slippage <= self.max_slippage
            && estimated_fee_lamports <= self.tx_fee_lamports * 2 // Example: allow up to 2x configured fee
    }

    // discover_direct_opportunities now returns Vec<MultiHopArbOpportunity>
    pub async fn discover_direct_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Err(ArbError::Unknown("Pools lock poisoned".to_string()));
            }
            Err(_) => {
                warn!("Timeout waiting for pools read lock in discover_direct_opportunities");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock".to_string(),
                ));
            }
        };

        let detector_guard = self.detector.lock().await;
        // Pass the current min_profit_threshold to the detector's method if it needs it,
        // or ensure the detector's internal threshold is up-to-date.
        let opportunities = detector_guard
            .find_all_opportunities(&pools_guard, &*self.metrics.lock().await) // find_all_opportunities now returns MultiHop
            .await?;
        Ok(opportunities)
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Err(ArbError::Unknown("Pools lock poisoned".to_string()));
            }
            Err(_) => {
                warn!("Timeout waiting for pools read lock in discover_multihop_opportunities");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock".to_string(),
                ));
            }
        };

        let detector_guard = self.detector.lock().await;
        let opportunities = detector_guard
            .find_all_multihop_opportunities(&pools_guard, &*self.metrics.lock().await)
            .await?; // Assuming this now returns the correct type
        Ok(opportunities)
    }

    pub async fn discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Err(ArbError::Unknown("Pools lock poisoned".to_string()));
            }
            Err(_) => {
                warn!("Timeout waiting for pools read lock in discover_multihop_opportunities_with_risk");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock".to_string(),
                ));
            }
        };

        let detector_guard = self.detector.lock().await;
        let opportunities = detector_guard
            .find_all_multihop_opportunities_with_risk(
                &pools_guard,
                &*self.metrics.lock().await,
                self.max_slippage,
                self.tx_fee_lamports,
            )
            .await?;
        Ok(opportunities)
    }

    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Err(ArbError::Unknown("Pools lock poisoned".to_string()));
            }
            Err(_) => {
                warn!("Timeout waiting for pools read lock in resolve_pools_for_opportunity");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock".to_string(),
                ));
            }
        };
        let mut resolved_pools = Vec::new();
        for hop_pool_address in &opportunity.pool_path { // Use pool_path from MultiHopArbOpportunity
            match pools_guard.get(hop_pool_address) {
                Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                None => {
                    // Find corresponding hop to get token symbols for logging
                    let hop_details = opportunity.hops.iter().find(|h| &h.pool == hop_pool_address);
                    let (input_token, output_token) = hop_details
                        .map_or( ("N/A".to_string(), "N/A".to_string()), 
                                |h| (h.input_token.clone(), h.output_token.clone()));

                    warn!(
                        "Pool {} for hop {}->{} not found in local cache.",
                        hop_pool_address, input_token, output_token
                    );
                    return Err(ArbError::Unknown(format!("Pool {} not found", hop_pool_address)));
                }
            }
        }
        Ok(resolved_pools)
    }


    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() < self.health_check_interval {
            return Ok(());
        }

        info!("[ArbitrageEngine] Performing system health check...");
        let mut overall_healthy = true;

        let mut ws_healthy = true;
        if let Some(manager_arc) = &self.ws_manager {
            let mut manager_guard = manager_arc.lock().await;
            if manager_guard.is_connected().await {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            } else {
                ws_healthy = false;
                warn!("[ArbitrageEngine] WebSocket disconnected.");
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] Attempting WebSocket reconnect (attempt {}/{})", attempts, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager_guard.start().await { // Assuming start handles reconnect
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                    error!("[ArbitrageEngine] WebSocket max reconnect attempts ({}) reached.", self.max_ws_reconnect_attempts);
                }
            }
        } else {
            info!("[ArbitrageEngine] WebSocket manager not configured, skipping check.");
            // Consider ws_healthy = false if ws is critical
        }
        if !ws_healthy { overall_healthy = false; }

        let mut price_healthy = true;
        if let Some(provider_arc) = &self.price_provider {
            let sol_symbol = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL");
            // Assuming CryptoDataProvider has get_price method
            match provider_arc.get_price(sol_symbol).await {
                Some(_) => info!("[ArbitrageEngine] Price provider successfully fetched price for {}.", sol_symbol),
                None => {
                    price_healthy = false;
                    warn!("[ArbitrageEngine] Price provider failed to get price for {}.", sol_symbol);
                }
            }
        } else {
            info!("[ArbitrageEngine] Price provider not configured, skipping check.");
        }
        if !price_healthy { overall_healthy = false; }


        let mut rpc_healthy = true;
        if let Some(client_arc) = &self.rpc_client { // Using the HA RPC Client
            if !client_arc.is_healthy().await { // Assumes HA client has is_healthy
                rpc_healthy = false;
                warn!("[ArbitrageEngine] Primary RPC endpoint (via HA client) is unhealthy.");
            }
        } else {
            info!("[ArbitrageEngine] RPC client not configured, skipping check.");
        }
        if !rpc_healthy { overall_healthy = false; }

        for dex_client_arc in &self.dex_providers {
            // Example check: try to get a quote for a common pair.
            // This is a placeholder; a real DexClient might have a dedicated check_health.
            // let health_input = "So11111111111111111111111111111111111111112"; // SOL
            // let health_output = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // USDC
            // if dex_client_arc.get_best_swap_quote(health_input, health_output, 1000000).await.is_err() {
            //     warn!("[ArbitrageEngine] DEX Client {} failed health check (quote fetch).", dex_client_arc.get_name());
            //     overall_healthy = false;
            // }
            debug!("[ArbitrageEngine] Conceptual health check for DEX Client: {}", dex_client_arc.get_name());
        }

        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let base_min_profit_from_config = self.config.min_profit_pct;

        let should_degrade = !overall_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode. WS: {}, Price: {}, RPC: {}", ws_healthy, price_healthy, rpc_healthy);
            let new_threshold = base_min_profit_from_config * degradation_factor;
            self.set_min_profit_threshold(new_threshold).await; // Uses the engine's method
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold * 100.0);
            self.metrics.lock().await.log_degradation_mode_change(true, Some(new_threshold));
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            self.set_min_profit_threshold(base_min_profit_from_config).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_min_profit_from_config * 100.0);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            self.metrics.lock().await.log_degradation_mode_change(false, Some(base_min_profit_from_config));
        }

        *last_check_guard = Instant::now();
        self.metrics.lock().await.set_system_health(overall_healthy);
        Ok(())
    }


    pub async fn update_pools(&self, new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        let mut writable_pools_ref = self.pools.write().await;
        let mut new_pools_count = 0;
        let mut updated_pools_count = 0;

        for (address, pool_info) in new_pools_data {
            if writable_pools_ref.contains_key(&address) {
                writable_pools_ref.insert(address, Arc::clone(&pool_info));
                updated_pools_count += 1;
            } else {
                writable_pools_ref.insert(address, Arc::clone(&pool_info));
                new_pools_count += 1;
            }
        }
        // Ensure metrics methods are adapted if they become async
        self.metrics.lock().await.log_pools_updated(new_pools_count, updated_pools_count, writable_pools_ref.len());
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_pools_count,
            updated_pools_count,
            writable_pools_ref.len()
        );
        Ok(())
    }
    
    // detect_arbitrage now returns Vec<MultiHopArbOpportunity>
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let detector_guard = self.detector.lock().await;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error) for all-hop: {}", e);
                return Err(ArbError::Unknown(format!("Pools lock poisoned for all-hop: {}", e)));
            }
            Err(_) => {
                warn!("Timeout acquiring read lock on pools for all-hop detection.");
                return Err(ArbError::TimeoutError("Timeout waiting for pools read lock for all-hop".to_string()));
            }
        };
        let metrics_guard = self.metrics.lock().await;
        // find_all_opportunities now returns MultiHopArbOpportunity
        detector_guard.find_all_opportunities(&pools_guard, &*metrics_guard).await
    }


    pub async fn run_health_checks(&self) {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() < self.health_check_interval {
            return;
        }
        info!("Running health checks...");
        let mut overall_healthy = true;

        if let Some(client_arc) = &self.rpc_client {
            if client_arc.is_healthy().await {
                info!("RPC connection to primary endpoint (HA client) is healthy.");
                 // Additional check
                match client_arc.primary_client.get_epoch_info().await { // Assuming primary_client is Arc<NonBlockingRpcClient>
                    Ok(epoch_info) => info!("RPC (HA primary) successfully fetched epoch info for epoch: {}", epoch_info.epoch),
                    Err(e) => {
                        error!("RPC health check (HA primary): Failed to get epoch info: {}", e);
                        overall_healthy = false;
                    }
                }
            } else {
                error!("RPC connection health check failed for primary endpoint (HA client).");
                overall_healthy = false;
            }
        } else {
            warn!("HA RPC client not available for health check.");
        }

        if let Some(ws_manager_arc) = &self.ws_manager {
            let mut manager_guard = ws_manager_arc.lock().await;
            if manager_guard.is_connected().await {
                info!("WebSocket connection is active.");
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            } else {
                error!("WebSocket connection is inactive.");
                overall_healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] Attempting WebSocket reconnect (attempt {}/{})", attempts, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager_guard.start().await { // Assuming start handles reconnect logic
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                    error!("[ArbitrageEngine] WebSocket max reconnect attempts ({}) reached.", self.max_ws_reconnect_attempts);
                }
            }
        } else {
            info!("WebSocket manager not configured, skipping connection health check.");
        }

        for provider_arc in &self.dex_providers {
            info!("Checking health of DEX provider: {}", provider_arc.get_name());
            // Placeholder for actual health check, e.g., a test quote
        }

        // Ensure metrics methods are adapted if they become async
        self.metrics.lock().await.set_system_health(overall_healthy);
        if overall_healthy {
            info!("All health checks passed.");
        } else {
            error!("One or more health checks failed.");
        }
        *last_check_guard = Instant::now();
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
                 // Assuming CryptoDataProvider has get_price
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

            let base_threshold = self.config.min_profit_pct;
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.5);

            let new_threshold = recommend_min_profit_threshold(
                current_volatility,
                base_threshold,
                volatility_factor,
            );
            
            // Update engine's shared threshold
            *self.min_profit_threshold.write().await = new_threshold;
            // Also update the detector's internal threshold
            self.detector.lock().await.set_min_profit_threshold(new_threshold);


            info!(
                "Dynamic minimum profit threshold updated to: {:.4}% (Volatility: {:.6}, Base: {:.4}%, Factor: {:.2})",
                new_threshold * 100.0, current_volatility, base_threshold * 100.0, volatility_factor
            );
            self.metrics.lock().await.log_dynamic_threshold_update(new_threshold);
        }
    }

    pub async fn get_current_status(&self) -> String {
        let detector_guard = self.detector.lock().await;
        format!(
            "ArbitrageEngine status: OK. Current min profit threshold: {:.4}%",
            detector_guard.get_min_profit_threshold() * 100.0
        )
    }
}