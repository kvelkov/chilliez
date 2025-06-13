// src/webhooks/pool_integration.rs
//! Integration service that combines static pool discovery with real-time webhook updates

use crate::webhooks::{WebhookIntegrationService, PoolUpdateEvent, PoolUpdateType};
use crate::discovery::PoolDiscoveryService;
use crate::config::Config;
use crate::utils::PoolInfo;
use crate::dex::quote::DexClient;
use anyhow::{Result as AnyhowResult};
use log::{info, warn, error, debug};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{interval, Duration, Instant};

/// Comprehensive pool management service combining static discovery with real-time updates
pub struct IntegratedPoolService {
    config: Arc<Config>,
    
    // Static pool discovery
    pool_discovery: Arc<PoolDiscoveryService>,
    discovered_pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    
    // Real-time webhook updates
    webhook_service: Option<WebhookIntegrationService>,
    
    // Combined pool management
    master_pool_cache: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    pool_update_stats: Arc<RwLock<PoolUpdateStats>>,
    
    // Communication channels
    pool_update_sender: mpsc::UnboundedSender<PoolUpdateNotification>,
    pool_update_receiver: Option<mpsc::UnboundedReceiver<PoolUpdateNotification>>,
}

/// Statistics for the integrated pool service
#[derive(Debug, Clone, Default)]
pub struct PoolUpdateStats {
    pub total_static_pools: usize,
    pub total_webhook_updates: u64,
    pub successful_merges: u64,
    pub failed_merges: u64,
    pub last_static_refresh: Option<Instant>,
    pub last_webhook_update: Option<Instant>,
    pub pools_by_dex: HashMap<String, usize>,
}

/// Notification types for pool updates
#[derive(Debug, Clone)]
pub enum PoolUpdateNotification {
    StaticDiscovery(Vec<PoolInfo>),
    WebhookUpdate(PoolUpdateEvent),
    MergeRequest(Pubkey),
}

impl IntegratedPoolService {
    /// Create a new integrated pool service
    pub fn new(
        config: Arc<Config>,
        dex_clients: Vec<Arc<dyn DexClient>>,
    ) -> AnyhowResult<Self> {
        let (pool_update_sender, pool_update_receiver) = mpsc::unbounded_channel();
        
        // Create pool discovery service
        let (discovery_sender, _discovery_receiver) = mpsc::channel(1000);
        let pool_discovery = Arc::new(PoolDiscoveryService::new(
            dex_clients,
            discovery_sender,
            100, // batch_size
            300, // max_pool_age_secs
            100, // delay_between_batches_ms
        ));
        
        // Create webhook service if enabled
        let webhook_service = if config.enable_webhooks {
            Some(WebhookIntegrationService::new(config.clone()))
        } else {
            None
        };

        Ok(Self {
            config,
            pool_discovery,
            discovered_pools: Arc::new(RwLock::new(HashMap::new())),
            webhook_service,
            master_pool_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_stats: Arc::new(RwLock::new(PoolUpdateStats::default())),
            pool_update_sender,
            pool_update_receiver: Some(pool_update_receiver),
        })
    }

    /// Initialize the integrated service
    pub async fn initialize(&mut self) -> AnyhowResult<()> {
        info!("🚀 Initializing Integrated Pool Service...");

        // Initialize webhook service if enabled
        if let Some(webhook_service) = &mut self.webhook_service {
            info!("📡 Initializing webhook service...");
            webhook_service.initialize().await?;
            webhook_service.start_notification_processor().await?;
            webhook_service.start_webhook_server().await?;
            
            // Register callback for webhook updates
            let sender = self.pool_update_sender.clone();
            webhook_service.add_pool_update_callback(move |event| {
                if let Err(e) = sender.send(PoolUpdateNotification::WebhookUpdate(event)) {
                    error!("Failed to send webhook update notification: {}", e);
                }
            }).await;
            
            info!("✅ Webhook service initialized and ready");
        } else {
            info!("🔕 Webhook service disabled - using polling mode only");
        }

        info!("✅ Integrated Pool Service initialized successfully");
        Ok(())
    }

    /// Start the integrated service with both static discovery and webhook processing
    pub async fn start(&mut self) -> AnyhowResult<()> {
        info!("🚀 Starting Integrated Pool Service...");

        // Start initial static pool discovery
        self.run_initial_pool_discovery().await?;

        // Start the notification processor
        self.start_notification_processor().await?;

        // Start periodic static pool refresh
        self.start_periodic_static_refresh().await?;

        info!("✅ Integrated Pool Service started successfully");
        Ok(())
    }

    /// Run initial static pool discovery to populate the base pool set
    async fn run_initial_pool_discovery(&self) -> AnyhowResult<()> {
        info!("🔍 Running initial static pool discovery...");
        
        let pools = self.pool_discovery.run_discovery_cycle().await?;
        let pool_count = pools.len();
        
        // Convert to HashMap for easier management
        let mut pool_map = HashMap::new();
        let mut stats_by_dex = HashMap::new();
        
        for pool in pools {
            let dex_name = format!("{:?}", pool.dex_type);
            *stats_by_dex.entry(dex_name).or_insert(0) += 1;
            pool_map.insert(pool.address, Arc::new(pool));
        }

        // Update discovered pools cache
        {
            let mut discovered = self.discovered_pools.write().await;
            *discovered = pool_map.clone();
        }

        // Update master cache
        {
            let mut master = self.master_pool_cache.write().await;
            master.extend(pool_map);
        }

        // Update stats
        {
            let mut stats = self.pool_update_stats.write().await;
            stats.total_static_pools = pool_count;
            stats.last_static_refresh = Some(Instant::now());
            stats.pools_by_dex = stats_by_dex;
        }

        // Notify webhook service about discovered pools
        if let Some(webhook_service) = &self.webhook_service {
            let pools_for_webhook: HashMap<Pubkey, Arc<PoolInfo>> = self.master_pool_cache.read().await.clone();
            webhook_service.update_pools(pools_for_webhook).await;
        }

        info!("✅ Initial static pool discovery completed: {} pools", pool_count);
        Ok(())
    }

    /// Start the notification processor for handling updates
    async fn start_notification_processor(&mut self) -> AnyhowResult<()> {
        if let Some(receiver) = self.pool_update_receiver.take() {
            let master_cache = self.master_pool_cache.clone();
            let stats = self.pool_update_stats.clone();
            
            tokio::spawn(async move {
                Self::notification_processor_loop(receiver, master_cache, stats).await;
            });
            
            info!("✅ Notification processor started");
        }
        
        Ok(())
    }

    /// Start periodic static pool refresh
    async fn start_periodic_static_refresh(&self) -> AnyhowResult<()> {
        let pool_discovery = self.pool_discovery.clone();
        let sender = self.pool_update_sender.clone();
        let refresh_interval = self.config.pool_refresh_interval_secs;
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(refresh_interval));
            
            loop {
                interval.tick().await;
                
                info!("🔄 Running periodic static pool refresh...");
                match pool_discovery.run_discovery_cycle().await {
                    Ok(pools) => {
                        info!("✅ Periodic refresh discovered {} pools", pools.len());
                        if let Err(e) = sender.send(PoolUpdateNotification::StaticDiscovery(pools)) {
                            error!("Failed to send static discovery notification: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("❌ Periodic refresh failed: {}", e);
                    }
                }
            }
        });
        
        info!("✅ Periodic static refresh started (every {} seconds)", refresh_interval);
        Ok(())
    }

    /// Main notification processing loop
    async fn notification_processor_loop(
        mut receiver: mpsc::UnboundedReceiver<PoolUpdateNotification>,
        master_cache: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        stats: Arc<RwLock<PoolUpdateStats>>,
    ) {
        info!("📡 Notification processor loop started");
        
        while let Some(notification) = receiver.recv().await {
            match notification {
                PoolUpdateNotification::StaticDiscovery(pools) => {
                    Self::handle_static_discovery(pools, &master_cache, &stats).await;
                }
                PoolUpdateNotification::WebhookUpdate(event) => {
                    Self::handle_webhook_update(event, &master_cache, &stats).await;
                }
                PoolUpdateNotification::MergeRequest(pool_address) => {
                    Self::handle_merge_request(pool_address, &master_cache, &stats).await;
                }
            }
        }
        
        warn!("📡 Notification processor loop ended");
    }

    /// Handle static pool discovery updates
    async fn handle_static_discovery(
        pools: Vec<PoolInfo>,
        master_cache: &Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        stats: &Arc<RwLock<PoolUpdateStats>>,
    ) {
        debug!("🔍 Processing static discovery update: {} pools", pools.len());
        
        let mut updated_count = 0;
        let mut new_count = 0;
        
        {
            let mut cache = master_cache.write().await;
            
            for pool in pools {
                let pool_address = pool.address;
                
                if cache.contains_key(&pool_address) {
                    // Update existing pool
                    cache.insert(pool_address, Arc::new(pool));
                    updated_count += 1;
                } else {
                    // Add new pool
                    cache.insert(pool_address, Arc::new(pool));
                    new_count += 1;
                }
            }
        }
        
        // Update stats
        {
            let mut stats_guard = stats.write().await;
            stats_guard.last_static_refresh = Some(Instant::now());
            stats_guard.total_static_pools = master_cache.read().await.len();
            stats_guard.successful_merges += 1;
        }
        
        info!("✅ Static discovery processed: {} new, {} updated pools", new_count, updated_count);
    }

    /// Handle real-time webhook updates
    async fn handle_webhook_update(
        event: PoolUpdateEvent,
        master_cache: &Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        stats: &Arc<RwLock<PoolUpdateStats>>,
    ) {
        debug!("📡 Processing webhook update for pool: {}", event.pool_address);
        
        // Update the pool's timestamp if we have it in cache
        {
            let mut cache = master_cache.write().await;
            if let Some(pool_arc) = cache.get_mut(&event.pool_address) {
                if let Some(pool) = Arc::get_mut(pool_arc) {
                    pool.last_update_timestamp = event.timestamp;
                    
                    // Could add more sophisticated updates based on event type
                    match event.update_type {
                        PoolUpdateType::Swap => {
                            debug!("💱 Swap detected in pool {}", event.pool_address);
                        }
                        PoolUpdateType::AddLiquidity | PoolUpdateType::RemoveLiquidity => {
                            debug!("💰 Liquidity change in pool {}", event.pool_address);
                        }
                        _ => {}
                    }
                }
            }
        }
        
        // Update stats
        {
            let mut stats_guard = stats.write().await;
            stats_guard.total_webhook_updates += 1;
            stats_guard.last_webhook_update = Some(Instant::now());
        }
        
        debug!("✅ Webhook update processed for pool: {}", event.pool_address);
    }

    /// Handle merge requests (for future use)
    async fn handle_merge_request(
        _pool_address: Pubkey,
        _master_cache: &Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        _stats: &Arc<RwLock<PoolUpdateStats>>,
    ) {
        debug!("🔗 Processing merge request (not implemented yet)");
    }

    /// Get the current pool cache
    pub async fn get_pools(&self) -> HashMap<Pubkey, Arc<PoolInfo>> {
        self.master_pool_cache.read().await.clone()
    }

    /// Get service statistics
    pub async fn get_stats(&self) -> IntegratedPoolStats {
        let pool_stats = self.pool_update_stats.read().await.clone();
        let pool_count = self.master_pool_cache.read().await.len();
        
        let webhook_stats = if let Some(webhook_service) = &self.webhook_service {
            Some(webhook_service.get_stats().await)
        } else {
            None
        };
        
        IntegratedPoolStats {
            total_pools: pool_count,
            static_discovery: pool_stats,
            webhook_stats,
        }
    }

    /// Get pools filtered by DEX type
    pub async fn get_pools_by_dex(&self, dex_type: &crate::utils::DexType) -> Vec<Arc<PoolInfo>> {
        self.master_pool_cache
            .read()
            .await
            .values()
            .filter(|pool| pool.dex_type == *dex_type)
            .cloned()
            .collect()
    }

    /// Get the most recently updated pools
    pub async fn get_recently_updated_pools(&self, limit: usize) -> Vec<Arc<PoolInfo>> {
        let mut pools: Vec<_> = self.master_pool_cache.read().await.values().cloned().collect();
        pools.sort_by(|a, b| b.last_update_timestamp.cmp(&a.last_update_timestamp));
        pools.truncate(limit);
        pools
    }
}

/// Combined statistics for the integrated pool service
#[derive(Debug, Clone)]
pub struct IntegratedPoolStats {
    pub total_pools: usize,
    pub static_discovery: PoolUpdateStats,
    pub webhook_stats: Option<crate::webhooks::WebhookStats>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn test_integrated_service_creation() {
        let config = Arc::new(Config::test_default());
        let service = IntegratedPoolService::new(config, vec![]);
        assert!(service.is_ok());
    }
}
