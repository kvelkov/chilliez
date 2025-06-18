// src/webhooks/server.rs
//! Webhook server for receiving QuickNode notifications (Axum-based)

use crate::webhooks::types::QuickNodeWebhookPayload;
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::webhooks::processor::PoolUpdateProcessor;
use axum::{extract::State, routing::post, Json, Router};
use axum::http::{StatusCode, HeaderMap};
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn, error, debug};

#[derive(Clone)]
pub struct AppState {
    pub opportunity_sender: UnboundedSender<MultiHopArbOpportunity>,
    pub auth_password: String,
}

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
    debug!(
        "🔍 Available headers: {:?}",
        headers.keys().collect::<Vec<_>>()
    );

    error!("❌ Webhook authentication failed - invalid or missing credentials");
    Err(StatusCode::UNAUTHORIZED)
}

// ================================================================================================
// QUICKNODE WEBHOOK SERVER
// ================================================================================================

#[derive(Clone)]
pub struct QuickNodeWebhookState {
    pub opportunity_sender: UnboundedSender<MultiHopArbOpportunity>,
}

pub fn create_quicknode_router(sender: UnboundedSender<MultiHopArbOpportunity>) -> Router {
    let state = QuickNodeWebhookState { opportunity_sender: sender };
    Router::new()
        .route("/quicknode", post(handle_quicknode_webhook))
        .route("/health", post(health_check))
        .with_state(state)
}

/// QuickNode webhook handler
async fn handle_quicknode_webhook(
    headers: HeaderMap,
    State(state): State<QuickNodeWebhookState>,
    Json(payload): Json<QuickNodeWebhookPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate authentication
    if let Err(status) = validate_webhook_auth(&headers) {
        warn!("❌ Webhook authentication failed");
        return Err(status);
    }

    info!("📡 Received QuickNode webhook");
    debug!("QuickNode payload: {:?}", payload);

    // Process the QuickNode opportunity
    match PoolUpdateProcessor::process_quicknode_opportunity(payload, &state.opportunity_sender, &PoolUpdateProcessor::new()).await {
        Ok(_) => Ok(Json(json!({
            "status": "success",
            "message": "QuickNode webhook processed successfully",
            "timestamp": chrono::Utc::now().timestamp()
        }))),
        Err(e) => {
            error!("Failed to process QuickNode webhook: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "service": "quicknode-webhook-server",
        "timestamp": chrono::Utc::now().timestamp()
    }))
}
