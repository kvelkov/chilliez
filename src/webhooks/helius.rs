// src/webhooks/helius.rs
//! Helius Webhook Manager for setting up and managing webhooks

use crate::webhooks::types::DexPrograms;
use anyhow::{anyhow, Result as AnyhowResult};
use log::{info, warn, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Helius webhook creation request
#[derive(Debug, Serialize)]
struct CreateWebhookRequest {
    #[serde(rename = "webhookURL")]
    webhook_url: String,
    #[serde(rename = "transactionTypes")]
    transaction_types: Vec<String>,
    #[serde(rename = "accountAddresses")]
    account_addresses: Vec<String>,
    #[serde(rename = "webhookType")]
    webhook_type: String,
    #[serde(rename = "txnStatus")]
    txn_status: String,
    #[serde(rename = "encoding")]
    encoding: String,
}

/// Helius webhook response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WebhookResponse {
    #[serde(rename = "webhookID")]
    webhook_id: String,
    #[serde(rename = "webhookURL")]
    webhook_url: String,
    #[serde(rename = "transactionTypes")]
    transaction_types: Vec<String>,
    #[serde(rename = "accountAddresses")]
    account_addresses: Vec<String>,
    #[serde(rename = "webhookType")]
    webhook_type: String,
}

/// Manages Helius webhooks for DEX monitoring
pub struct HeliusWebhookManager {
    api_key: String,
    base_url: String,
    client: Client,
    webhook_endpoint: String,
    active_webhooks: HashMap<String, String>, // program_id -> webhook_id
}

impl HeliusWebhookManager {
    /// Create a new Helius webhook manager
    pub fn new(api_key: String, webhook_endpoint: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.helius.xyz/v0".to_string(),
            client: Client::new(),
            webhook_endpoint,
            active_webhooks: HashMap::new(),
        }
    }

    /// Set up webhooks for all DEX programs
    pub async fn setup_dex_webhooks(&mut self) -> AnyhowResult<()> {
        info!("Setting up Helius webhooks for DEX programs...");

        // Create webhook for each major DEX program
        let programs = DexPrograms::all_program_ids();
        
        for program_id in programs {
            match self.create_program_webhook(program_id).await {
                Ok(webhook_id) => {
                    info!("✅ Created webhook for program {}: {}", program_id, webhook_id);
                    self.active_webhooks.insert(program_id.to_string(), webhook_id);
                }
                Err(e) => {
                    error!("❌ Failed to create webhook for program {}: {}", program_id, e);
                }
            }
        }

        info!("Webhook setup complete. Active webhooks: {}", self.active_webhooks.len());
        Ok(())
    }

    /// Create a webhook for a specific program
    async fn create_program_webhook(&self, program_id: &str) -> AnyhowResult<String> {
        let webhook_request = CreateWebhookRequest {
            webhook_url: self.webhook_endpoint.clone(),
            transaction_types: vec!["Any".to_string()],
            account_addresses: vec![program_id.to_string()],
            webhook_type: "enhanced".to_string(),
            txn_status: "success".to_string(),
            encoding: "jsonParsed".to_string(),
        };

        let url = format!("{}/webhooks?api-key={}", self.base_url, self.api_key);
        
        let response = self.client
            .post(&url)
            .json(&webhook_request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send webhook creation request: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Webhook creation failed: {}", error_text));
        }

        let webhook_response: WebhookResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse webhook response: {}", e))?;

        Ok(webhook_response.webhook_id)
    }

    /// Create a webhook for specific pool addresses (for high-value pools)
    pub async fn create_pool_specific_webhook(&mut self, pool_addresses: Vec<String>) -> AnyhowResult<String> {
        info!("Creating pool-specific webhook for {} addresses", pool_addresses.len());

        let webhook_request = CreateWebhookRequest {
            webhook_url: self.webhook_endpoint.clone(),
            transaction_types: vec!["Any".to_string()],
            account_addresses: pool_addresses,
            webhook_type: "enhanced".to_string(),
            txn_status: "success".to_string(),
            encoding: "jsonParsed".to_string(),
        };

        let url = format!("{}/webhooks?api-key={}", self.base_url, self.api_key);
        
        let response = self.client
            .post(&url)
            .json(&webhook_request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send pool webhook creation request: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Pool webhook creation failed: {}", error_text));
        }

        let webhook_response: WebhookResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse pool webhook response: {}", e))?;

        info!("✅ Created pool-specific webhook: {}", webhook_response.webhook_id);
        Ok(webhook_response.webhook_id)
    }

    /// List all active webhooks
    pub async fn list_webhooks(&self) -> AnyhowResult<Vec<WebhookResponse>> {
        let url = format!("{}/webhooks?api-key={}", self.base_url, self.api_key);
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to list webhooks: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to list webhooks: {}", response.status()));
        }

        let webhooks: Vec<WebhookResponse> = response.json().await
            .map_err(|e| anyhow!("Failed to parse webhooks list: {}", e))?;

        Ok(webhooks)
    }

    /// Delete a webhook
    pub async fn delete_webhook(&mut self, webhook_id: &str) -> AnyhowResult<()> {
        let url = format!("{}/webhooks/{}?api-key={}", self.base_url, webhook_id, self.api_key);
        
        let response = self.client
            .delete(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to delete webhook: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to delete webhook: {}", response.status()));
        }

        // Remove from active webhooks
        self.active_webhooks.retain(|_, id| id != webhook_id);
        info!("✅ Deleted webhook: {}", webhook_id);
        Ok(())
    }

    /// Clean up all webhooks (useful for testing)
    pub async fn cleanup_all_webhooks(&mut self) -> AnyhowResult<()> {
        info!("Cleaning up all active webhooks...");
        
        let webhooks = self.list_webhooks().await?;
        
        for webhook in webhooks {
            if let Err(e) = self.delete_webhook(&webhook.webhook_id).await {
                warn!("Failed to delete webhook {}: {}", webhook.webhook_id, e);
            }
        }

        self.active_webhooks.clear();
        info!("✅ All webhooks cleaned up");
        Ok(())
    }

    /// Get active webhook count
    pub fn active_webhook_count(&self) -> usize {
        self.active_webhooks.len()
    }

    /// Get active webhook IDs
    pub fn get_active_webhooks(&self) -> &HashMap<String, String> {
        &self.active_webhooks
    }
}
