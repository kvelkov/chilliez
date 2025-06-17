// src/webhooks/mod.rs
//! Helius Webhook integration for real-time DEX pool updates
//!
//! This module provides:
//! - Helius SDK integration for enhanced webhook management
//! - Webhook server to receive Helius notifications
//! - Pool update processing pipeline
//! - Real-time event handling for DEX programs

pub mod helius;
pub mod helius_sdk; // Enhanced Helius SDK integration
pub mod helius_sdk_stub; // Stub module for Helius SDK integration
pub mod integration;
pub mod processor;
pub mod server;
pub mod types; // Consolidated integration service

// Core functionality
pub use processor::PoolUpdateProcessor;
pub use server::{EnhancedWebhookServer, HeliusWebhookManager, WebhookServer, WebhookServerStats};
pub use types::*;

// Consolidated integration exports
pub use integration::{
    IntegratedPoolService, IntegratedPoolStats, MonitoredPool, PoolEvent, PoolEventType,
    PoolMonitorStats, PoolMonitoringCoordinator, PoolUpdateNotification, WebhookIntegrationService,
    WebhookMetadata, WebhookStats,
};

// Helius SDK integration (currently stubbed due to dependency conflicts)
pub use helius_sdk_stub::{
    Helius, HeliusConfig as WebhookConfig, HeliusManager as EnhancedHeliusWebhookManager,
};
