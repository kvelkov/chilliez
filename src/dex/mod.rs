// src/dex/mod.rs

// Module declarations for each DEX client and utility
// dex_api_templates.rs and dexclient_stubs.rs are removed.
pub mod http_utils;
pub mod http_utils_shared;

#[cfg(any(test, debug_assertions))]
pub mod integration_test;

pub mod lifinity;
pub mod meteora;
pub mod orca;
pub mod phoenix;
pub mod pool;
pub mod quote;
pub mod raydium;
pub mod whirlpool;       // This is for the Whirlpool DEX *API client*
pub mod whirlpool_parser; // For parsing Whirlpool on-chain account data

// Re-export the main DexClient trait for easier access
pub use quote::DexClient;

use crate::cache::Cache; // Import the Redis Cache
use crate::config::Config; // Import the application Config

use log::info;
use std::sync::Arc;

// Import client implementations
use crate::dex::{
    lifinity::LifinityClient,
    meteora::MeteoraClient,
    orca::OrcaClient,
    phoenix::PhoenixClient,
    raydium::RaydiumClient,
    whirlpool::WhirlpoolClient as WhirlpoolApiClient, // Alias if needed
};

/// Initializes and returns all supported DEX API client instances.
/// Each client is configured with shared cache and application configuration.
///
/// # Arguments
/// * `cache` - An Arc-wrapped `Cache` instance for Redis caching.
/// * `app_config` - An Arc-wrapped `Config` instance for application settings (e.g., cache TTLs).
pub fn get_all_clients(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Box<dyn DexClient>> {
    let mut clients: Vec<Box<dyn DexClient>> = Vec::new();

    info!("Initializing DEX API clients with Cache and Config integration...");

    // Helper to get specific DEX quote TTL or default
    let get_dex_ttl = |dex_name: &str| -> Option<u64> {
        app_config.dex_quote_cache_ttl_secs
            .as_ref()
            .and_then(|map| map.get(dex_name).copied())
            .or(Some(app_config.redis_default_ttl_secs)) // Fallback to default Redis TTL
    };

    // Orca Client
    clients.push(Box::new(OrcaClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Orca"), // Pass specific or default TTL
    )));
    info!("- Orca client initialized.");

    // Raydium Client
    clients.push(Box::new(RaydiumClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Raydium"),
    )));
    info!("- Raydium client initialized.");

    // Meteora Client
    clients.push(Box::new(MeteoraClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Meteora"),
    )));
    info!("- Meteora client initialized.");

    // Lifinity Client
    clients.push(Box::new(LifinityClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Lifinity"),
    )));
    info!("- Lifinity client initialized.");

    // Phoenix Client
    clients.push(Box::new(PhoenixClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Phoenix"),
    )));
    info!("- Phoenix client initialized.");

    // Whirlpool API Client
    clients.push(Box::new(WhirlpoolApiClient::new(
        Arc::clone(&cache),
        get_dex_ttl("Whirlpool"),
    )));
    info!("- Whirlpool API client initialized.");

    info!(
        "Total {} DEX API clients initialized successfully.",
        clients.len()
    );
    clients
}

/// Asynchronously initializes and returns all DEX client instances, wrapped in `Arc`.
/// Suitable for concurrent usage.
///
/// # Arguments
/// * `cache` - An Arc-wrapped `Cache` instance for Redis caching.
/// * `app_config` - An Arc-wrapped `Config` instance for application settings.
pub async fn get_all_clients_arc(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    // If client initializations were async, they would be awaited here.
    // For now, it wraps the synchronous get_all_clients.
    get_all_clients(cache, app_config)
        .into_iter()
        .map(Arc::from)
        .collect()
}
