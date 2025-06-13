// src/webhooks/server.rs
//! Webhook server to receive Helius notifications

use crate::webhooks::types::HeliusWebhookNotification;
use crate::webhooks::processor::PoolUpdateProcessor;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use log::{info, warn, error};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

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

/// Main webhook handler for Helius notifications
async fn handle_webhook(
    State(state): State<WebhookState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // Log the incoming webhook for debugging
    info!("📡 Received webhook notification");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhooks::processor::PoolUpdateProcessor;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_webhook_server_creation() {
        let processor = Arc::new(PoolUpdateProcessor::new());
        let (tx, _rx) = mpsc::unbounded_channel();
        
        let server = WebhookServer::new(8080, processor, tx);
        assert_eq!(server.port, 8080);
    }
}
