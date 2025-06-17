// src/webhooks/processor.rs
//! Pool update processor for handling real-time webhook notifications

use crate::utils::PoolInfo;
use crate::webhooks::types::{HeliusWebhookNotification, PoolUpdateEvent, PoolUpdateType};
use anyhow::{anyhow, Result as AnyhowResult};
use log::{debug, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Statistics for the pool update processor
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ProcessorStats {
    pub total_notifications: u64,
    pub successful_updates: u64,
    pub failed_updates: u64,
    pub swap_events: u64,
    pub liquidity_events: u64,
    pub unknown_events: u64,
    pub last_update_timestamp: u64,
}

/// Processes webhook notifications and updates pool information
pub struct PoolUpdateProcessor {
    stats: Arc<RwLock<ProcessorStats>>,
    pool_cache: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    update_callbacks: Arc<RwLock<Vec<Box<dyn Fn(PoolUpdateEvent) + Send + Sync>>>>,
}

impl PoolUpdateProcessor {
    /// Create a new pool update processor
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(ProcessorStats::default())),
            pool_cache: Arc::new(RwLock::new(HashMap::new())),
            update_callbacks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Process a Helius webhook notification
    pub async fn process_notification(
        &self,
        notification: &HeliusWebhookNotification,
    ) -> AnyhowResult<()> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_notifications += 1;
            stats.last_update_timestamp = notification.timestamp;
        }

        debug!(
            "Processing notification for signature: {}",
            notification.txn_signature
        );

        // Parse the pool update event
        let update_event = match self.parse_notification(notification).await {
            Ok(event) => event,
            Err(e) => {
                warn!("Failed to parse notification: {}", e);
                self.increment_failed_updates().await;
                return Err(e);
            }
        };

        // Update event type counters
        self.increment_event_counter(&update_event.update_type)
            .await;

        // Use enhanced pool update that considers static pool information
        if let Err(e) = self.enhanced_pool_update(&update_event).await {
            warn!("Failed to perform enhanced pool update: {}", e);
        }

        // Notify callbacks
        self.notify_callbacks(update_event).await;

        // Update success stats
        self.increment_successful_updates().await;

        Ok(())
    }

    /// Parse a webhook notification into a pool update event
    async fn parse_notification(
        &self,
        notification: &HeliusWebhookNotification,
    ) -> AnyhowResult<PoolUpdateEvent> {
        // Parse program ID
        let program_id = Pubkey::from_str(&notification.program_id)
            .map_err(|e| anyhow!("Invalid program ID: {}", e))?;

        // Determine update type based on token transfers and account changes
        let update_type = self.determine_update_type(notification).await;

        // Find pool address from accounts
        let pool_address = self.find_pool_address(notification, &program_id).await?;

        Ok(PoolUpdateEvent {
            pool_address,
            program_id,
            signature: notification.txn_signature.clone(),
            timestamp: notification.timestamp,
            slot: notification.slot,
            update_type,
            token_transfers: notification.token_transfers.clone().unwrap_or_default(),
            account_changes: notification.account_data.clone().unwrap_or_default(),
        })
    }

    /// Determine the type of pool update from the notification
    async fn determine_update_type(
        &self,
        notification: &HeliusWebhookNotification,
    ) -> PoolUpdateType {
        // Check for token transfers to determine operation type
        if let Some(transfers) = &notification.token_transfers {
            if transfers.len() >= 2 {
                // Multiple token transfers usually indicate a swap
                return PoolUpdateType::Swap;
            } else if transfers.len() == 1 {
                // Single transfer might be add/remove liquidity
                // Would need more sophisticated logic to determine
                return PoolUpdateType::PriceUpdate;
            }
        }

        // Check account data changes
        if let Some(account_data) = &notification.account_data {
            if account_data
                .iter()
                .any(|data| data.native_balance_change != 0)
            {
                return PoolUpdateType::PriceUpdate;
            }
        }

        PoolUpdateType::Unknown
    }

    /// Find the pool address from the notification accounts
    async fn find_pool_address(
        &self,
        notification: &HeliusWebhookNotification,
        program_id: &Pubkey,
    ) -> AnyhowResult<Pubkey> {
        // For most DEXs, the pool address is usually the first account
        // or can be identified by specific patterns

        if notification.accounts.is_empty() {
            return Err(anyhow!("No accounts in notification"));
        }

        // Try to parse each account as a potential pool address
        for account_str in &notification.accounts {
            if let Ok(account_pubkey) = Pubkey::from_str(account_str) {
                // Check if this looks like a pool account for the specific DEX
                if self
                    .is_likely_pool_account(&account_pubkey, program_id)
                    .await
                {
                    return Ok(account_pubkey);
                }
            }
        }

        // Fallback: use the first valid pubkey
        Pubkey::from_str(&notification.accounts[0])
            .map_err(|e| anyhow!("Failed to parse pool address: {}", e))
    }

    /// Check if an account is likely a pool account for a given program
    async fn is_likely_pool_account(&self, account: &Pubkey, _program_id: &Pubkey) -> bool {
        // Check if we already know about this pool
        let pool_cache = self.pool_cache.read().await;
        if pool_cache.contains_key(account) {
            return true;
        }

        // Program-specific logic could be added here
        // For now, assume it's a pool account if it's in the notification
        true
    }

    /// Update pool information based on the event
    #[allow(dead_code)]
    async fn update_pool_info(&self, event: &PoolUpdateEvent) -> AnyhowResult<()> {
        let mut pool_cache = self.pool_cache.write().await;

        // If we have the pool in cache, we could update its information
        if let Some(pool_info) = pool_cache.get_mut(&event.pool_address) {
            // Update timestamp
            if let Some(pool_arc) = Arc::get_mut(pool_info) {
                pool_arc.last_update_timestamp = event.timestamp;
            }

            info!("Updated pool info for {}", event.pool_address);
        } else {
            debug!("Pool {} not in cache, skipping update", event.pool_address);
        }

        Ok(())
    }

    /// Notify all registered callbacks about the pool update
    async fn notify_callbacks(&self, event: PoolUpdateEvent) {
        let callbacks = self.update_callbacks.read().await;

        for callback in callbacks.iter() {
            callback(event.clone());
        }
    }

    /// Add a callback to be notified of pool updates
    pub async fn add_update_callback<F>(&self, callback: F)
    where
        F: Fn(PoolUpdateEvent) + Send + Sync + 'static,
    {
        let mut callbacks = self.update_callbacks.write().await;
        callbacks.push(Box::new(callback));
    }

    /// Update the pool cache with new pool information
    pub async fn update_pool_cache(&self, pools: HashMap<Pubkey, Arc<PoolInfo>>) {
        let mut cache = self.pool_cache.write().await;
        cache.extend(pools);
        info!("Updated pool cache with {} pools", cache.len());
    }

    /// Register static pools for webhook monitoring
    /// This method takes pools discovered via static methods and sets them up for real-time updates
    pub async fn register_static_pools(
        &self,
        pools: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> AnyhowResult<()> {
        info!(
            "🔗 Registering {} static pools for webhook monitoring",
            pools.len()
        );

        // Add to pool cache
        {
            let mut cache = self.pool_cache.write().await;
            cache.extend(pools.clone());
        }

        // Log registration by DEX type
        let mut dex_counts = HashMap::new();
        for pool in pools.values() {
            let dex_name = format!("{:?}", pool.dex_type);
            *dex_counts.entry(dex_name).or_insert(0) += 1;
        }

        info!("📊 Registered pools by DEX:");
        for (dex, count) in dex_counts {
            info!("   {}: {} pools", dex, count);
        }

        info!("✅ Static pool registration complete");
        Ok(())
    }

    /// Get pools filtered by DEX type for monitoring
    pub async fn get_pools_by_dex(&self, dex_type: &crate::utils::DexType) -> Vec<Arc<PoolInfo>> {
        let cache = self.pool_cache.read().await;
        cache
            .values()
            .filter(|pool| pool.dex_type == *dex_type)
            .cloned()
            .collect()
    }

    /// Check if a pool is being monitored
    pub async fn is_pool_monitored(&self, pool_address: &Pubkey) -> bool {
        let cache = self.pool_cache.read().await;
        cache.contains_key(pool_address)
    }

    /// Get statistics about monitored pools
    pub async fn get_monitoring_stats(&self) -> MonitoringStats {
        let cache = self.pool_cache.read().await;
        let stats = self.stats.read().await;

        let mut pools_by_dex = HashMap::new();
        for pool in cache.values() {
            let dex_name = format!("{:?}", pool.dex_type);
            *pools_by_dex.entry(dex_name).or_insert(0) += 1;
        }

        MonitoringStats {
            total_monitored_pools: cache.len(),
            pools_by_dex,
            total_webhook_events: stats.total_notifications,
            active_monitoring: stats.total_notifications > 0,
            last_event_timestamp: if stats.last_update_timestamp > 0 {
                Some(stats.last_update_timestamp)
            } else {
                None
            },
        }
    }

    /// Get current processor statistics
    pub async fn get_stats(&self) -> ProcessorStats {
        self.stats.read().await.clone()
    }

    /// Increment successful updates counter
    async fn increment_successful_updates(&self) {
        let mut stats = self.stats.write().await;
        stats.successful_updates += 1;
    }

    /// Increment failed updates counter
    async fn increment_failed_updates(&self) {
        let mut stats = self.stats.write().await;
        stats.failed_updates += 1;
    }

    /// Update event type counters
    pub async fn increment_event_counter(&self, update_type: &PoolUpdateType) {
        let mut stats = self.stats.write().await;
        match update_type {
            PoolUpdateType::Swap => stats.swap_events += 1,
            PoolUpdateType::AddLiquidity | PoolUpdateType::RemoveLiquidity => {
                stats.liquidity_events += 1
            }
            PoolUpdateType::PriceUpdate => {} // Don't count price updates separately
            PoolUpdateType::Unknown => stats.unknown_events += 1,
        }
    }

    /// Get the current pool cache
    pub async fn get_pool_cache(&self) -> HashMap<Pubkey, Arc<PoolInfo>> {
        self.pool_cache.read().await.clone()
    }

    /// Clear the pool cache
    pub async fn clear_pool_cache(&self) {
        let mut cache = self.pool_cache.write().await;
        cache.clear();
    }

    /// Enhanced pool update that considers static pool information
    pub async fn enhanced_pool_update(&self, event: &PoolUpdateEvent) -> AnyhowResult<()> {
        let mut pool_cache = self.pool_cache.write().await;

        if let Some(pool_info) = pool_cache.get_mut(&event.pool_address) {
            // Create an enhanced update that preserves static pool information
            if let Some(pool_arc) = Arc::get_mut(pool_info) {
                // Update timestamp
                pool_arc.last_update_timestamp = event.timestamp;

                // Could update other fields based on event data
                // For example, update liquidity if we detect liquidity changes
                match event.update_type {
                    PoolUpdateType::Swap => {
                        debug!("💱 Processing swap event for pool {}", event.pool_address);
                        // Update swap-related metrics
                    }
                    PoolUpdateType::AddLiquidity | PoolUpdateType::RemoveLiquidity => {
                        debug!(
                            "💰 Processing liquidity event for pool {}",
                            event.pool_address
                        );
                        // Update liquidity-related metrics
                    }
                    PoolUpdateType::PriceUpdate => {
                        debug!("📈 Processing price update for pool {}", event.pool_address);
                        // Update price-related metrics
                    }
                    PoolUpdateType::Unknown => {
                        debug!(
                            "❓ Processing unknown event type for pool {}",
                            event.pool_address
                        );
                    }
                }
            }

            info!(
                "✅ Enhanced update completed for pool {}",
                event.pool_address
            );
        } else {
            debug!(
                "❌ Pool {} not found in monitored pools",
                event.pool_address
            );
        }

        Ok(())
    }
}

/// Statistics for pool monitoring
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitoringStats {
    pub total_monitored_pools: usize,
    pub pools_by_dex: HashMap<String, usize>,
    pub total_webhook_events: u64,
    pub active_monitoring: bool,
    pub last_event_timestamp: Option<u64>,
}

impl Default for PoolUpdateProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_processor_creation() {
        let processor = PoolUpdateProcessor::new();
        let stats = processor.get_stats().await;
        assert_eq!(stats.total_notifications, 0);
    }

    #[tokio::test]
    async fn test_stats_update() {
        let processor = PoolUpdateProcessor::new();
        processor.increment_successful_updates().await;

        let stats = processor.get_stats().await;
        assert_eq!(stats.successful_updates, 1);
    }
}
