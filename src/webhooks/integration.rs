// src/webhooks/integration.rs
// Comprehensive integration service that combines webhook management with pool discovery and monitoring

// Corrected and simplified anyhow import
use anyhow::{anyhow, Context, Result as AnyhowResult};
use crate::webhooks::{HeliusWebhookManager, WebhookServer, PoolUpdateProcessor};
use crate::webhooks::helius_sdk_stub::EnhancedTransaction; // Added import
use crate::webhooks::types::{HeliusWebhookNotification, PoolUpdateEvent, PoolUpdateType};
use crate::config::Config;
use crate::utils::{PoolInfo, DexType};
use crate::dex::{PoolValidationConfig, validate_single_pool, api::DexClient, BannedPairsManager};
use log::{info, warn, error, debug};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration, Instant};
use serde::{Deserialize, Serialize};

/// Comprehensive webhook integration service that manages both Helius webhooks and pool updates
pub struct WebhookIntegrationService {
    config: Arc<Config>,
    webhook_manager: Option<HeliusWebhookManager>,
    pool_processor: Arc<PoolUpdateProcessor>,
    notification_receiver: Option<mpsc::UnboundedReceiver<HeliusWebhookNotification>>,
    notification_sender: mpsc::UnboundedSender<HeliusWebhookNotification>,
    pool_cache: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
}

impl WebhookIntegrationService {
    /// Create a new webhook integration service
    pub fn new(config: Arc<Config>) -> Self {
        let (notification_sender, notification_receiver) = mpsc::unbounded_channel();
        let pool_processor = Arc::new(PoolUpdateProcessor::new());

        Self {
            config,
            webhook_manager: None,
            pool_processor,
            notification_receiver: Some(notification_receiver),
            notification_sender,
            pool_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the webhook service if enabled
    pub async fn initialize(&mut self) -> AnyhowResult<()> {
        if !self.config.enable_webhooks {
            info!("🔕 Webhooks disabled in configuration - using polling mode");
            return Ok(());
        }

        // Get API key from environment (assuming you add it to .env)
        let api_key = std::env::var("RPC_URL").ok()
            .and_then(|url| {
                // Extract API key from Helius URL
                if url.contains("helius-rpc.com") {
                    url.split("api-key=").nth(1).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("Helius API key not found in RPC_URL"))?;

        let webhook_url = self.config.webhook_url.clone()
            .ok_or_else(|| anyhow!("WEBHOOK_URL not configured"))?;

        // Initialize webhook manager
        let mut webhook_manager = HeliusWebhookManager::new(api_key, webhook_url);
        
        // Setup DEX webhooks
        webhook_manager.setup_dex_webhooks().await?;
        
        self.webhook_manager = Some(webhook_manager);
        info!("✅ Webhook integration service initialized successfully");

        Ok(())
    }

    /// Start the webhook server (if webhooks are enabled)
    pub async fn start_webhook_server(&self) -> AnyhowResult<()> {
        if !self.config.enable_webhooks {
            return Ok(());
        }

        let port = self.config.webhook_port.unwrap_or(8080);
        
        let server = WebhookServer::new(
            port,
            self.pool_processor.clone(),
            self.notification_sender.clone(),
        );

        info!("🚀 Starting webhook server on port {}", port);
        
        // Start server in background
        tokio::spawn(async move {
            if let Err(e) = server.start().await {
                error!("Webhook server error: {}", e);
            }
        });

        Ok(())
    }

    /// Start the notification processing loop
    pub async fn start_notification_processor(&mut self) -> AnyhowResult<()> {
        if let Some(mut receiver) = self.notification_receiver.take() {
            let processor = self.pool_processor.clone();
            let _pool_cache = self.pool_cache.clone();

            tokio::spawn(async move {
                info!("📡 Starting webhook notification processor...");
                
                while let Some(notification) = receiver.recv().await {
                    if let Err(e) = Self::process_notification_internal(&processor, &notification).await {
                        warn!("Failed to process notification: {}", e);
                    }
                }
                
                warn!("Notification processor stopped");
            });
        }

        Ok(())
    }

    /// Internal notification processing
    async fn process_notification_internal(
        processor: &Arc<PoolUpdateProcessor>,
        notification: &HeliusWebhookNotification,
    ) -> AnyhowResult<()> {
        processor.process_notification(notification).await?;
        Ok(())
    }

    /// Update the pool cache with discovered pools
    pub async fn update_pools(&self, pools: HashMap<Pubkey, Arc<PoolInfo>>) {
        // Update our internal cache
        {
            let mut cache = self.pool_cache.write().await;
            cache.extend(pools.clone());
        }

        // Update the processor cache
        self.pool_processor.update_pool_cache(pools).await;
        
        info!("Updated webhook service with {} pools", self.pool_cache.read().await.len());
    }

    /// Add a callback for pool updates
    pub async fn add_pool_update_callback<F>(&self, callback: F)
    where
        F: Fn(crate::webhooks::types::PoolUpdateEvent) + Send + Sync + 'static
    {
        self.pool_processor.add_update_callback(callback).await;
    }

    /// Get webhook statistics
    pub async fn get_stats(&self) -> WebhookStats {
        let processor_stats = self.pool_processor.get_stats().await;
        let pool_count = self.pool_cache.read().await.len();
        let webhook_count = self.webhook_manager.as_ref()
            .map(|wm| wm.get_active_webhooks().len())
            .unwrap_or(0);

        WebhookStats {
            enabled: self.config.enable_webhooks,
            active_webhooks: webhook_count,
            pools_in_cache: pool_count,
            total_notifications: processor_stats.total_notifications,
            successful_updates: processor_stats.successful_updates,
            failed_updates: processor_stats.failed_updates,
            swap_events: processor_stats.swap_events,
            liquidity_events: processor_stats.liquidity_events,
        }
    }

    /// Cleanup webhooks (useful for testing or shutdown)
    pub async fn cleanup(&mut self) -> AnyhowResult<()> {
        if let Some(webhook_manager) = &mut self.webhook_manager {
            webhook_manager.cleanup_all_webhooks().await?;
            info!("✅ Webhook cleanup completed");
        }
        Ok(())
    }

    /// Check if webhooks are enabled and working
    pub fn is_webhook_enabled(&self) -> bool {
        self.config.enable_webhooks && self.webhook_manager.is_some()
    }

    /// Get the pool cache
    pub async fn get_pool_cache(&self) -> HashMap<Pubkey, Arc<PoolInfo>> {
        self.pool_cache.read().await.clone()
    }
}

/// Comprehensive pool management service combining webhook-based real-time updates
/// with optional static discovery through DEX client APIs
pub struct IntegratedPoolService {
    config: Arc<Config>,
    
    // Static pool discovery through DEX clients
    dex_clients: Vec<Arc<dyn DexClient>>,
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

/// Pool monitoring coordinator with enhanced event processing
pub struct PoolMonitoringCoordinator {
    config: Arc<Config>,
    webhook_manager: HeliusWebhookManager,
    
    // Pool management and validation
    monitored_pools: Arc<RwLock<HashMap<Pubkey, MonitoredPool>>>,
    validation_config: PoolValidationConfig,
    banned_pairs_manager: Arc<BannedPairsManager>,
    
    // Webhook management
    active_webhooks: Arc<RwLock<HashMap<String, WebhookMetadata>>>,
    webhook_addresses: Arc<RwLock<HashSet<String>>>,
    
    // Event processing
    event_sender: mpsc::UnboundedSender<PoolEvent>,
    event_receiver: Option<mpsc::UnboundedReceiver<PoolEvent>>,
    
    // Statistics and monitoring
    stats: Arc<RwLock<PoolMonitorStats>>,
}

/// Metadata for managed webhooks
#[derive(Debug, Clone)]
pub struct WebhookMetadata {
    pub webhook_id: String,
    pub created_at: Instant,
    pub address_count: usize,
    pub dex_types: HashSet<String>,
    pub last_update: Option<Instant>,
}

/// Information about a monitored pool
#[derive(Debug, Clone)]
pub struct MonitoredPool {
    pub pool_info: Arc<PoolInfo>,
    pub webhook_id: Option<String>,
    pub last_seen: Instant,
    pub event_count: u64,
    pub dex_type: String,
}

/// Pool events from webhook notifications
#[derive(Debug)]
pub enum PoolEvent {
    PoolUpdate {
        pool_address: Pubkey,
        transaction: EnhancedTransaction, // Added transaction field
        event_type: PoolEventType,
    },
    NewPoolDetected {
        pool_address: Pubkey,
        pool_info: Arc<PoolInfo>,
    },
    PoolRemoved {
        pool_address: Pubkey,
    },
    WebhookError {
        webhook_id: String,
        error_message: String,
    },
}

/// Types of pool events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolEventType {
    Swap,
    LiquidityAdd,
    LiquidityRemove,
    PoolCreation,
    PriceUpdate,
    Unknown,
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

/// Statistics for pool monitoring
#[derive(Debug, Clone)]
pub struct PoolMonitorStats {
    pub total_pools_monitored: usize,
    pub active_webhooks: usize,
    pub total_addresses_monitored: usize,
    pub events_processed: u64,
    pub events_by_type: HashMap<String, u64>,
    pub last_event_time: Option<Instant>,
    pub uptime: Duration,
    pub start_time: Instant,
}

impl Default for PoolMonitorStats {
    fn default() -> Self {
        Self {
            total_pools_monitored: 0,
            active_webhooks: 0,
            total_addresses_monitored: 0,
            events_processed: 0,
            events_by_type: HashMap::new(),
            last_event_time: None,
            uptime: Duration::from_secs(0),
            start_time: Instant::now(),
        }
    }
}

/// Notification types for pool updates
#[derive(Debug, Clone)]
pub enum PoolUpdateNotification {
    StaticDiscovery(Vec<PoolInfo>),
    WebhookUpdate(PoolUpdateEvent),
    MergeRequest(Pubkey),
}

/// Webhook service statistics
#[derive(Debug, Clone)]
pub struct WebhookStats {
    pub enabled: bool,
    pub active_webhooks: usize,
    pub pools_in_cache: usize,
    pub total_notifications: u64,
    pub successful_updates: u64,
    pub failed_updates: u64,
    pub swap_events: u64,
    pub liquidity_events: u64,
}

/// Combined statistics for the integrated pool service
#[derive(Debug, Clone)]
pub struct IntegratedPoolStats {
    pub total_pools: usize,
    pub static_discovery: PoolUpdateStats,
    pub webhook_stats: Option<WebhookStats>,
}

impl IntegratedPoolService {
    /// Create a new integrated pool service
    pub fn new(
        config: Arc<Config>,
        dex_clients: Vec<Arc<dyn DexClient>>,
    ) -> AnyhowResult<Self> {
        let (pool_update_sender, pool_update_receiver) = mpsc::unbounded_channel();
        
        // Create webhook service if enabled
        let webhook_service = if config.enable_webhooks {
            Some(WebhookIntegrationService::new(config.clone()))
        } else {
            None
        };

        Ok(Self {
            config,
            dex_clients,
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
        info!("🔍 Running initial static pool discovery using DEX clients...");
        
        let mut all_pools = Vec::new();
        
        // Discover pools from all DEX clients directly
        for client in &self.dex_clients {
            let client_name = client.get_name();
            info!("Discovering pools from DEX: {}", client_name);
            
            match client.discover_pools().await {
                Ok(pools) => {
                    let pool_count = pools.len();
                    info!("Successfully discovered {} pools from {}", pool_count, client_name);
                    all_pools.extend(pools);
                }
                Err(e) => {
                    error!("Failed to discover pools from {}: {}", client_name, e);
                }
            }
        }
        
        let pool_count = all_pools.len();
        
        // Convert to HashMap for easier management
        let mut pool_map = HashMap::new();
        let mut stats_by_dex = HashMap::new();
        
        for pool in all_pools {
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
        let dex_clients = self.dex_clients.clone();
        let sender = self.pool_update_sender.clone();
        let refresh_interval = self.config.pool_refresh_interval_secs;
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(refresh_interval));
            
            loop {
                interval.tick().await;
                
                info!("🔄 Running periodic static pool refresh...");
                
                let mut all_pools = Vec::new();
                
                // Discover pools from all DEX clients directly
                for client in &dex_clients {
                    match client.discover_pools().await {
                        Ok(pools) => {
                            all_pools.extend(pools);
                        }
                        Err(e) => {
                            error!("Failed to discover pools from {}: {}", client.get_name(), e);
                        }
                    }
                }
                
                info!("✅ Periodic refresh discovered {} pools", all_pools.len());
                if let Err(e) = sender.send(PoolUpdateNotification::StaticDiscovery(all_pools)) {
                    error!("Failed to send static discovery notification: {}", e);
                    break;
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
    pub async fn get_pools_by_dex(&self, dex_type: &DexType) -> Vec<Arc<PoolInfo>> {
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

impl PoolMonitoringCoordinator {
    /// Create a new pool monitoring coordinator
    pub fn new(
        config: Arc<Config>,
        webhook_manager: HeliusWebhookManager,
    ) -> AnyhowResult<Self> {
        info!("🎯 Initializing Pool Monitoring Coordinator");
        
        // Create event channel
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        let stats = PoolMonitorStats {
            start_time: Instant::now(),
            ..Default::default()
        };
        
        Ok(Self {
            config,
            webhook_manager,
            monitored_pools: Arc::new(RwLock::new(HashMap::new())),
            validation_config: PoolValidationConfig::default(),
            banned_pairs_manager: Arc::new(
                BannedPairsManager::new(std::path::Path::new("banned_pairs_log.csv"))
                    .unwrap_or_else(|e| {
                        warn!("Failed to load banned pairs: {}, creating empty manager", e);
                        // Since we can't create manually due to private fields, just use the Path method
                        BannedPairsManager::new(std::path::Path::new("/dev/null")).unwrap_or_else(|_| panic!("Cannot create BannedPairsManager"))
                    })
            ),
            active_webhooks: Arc::new(RwLock::new(HashMap::new())),
            webhook_addresses: Arc::new(RwLock::new(HashSet::new())),
            event_sender,
            event_receiver: Some(event_receiver),
            stats: Arc::new(RwLock::new(stats)),
        })
    }
    
    /// Initialize the coordinator
    pub async fn initialize(&mut self) -> AnyhowResult<()> {
        info!("🚀 Initializing Pool Monitoring Coordinator...");
        
        // Start pool discovery
        info!("🔍 Starting pool discovery service...");
        
        // Get existing webhooks to understand current state
        self.sync_existing_webhooks().await?;
        
        info!("✅ Pool Monitoring Coordinator initialized successfully");
        Ok(())
    }
    
    /// Start the monitoring coordinator
    pub async fn start(&mut self) -> AnyhowResult<()> {
        info!("🎬 Starting Pool Monitoring Coordinator...");
        
        // Take the event receiver
        let event_receiver = self.event_receiver.take()
            .context("Event receiver already taken")?;
        
        // Clone necessary data for async tasks
        let stats = self.stats.clone();
        let monitored_pools = self.monitored_pools.clone();
        let config = self.config.clone();
        let validation_config = self.validation_config.clone();
        let banned_pairs_manager = self.banned_pairs_manager.clone();
        
        // Start event processing task
        let _event_processor = tokio::spawn(async move {
            Self::process_events(event_receiver, stats, monitored_pools, config, validation_config, banned_pairs_manager).await
        });
        
        // Start pool discovery and monitoring
        self.start_pool_monitoring().await?;
        
        // Start stats update task
        self.start_stats_updater().await;
        
        info!("✅ Pool Monitoring Coordinator started successfully");
        Ok(())
    }
    
    /// Sync existing webhooks to understand current state
    async fn sync_existing_webhooks(&mut self) -> AnyhowResult<()> {
        info!("🔄 Syncing existing webhooks...");
        
        let active_webhooks_map = self.webhook_manager.get_active_webhooks();
        let mut active_webhooks = self.active_webhooks.write().await;
        let mut webhook_addresses = self.webhook_addresses.write().await;
        
        for (webhook_id, webhook_url) in active_webhooks_map {
            let metadata = WebhookMetadata {
                webhook_id: webhook_id.clone(),
                created_at: Instant::now(), // We don't have actual creation time
                address_count: 0, // We don't have this information from the simple map
                dex_types: HashSet::new(), // Would need to analyze addresses
                last_update: None,
            };
            
            active_webhooks.insert(webhook_id.clone(), metadata);
            
            // Since we only have webhook URL, we can't extract addresses
            // In a real implementation, we'd need to query the webhook details
            webhook_addresses.insert(webhook_url.clone());
        }
        
        info!("✅ Synced {} existing webhooks", active_webhooks.len());
        Ok(())
    }
    
    /// Start pool monitoring based on discovered pools
    async fn start_pool_monitoring(&mut self) -> AnyhowResult<()> {
        info!("🎯 Starting pool monitoring...");
        
        // For now, we'll create a demo webhook with some test addresses
        // In a real implementation, this would be driven by pool discovery
        let demo_addresses = vec![
            "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP".to_string(), // Orca USDC-USDT
            "58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2".to_string(),  // Raydium SOL-USDC
        ];
        
        let webhook_id = self.webhook_manager
            .create_webhook(demo_addresses.clone())
            .await?;
            
        info!("✅ Created demo webhook: {}", webhook_id);
        
        // Register the webhook
        let metadata = WebhookMetadata {
            webhook_id: webhook_id.clone(),
            created_at: Instant::now(),
            address_count: demo_addresses.len(),
            dex_types: ["orca", "raydium"].iter().map(|s| s.to_string()).collect(),
            last_update: None,
        };
        
        self.active_webhooks.write().await.insert(webhook_id, metadata);
        
        for address in demo_addresses {
            self.webhook_addresses.write().await.insert(address);
        }
        
        Ok(())
    }
    
    /// Process events from webhooks
    async fn process_events(
        mut event_receiver: mpsc::UnboundedReceiver<PoolEvent>,
        stats: Arc<RwLock<PoolMonitorStats>>,
        monitored_pools: Arc<RwLock<HashMap<Pubkey, MonitoredPool>>>,
        _config: Arc<Config>,
        validation_config: PoolValidationConfig,
        banned_pairs_manager: Arc<BannedPairsManager>,
    ) {
        info!("🔄 Starting event processing loop...");
        
        while let Some(event) = event_receiver.recv().await {
            match event {
                PoolEvent::PoolUpdate { pool_address, transaction, event_type } => { // Destructure transaction
                    debug!("📊 Processing pool update for {}: {:?}", pool_address, event_type);
                    
                    // Update statistics
                    {
                        let mut stats_guard = stats.write().await;
                        stats_guard.events_processed += 1;
                        stats_guard.last_event_time = Some(Instant::now());
                        
                        let event_type_str = format!("{:?}", event_type);
                        *stats_guard.events_by_type.entry(event_type_str).or_insert(0) += 1;
                    }
                    
                    // Update monitored pool
                    {
                        let mut pools = monitored_pools.write().await;
                        if let Some(monitored_pool) = pools.get_mut(&pool_address) {
                            monitored_pool.last_seen = Instant::now();
                            monitored_pool.event_count += 1;
                        }
                    }
                    
                    // Process the actual transaction data
                    Self::process_pool_update(&pool_address, &transaction, &event_type).await; // Pass transaction
                }
                PoolEvent::NewPoolDetected { pool_address, pool_info } => {
                    info!("🆕 New pool detected: {}", pool_address);
                    
                    // Validate the pool before adding it to monitoring
                    match validate_single_pool(&pool_info, &validation_config, &banned_pairs_manager).await {
                        Ok(false) | Err(_) => {
                            warn!("🚫 Pool {} failed validation, not adding to monitoring", pool_address);
                            continue;
                        }
                        Ok(true) => {}
                    }
                    
                    info!("✅ Pool {} passed validation, adding to monitoring", pool_address);
                    let monitored_pool = MonitoredPool {
                        pool_info,
                        webhook_id: None,
                        last_seen: Instant::now(),
                        event_count: 0,
                        dex_type: "unknown".to_string(),
                    };
                    
                    monitored_pools.write().await.insert(pool_address, monitored_pool);
                    stats.write().await.total_pools_monitored += 1;
                }
                PoolEvent::PoolRemoved { pool_address } => {
                    info!("🗑️ Pool removed: {}", pool_address);
                    monitored_pools.write().await.remove(&pool_address);
                }
                PoolEvent::WebhookError { webhook_id, error_message } => {
                    error!("❌ Webhook error for {}: {}", webhook_id, error_message);
                }
            }
        }
        
        warn!("⚠️ Event processing loop ended");
    }
    
    /// Process a pool update from webhook
    async fn process_pool_update(
        pool_address: &Pubkey,
        transaction: &EnhancedTransaction, // Added transaction parameter
        event_type: &PoolEventType,
    ) {
        debug!("🔄 Processing transaction for pool {}: {:?}", pool_address, event_type);
        // Use the transaction data
        debug!("Transaction description: {}", transaction.description);
        
        // Process transaction type
        match event_type {
            PoolEventType::Swap => {
                debug!("Processing swap transaction for pool {}", pool_address);
            }
            PoolEventType::LiquidityAdd | PoolEventType::LiquidityRemove => {
                debug!("Processing liquidity change for pool {}", pool_address);
            }
            PoolEventType::PoolCreation => {
                info!("New pool creation detected: {}", pool_address);
            }
            PoolEventType::PriceUpdate => {
                debug!("Price update for pool {}", pool_address);
            }
            PoolEventType::Unknown => {
                debug!("Unknown transaction type for pool {}", pool_address);
            }
        }
    }
    
    /// Start the statistics updater
    async fn start_stats_updater(&self) {
        let stats = self.stats.clone();
        let monitored_pools = self.monitored_pools.clone();
        let active_webhooks = self.active_webhooks.clone();
        let webhook_addresses = self.webhook_addresses.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                let mut stats_guard = stats.write().await;
                let pools = monitored_pools.read().await;
                let webhooks = active_webhooks.read().await;
                let addresses = webhook_addresses.read().await;
                
                stats_guard.total_pools_monitored = pools.len();
                stats_guard.active_webhooks = webhooks.len();
                stats_guard.total_addresses_monitored = addresses.len();
                stats_guard.uptime = stats_guard.start_time.elapsed();
                
                debug!("📊 Updated stats: {} pools, {} webhooks, {} addresses", 
                    stats_guard.total_pools_monitored,
                    stats_guard.active_webhooks,
                    stats_guard.total_addresses_monitored
                );
            }
        });
    }
    
    /// Add pools to monitoring
    pub async fn add_pools_to_monitoring(&mut self, pools: Vec<Arc<PoolInfo>>) -> AnyhowResult<()> {
        info!("➕ Adding {} pools to monitoring", pools.len());
        
        let mut new_addresses = Vec::new();
        
        for pool in pools {
            let pool_address = pool.address;
            new_addresses.push(pool_address.to_string());
            
            let monitored_pool = MonitoredPool {
                pool_info: pool.clone(),
                webhook_id: None,
                last_seen: Instant::now(),
                event_count: 0,
                dex_type: format!("{:?}", pool.dex_type),
            };
            
            self.monitored_pools.write().await.insert(pool_address, monitored_pool);
        }
        
        // Add addresses to existing webhook or create new one
        let webhook_id = self.webhook_manager
            .create_webhook(new_addresses.clone())
            .await?;
            
        info!("✅ Created new webhook {} for {} addresses", webhook_id, new_addresses.len());
        
        // Update address tracking
        for address in new_addresses {
            self.webhook_addresses.write().await.insert(address);
        }
        
        Ok(())
    }
    
    /// Get current statistics
    pub async fn get_stats(&self) -> PoolMonitorStats {
        self.stats.read().await.clone()
    }
    
    /// Get monitored pools
    pub async fn get_monitored_pools(&self) -> HashMap<Pubkey, MonitoredPool> {
        self.monitored_pools.read().await.clone()
    }
    
    /// Send a pool event (for testing or external integration)
    pub fn send_event(&self, event: PoolEvent) -> AnyhowResult<()> {
        self.event_sender.send(event)
            .map_err(|e| anyhow!("Failed to send event: {}", e))
    }
    
    /// Update pool validation configuration
    pub fn set_validation_config(&mut self, config: PoolValidationConfig) {
        info!("🔧 Updating pool validation configuration");
        self.validation_config = config;
    }
    
    /// Get current pool validation configuration
    pub fn get_validation_config(&self) -> &PoolValidationConfig {
        &self.validation_config
    }
}

impl std::fmt::Display for PoolMonitorStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, 
            "Pools: {}, Webhooks: {}, Addresses: {}, Events: {}, Uptime: {:?}",
            self.total_pools_monitored,
            self.active_webhooks,
            self.total_addresses_monitored,
            self.events_processed,
            self.uptime
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn test_webhook_service_creation() {
        let config = Arc::new(Config::test_default());
        let service = WebhookIntegrationService::new(config);
        
        assert!(!service.is_webhook_enabled());
    }

    #[tokio::test]
    async fn test_webhook_stats() {
        let config = Arc::new(Config::test_default());
        let service = WebhookIntegrationService::new(config);
        let stats = service.get_stats().await;
        
        assert_eq!(stats.pools_in_cache, 0);
        assert_eq!(stats.total_notifications, 0);
    }

    #[tokio::test]
    async fn test_integrated_service_creation() {
        let config = Arc::new(Config::test_default());
        let service = IntegratedPoolService::new(config, vec![]);
        assert!(service.is_ok());
    }
    
    #[test]
    fn test_pool_event_types() {
        let event = PoolEventType::Swap;
        assert_eq!(format!("{:?}", event), "Swap");
    }
    
    #[test]
    fn test_stats_display() {
        let stats = PoolMonitorStats {
            total_pools_monitored: 10,
            active_webhooks: 2,
            total_addresses_monitored: 25,
            events_processed: 100,
            uptime: Duration::from_secs(3600),
            ..Default::default()
        };
        
        let display = format!("{}", stats);
        assert!(display.contains("Pools: 10"));
        assert!(display.contains("Webhooks: 2"));
    }
}
