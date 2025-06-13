// src/webhooks/integration.rs
//! Integration service that combines webhook management with pool discovery

use crate::webhooks::{HeliusWebhookManager, WebhookServer, PoolUpdateProcessor};
use crate::webhooks::types::HeliusWebhookNotification;
use crate::config::Config;
use crate::utils::PoolInfo;
use anyhow::{anyhow, Result as AnyhowResult};
use log::{info, warn, error};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Integrated webhook service that manages both Helius webhooks and pool updates
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
            .map(|wm| wm.active_webhook_count())
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
}
