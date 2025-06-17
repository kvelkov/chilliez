// src/webhooks/server.rs
//! Webhook server implementations for receiving Helius notifications
//! 
//! This module consolidates:
//! - Basic WebhookServer for simple webhook handling
//! - EnhancedWebhookServer for advanced webhook processing
//! - HeliusWebhookManager for webhook setup and management

use crate::webhooks::types::{HeliusWebhookNotification, DexPrograms};
use crate::webhooks::processor::PoolUpdateProcessor;
use crate::webhooks::integration::{PoolEvent, PoolEventType};
use crate::webhooks::helius_sdk_stub::EnhancedTransaction;
// Temporarily disabled QuickNode integration while fixing imports
// use crate::streams::quicknode::{QuickNodeEvent, DexSwapEvent};
use axum::{
    extract::{Path, State},
    http::{StatusCode, HeaderMap},
    response::{Json, IntoResponse},
    routing::{get, post},
    Router,
};
use anyhow::{anyhow, Result as AnyhowResult};
use log::{info, warn, error, debug};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ================================================================================================
// AUTHENTICATION UTILITIES
// ================================================================================================

/// Validate webhook authentication using password header
fn validate_webhook_auth(headers: &HeaderMap) -> Result<(), StatusCode> {
    // Get expected password from environment
    let expected_password = match std::env::var("WEBHOOK_AUTH_PASSWORD") {
        Ok(password) => password,
        Err(_) => {
            warn!("⚠️ WEBHOOK_AUTH_PASSWORD not set - allowing all requests (insecure)");
            return Ok(());
        }
    };

    // Check authHeader (Helius specific header format)
    if let Some(auth_header) = headers.get("authheader") {
        if let Ok(auth_value) = auth_header.to_str() {
            if auth_value == expected_password {
                debug!("✅ Webhook authenticated via authHeader");
                return Ok(());
            }
        }
    }

    // Check Authorization header (standard format)
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_value) = auth_header.to_str() {
            // Support Bearer token format: "Bearer heligo567"
            if let Some(token) = auth_value.strip_prefix("Bearer ") {
                if token == expected_password {
                    debug!("✅ Webhook authenticated via Authorization Bearer");
                    return Ok(());
                }
            }
            // Support direct password format
            if auth_value == expected_password {
                debug!("✅ Webhook authenticated via Authorization");
                return Ok(());
            }
        }
    }

    // Check X-Webhook-Password header (alternative)
    if let Some(password_header) = headers.get("x-webhook-password") {
        if let Ok(password_value) = password_header.to_str() {
            if password_value == expected_password {
                debug!("✅ Webhook authenticated via X-Webhook-Password");
                return Ok(());
            }
        }
    }

    // Log available headers for debugging
    debug!("🔍 Available headers: {:?}", headers.keys().collect::<Vec<_>>());

    error!("❌ Webhook authentication failed - invalid or missing credentials");
    Err(StatusCode::UNAUTHORIZED)
}

// ================================================================================================
// BASIC WEBHOOK SERVER
// ================================================================================================

/// Webhook server state
#[derive(Clone)]
pub struct WebhookState {
    pub pool_processor: Arc<PoolUpdateProcessor>,
    pub notification_sender: mpsc::UnboundedSender<HeliusWebhookNotification>,
}

/// Webhook server for receiving Helius notifications
pub struct WebhookServer {
    port: u16,
    state: WebhookState,
}

impl WebhookServer {
    /// Create a new webhook server
    pub fn new(
        port: u16, 
        pool_processor: Arc<PoolUpdateProcessor>,
        notification_sender: mpsc::UnboundedSender<HeliusWebhookNotification>,
    ) -> Self {
        let state = WebhookState {
            pool_processor,
            notification_sender,
        };

        Self { port, state }
    }

    /// Start the webhook server
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let app = self.create_router();

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await?;

        info!("🚀 Webhook server starting on port {}", self.port);
        info!("📡 Ready to receive Helius notifications at /webhook");

        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Create the router with all webhook endpoints
    fn create_router(&self) -> Router {
        Router::new()
            .route("/webhook", post(handle_webhook))
            .route("/health", post(health_check))
            .route("/status", post(webhook_status))
            .with_state(self.state.clone())
    }
}

/// Main webhook handler for Helius notifications with authentication
async fn handle_webhook(
    headers: HeaderMap,
    State(state): State<WebhookState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // Validate authentication
    if let Err(status) = validate_webhook_auth(&headers) {
        warn!("❌ Webhook authentication failed");
        return Err(status);
    }

    // Log the incoming webhook for debugging
    info!("📡 Received authenticated webhook notification");

    // Parse the Helius notification
    let notification: HeliusWebhookNotification = match serde_json::from_value(payload.clone()) {
        Ok(notif) => notif,
        Err(e) => {
            error!("Failed to parse webhook notification: {}", e);
            error!("Raw payload: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Send to processing pipeline
    if let Err(e) = state.notification_sender.send(notification.clone()) {
        error!("Failed to send notification to processor: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Process the notification immediately for critical updates
    match state.pool_processor.process_notification(&notification).await {
        Ok(()) => {
            info!("✅ Successfully processed webhook notification for signature: {}", 
                  notification.txn_signature);
        }
        Err(e) => {
            warn!("⚠️ Failed to process webhook notification: {}", e);
            // Don't return error - we still received and queued the notification
        }
    }

    // Return success response
    Ok(Json(json!({
        "status": "success",
        "message": "Webhook processed successfully",
        "signature": notification.txn_signature,
        "timestamp": chrono::Utc::now().timestamp()
    })))
}

/// Health check endpoint
async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "helius-webhook-server",
        "timestamp": chrono::Utc::now().timestamp()
    }))
}

/// Webhook status endpoint
async fn webhook_status(State(state): State<WebhookState>) -> Json<Value> {
    let processor_stats = state.pool_processor.get_stats().await;
    
    Json(json!({
        "status": "operational",
        "webhook_server": "running",
        "processor_stats": processor_stats,
        "timestamp": chrono::Utc::now().timestamp()
    }))
}

// ================================================================================================
// ENHANCED WEBHOOK SERVER
// ================================================================================================

/// Statistics for the enhanced webhook server
#[derive(Debug, Clone, Default)]
pub struct WebhookServerStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub events_sent: u64,
    pub last_request_time: Option<std::time::Instant>,
    pub requests_by_type: HashMap<String, u64>,
}

/// Helius webhook notification payload (simplified)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeliusWebhookPayload {
    pub signature: String,
    pub slot: u64,
    pub timestamp: i64,
    pub fee: u64,
    #[serde(rename = "feePayer")]
    pub fee_payer: String,
    #[serde(rename = "type")]
    pub transaction_type: String,
    pub source: String,
    pub accounts: Vec<String>,
}

/// Enhanced server state
#[derive(Clone)]
pub struct EnhancedServerState {
    event_sender: mpsc::UnboundedSender<PoolEvent>,
    stats: Arc<RwLock<WebhookServerStats>>,
}

/// Enhanced webhook server for processing Helius notifications
pub struct EnhancedWebhookServer {
    port: u16,
    event_sender: mpsc::UnboundedSender<PoolEvent>,
    webhook_stats: Arc<RwLock<WebhookServerStats>>,
}

impl EnhancedWebhookServer {
    /// Create a new enhanced webhook server
    pub fn new(
        port: u16,
        event_sender: mpsc::UnboundedSender<PoolEvent>,
    ) -> Self {
        info!("🚀 Creating Enhanced Webhook Server on port {}", port);
        
        Self {
            port,
            event_sender,
            webhook_stats: Arc::new(RwLock::new(WebhookServerStats::default())),
        }
    }
    
    /// Start the webhook server
    pub async fn start(&self) -> AnyhowResult<()> {
        info!("🎬 Starting Enhanced Webhook Server on port {}", self.port);
        
        let state = EnhancedServerState {
            event_sender: self.event_sender.clone(),
            stats: self.webhook_stats.clone(),
        };
        
        let app = Router::new()
            .route("/webhook", post(handle_enhanced_webhook))
            .route("/webhook/:id", post(handle_enhanced_webhook_with_id))
            .route("/health", get(enhanced_health_check))
            .with_state(state);
        
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;
        info!("✅ Enhanced Webhook Server listening on port {}", self.port);
        
        axum::serve(listener, app).await?;
        
        Ok(())
    }
    
    /// Get current statistics
    pub async fn get_stats(&self) -> WebhookServerStats {
        self.webhook_stats.read().await.clone()
    }
}

/// Enhanced webhook handler with authentication
async fn handle_enhanced_webhook(
    headers: HeaderMap,
    State(state): State<EnhancedServerState>,
    Json(payload): Json<HeliusWebhookPayload>,
) -> Result<Json<Value>, StatusCode> {
    // Validate authentication
    if let Err(status) = validate_webhook_auth(&headers) {
        warn!("❌ Enhanced webhook authentication failed");
        return Err(status);
    }

    let mut stats = state.stats.write().await;
    stats.total_requests += 1;
    stats.last_request_time = Some(std::time::Instant::now());
    drop(stats);

    debug!("📡 Enhanced webhook received: {}", payload.signature);

    // Convert payload to pool event
    if let Ok(pool_event) = convert_payload_to_event(&payload) {
        if let Err(e) = state.event_sender.send(pool_event) {
            error!("Failed to send pool event: {}", e);
            let mut stats = state.stats.write().await;
            stats.failed_requests += 1;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        let mut stats = state.stats.write().await;
        stats.successful_requests += 1;
        stats.events_sent += 1;
    }

    Ok(Json(json!({
        "status": "success",
        "signature": payload.signature,
        "timestamp": chrono::Utc::now().timestamp()
    })))
}

/// Enhanced webhook handler with ID
async fn handle_enhanced_webhook_with_id(
    Path(id): Path<String>,
    headers: HeaderMap,
    State(state): State<EnhancedServerState>,
    Json(payload): Json<HeliusWebhookPayload>,
) -> Result<Json<Value>, StatusCode> {
    info!("📡 Enhanced webhook received with ID {}: {}", id, payload.signature);
    handle_enhanced_webhook(headers, State(state), Json(payload)).await
}

/// Enhanced health check
async fn enhanced_health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "enhanced-helius-webhook-server",
        "timestamp": chrono::Utc::now().timestamp()
    }))
}

/// Convert webhook payload to pool event
fn convert_payload_to_event(payload: &HeliusWebhookPayload) -> AnyhowResult<PoolEvent> {
    use crate::webhooks::helius_sdk_stub::{TransactionType, Source, TransactionEvent};
    
    // Parse pool address from accounts
    let pool_address = payload.accounts.first()
        .ok_or_else(|| anyhow!("No accounts in payload"))?;
    
    let pool_pubkey = Pubkey::from_str(pool_address)
        .map_err(|_| anyhow!("Invalid pool address: {}", pool_address))?;

    // Create a mock enhanced transaction
    let enhanced_tx = EnhancedTransaction {
        signature: payload.signature.clone(),
        slot: payload.slot,
        timestamp: payload.timestamp as u64,
        fee: payload.fee,
        fee_payer: payload.fee_payer.clone(),
        transaction_error: None,
        description: format!("Pool transaction on {}", pool_address),
        transaction_type: TransactionType::Swap,
        source: Source::Unknown,
        account_data: payload.accounts.clone(),
        native_transfers: None,
        token_transfers: None,
        instructions: vec![],
        events: TransactionEvent,
    };

    Ok(PoolEvent::PoolUpdate {
        pool_address: pool_pubkey,
        transaction: enhanced_tx,
        event_type: PoolEventType::Swap, // Default to swap for now
    })
}

// ================================================================================================
// HELIUS WEBHOOK MANAGER
// ================================================================================================

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
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to create webhook: {}", error_text));
        }

        let webhook_response: WebhookResponse = response.json().await?;
        Ok(webhook_response.webhook_id)
    }

    /// Create a webhook for specific addresses
    pub async fn create_webhook(&mut self, addresses: Vec<String>) -> AnyhowResult<String> {
        let webhook_request = CreateWebhookRequest {
            webhook_url: self.webhook_endpoint.clone(),
            transaction_types: vec!["Any".to_string()],
            account_addresses: addresses,
            webhook_type: "enhanced".to_string(),
            txn_status: "success".to_string(),
            encoding: "jsonParsed".to_string(),
        };

        let url = format!("{}/webhooks?api-key={}", self.base_url, self.api_key);
        
        let response = self.client
            .post(&url)
            .json(&webhook_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to create webhook: {}", error_text));
        }

        let webhook_response: WebhookResponse = response.json().await?;
        let webhook_id = webhook_response.webhook_id.clone();
        
        // Add to active webhooks
        self.active_webhooks.insert(format!("addresses_{}", webhook_id), webhook_id.clone());
        
        Ok(webhook_id)
    }

    /// Delete a webhook
    pub async fn delete_webhook(&mut self, webhook_id: &str) -> AnyhowResult<()> {
        let url = format!("{}/webhooks/{}?api-key={}", self.base_url, webhook_id, self.api_key);
        
        let response = self.client.delete(&url).send().await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to delete webhook: {}", error_text));
        }

        // Remove from active webhooks
        self.active_webhooks.retain(|_, id| id != webhook_id);
        info!("🗑️ Deleted webhook: {}", webhook_id);
        
        Ok(())
    }

    /// Get all active webhooks
    pub fn get_active_webhooks(&self) -> &HashMap<String, String> {
        &self.active_webhooks
    }

    /// Clean up all webhooks
    pub async fn cleanup_all_webhooks(&mut self) -> AnyhowResult<()> {
        info!("🧹 Cleaning up all active webhooks...");
        
        let webhook_ids: Vec<String> = self.active_webhooks.values().cloned().collect();
        
        for webhook_id in webhook_ids {
            if let Err(e) = self.delete_webhook(&webhook_id).await {
                error!("Failed to delete webhook {}: {}", webhook_id, e);
            }
        }
        
        self.active_webhooks.clear();
        info!("✅ Webhook cleanup complete");
        
        Ok(())
    }
}

impl std::fmt::Display for WebhookServerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Requests: Total={}, Success={}, Failed={}, Events Sent: {}",
            self.total_requests,
            self.successful_requests,
            self.failed_requests,
            self.events_sent
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhooks::processor::PoolUpdateProcessor;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_basic_webhook_server_creation() {
        let processor = Arc::new(PoolUpdateProcessor::new());
        let (tx, _rx) = mpsc::unbounded_channel();
        
        let server = WebhookServer::new(8080, processor, tx);
        assert_eq!(server.port, 8080);
    }

    #[tokio::test]
    async fn test_enhanced_webhook_server_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        let server = EnhancedWebhookServer::new(8081, tx);
        assert_eq!(server.port, 8081);
    }

    #[test]
    fn test_helius_webhook_manager_creation() {
        let manager = HeliusWebhookManager::new(
            "test_api_key".to_string(),
            "http://localhost:8080/webhook".to_string()
        );
        
        assert_eq!(manager.api_key, "test_api_key");
        assert_eq!(manager.webhook_endpoint, "http://localhost:8080/webhook");
    }
}

/// QuickNode webhook handler
async fn handle_quicknode_simple(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    info!("📡 Received QuickNode webhook");
    debug!("QuickNode payload: {}", payload);
    (StatusCode::OK, "OK")
}

/// Update the router to include QuickNode endpoint
pub fn create_webhook_router() -> Router<WebhookState> {
    Router::new()
        .route("/webhook", post(handle_webhook)) // Existing Helius webhook
        .route("/quicknode", post(handle_quicknode_simple)) // New QuickNode webhook
        .route("/health", get(health_check))
}
