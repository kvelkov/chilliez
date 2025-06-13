// src/webhooks/mod.rs
//! Helius Webhook integration for real-time DEX pool updates
//! 
//! This module provides:
//! - Helius SDK integration for enhanced webhook management
//! - Webhook server to receive Helius notifications
//! - Pool update processing pipeline
//! - Real-time event handling for DEX programs

pub mod helius;
pub mod helius_sdk;        // NEW: Enhanced Helius SDK integration
pub mod helius_sdk_stub;   // NEW: Stub module for Helius SDK integration
pub mod pool_monitor;      // NEW: Pool monitoring coordinator using Helius SDK
pub mod enhanced_server;   // NEW: Enhanced webhook server for Helius notifications
pub mod server;
pub mod processor;
pub mod types;
pub mod integration;
pub mod pool_integration;

// Legacy webhook management (being phased out)
pub use helius::HeliusWebhookManager;

// Helius SDK integration (currently stubbed due to dependency conflicts)
pub use helius_sdk_stub::{HeliusManager as EnhancedHeliusWebhookManager, HeliusConfig as WebhookConfig, Helius};
pub use pool_monitor::{PoolMonitoringCoordinator, PoolEvent, PoolEventType, PoolMonitorStats, MonitoredPool};
pub use enhanced_server::{EnhancedWebhookServer, WebhookServerStats, HeliusWebhookPayload};

// Core functionality
pub use server::WebhookServer;
pub use processor::PoolUpdateProcessor;
pub use integration::{WebhookIntegrationService, WebhookStats};
pub use pool_integration::{IntegratedPoolService, IntegratedPoolStats, PoolUpdateNotification};
pub use types::*;
