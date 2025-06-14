// src/webhooks/mod.rs
//! Helius Webhook integration for real-time DEX pool updates
//! 
//! This module provides:
//! - Helius SDK integration for enhanced webhook management
//! - Webhook server to receive Helius notifications
//! - Pool update processing pipeline
//! - Real-time event handling for DEX programs

pub mod helius;
pub mod helius_sdk;        // Enhanced Helius SDK integration
pub mod helius_sdk_stub;   // Stub module for Helius SDK integration
pub mod server;
pub mod processor;
pub mod types;
pub mod integration;       // Consolidated integration service

// Core functionality
pub use server::{WebhookServer, HeliusWebhookManager, EnhancedWebhookServer, WebhookServerStats};
pub use processor::PoolUpdateProcessor;
pub use types::*;

// Consolidated integration exports
pub use integration::{
    WebhookIntegrationService, WebhookStats,
    IntegratedPoolService, IntegratedPoolStats, PoolUpdateNotification,
    PoolMonitoringCoordinator, PoolEvent, PoolEventType, PoolMonitorStats, 
    MonitoredPool, WebhookMetadata
};

// Helius SDK integration (currently stubbed due to dependency conflicts)
pub use helius_sdk_stub::{HeliusManager as EnhancedHeliusWebhookManager, HeliusConfig as WebhookConfig, Helius};
