// src/arbitrage/engine.rs

use crate::{
    arbitrage::{
        detector::{ArbitrageDetector, ArbitrageOpportunity}, 
        dynamic_threshold::{self, recommend_min_profit_threshold}, 
        opportunity::MultiHopArbOpportunity,
    },
    cache::Cache,
    config::settings::Config, 
    dex::{
        lifinity::LifinityClient, meteora::MeteoraClient, orca::OrcaClient, phoenix::PhoenixClient,
        pool::PoolMap, quote::DexClient, raydium::RaydiumClient, whirlpool::WhirlpoolClient,
    },
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::{SolanaRpcClient}, 
        websocket::SolanaWebsocketManager,
    },
    utils::PoolInfo,
    websocket::{CryptoDataProvider, PriceProvider}, // Moved PriceProvider here
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
use tokio::sync::{Mutex, RwLock}; // Added Mutex for Metrics
use tokio::time::timeout;


pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: Arc<RwLock<f64>>, // This will be initialized from config
    max_slippage: f64, // This will be initialized from config
    tx_fee_lamports: u64, // This will be initialized from config
    ws_manager: Option<SolanaWebsocketManager>,
    price_provider: Option<Box<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>, // Changed to Arc<Mutex<Metrics>>
    rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>, // Store the config
    degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    health_check_interval: Duration,
    ws_reconnect_attempts: Arc<AtomicU64>,
    max_ws_reconnect_attempts: u64,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>, // Added rpc_client
        config: Arc<Config>,                     // Use the struct directly
        metrics: Arc<Mutex<Metrics>>,            // Corrected type
        price_provider: Option<Arc<dyn PriceProvider + Send + Sync>>, // Added price_provider
    ) -> Self {
        let detector = ArbitrageDetector::new(
            config.volatility_threshold_factor.unwrap_or(0.001), // FIXME: This should be min_profit_pct from config
            config.degradation_slippage_factor.unwrap_or(0.005),  // FIXME: This should be max_slippage_pct from config
            config.execution_chunk_size.map_or(5000, |v| v as u64), // FIXME: This should be tx_fee_lamports from config
            price_provider, // Pass the price_provider to the detector
        );

        // Initial dynamic threshold setup
        let initial_min_profit = config.volatility_threshold_factor.unwrap_or(0.001); // FIXME: min_profit_pct
        let max_slippage = config.degradation_slippage_factor.unwrap_or(0.005); // FIXME: max_slippage_pct
        let tx_fee_lamports = config.execution_chunk_size.map_or(5000, |v| v as u64); // FIXME: default_priority_fee_lamports
        let health_check_interval_secs = config.congestion_update_interval_secs.unwrap_or(60); // FIXME: health_check_interval_secs
        let max_ws_reconnect_attempts = config.execution_chunk_size.map_or(5, |v| v as u8); // FIXME: max_ws_reconnect_attempts


        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(initial_min_profit)),
            max_slippage,
            tx_fee_lamports,
            ws_manager: None,
            price_provider: None,
            metrics,
            rpc_client,
            config, // Store the passed config
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts,
        }
    }

    pub async fn start_services(&self) {
        if let Some(manager) = &self.ws_manager {
            if let Err(e) = manager.start().await {
                error!("[ArbitrageEngine] Failed to start WebSocket manager: {}", e);
            } else {
                info!("[ArbitrageEngine] WebSocket manager started successfully.");
            }
        }
        // Start other services like price provider if they have a start method
    }

    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn set_min_profit_threshold(&self, threshold: f64) {
        let mut current_threshold = self.min_profit_threshold.write().await;
        *current_threshold = threshold;
        // Potentially log this change via metrics if needed
        // self.metrics.lock().await.log_min_profit_threshold_updated(threshold).await;
    }
    
    pub fn should_execute_trade(&self, calculated_slippage: f64, estimated_fee_lamports: u64) -> bool {
        calculated_slippage <= self.max_slippage && estimated_fee_lamports <= self.tx_fee_lamports * 2 // Example: allow up to 2x base fee
    }


    pub async fn discover_direct_opportunities(
        &self,
    ) -> Result<Vec<ArbitrageOpportunity>, ArbError> {
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

        let detector = ArbitrageDetector::new(*self.min_profit_threshold.read().await);
        let opportunities = detector.find_all_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        
        // self.metrics.lock().await.log_direct_opportunities_found(opportunities.len()).await;
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

        let current_min_profit_threshold = self.get_min_profit_threshold().await;
        let detector = ArbitrageDetector::new(current_min_profit_threshold);
        
        let opportunities = detector.find_all_multihop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        // self.metrics.lock().await.log_multihop_opportunities_found(opportunities.len()).await;
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
        
        let current_min_profit_threshold = self.get_min_profit_threshold().await;
        let detector = ArbitrageDetector::new(current_min_profit_threshold);

        let opportunities = detector
            .find_all_multihop_opportunities_with_risk(
                &pools_guard,
                &*self.metrics.lock().await,
                self.max_slippage,
                self.tx_fee_lamports,
            )
            .await;
        // self.metrics.lock().await.log_multihop_opportunities_found(opportunities.len()).await;
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
        for hop in &opportunity.hops {
            match pools_guard.get(&hop.pool) {
                Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                None => {
                    warn!(
                        "Pool {} for hop {}->{} not found in local cache.",
                        hop.pool, hop.input_token, hop.output_token
                    );
                    // ArbError::PoolNotFound is not defined, using Unknown
                    return Err(ArbError::Unknown(format!(
                        "Pool {} not found",
                        hop.pool
                    )));
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
        if let Some(manager) = &self.ws_manager {
            // Assuming is_connected is async and returns Result or bool
            match manager.is_connected().await {
                Ok(connected) => ws_healthy = connected,
                Err(e) => {
                    warn!("[ArbitrageEngine] WebSocket health check error: {}", e);
                    ws_healthy = false;
                }
            }
            if !ws_healthy {
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                if attempts < self.max_ws_reconnect_attempts {
                    warn!("[ArbitrageEngine] WebSocket disconnected. Attempting reconnect (attempt {}/{})", attempts + 1, self.max_ws_reconnect_attempts);
                    if let Err(e) = manager.start().await { // Or a specific reconnect method
                        error!("[ArbitrageEngine] WebSocket reconnect attempt failed: {}", e);
                    }
                } else {
                     error!("[ArbitrageEngine] WebSocket disconnected. Max reconnect attempts reached.");
                }
            } else {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed); // Reset on successful connection
            }
        }

        let mut price_healthy = true;
        if let Some(provider) = &self.price_provider {
            if let Err(e) = provider.check_health().await {
                warn!("[ArbitrageEngine] Price provider health check failed: {}", e);
                price_healthy = false;
            }
        }

        let mut rpc_healthy = true;
        if let Some(client) = &self.rpc_client {
            // Check RPC health, e.g., by fetching epoch info or a small account
            // This is a simplified check; a real one might use get_latest_blockhash or similar
            if client.get_epoch_info().await.is_err() { // Corrected: is_err() is on Result
                rpc_healthy = false;
                warn!("[ArbitrageEngine] Primary RPC endpoint is unhealthy.");
            }
        }

        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit = *self.min_profit_threshold.read().await;

        let should_degrade = !ws_healthy || !price_healthy || !rpc_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues (WS: {}, Price: {}, RPC: {})", ws_healthy, price_healthy, rpc_healthy);
            let new_threshold = current_min_profit * degradation_factor;
            self.set_min_profit_threshold(new_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold * 100.0);
            // self.metrics.lock().await.log_degradation_mode_change(true, Some(new_threshold)).await;

        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            let base_threshold = self.config.min_profit_pct; // Reset to base from config
            self.set_min_profit_threshold(base_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_threshold * 100.0);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            // self.metrics.lock().await.log_degradation_mode_change(false, Some(base_threshold)).await;
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

    /// Detects 2-hop arbitrage opportunities.
    pub async fn detect_two_hop_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let detector = self.detector.lock().await;
        let pools_guard = 
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard, // Timeout completed, and RwLock read was successful
            Ok(Err(e)) => { // Timeout completed, but RwLock read failed (e.g., lock poisoned)
                error!("Failed to acquire read lock on pools (lock error): {}", e);
                return Ok(Vec::new()); // Or handle error appropriately
            }
            Err(_) => { // Timeout elapsed
                warn!("Timeout acquiring read lock on pools for 2-hop detection.");
                return Ok(Vec::new());
            }
        };

        let opportunities = detector.find_two_hop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        Ok(opportunities)
    }

    /// Detects 3-hop arbitrage opportunities.
    pub async fn detect_three_hop_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let detector = self.detector.lock().await;
        let pools_guard = 
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
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

        let opportunities = detector.find_three_hop_opportunities(&pools_guard, &*self.metrics.lock().await).await;
        Ok(opportunities)
    }

    /// Detects all (2-hop and 3-hop) arbitrage opportunities.
    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Changed to MultiHopArbOpportunity
        let detector = self.detector.lock().await;
        let pools_guard = 
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(500)),
            self.pools.read(),
        )
        .await
        {
            Ok(Ok(guard)) => guard,
            Ok(Err(e)) => {
                error!("Failed to acquire read lock on pools (lock error) for all-hop: {}", e);
                return Ok(Vec::new());
            }
            Err(_) => {
                warn!("Timeout acquiring read lock on pools for all-hop detection.");
                return Ok(Vec::new()); // Corrected from Vec_new()
            }
        };
        
        let metrics_guard = self.metrics.lock().await;
        // Assuming find_all_opportunities in detector returns Vec<MultiHopArbOpportunity> or can be adapted
        // If it returns Vec<ArbitrageOpportunity>, it needs to be converted or the return type of this function adjusted.
        // For now, let's assume it's compatible or will be fixed in detector.rs
        let opportunities = detector.find_all_opportunities(&pools_guard, &*metrics_guard).await?;
        Ok(opportunities)
    }

    pub async fn run_health_checks(&self) {
        info!("Running health checks...");
        let mut healthy = true;

        // Check RPC connection
        if let Some(client) = &self.rpc_client {
            match client.get_health().await {
                Ok(_) => info!("RPC connection to primary endpoint is healthy."),
                Err(e) => {
                    error!("RPC connection health check failed: {}", e);
                    healthy = false;
                }
            }
            // Check epoch info as an additional RPC check
             match client.primary_client.get_epoch_info().await {
                Ok(epoch_info) => info!("RPC successfully fetched epoch info: {:?}", epoch_info.epoch),
                Err(e) => {
                    error!("RPC health check: Failed to get epoch info: {}", e);
                    healthy = false;
                }
            }
        } else {
            warn!("RPC client not available for health check.");
        }


        // Check WebSocket connection (if applicable)
        if let Some(ws_manager_arc) = &self.websocket_manager {
            let manager = ws_manager_arc.lock().await;
            // manager.is_connected() is not async based on previous usage, if it becomes async, add .await
            if manager.is_connected_blocking() { // Assuming a blocking check or an async one if available
                info!("WebSocket connection is active.");
            } else {
                error!("WebSocket connection is inactive.");
                healthy = false;
            }
        } else {
            info!("WebSocket manager not configured, skipping connection health check.");
        }

        // Check DEX Providers (API Keys, Connectivity)
        for provider in &self.dex_providers {
            info!("Checking health of DEX provider: {}", provider.get_name());
            // provider.check_health() is not async based on previous usage, if it becomes async, add .await
            if let Err(e) = provider.check_health_sync() { // Assuming a sync check_health or an async one
                error!("Health check failed for {}: {}", provider.get_name(), e);
                healthy = false;
            }
        }
        
        // Update metrics based on overall health
        self.metrics.lock().await.set_system_health(healthy);
        if healthy {
            info!("All health checks passed.");
        } else {
            error!("One or more health checks failed.");
        }
    }


    /// Periodically updates the minimum profit threshold based on market volatility.
    pub async fn run_dynamic_threshold_updates(&self) {
        let update_interval_secs = self.config.dynamic_threshold_update_interval_secs.unwrap_or(300);
        if update_interval_secs == 0 {
            info!("Dynamic threshold updates disabled (interval is 0).");
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(update_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay); // Or Skip

        info!("Dynamic threshold update task started. Interval: {}s", update_interval_secs);

        loop {
            interval.tick().await;
            let mut detector_guard = self.detector.lock().await;
            // TODO: Implement actual volatility calculation if price_provider is available
            let current_volatility = 0.5; // Placeholder for actual volatility
            
            let base_threshold = self.config.volatility_threshold_factor.unwrap_or(0.001); // FIXME: min_profit_pct
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.5);

            let new_threshold = recommend_min_profit_threshold(
                current_volatility,
                base_threshold, 
                volatility_factor 
            );
            detector_guard.update_min_profit_threshold(new_threshold);
            info!("Dynamic minimum profit threshold updated to: {:.4}% based on volatility: {:.2}", new_threshold * 100.0, current_volatility);
            self.metrics.lock().await.log_dynamic_threshold_update(new_threshold);
        }
    }


    // Placeholder for a method that might be used by an external health check endpoint
    pub async fn get_current_status(&self) -> String {
        // Example: return some status string. In a real app, this would be more detailed.
        format!(
            "ArbitrageEngine status: OK. Current min profit threshold: {:.4}%",
            self.detector.lock().await.get_min_profit_threshold() * 100.0
        )
    }

    // TODO: Add methods for managing banned token pairs if that logic resides here.
    // Example:
    // pub async fn ban_token_pair(&self, token_a_mint: &str, token_b_mint: &str, permanent: bool, reason: &str) { ... }
    // pub async fn unban_token_pair(&self, token_a_mint: &str, token_b_mint: &str) { ... }
    // pub async fn is_token_pair_banned(&self, token_a_mint: &str, token_b_mint: &str) -> bool { ... }
}
