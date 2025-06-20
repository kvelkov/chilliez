// src/webhooks/mod.rs
//! Webhook integration for real-time DEX pool updates
//!
//! This module provides:
//! - Webhook server to receive QuickNode notifications
//! - Pool update processing pipeline
//! - Real-time event handling for DEX programs

pub mod integration;
pub mod processor;
pub mod server;
pub mod types; // Consolidated integration service

// Core functionality
pub use processor::PoolUpdateProcessor;
pub use types::*;

// Consolidated integration exports
pub use integration::{
    IntegratedPoolService, IntegratedPoolStats, MonitoredPool, PoolEvent, PoolEventType,
    PoolMonitorStats, PoolMonitoringCoordinator, PoolUpdateNotification, WebhookIntegrationService,
    WebhookMetadata, WebhookStats,
};
