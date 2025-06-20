// src/dex/mod.rs
//! This module aggregates all DEX-specific clients and services.
//! It provides a unified interface for interacting with various decentralized exchanges on Solana.

// --- Module Declarations ---

// Service and utility modules
pub mod banned_pairs;
pub mod pool;
pub mod pool_discovery;
pub mod quote;
pub mod raydium_models; // <<< FIX: Added this line to make the module public

// Individual DEX client modules
pub mod lifinity;
pub mod meteora;
pub mod orca;
pub mod raydium;

// Conditional compilation for tests
#[cfg(any(test, debug_assertions))]
pub mod integration_test;
#[cfg(test)]
mod meteora_test;
#[cfg(test)]
mod orca_integration_test;
#[cfg(test)]
mod orca_test;
#[cfg(test)]
mod raydium_test;

// --- Public Re-exports ---
pub use quote::{DexClient, PoolDiscoverable};

// --- Imports for Client Aggregation ---
use crate::cache::Cache;
use crate::config::Config;
use log::info;
use std::sync::Arc;

/// Initializes and returns all supported DEX API client instances.
pub fn get_all_clients(
    _cache: Arc<Cache>,
    _app_config: Arc<Config>,
) -> Vec<Box<dyn DexClient>> {
    info!("Initializing all DEX clients...");
    let clients: Vec<Box<dyn DexClient>> = vec![
        Box::new(orca::OrcaClient::new()),
        Box::new(raydium::RaydiumClient::new()),
        Box::new(meteora::MeteoraClient::new()),
        Box::new(lifinity::LifinityClient::new()),
    ];
    for client in &clients {
        info!("- {} client initialized.", client.get_name());
    }
    info!("Total {} DEX clients initialized successfully.", clients.len());
    clients
}

/// Asynchronously initializes and returns all DEX client instances, wrapped in an Arc for shared ownership.
pub async fn get_all_clients_arc(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    get_all_clients(cache, app_config)
        .into_iter()
        .map(Arc::from)
        .collect()
}