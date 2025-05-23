// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::{ArbitrageDetector, ArbitrageOpportunity},
        dynamic_threshold::{self, recommend_min_profit_threshold, VolatilityTracker}, // Added VolatilityTracker
        opportunity::MultiHopArbOpportunity,
    },
    config::settings::Config,
    dex::quote::DexClient, // Combined DEX imports
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::SolanaWebsocketManager,
    },
    utils::{DexType, PoolInfo, TokenAmount}, // Added TokenAmount, DexType
    websocket::CryptoDataProvider, // Corrected trait name
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
    min_profit_threshold: Arc<RwLock<f64>>,
    max_slippage: f64,
    tx_fee_lamports: u64,
    // ws_manager: Option<SolanaWebsocketManager>, // Needs to be Arc<Mutex<>> for shared mutability
    ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>, // Use Arc and correct trait
    metrics: Arc<Mutex<Metrics>>,
    rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>,
    degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    health_check_interval: Duration,
    ws_reconnect_attempts: Arc<AtomicU64>,
    max_ws_reconnect_attempts: u64, // Ensure this matches AtomicU64 if used for comparison/limits
    detector: Arc<Mutex<ArbitrageDetector>>, // Added detector field
    // For run_health_checks
    websocket_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>, // Redundant with ws_manager, choose one
    dex_providers: Vec<Arc<dyn DexClient>>,                   // Added field
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>, // Corrected trait
        // These are passed from main.rs
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        dex_api_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        let initial_min_profit = config.min_profit_pct;
        let max_slippage_val = config.max_slippage_pct;
        let tx_fee_lamports_val = config.default_priority_fee_lamports;
        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);

        // Create the detector instance correctly
        let detector = Arc::new(Mutex::new(ArbitrageDetector::new(
            initial_min_profit, // Pass only the min_profit_threshold
        )));

        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(initial_min_profit)),
            max_slippage: max_slippage_val,
            tx_fee_lamports: tx_fee_lamports_val,
            ws_manager: ws_manager_instance.clone(), // Use the passed instance
            price_provider,
            metrics,
            rpc_client,
            config,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,                                    // Store the detector
            websocket_manager: ws_manager_instance, // Use the passed instance for this field too
            dex_providers: dex_api_clients,         // Store dex_api_clients
        }
    }

    pub async fn start_services(&self) {
        // Use self.ws_manager or self.websocket_manager consistently. Let's use self.ws_manager.
        if let Some(manager_arc) = &self.ws_manager {
            let mut manager_guard = manager_arc.lock().await; // Lock to get access
            if let Err(e) = manager_guard.start().await {
                error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }
        // Start other services like price provider if they have a start method
        // e.g., if let Some(pp) = &self.price_provider { pp.start().await; }
    }

    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn set_min_profit_threshold(&self, threshold: f64) {
        let mut current_threshold_guard = self.min_profit_threshold.write().await;
        *current_threshold_guard = threshold;
        self.metrics.lock().await.log_dynamic_threshold_update(threshold); // Log change
    }

    pub fn should_execute_trade(&self, calculated_slippage: f64, estimated_fee_lamports: u64) -> bool {
        calculated_slippage <= self.max_slippage && estimated_fee_lamports <= self.tx_fee_lamports * 2
    }

    pub async fn discover_direct_opportunities(
        &self,
    ) -> Result<Vec<ArbitrageOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
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

        let detector_guard = self.detector.lock().await; // Lock the detector
        // Pass the min_profit_threshold from the engine's Arc<RwLock<f64>>
        let current_min_profit = *self.min_profit_threshold.read().await;
        // If ArbitrageDetector's methods need the threshold, they should take it as an argument
        // or the detector itself should be updated if its internal threshold needs to change.
        // For now, assuming find_all_opportunities uses its internally set threshold.
        let opportunities = detector_guard.find_all_opportunities(&pools_guard, &*self.metrics.lock().await).await?;
        // metrics.record_opportunities_found(...) if method exists
        Ok(opportunities)
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
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
        // let current_min_profit_threshold = self.get_min_profit_threshold().await;
        // Pass current_min_profit_threshold if detector methods need it
        let opportunities = detector_guard.find_all_multihop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        // metrics.record_opportunities_found(...)
        Ok(opportunities)
    }

    pub async fn discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
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
        // let current_min_profit_threshold = self.get_min_profit_threshold().await;
        let opportunities = detector_guard.find_all_multihop_opportunities_with_risk(
            &pools_guard,
            &*self.metrics.lock().await,
            self.max_slippage,
            self.tx_fee_lamports,
        ).await;
        // metrics.record_opportunities_found(...)
        Ok(opportunities)
    }

    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
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
        for hop in &opportunity.hops {
            match pools_guard.get(&hop.pool) {
                Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                None => {
                    warn!(
                        "Pool {} for hop {}->{} not found in local cache.",
                        hop.pool, hop.input_token, hop.output_token
                    );
                    return Err(ArbError::Unknown(format!("Pool {} not found", hop.pool)));
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

        // WebSocket Health
        let mut ws_healthy = true;
        if let Some(manager_arc) = &self.ws_manager { // Use self.ws_manager consistently
            let mut manager_guard = manager_arc.lock().await;
            if manager_guard.is_connected().await { // Assuming is_connected is async
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed); // Reset on success
            } else {
                ws_healthy = false;
                warn!("[ArbitrageEngine] WebSocket disconnected.");
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] Attempting WebSocket reconnect (attempt {}/{})", attempts, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager_guard.start().await { // Or a dedicated reconnect method
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                    error!("[ArbitrageEngine] WebSocket max reconnect attempts ({}) reached.", self.max_ws_reconnect_attempts);
                }
            }
        } else {
            info!("[ArbitrageEngine] WebSocket manager not configured, skipping check.");
            ws_healthy = true; // Or false if it's critical and not configured
        }
        if !ws_healthy { overall_healthy = false; }

        // Price Provider Health
        let mut price_healthy = true;
        if let Some(provider_arc) = &self.price_provider {
            // Assuming CryptoDataProvider might have a check_health method in the future
            // For now, let's simulate trying to get a common price
            let sol_symbol = self.config.health_check_token_symbol.as_deref().unwrap_or("SOL");
            if provider_arc.get_price(sol_symbol).await.is_none() {
                price_healthy = false;
                warn!("[ArbitrageEngine] Price provider failed to get price for {}.", sol_symbol);
            }
        } else {
            info!("[ArbitrageEngine] Price provider not configured, skipping check.");
            price_healthy = true; // Or false if critical
        }
        if !price_healthy { overall_healthy = false; }


        // RPC Health
        let mut rpc_healthy = true;
        if let Some(client_arc) = &self.rpc_client {
            if !client_arc.is_healthy().await {
                rpc_healthy = false;
                warn!("[ArbitrageEngine] Primary RPC endpoint is unhealthy.");
            }
        } else {
            info!("[ArbitrageEngine] RPC client not configured, skipping check.");
            rpc_healthy = true; // Or false if critical
        }
        if !rpc_healthy { overall_healthy = false; }

        // DEX Client Health (placeholder)
        // In a real scenario, each DexClient might have a check_health method
        for dex_client_arc in &self.dex_providers {
            // if !dex_client_arc.check_health().await { overall_healthy = false; warn!(...); }
        }


        // Degradation Mode Logic
        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit_guard = self.min_profit_threshold.read().await;
        let base_min_profit_from_config = self.config.min_profit_pct; // Base threshold from config

        let should_degrade = !overall_healthy; // Degrade if any component is unhealthy
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode. WS: {}, Price: {}, RPC: {}", ws_healthy, price_healthy, rpc_healthy);
            let new_threshold = base_min_profit_from_config * degradation_factor;
            drop(current_min_profit_guard); // Release read lock before acquiring write lock
            self.set_min_profit_threshold(new_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold * 100.0);
            self.metrics.lock().await.log_degradation_mode_change(true, Some(new_threshold));
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            drop(current_min_profit_guard); // Release read lock
            self.set_min_profit_threshold(base_min_profit_from_config).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_min_profit_from_config * 100.0);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed); // Reset WS reconnect attempts
            self.metrics.lock().await.log_degradation_mode_change(false, Some(base_min_profit_from_config));
        }

        *last_check_guard = Instant::now(); // Update timestamp
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
        self.metrics.lock().await.log_pools_updated(new_pools_count, updated_pools_count, writable_pools_ref.len());
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_pools_count,
            updated_pools_count,
            writable_pools_ref.len()
        );
        Ok(())
    }

    pub async fn detect_two_hop_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let detector_guard = self.detector.lock().await;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Ok(Vec::new());
            }
            Err(_) => {
                warn!("Timeout acquiring read lock on pools for 2-hop detection.");
                return Ok(Vec::new());
            }
        };
        let opportunities = detector_guard.find_two_hop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        Ok(opportunities)
    }

    pub async fn detect_three_hop_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let detector_guard = self.detector.lock().await;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error) for 3-hop: {}", e);
                return Ok(Vec::new());
            }
            Err(_) => {
                warn!("Timeout acquiring read lock on pools for 3-hop detection.");
                return Ok(Vec::new());
            }
        };
        let opportunities = detector_guard.find_three_hop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        Ok(opportunities)
    }

    // detect_arbitrage now correctly calls find_all_opportunities which returns Result<Vec<ArbitrageOpportunity>, ArbError>
    pub async fn detect_arbitrage(&self) -> Result<Vec<ArbitrageOpportunity>, ArbError> {
        let detector_guard = self.detector.lock().await;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        ).await {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error) for all-hop: {}", e);
                // Return the error instead of an empty Vec
                return Err(ArbError::Unknown(format!("Pools lock poisoned for all-hop: {}", e)));
            }
            Err(_) => {
                warn!("Timeout acquiring read lock on pools for all-hop detection.");
                 return Err(ArbError::TimeoutError("Timeout waiting for pools read lock for all-hop".to_string()));
            }
        };
        let metrics_guard = self.metrics.lock().await;
        detector_guard.find_all_opportunities(&pools_guard, &*metrics_guard).await
    }

    pub async fn run_health_checks(&self) {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() < self.health_check_interval {
            // Not logging here to avoid spam if called frequently before interval passes
            return;
        }
        info!("Running health checks...");
        let mut overall_healthy = true;

        // Check RPC connection
        if let Some(client_arc) = &self.rpc_client {
            if client_arc.is_healthy().await {
                info!("RPC connection to primary endpoint is healthy.");
            } else {
                error!("RPC connection health check failed for primary endpoint.");
                overall_healthy = false;
            }
            // Additional check
            match client_arc.primary_client.get_epoch_info().await {
                Ok(epoch_info) => info!("RPC successfully fetched epoch info for epoch: {}", epoch_info.epoch),
                Err(e) => {
                    error!("RPC health check: Failed to get epoch info: {}", e);
                    overall_healthy = false;
                }
            }
        } else {
            warn!("RPC client not available for health check.");
            // Decide if this makes the system unhealthy overall
            // overall_healthy = false;
        }

        // Check WebSocket connection
        if let Some(ws_manager_arc) = &self.ws_manager { // Use self.ws_manager
            let mut manager_guard = ws_manager_arc.lock().await;
            if manager_guard.is_connected().await { // Assuming is_connected() is async
                info!("WebSocket connection is active.");
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed); // Reset on success
            } else {
                error!("WebSocket connection is inactive.");
                overall_healthy = false;
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
            info!("WebSocket manager not configured, skipping connection health check.");
        }

        // Check DEX Providers
        for provider_arc in &self.dex_providers { // Use self.dex_providers
            info!("Checking health of DEX provider: {}", provider_arc.get_name());
            // Placeholder: A real DexClient trait might have an async check_health method.
            // Simulating a check, e.g., by trying to get a quote for a known pair.
            // if provider_arc.get_best_swap_quote("SOL_MINT_ADDRESS", "USDC_MINT_ADDRESS", 1).await.is_err() {
            //     error!("Health check failed for DEX provider: {}", provider_arc.get_name());
            //     overall_healthy = false;
            // }
        }

        self.metrics.lock().await.set_system_health(overall_healthy);
        if overall_healthy {
            info!("All health checks passed.");
        } else {
            error!("One or more health checks failed.");
        }
        *last_check_guard = Instant::now(); // Update timestamp
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        let update_interval_secs = self.config.dynamic_threshold_update_interval_secs.unwrap_or(300);
        if update_interval_secs == 0 {
            info!("Dynamic threshold updates disabled (interval is 0).");
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(update_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Assuming VolatilityTracker is part of the engine state or created here
        let mut volatility_tracker = VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));

        info!("Dynamic threshold update task started. Interval: {}s", update_interval_secs);

        loop {
            interval.tick().await;
            let mut detector_guard = self.detector.lock().await; // detector is Arc<Mutex<ArbitrageDetector>>

            // Fetch current price of a major asset
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

            let base_threshold = self.config.min_profit_pct; // Use correct config field
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.5);

            let new_threshold = recommend_min_profit_threshold(
                current_volatility,
                base_threshold,
                volatility_factor,
            );

            // Update the threshold in ArbitrageDetector
            detector_guard.set_min_profit_threshold(new_threshold); // Assuming ArbitrageDetector has this method

            // Also update the engine's shared threshold for other potential uses
            *self.min_profit_threshold.write().await = new_threshold;


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
            detector_guard.get_min_profit_threshold() * 100.0 // Assuming ArbitrageDetector has this method
        )
    }
}