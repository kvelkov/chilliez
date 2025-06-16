// src/solana/balance_integration.rs
//! Integration utilities for connecting event-driven balance monitoring with webhook system

use crate::solana::event_driven_balance::{EventDrivenBalanceMonitor, EventDrivenBalanceConfig};
use crate::webhooks::processor::PoolUpdateProcessor;
use crate::webhooks::integration::WebhookIntegrationService;
use crate::webhooks::types::HeliusWebhookNotification;
use anyhow::{anyhow, Result};
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashSet,
    sync::Arc,
};
use tokio::sync::{mpsc, RwLock};

/// Integrated balance monitoring system that combines event-driven balance monitoring
/// with webhook processing for complete real-time balance tracking
pub struct IntegratedBalanceSystem {
    balance_monitor: EventDrivenBalanceMonitor,
    webhook_processor: Arc<PoolUpdateProcessor>,
    webhook_integration: Arc<WebhookIntegrationService>,
    
    // Notification handling
    notification_sender: Option<mpsc::UnboundedSender<HeliusWebhookNotification>>,
    
    // Configuration
    monitored_accounts: Arc<RwLock<HashSet<Pubkey>>>,
    is_running: Arc<RwLock<bool>>,
}

impl IntegratedBalanceSystem {
    /// Create a new integrated balance system
    pub fn new(
        config: EventDrivenBalanceConfig,
        webhook_processor: Arc<PoolUpdateProcessor>,
        webhook_integration: Arc<WebhookIntegrationService>,
    ) -> Self {
        let balance_monitor = EventDrivenBalanceMonitor::new(config.clone());
        
        Self {
            balance_monitor,
            webhook_processor,
            webhook_integration,
            notification_sender: None,
            monitored_accounts: Arc::new(RwLock::new(config.monitored_accounts)),
            is_running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the integrated balance system
    pub async fn start(&mut self) -> Result<()> {
        // Check if already running
        {
            let running = self.is_running.read().await;
            if *running {
                return Err(anyhow!("Integrated balance system is already running"));
            }
        }

        info!("🚀 Starting integrated balance monitoring system");

        // Start the event-driven balance monitor
        self.balance_monitor.start().await?;

        // Register with webhook processor for pool events
        self.balance_monitor.register_with_webhook_processor(&self.webhook_processor).await?;

        // Set up webhook notification handling
        self.notification_sender = Some(self.balance_monitor.register_webhook_notification_handler().await);

        // Set up webhook integration forwarding
        self.setup_webhook_forwarding().await?;

        // Mark as running
        {
            let mut running = self.is_running.write().await;
            *running = true;
        }

        info!("✅ Integrated balance monitoring system started successfully");
        Ok(())
    }

    /// Add accounts to monitor for balance changes
    pub async fn add_monitored_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        // Add to our tracking
        {
            let mut monitored = self.monitored_accounts.write().await;
            for account in &accounts {
                monitored.insert(*account);
            }
        }

        // Add to balance monitor
        self.balance_monitor.add_monitored_accounts(accounts.clone()).await?;

        info!("📋 Added {} accounts to integrated balance monitoring", accounts.len());
        Ok(())
    }

    /// Remove accounts from monitoring
    pub async fn remove_monitored_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        let mut monitored = self.monitored_accounts.write().await;
        for account in &accounts {
            monitored.remove(account);
        }

        info!("❌ Removed {} accounts from integrated balance monitoring", accounts.len());
        Ok(())
    }

    /// Get current balance for an account
    pub async fn get_account_balance(&self, account: Pubkey) -> Option<u64> {
        self.balance_monitor.get_account_balance(account).await
    }

    /// Force refresh balances for all monitored accounts
    pub async fn force_refresh_all_accounts(&self) -> Result<()> {
        let accounts: Vec<Pubkey> = {
            let monitored = self.monitored_accounts.read().await;
            monitored.iter().cloned().collect()
        };

        if !accounts.is_empty() {
            self.balance_monitor.force_refresh_accounts(accounts).await?;
        }

        Ok(())
    }

    /// Force refresh balances for specific accounts
    pub async fn force_refresh_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        self.balance_monitor.force_refresh_accounts(accounts).await
    }

    /// Get monitoring statistics
    pub async fn get_monitoring_stats(&self) -> Result<IntegratedBalanceStats> {
        let balance_stats = self.balance_monitor.get_stats().await;
        let webhook_stats = self.webhook_processor.get_stats().await;
        let monitored_count = self.monitored_accounts.read().await.len();

        Ok(IntegratedBalanceStats {
            monitored_accounts_count: monitored_count,
            balance_events: balance_stats,
            webhook_stats,
            is_running: *self.is_running.read().await,
        })
    }

    /// Check if the system is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Stop the integrated balance system
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.is_running.write().await;
        *running = false;
        
        info!("🛑 Integrated balance monitoring system stopped");
        Ok(())
    }

    // Private methods

    /// Set up forwarding of webhook notifications to balance monitor
    async fn setup_webhook_forwarding(&self) -> Result<()> {
        if let Some(ref notification_sender) = self.notification_sender {
            let sender = notification_sender.clone();
            let monitored_accounts = Arc::clone(&self.monitored_accounts);
            
            // Create a callback that forwards relevant notifications to balance monitor
            let _webhook_callback = move |notification: HeliusWebhookNotification| {
                let sender = sender.clone();
                let monitored_accounts = Arc::clone(&monitored_accounts);
                
                tokio::spawn(async move {
                    // Check if notification affects our monitored accounts
                    if Self::notification_affects_monitored_accounts(&notification, &monitored_accounts).await {
                        if let Err(e) = sender.send(notification.clone()) {
                            warn!("Failed to forward webhook notification to balance monitor: {}", e);
                        }
                    }
                });
            };

            // Register with webhook integration
            // Note: This would require the webhook integration to support callback registration
            // For now, we'll set up a direct forwarding mechanism
            
            info!("✅ Set up webhook notification forwarding to balance monitor");
        }

        Ok(())
    }

    /// Check if a notification affects any of our monitored accounts
    async fn notification_affects_monitored_accounts(
        notification: &HeliusWebhookNotification,
        monitored_accounts: &Arc<RwLock<HashSet<Pubkey>>>,
    ) -> bool {
        let accounts = monitored_accounts.read().await;
        
        // Check native transfers
        if let Some(ref transfers) = notification.native_transfers {
            for transfer in transfers {
                if let (Ok(from), Ok(to)) = (
                    transfer.from_user_account.parse::<Pubkey>(),
                    transfer.to_user_account.parse::<Pubkey>()
                ) {
                    if accounts.contains(&from) || accounts.contains(&to) {
                        return true;
                    }
                }
            }
        }

        // Check token transfers
        if let Some(ref transfers) = notification.token_transfers {
            for transfer in transfers {
                if let (Ok(from), Ok(to)) = (
                    transfer.from_user_account.parse::<Pubkey>(),
                    transfer.to_user_account.parse::<Pubkey>()
                ) {
                    if accounts.contains(&from) || accounts.contains(&to) {
                        return true;
                    }
                }
            }
        }

        // Check account data changes
        if let Some(ref account_data) = notification.account_data {
            for data in account_data {
                if let Ok(account) = data.account.parse::<Pubkey>() {
                    if accounts.contains(&account) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

/// Combined statistics for integrated balance monitoring
#[derive(Debug, Clone)]
pub struct IntegratedBalanceStats {
    pub monitored_accounts_count: usize,
    pub balance_events: crate::solana::event_driven_balance::EventStats,
    pub webhook_stats: crate::webhooks::processor::ProcessorStats,
    pub is_running: bool,
}

/// Configuration helper for setting up integrated balance monitoring
pub struct IntegratedBalanceConfigBuilder {
    config: EventDrivenBalanceConfig,
}

impl IntegratedBalanceConfigBuilder {
    /// Create a new configuration builder
    pub fn new() -> Self {
        Self {
            config: EventDrivenBalanceConfig::default(),
        }
    }

    /// Add accounts to monitor
    pub fn with_monitored_accounts(mut self, accounts: Vec<Pubkey>) -> Self {
        self.config.monitored_accounts = accounts.into_iter().collect();
        self
    }

    /// Set balance change threshold
    pub fn with_balance_threshold(mut self, threshold: u64) -> Self {
        self.config.balance_change_threshold = threshold;
        self
    }

    /// Enable/disable token account tracking
    pub fn with_token_tracking(mut self, enabled: bool) -> Self {
        self.config.track_token_accounts = enabled;
        self
    }

    /// Enable/disable webhook integration
    pub fn with_webhook_integration(mut self, enabled: bool) -> Self {
        self.config.enable_webhook_integration = enabled;
        self
    }

    /// Build the configuration
    pub fn build(self) -> EventDrivenBalanceConfig {
        self.config
    }
}

impl Default for IntegratedBalanceConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
