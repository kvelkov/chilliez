// src/solana/event_driven_balance.rs
//! Event-driven balance monitoring that reacts to webhook events instead of polling

use crate::solana::balance_monitor::{BalanceMonitor, BalanceMonitorConfig, BalanceUpdate};
use crate::webhooks::processor::PoolUpdateProcessor;
use crate::webhooks::types::{PoolUpdateEvent, PoolUpdateType, HeliusWebhookNotification};
use anyhow::{anyhow, Result};
use log::{debug, info, warn, error};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::{mpsc, RwLock};

/// Configuration for event-driven balance monitoring
#[derive(Debug, Clone)]
pub struct EventDrivenBalanceConfig {
    pub base_config: BalanceMonitorConfig,
    pub monitored_accounts: HashSet<Pubkey>,
    pub track_token_accounts: bool,
    pub balance_change_threshold: u64, // Minimum balance change to trigger update
    pub enable_webhook_integration: bool,
}

impl Default for EventDrivenBalanceConfig {
    fn default() -> Self {
        Self {
            base_config: BalanceMonitorConfig::default(),
            monitored_accounts: HashSet::new(),
            track_token_accounts: true,
            balance_change_threshold: 1000, // 0.000001 SOL minimum
            enable_webhook_integration: true,
        }
    }
}

/// Event-driven balance monitor that reacts to webhook events
pub struct EventDrivenBalanceMonitor {
    balance_monitor: BalanceMonitor,
    config: EventDrivenBalanceConfig,
    
    // Event processing
    event_sender: mpsc::UnboundedSender<BalanceEventTrigger>,
    event_receiver: Option<mpsc::UnboundedReceiver<BalanceEventTrigger>>,
    
    // Account tracking
    monitored_accounts: Arc<RwLock<HashSet<Pubkey>>>,
    account_balance_cache: Arc<RwLock<HashMap<Pubkey, u64>>>,
    
    // Statistics
    event_stats: Arc<RwLock<EventStats>>,
}

/// Internal event that triggers balance updates
#[derive(Debug, Clone)]
pub enum BalanceEventTrigger {
    WebhookEvent {
        notification: HeliusWebhookNotification,
        pool_event: PoolUpdateEvent,
    },
    DirectBalanceChange {
        account: Pubkey,
        new_balance: u64,
        change_amount: i64,
        trigger_source: String,
    },
    ForceRefresh {
        accounts: Vec<Pubkey>,
    },
}

/// Statistics for event-driven balance monitoring
#[derive(Debug, Clone, Default)]
pub struct EventStats {
    pub total_webhook_events: u64,
    pub balance_triggering_events: u64,
    pub native_transfer_events: u64,
    pub token_transfer_events: u64,
    pub balance_updates_processed: u64,
    pub last_event_timestamp: u64,
}

impl EventDrivenBalanceMonitor {
    /// Create a new event-driven balance monitor
    pub fn new(config: EventDrivenBalanceConfig) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        Self {
            balance_monitor: BalanceMonitor::new(config.base_config.clone()),
            config: config.clone(),
            event_sender,
            event_receiver: Some(event_receiver),
            monitored_accounts: Arc::new(RwLock::new(config.monitored_accounts)),
            account_balance_cache: Arc::new(RwLock::new(HashMap::new())),
            event_stats: Arc::new(RwLock::new(EventStats::default())),
        }
    }

    /// Start the event-driven balance monitor
    pub async fn start(&mut self) -> Result<()> {
        info!("🚀 Starting event-driven balance monitor");
        
        // Start the underlying balance monitor
        self.balance_monitor.start().await?;
        
        // Subscribe to accounts via WebSocket for fallback
        self.subscribe_to_monitored_accounts().await?;
        
        // Start event processing task
        self.spawn_event_processor().await?;
        
        info!("✅ Event-driven balance monitor started successfully");
        Ok(())
    }

    /// Register this monitor with a webhook processor
    pub async fn register_with_webhook_processor(&self, processor: &PoolUpdateProcessor) -> Result<()> {
        if !self.config.enable_webhook_integration {
            info!("Webhook integration disabled in config");
            return Ok(());
        }

        let event_sender = self.event_sender.clone();
        let monitored_accounts = Arc::clone(&self.monitored_accounts);
        
        // Register callback for pool update events
        processor.add_update_callback(move |pool_event: PoolUpdateEvent| {
            let event_sender = event_sender.clone();
            let monitored_accounts = Arc::clone(&monitored_accounts);
            
            tokio::spawn(async move {
                // Check if this event affects our monitored accounts
                if Self::should_process_pool_event(&pool_event, &monitored_accounts).await {
                    // We need the original notification to extract balance changes
                    // For now, we'll create a trigger event with the pool info
                    let trigger = BalanceEventTrigger::DirectBalanceChange {
                        account: pool_event.pool_address,
                        new_balance: 0, // Will be updated by WebSocket or direct query
                        change_amount: 0, // Will be calculated
                        trigger_source: format!("pool_event_{:?}", pool_event.update_type),
                    };
                    
                    if let Err(e) = event_sender.send(trigger) {
                        warn!("Failed to send balance event trigger: {}", e);
                    }
                }
            });
        }).await;

        info!("✅ Registered with webhook processor for balance events");
        Ok(())
    }

    /// Register a direct webhook notification handler
    pub async fn register_webhook_notification_handler(&self) -> mpsc::UnboundedSender<HeliusWebhookNotification> {
        let (notification_sender, mut notification_receiver) = mpsc::unbounded_channel::<HeliusWebhookNotification>();
        let event_sender = self.event_sender.clone();
        let monitored_accounts = Arc::clone(&self.monitored_accounts);
        let stats = Arc::clone(&self.event_stats);
        
        tokio::spawn(async move {
            while let Some(notification) = notification_receiver.recv().await {
                // Update stats
                {
                    let mut stats = stats.write().await;
                    stats.total_webhook_events += 1;
                    stats.last_event_timestamp = notification.timestamp;
                }

                // Process webhook notification for balance changes
                if let Some(balance_triggers) = Self::extract_balance_triggers(&notification, &monitored_accounts).await {
                    for trigger in balance_triggers {
                        if let Err(e) = event_sender.send(trigger) {
                            warn!("Failed to send webhook balance trigger: {}", e);
                        }
                    }
                }
            }
        });

        notification_sender
    }

    /// Add accounts to monitor
    pub async fn add_monitored_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        let mut monitored = self.monitored_accounts.write().await;
        for account in accounts {
            monitored.insert(account);
            
            // Subscribe to account via WebSocket for real-time updates
            // Note: This would require access to the BalanceMonitor's subscribe method
            // We'll handle this in the integration
        }
        
        info!("Added {} accounts to monitoring", monitored.len());
        Ok(())
    }

    /// Get current balance for an account
    pub async fn get_account_balance(&self, account: Pubkey) -> Option<u64> {
        // Try cache first
        {
            let cache = self.account_balance_cache.read().await;
            if let Some(&balance) = cache.get(&account) {
                return Some(balance);
            }
        }
        
        // Fallback to balance monitor
        if let Some((balance, _)) = self.balance_monitor.get_confirmed_balance(account).await {
            return Some(balance);
        }
        
        None
    }

    /// Force refresh balances for specific accounts
    pub async fn force_refresh_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        let trigger = BalanceEventTrigger::ForceRefresh { accounts };
        self.event_sender.send(trigger)
            .map_err(|e| anyhow!("Failed to send force refresh trigger: {}", e))?;
        Ok(())
    }

    /// Get event statistics
    pub async fn get_stats(&self) -> EventStats {
        self.event_stats.read().await.clone()
    }

    /// Take the balance update receiver from the underlying monitor
    pub fn take_balance_update_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<BalanceUpdate>> {
        self.balance_monitor.take_balance_update_receiver()
    }

    // Private methods

    /// Subscribe to monitored accounts via WebSocket
    async fn subscribe_to_monitored_accounts(&mut self) -> Result<()> {
        let accounts: Vec<Pubkey> = {
            let monitored = self.monitored_accounts.read().await;
            monitored.iter().cloned().collect()
        };

        for account in accounts {
            if let Err(e) = self.balance_monitor.subscribe_to_account(account).await {
                warn!("Failed to subscribe to account {}: {}", account, e);
            }
        }

        Ok(())
    }

    /// Spawn the event processor task
    async fn spawn_event_processor(&mut self) -> Result<()> {
        let mut event_receiver = self.event_receiver.take()
            .ok_or_else(|| anyhow!("Event receiver already taken"))?;
        
        let account_cache = Arc::clone(&self.account_balance_cache);
        let stats = Arc::clone(&self.event_stats);
        let config = self.config.clone();
        
        tokio::spawn(async move {
            while let Some(event) = event_receiver.recv().await {
                if let Err(e) = Self::process_balance_event(event, &account_cache, &stats, &config).await {
                    error!("Failed to process balance event: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Check if a pool event should trigger balance processing
    async fn should_process_pool_event(
        pool_event: &PoolUpdateEvent, 
        monitored_accounts: &Arc<RwLock<HashSet<Pubkey>>>
    ) -> bool {
        match pool_event.update_type {
            PoolUpdateType::Swap | PoolUpdateType::AddLiquidity | PoolUpdateType::RemoveLiquidity => {
                // Check if any token transfers involve monitored accounts
                let accounts = monitored_accounts.read().await;
                
                for transfer in &pool_event.token_transfers {
                    if let (Ok(from_account), Ok(to_account)) = (
                        Pubkey::from_str(&transfer.from_user_account),
                        Pubkey::from_str(&transfer.to_user_account)
                    ) {
                        if accounts.contains(&from_account) || accounts.contains(&to_account) {
                            return true;
                        }
                    }
                }
                
                false
            }
            _ => false,
        }
    }

    /// Extract balance triggers from webhook notification
    async fn extract_balance_triggers(
        notification: &HeliusWebhookNotification,
        monitored_accounts: &Arc<RwLock<HashSet<Pubkey>>>
    ) -> Option<Vec<BalanceEventTrigger>> {
        let mut triggers = Vec::new();
        let accounts = monitored_accounts.read().await;

        // Process native transfers
        if let Some(ref native_transfers) = notification.native_transfers {
            for transfer in native_transfers {
                if let (Ok(from_account), Ok(to_account)) = (
                    Pubkey::from_str(&transfer.from_user_account),
                    Pubkey::from_str(&transfer.to_user_account)
                ) {
                    if accounts.contains(&from_account) {
                        triggers.push(BalanceEventTrigger::DirectBalanceChange {
                            account: from_account,
                            new_balance: 0, // Will be queried
                            change_amount: -(transfer.amount as i64),
                            trigger_source: "native_transfer_out".to_string(),
                        });
                    }
                    
                    if accounts.contains(&to_account) {
                        triggers.push(BalanceEventTrigger::DirectBalanceChange {
                            account: to_account,
                            new_balance: 0, // Will be queried
                            change_amount: transfer.amount as i64,
                            trigger_source: "native_transfer_in".to_string(),
                        });
                    }
                }
            }
        }

        // Process account data changes
        if let Some(ref account_data) = notification.account_data {
            for data in account_data {
                if let Ok(account) = Pubkey::from_str(&data.account) {
                    if accounts.contains(&account) && data.native_balance_change != 0 {
                        triggers.push(BalanceEventTrigger::DirectBalanceChange {
                            account,
                            new_balance: 0, // Will be queried
                            change_amount: data.native_balance_change,
                            trigger_source: "account_data_change".to_string(),
                        });
                    }
                }
            }
        }

        if triggers.is_empty() {
            None
        } else {
            Some(triggers)
        }
    }

    /// Process a balance event trigger
    async fn process_balance_event(
        event: BalanceEventTrigger,
        _account_cache: &Arc<RwLock<HashMap<Pubkey, u64>>>,
        stats: &Arc<RwLock<EventStats>>,
        config: &EventDrivenBalanceConfig,
    ) -> Result<()> {
        match event {
            BalanceEventTrigger::DirectBalanceChange { 
                account, 
                new_balance: _, 
                change_amount, 
                trigger_source 
            } => {
                // Update stats
                {
                    let mut stats = stats.write().await;
                    stats.balance_triggering_events += 1;
                    if trigger_source.contains("native_transfer") {
                        stats.native_transfer_events += 1;
                    } else if trigger_source.contains("token_transfer") {
                        stats.token_transfer_events += 1;
                    }
                }

                // Check if change meets threshold
                if change_amount.abs() as u64 >= config.balance_change_threshold {
                    debug!("🏦 Significant balance change detected for {}: {} ({})", 
                        account, change_amount, trigger_source);
                    
                    // Update cache (we'd need to query the actual balance here)
                    // For now, we'll just log the event
                    info!("💰 Balance event: {} changed by {} lamports ({})", 
                        account, change_amount, trigger_source);
                }
            }
            
            BalanceEventTrigger::ForceRefresh { accounts } => {
                info!("🔄 Force refreshing balances for {} accounts", accounts.len());
                // Implementation would query actual balances and update cache
            }
            
            BalanceEventTrigger::WebhookEvent { notification: _, pool_event } => {
                debug!("📨 Processing webhook pool event: {:?}", pool_event.update_type);
                // Process pool event for balance implications
            }
        }

        Ok(())
    }
}
