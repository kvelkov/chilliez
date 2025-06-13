//! Pool Monitoring Coordinator using Helius SDK
//! 
//! This module provides a comprehensive pool monitoring system that leverages
//! the Helius SDK for ultra-fast webhook management and event processing.

use crate::helius_client::HeliusManager;
use crate::webhooks::helius_sdk::{HeliusWebhookManager, WebhookConfig};
use crate::discovery::PoolDiscoveryService;
use crate::config::Config;
use crate::utils::PoolInfo;
use helius::types::EnhancedTransaction;
use helius::Helius;
use anyhow::{Result, Context};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{interval, Duration, Instant};
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// Enhanced Pool Monitoring Coordinator using Helius SDK
pub struct PoolMonitoringCoordinator {
    config: Arc<Config>,
    helius_manager: Arc<HeliusManager>,
    webhook_manager: HeliusWebhookManager,
    
    // Pool discovery and management
    pool_discovery: Arc<PoolDiscoveryService>,
    monitored_pools: Arc<RwLock<HashMap<Pubkey, MonitoredPool>>>,
    
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
        transaction: EnhancedTransaction,
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

impl PoolMonitoringCoordinator {
    /// Create a new pool monitoring coordinator
    pub fn new(
        config: Arc<Config>,
        helius_manager: Arc<HeliusManager>,
        pool_discovery: Arc<PoolDiscoveryService>,
    ) -> Result<Self> {
        info!("🎯 Initializing Pool Monitoring Coordinator with Helius SDK");
        
        // Create webhook configuration from environment
        let webhook_config = WebhookConfig::from_env()
            .context("Failed to create webhook configuration")?;
        
        // Create a new Helius client from the manager's config
        let new_helius_client = Helius::new(
            &helius_manager.config().api_key,
            helius_manager.config().cluster.clone()
        ).context("Failed to create Helius client for webhook manager")?;
        
        // Create webhook manager
        let webhook_manager = HeliusWebhookManager::new(
            new_helius_client,
            webhook_config,
        );
        
        // Create event channel
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        let stats = PoolMonitorStats {
            start_time: Instant::now(),
            ..Default::default()
        };
        
        Ok(Self {
            config,
            helius_manager,
            webhook_manager,
            pool_discovery,
            monitored_pools: Arc::new(RwLock::new(HashMap::new())),
            active_webhooks: Arc::new(RwLock::new(HashMap::new())),
            webhook_addresses: Arc::new(RwLock::new(HashSet::new())),
            event_sender,
            event_receiver: Some(event_receiver),
            stats: Arc::new(RwLock::new(stats)),
        })
    }
    
    /// Initialize the coordinator
    pub async fn initialize(&mut self) -> Result<()> {
        info!("🚀 Initializing Pool Monitoring Coordinator...");
        
        // Start pool discovery
        info!("🔍 Starting pool discovery service...");
        // Note: Pool discovery initialization would happen here
        
        // Get existing webhooks to understand current state
        self.sync_existing_webhooks().await?;
        
        info!("✅ Pool Monitoring Coordinator initialized successfully");
        Ok(())
    }
    
    /// Start the monitoring coordinator
    pub async fn start(&mut self) -> Result<()> {
        info!("🎬 Starting Pool Monitoring Coordinator...");
        
        // Take the event receiver
        let event_receiver = self.event_receiver.take()
            .context("Event receiver already taken")?;
        
        // Clone necessary data for async tasks
        let stats = self.stats.clone();
        let monitored_pools = self.monitored_pools.clone();
        let config = self.config.clone();
        
        // Start event processing task
        let _event_processor = tokio::spawn(async move {
            Self::process_events(event_receiver, stats, monitored_pools, config).await
        });
        
        // Start pool discovery and monitoring
        self.start_pool_monitoring().await?;
        
        // Start stats update task
        self.start_stats_updater().await;
        
        info!("✅ Pool Monitoring Coordinator started successfully");
        Ok(())
    }
    
    /// Sync existing webhooks to understand current state
    async fn sync_existing_webhooks(&mut self) -> Result<()> {
        info!("🔄 Syncing existing webhooks...");
        
        match self.webhook_manager.get_all_webhooks().await {
            Ok(webhooks) => {
                let mut active_webhooks = self.active_webhooks.write().await;
                let mut webhook_addresses = self.webhook_addresses.write().await;
                
                for webhook in webhooks {
                    let metadata = WebhookMetadata {
                        webhook_id: webhook.webhook_id.clone(),
                        created_at: Instant::now(), // We don't have actual creation time
                        address_count: webhook.account_addresses.len(),
                        dex_types: HashSet::new(), // Would need to analyze addresses
                        last_update: None,
                    };
                    
                    active_webhooks.insert(webhook.webhook_id.clone(), metadata);
                    
                    for address in webhook.account_addresses {
                        webhook_addresses.insert(address);
                    }
                }
                
                info!("✅ Synced {} existing webhooks", active_webhooks.len());
                Ok(())
            }
            Err(e) => {
                warn!("⚠️ Failed to sync existing webhooks: {}", e);
                // Continue without existing webhooks
                Ok(())
            }
        }
    }
    
    /// Start pool monitoring based on discovered pools
    async fn start_pool_monitoring(&mut self) -> Result<()> {
        info!("🎯 Starting pool monitoring...");
        
        // For now, we'll create a demo webhook with some test addresses
        // In a real implementation, this would be driven by pool discovery
        let demo_addresses = vec![
            "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP".to_string(), // Orca USDC-USDT
            "58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2".to_string(),  // Raydium SOL-USDC
        ];
        
        let webhook_id = self.webhook_manager
            .create_dex_pool_webhook(demo_addresses.clone(), Some("demo".to_string()))
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
    ) {
        info!("🔄 Starting event processing loop...");
        
        while let Some(event) = event_receiver.recv().await {
            match event {
                PoolEvent::PoolUpdate { pool_address, transaction, event_type } => {
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
                    Self::process_pool_transaction(&pool_address, &transaction, &event_type).await;
                }
                PoolEvent::NewPoolDetected { pool_address, pool_info } => {
                    info!("🆕 New pool detected: {}", pool_address);
                    
                    let monitored_pool = MonitoredPool {
                        pool_info,
                        webhook_id: None,
                        last_seen: Instant::now(),
                        event_count: 0,
                        dex_type: "unknown".to_string(),
                    };
                    
                    monitored_pools.write().await.insert(pool_address, monitored_pool);
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
    
    /// Process a pool transaction from webhook
    async fn process_pool_transaction(
        pool_address: &Pubkey,
        transaction: &EnhancedTransaction,
        event_type: &PoolEventType,
    ) {
        debug!("🔄 Processing transaction for pool {}: {:?}", pool_address, event_type);
        
        // Extract relevant information from the enhanced transaction
        debug!("Transaction description: {}", transaction.description);
        
        // Process transaction type
        match event_type {
            PoolEventType::Swap => {
                debug!("Processing swap transaction for pool {}", pool_address);
                // Extract swap details from enhanced transaction
            }
            PoolEventType::LiquidityAdd | PoolEventType::LiquidityRemove => {
                debug!("Processing liquidity change for pool {}", pool_address);
                // Extract liquidity change details
            }
            PoolEventType::PoolCreation => {
                info!("New pool creation detected: {}", pool_address);
                // Handle new pool registration
            }
            PoolEventType::PriceUpdate => {
                debug!("Price update for pool {}", pool_address);
                // Update pool price information
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
    pub async fn add_pools_to_monitoring(&mut self, pools: Vec<Arc<PoolInfo>>) -> Result<()> {
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
        if let Some((webhook_id, _)) = self.active_webhooks.read().await.iter().next() {
            // Add to existing webhook
            self.webhook_manager
                .add_addresses_to_webhook(webhook_id, new_addresses.clone())
                .await?;
                
            info!("✅ Added {} addresses to existing webhook {}", new_addresses.len(), webhook_id);
        } else {
            // Create new webhook
            let webhook_id = self.webhook_manager
                .create_dex_pool_webhook(new_addresses.clone(), None)
                .await?;
                
            info!("✅ Created new webhook {} for {} addresses", webhook_id, new_addresses.len());
        }
        
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
    pub fn send_event(&self, event: PoolEvent) -> Result<()> {
        self.event_sender.send(event)
            .map_err(|e| anyhow::anyhow!("Failed to send event: {}", e))
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
