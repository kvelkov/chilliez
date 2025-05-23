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
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::{DexType, PoolInfo, TokenAmount},
    websocket::CryptoDataProvider,
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
    min_profit_threshold: Arc<RwLock<f64>>, // Percentage, e.g., 0.5 for 0.5%
    max_slippage: f64,                      // Fractional, e.g., 0.005 for 0.5%
    tx_fee_lamports: u64,
    ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
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
    dex_providers: Vec<Arc<dyn DexClient>>,
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
        let initial_min_profit_pct = config.min_profit_pct * 100.0; // Store as percentage for detector
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
            ws_manager: ws_manager_instance,
            price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(
                Instant::now() - Duration::from_secs(health_check_interval_secs * 2),
            )), // Ensure first check runs soon
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,
            dex_providers: dex_api_clients,
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
        *self.min_profit_threshold.read().await // This is stored as percentage
    }

    pub async fn set_min_profit_threshold(&self, threshold_pct: f64) {
        let mut current_threshold_guard = self.min_profit_threshold.write().await;
        *current_threshold_guard = threshold_pct;
        self.metrics
            .lock()
            .await
            .log_dynamic_threshold_update(threshold_pct / 100.0); // Log as fraction for consistency in metrics if needed
    }

    // This method seems more for internal use if executor handles its own limits
    pub fn should_execute_trade(
        &self,
        calculated_slippage_fraction: f64,
        estimated_fee_lamports: u64,
    ) -> bool {
        calculated_slippage_fraction <= self.max_slippage // max_slippage is fractional
            && estimated_fee_lamports <= self.tx_fee_lamports * 2 // Example cap
    }

    pub async fn discover_direct_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)), // Increased timeout slightly
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("discover_direct_opportunities: Failed to acquire read lock on pools (poisoned): {}", e);
                return Err(ArbError::Unknown(format!("Pools lock poisoned: {}", e)));
            }
            Err(_) => {
                // Timeout error
                warn!("discover_direct_opportunities: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock in discover_direct_opportunities"
                        .to_string(),
                ));
            }
        };

        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        // Update detector's threshold before running detection
        // detector_guard.set_min_profit_threshold(*self.min_profit_threshold.read().await); // Done in dynamic updater usually
        detector_guard
            .find_all_opportunities(&pools_guard, &mut metrics_guard)
            .await
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1500)), // Slightly more for multi-hop
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("discover_multihop_opportunities: Failed to acquire read lock on pools (poisoned): {}", e);
                return Err(ArbError::Unknown(format!("Pools lock poisoned: {}", e)));
            }
            Err(_) => {
                warn!("discover_multihop_opportunities: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError(
                    "Timeout waiting for pools read lock in discover_multihop_opportunities"
                        .to_string(),
                ));
            }
        };

        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        detector_guard
            .find_all_multihop_opportunities(&pools_guard, &mut metrics_guard)
            .await
    }

    pub async fn discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let pools_guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("discover_multihop_opportunities_with_risk: Failed to acquire read lock on pools (poisoned): {}",e);
                return Err(ArbError::Unknown(format!("Pools lock poisoned: {}", e)));
            }
            Err(_) => {
                warn!("discover_multihop_opportunities_with_risk: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError("Timeout waiting for pools read lock in discover_multihop_opportunities_with_risk".to_string()));
            }
        };

        let mut metrics_guard = self.metrics.lock().await;
        let detector_guard = self.detector.lock().await;
        detector_guard
            .find_all_multihop_opportunities_with_risk(
                &pools_guard,
                &mut metrics_guard,
                self.max_slippage, // Pass fractional slippage
                self.tx_fee_lamports,
            )
            .await
    }

    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let pools_guard = self.pools.read().await; // Consider timeout here as well
        let mut resolved_pools = Vec::new();
        for hop_pool_address in &opportunity.pool_path {
            match pools_guard.get(hop_pool_address) {
                Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                None => {
                    let hop_details = opportunity
                        .hops
                        .iter()
                        .find(|h| &h.pool == hop_pool_address);
                    let (input_token, output_token) = hop_details
                        .map_or(("N/A".to_string(), "N/A".to_string()), |h| {
                            (h.input_token.clone(), h.output_token.clone())
                        });
                    warn!(
                        "Pool {} for hop {}->{} not found in local cache.",
                        hop_pool_address, input_token, output_token
                    );
                    return Err(ArbError::PoolNotFound(hop_pool_address.to_string()));
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
        drop(last_check_guard); // Release write lock before potentially long health check

        self.run_health_checks().await; // Call the actual health check logic

        // Re-acquire write lock to update timestamp
        *self.last_health_check.write().await = Instant::now();
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        // Note: This method is called by maybe_check_health, timestamp update is handled there.
        info!("[ArbitrageEngine] Performing system health check...");
        let mut overall_healthy = true;

        // WebSocket Health
        let mut ws_healthy = true;
        if let Some(manager_arc) = &self.ws_manager {
            // Use self.ws_manager consistently
            let mut manager_guard = manager_arc.lock().await;
            if manager_guard.is_connected().await {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            } else {
                ws_healthy = false;
                warn!("[ArbitrageEngine] WebSocket disconnected.");
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts <= self.max_ws_reconnect_attempts {
                    warn!(
                        "[ArbitrageEngine] Attempting WebSocket reconnect (attempt {}/{})",
                        attempts, self.max_ws_reconnect_attempts
                    );
                    if let Err(e) = manager_guard.start().await {
                        // Assuming start also handles reconnect
                        error!(
                            "[ArbitrageEngine] WebSocket reconnect attempt failed: {}",
                            e
                        );
                    }
                } else {
                    error!(
                        "[ArbitrageEngine] WebSocket max reconnect attempts ({}) reached.",
                        self.max_ws_reconnect_attempts
                    );
                }
            }
        } else {
            info!("[ArbitrageEngine] WebSocket manager not configured, skipping check.");
            // ws_healthy = false; // If WS is critical, set to false
        }
        if !ws_healthy {
            overall_healthy = false;
        }

        // Price Provider Health
        let mut price_healthy = true;
        if let Some(provider_arc) = &self.price_provider {
            let health_token_symbol = self
                .config
                .health_check_token_symbol
                .as_deref()
                .unwrap_or("SOL");
            match provider_arc.get_price(health_token_symbol).await {
                Some(price) => info!(
                    "[ArbitrageEngine] Price provider healthy, price for {}: {}",
                    health_token_symbol, price
                ),
                None => {
                    price_healthy = false;
                    warn!(
                        "[ArbitrageEngine] Price provider failed to get price for {}.",
                        health_token_symbol
                    );
                }
            }
        } else {
            info!("[ArbitrageEngine] Price provider not configured, skipping check.");
            // price_healthy = false; // If price provider is critical
        }
        if !price_healthy {
            overall_healthy = false;
        }

        // RPC Health
        let mut rpc_healthy = true;
        if let Some(client_arc) = &self.rpc_client {
            if client_arc.is_healthy().await {
                match client_arc.primary_client.get_epoch_info().await {
                    Ok(epoch_info) => info!(
                        "[ArbitrageEngine] RPC (HA primary) healthy. Current epoch: {}",
                        epoch_info.epoch
                    ),
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
            // rpc_healthy = false; // If RPC is critical
        }
        if !rpc_healthy {
            overall_healthy = false;
        }

        // DEX Client Health (Conceptual)
        for dex_client in &self.dex_providers {
            debug!(
                "[ArbitrageEngine] Conceptual health check for DEX: {}",
                dex_client.get_name()
            );
            // TODO: Implement actual health check for each DexClient if trait supports it or via test quote
        }

        // Degradation Mode Logic
        let degradation_profit_factor = self.config.degradation_profit_factor.unwrap_or(1.5); // e.g., 1.5x
        let base_min_profit_pct = self.config.min_profit_pct * 100.0; // Base threshold from config (as percentage)

        let should_degrade = !overall_healthy;
        let was_degraded = self
            .degradation_mode
            .swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode. Overall Health: {}. Details - WS: {}, PriceProvider: {}, RPC: {}", overall_healthy, ws_healthy, price_healthy, rpc_healthy);
            let new_threshold_pct = base_min_profit_pct * degradation_profit_factor;
            self.set_min_profit_threshold(new_threshold_pct).await; // This method expects percentage
            info!(
                "[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.",
                new_threshold_pct
            );
            self.metrics
                .lock()
                .await
                .log_degradation_mode_change(true, Some(new_threshold_pct / 100.0));
        // Log as fraction
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            self.set_min_profit_threshold(base_min_profit_pct).await; // Reset to base (percentage)
            info!(
                "[ArbitrageEngine] Min profit threshold reset to {:.4}%.",
                base_min_profit_pct
            );
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            self.metrics
                .lock()
                .await
                .log_degradation_mode_change(false, Some(base_min_profit_pct / 100.0));
            // Log as fraction
        }
        self.metrics.lock().await.set_system_health(overall_healthy);
    }

    pub async fn update_pools(
        &self,
        new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        let mut writable_pools_ref = self.pools.write().await;
        let mut new_pools_count = 0;
        let mut updated_pools_count = 0;

        for (address, pool_info_arc) in new_pools_data {
            // Iterate over Arc directly
            if writable_pools_ref.contains_key(&address) {
                writable_pools_ref.insert(address, pool_info_arc); // pool_info_arc is already Arc
                updated_pools_count += 1;
            } else {
                writable_pools_ref.insert(address, pool_info_arc); // pool_info_arc is already Arc
                new_pools_count += 1;
            }
        }
        self.metrics.lock().await.log_pools_updated(
            new_pools_count,
            updated_pools_count,
            writable_pools_ref.len(),
        );
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_pools_count,
            updated_pools_count,
            writable_pools_ref.len()
        );
        Ok(())
    }

    // Simplified detect_arbitrage, assuming it's for any type of multi-hop for now.
    // If "direct" means only 2-hop, then discover_direct_opportunities should be used.
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_multihop_opportunities().await // Or specific type of discovery
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        let update_interval_secs = self
            .config
            .dynamic_threshold_update_interval_secs
            .unwrap_or(300);
        if update_interval_secs == 0 {
            info!("Dynamic threshold updates disabled (interval is 0).");
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(update_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut volatility_tracker =
            VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));
        info!(
            "Dynamic threshold update task started. Interval: {}s",
            update_interval_secs
        );

        loop {
            interval.tick().await;

            let sol_symbol = self
                .config
                .health_check_token_symbol
                .as_deref()
                .unwrap_or("SOL");
            let current_price_of_major_asset = if let Some(provider) = &self.price_provider {
                provider.get_price(sol_symbol).await.unwrap_or_else(|| {
                    warn!(
                        "Could not fetch price for {} for volatility. Using 1.0.",
                        sol_symbol
                    );
                    1.0
                })
            } else {
                warn!("Price provider not configured for volatility. Using 1.0.");
                1.0
            };

            volatility_tracker.add_price(current_price_of_major_asset);
            let current_volatility = volatility_tracker.volatility();

            // config.min_profit_pct is fractional (e.g., 0.001 for 0.1%)
            // recommend_min_profit_threshold expects base_threshold as fractional
            // set_min_profit_threshold on engine expects percentage
            let base_threshold_fractional = self.config.min_profit_pct;
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.5);

            let new_threshold_fractional = recommend_min_profit_threshold(
                current_volatility,
                base_threshold_fractional,
                volatility_factor,
            );

            let new_threshold_pct = new_threshold_fractional * 100.0;

            self.set_min_profit_threshold(new_threshold_pct).await; // Sets engine's Arc<RwLock<f64>>
            self.detector
                .lock()
                .await
                .set_min_profit_threshold(new_threshold_pct); // Updates detector's internal

            info!(
                "Dynamic minimum profit threshold updated to: {:.4}% (Volatility: {:.6}, Base (frac): {:.5}, Factor: {:.2})",
                new_threshold_pct, current_volatility, base_threshold_fractional, volatility_factor
            );
            // log_dynamic_threshold_update in metrics expects fractional
            self.metrics
                .lock()
                .await
                .log_dynamic_threshold_update(new_threshold_fractional);
        }
    }

    pub async fn get_current_status(&self) -> String {
        let current_threshold_pct = self.get_min_profit_threshold().await; // Gets as percentage
        format!(
            "ArbitrageEngine status: OK. Current min profit threshold: {:.4}%",
            current_threshold_pct
        )
    }
}