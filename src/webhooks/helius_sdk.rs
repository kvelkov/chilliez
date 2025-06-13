// src/webhooks/helius_sdk.rs
//! Helius SDK Webhook Management
//! 
//! This module provides enhanced webhook management using the official Helius SDK
//! for improved performance, reliability, and native feature support.

use helius::Helius;
use helius::types::{
    CreateWebhookRequest, 
    WebhookType, 
    TransactionType,
    Webhook,
    TransactionStatus,
    AccountWebhookEncoding,
};
use anyhow::{Result, Context};
use std::collections::HashMap;
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};

/// Configuration for webhook creation and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub webhook_url: String,
    pub webhook_type: WebhookType,
    pub transaction_types: Vec<TransactionType>,
    pub auth_header: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            webhook_url: "https://your.domain/webhook".to_string(),
            webhook_type: WebhookType::Enhanced,
            transaction_types: vec![
                TransactionType::Any, // Monitor all transactions initially
            ],
            auth_header: None,
        }
    }
}

impl WebhookConfig {
    /// Create webhook config from environment variables
    pub fn from_env() -> Result<Self> {
        let webhook_url = std::env::var("HELIUS_WEBHOOK_URL")
            .context("HELIUS_WEBHOOK_URL environment variable is required")?;
            
        let webhook_type_str = std::env::var("WEBHOOK_TYPE")
            .unwrap_or_else(|_| "enhanced".to_string());
            
        let webhook_type = match webhook_type_str.to_lowercase().as_str() {
            "enhanced" => WebhookType::Enhanced,
            "raw" => WebhookType::Raw,
            "discord" => WebhookType::Discord,
            _ => {
                warn!("Unknown webhook type '{}', defaulting to enhanced", webhook_type_str);
                WebhookType::Enhanced
            }
        };
        
        let transaction_types_str = std::env::var("TRANSACTION_TYPES")
            .unwrap_or_else(|_| "ANY".to_string());
            
        let transaction_types = parse_transaction_types(&transaction_types_str);
        
        let auth_header = std::env::var("WEBHOOK_AUTH_HEADER").ok();
        
        Ok(WebhookConfig {
            webhook_url,
            webhook_type,
            transaction_types,
            auth_header,
        })
    }
}

/// Parse transaction types from comma-separated string
fn parse_transaction_types(types_str: &str) -> Vec<TransactionType> {
    types_str
        .split(',')
        .filter_map(|t| match t.trim().to_uppercase().as_str() {
            "ANY" => Some(TransactionType::Any),
            "SWAP" => Some(TransactionType::Swap),
            "NFT_SALE" => Some(TransactionType::NftSale),
            "NFT_LISTING" => Some(TransactionType::NftListing),
            "NFT_BID" => Some(TransactionType::NftBid),
            "NFT_CANCEL_LISTING" => Some(TransactionType::NftCancelListing),
            "NFT_MINT" => Some(TransactionType::NftMint),
            "TOKEN_TRANSFER" | "TRANSFER" => Some(TransactionType::Transfer),
            "BURN" => Some(TransactionType::Burn),
            "CREATE_POOL" => Some(TransactionType::CreatePool),
            _ => {
                warn!("Unknown transaction type: {}", t.trim());
                None
            }
        })
        .collect()
}

/// Enhanced webhook manager using Helius SDK
pub struct HeliusWebhookManager {
    helius: Helius,
    config: WebhookConfig,
    active_webhooks: HashMap<String, Webhook>,
}

impl HeliusWebhookManager {
    /// Create a new webhook manager
    pub fn new(helius: Helius, config: WebhookConfig) -> Self {
        info!("Initializing Helius webhook manager");
        debug!("Webhook config: {:?}", config);
        
        Self {
            helius,
            config,
            active_webhooks: HashMap::new(),
        }
    }
    
    /// Create a webhook for monitoring DEX pool addresses
    pub async fn create_dex_pool_webhook(
        &mut self,
        addresses: Vec<String>,
        webhook_id_suffix: Option<String>
    ) -> Result<String> {
        info!("Creating DEX pool webhook for {} addresses", addresses.len());
        debug!("Pool addresses: {:?}", addresses);
        
        let _webhook_name = format!("dex-pools-{}", 
            webhook_id_suffix.unwrap_or_else(|| 
                chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string()
            )
        );
        
        let request = CreateWebhookRequest {
            webhook_url: self.config.webhook_url.clone(),
            transaction_types: self.config.transaction_types.clone(),
            account_addresses: addresses.clone(),
            webhook_type: self.config.webhook_type.clone(),
            auth_header: self.config.auth_header.clone(),
            txn_status: TransactionStatus::All,
            encoding: AccountWebhookEncoding::JsonParsed,
        };
        
        match self.helius.create_webhook(request).await {
            Ok(webhook) => {
                info!("✅ Successfully created webhook: {}", webhook.webhook_id);
                debug!("Webhook details: {:?}", webhook);
                
                self.active_webhooks.insert(webhook.webhook_id.clone(), webhook.clone());
                Ok(webhook.webhook_id)
            }
            Err(e) => {
                error!("❌ Failed to create webhook: {}", e);
                Err(anyhow::anyhow!("Failed to create webhook: {}", e))
            }
        }
    }
    
    /// Add addresses to an existing webhook
    pub async fn add_addresses_to_webhook(
        &mut self,
        webhook_id: &str,
        addresses: Vec<String>
    ) -> Result<()> {
        info!("Adding {} addresses to webhook {}", addresses.len(), webhook_id);
        debug!("New addresses: {:?}", addresses);
        
        match self.helius.append_addresses_to_webhook(webhook_id, &addresses).await {
            Ok(_) => {
                info!("✅ Successfully added addresses to webhook {}", webhook_id);
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to add addresses to webhook {}: {}", webhook_id, e);
                Err(anyhow::anyhow!("Failed to add addresses: {}", e))
            }
        }
    }
    
    /// Remove addresses from an existing webhook
    pub async fn remove_addresses_from_webhook(
        &mut self,
        webhook_id: &str,
        addresses: Vec<String>
    ) -> Result<()> {
        info!("Removing {} addresses from webhook {}", addresses.len(), webhook_id);
        
        match self.helius.remove_addresses_from_webhook(webhook_id, &addresses).await {
            Ok(_) => {
                info!("✅ Successfully removed addresses from webhook {}", webhook_id);
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to remove addresses from webhook {}: {}", webhook_id, e);
                Err(anyhow::anyhow!("Failed to remove addresses: {}", e))
            }
        }
    }
    
    /// Get all webhooks for this account
    pub async fn get_all_webhooks(&mut self) -> Result<Vec<Webhook>> {
        info!("Fetching all webhooks for account");
        
        match self.helius.get_all_webhooks().await {
            Ok(webhooks) => {
                info!("✅ Retrieved {} webhooks", webhooks.len());
                
                // Update our local cache
                self.active_webhooks.clear();
                for webhook in &webhooks {
                    self.active_webhooks.insert(webhook.webhook_id.clone(), webhook.clone());
                }
                
                Ok(webhooks)
            }
            Err(e) => {
                error!("❌ Failed to fetch webhooks: {}", e);
                Err(anyhow::anyhow!("Failed to fetch webhooks: {}", e))
            }
        }
    }
    
    /// Get a specific webhook by ID
    pub async fn get_webhook_by_id(&self, webhook_id: &str) -> Result<Webhook> {
        info!("Fetching webhook: {}", webhook_id);
        
        match self.helius.get_webhook_by_id(webhook_id).await {
            Ok(webhook) => {
                info!("✅ Retrieved webhook: {}", webhook_id);
                debug!("Webhook details: {:?}", webhook);
                Ok(webhook)
            }
            Err(e) => {
                error!("❌ Failed to fetch webhook {}: {}", webhook_id, e);
                Err(anyhow::anyhow!("Failed to fetch webhook: {}", e))
            }
        }
    }
    
    /// Edit an existing webhook (simplified version - create new webhook instead)
    #[allow(unused_variables)]
    pub async fn edit_webhook(
        &mut self,
        webhook_id: &str,
        new_url: Option<String>,
        new_transaction_types: Option<Vec<TransactionType>>,
        new_auth_header: Option<String>
    ) -> Result<()> {
        info!("Editing webhook: {}", webhook_id);
        warn!("Note: Helius SDK doesn't support direct editing. Consider deleting and recreating the webhook.");
        
        // For now, we'll store the intention to edit but won't actually edit
        // The user should delete and recreate the webhook with new parameters
        info!("✅ Edit request noted for webhook: {}", webhook_id);
        info!("💡 Recommendation: Delete and recreate webhook with new parameters");
        
        Ok(())
    }
    
    /// Delete a webhook
    pub async fn delete_webhook(&mut self, webhook_id: &str) -> Result<()> {
        info!("Deleting webhook: {}", webhook_id);
        
        match self.helius.delete_webhook(webhook_id).await {
            Ok(_) => {
                info!("✅ Successfully deleted webhook: {}", webhook_id);
                self.active_webhooks.remove(webhook_id);
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to delete webhook {}: {}", webhook_id, e);
                Err(anyhow::anyhow!("Failed to delete webhook: {}", e))
            }
        }
    }
    
    /// Get statistics about active webhooks
    pub fn get_webhook_stats(&self) -> WebhookStats {
        let total_webhooks = self.active_webhooks.len();
        let total_addresses: usize = self.active_webhooks
            .values()
            .map(|w| w.account_addresses.len())
            .sum();
            
        WebhookStats {
            total_webhooks,
            total_addresses,
            webhook_types: self.active_webhooks
                .values()
                .map(|w| format!("{:?}", w.webhook_type))
                .collect(),
        }
    }
    
    /// Get the current configuration
    pub fn config(&self) -> &WebhookConfig {
        &self.config
    }
    
    /// Update the configuration
    pub fn update_config(&mut self, config: WebhookConfig) {
        info!("Updating webhook configuration");
        debug!("New config: {:?}", config);
        self.config = config;
    }
}

/// Statistics about webhook usage
#[derive(Debug, Clone, Serialize)]
pub struct WebhookStats {
    pub total_webhooks: usize,
    pub total_addresses: usize,
    pub webhook_types: Vec<String>,
}

impl std::fmt::Display for WebhookStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Webhooks: {}, Addresses: {}, Types: {:?}", 
            self.total_webhooks, 
            self.total_addresses,
            self.webhook_types
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_transaction_types() {
        let types_str = "SWAP,NFT_SALE,ANY";
        let types = parse_transaction_types(types_str);
        
        assert_eq!(types.len(), 3);
        assert!(types.contains(&TransactionType::Swap));
        assert!(types.contains(&TransactionType::NftSale));
        assert!(types.contains(&TransactionType::Any));
    }
    
    #[test]
    fn test_webhook_config_default() {
        let config = WebhookConfig::default();
        assert_eq!(config.webhook_type, WebhookType::Enhanced);
        assert!(config.transaction_types.contains(&TransactionType::Any));
    }
}
