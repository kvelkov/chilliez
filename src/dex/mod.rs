// src/dex/mod.rs
//! DEX module with flat, focused structure.
//! Provides high-level API, pool discovery, and DEX client implementations.

// =====================================================================================
// CORE MODULES
// =====================================================================================

/// Core DEX API: traits, quotes, and quoting engine
pub mod api;

/// Pool discovery, management, and banned pairs filtering
pub mod discovery;

/// Advanced mathematical calculations for CLMM and AMM pools
pub mod math;

/// DEX client implementations
pub mod clients;

// =====================================================================================
// TESTS
// =====================================================================================

#[cfg(any(test, debug_assertions))]
pub mod dex_tests;
pub mod integration_test;

// =====================================================================================
// PUBLIC API RE-EXPORTS
// =====================================================================================

// Core API exports
pub use api::{
    DexClient, PoolDiscoverable, Quote, SwapInfo, 
    QuotingEngineOperations
};

// Discovery and management exports
pub use discovery::{
    PoolDiscoveryService, PoolValidationConfig, BannedPairsManager,
    find_dex_client_for_pool, group_pools_by_dex, find_pools_for_pair,
    validate_pools, validate_single_pool, POOL_PARSER_REGISTRY
};

// Client exports
pub use clients::{
    OrcaClient, RaydiumClient, MeteoraClient, LifinityClient
    // PhoenixClient is currently disabled
};
// --- Publicly re-export concrete client types ---
// These lines make the client structs available directly under the `dex` module,
// e.g., as `crate::dex::OrcaClient`
// =====================================================================================
// HELPER FUNCTIONS
// =====================================================================================

use crate::cache::Cache;
use crate::config::settings::Config;
use log::info;
use std::sync::Arc;

/// Initializes and returns all supported DEX API client instances.
/// Each client is configured with shared cache and application configuration.
pub fn get_all_clients(
    _cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Box<dyn DexClient>> {
    let mut clients: Vec<Box<dyn DexClient>> = Vec::new();

    info!("Initializing DEX API clients with Cache and Config integration...");

    let _get_dex_ttl = |dex_name: &str| -> Option<u64> {
        app_config.dex_quote_cache_ttl_secs
            .as_ref()
            .and_then(|map| map.get(dex_name).copied())
            .or(Some(app_config.redis_default_ttl_secs))
    };

    // Use the new clients module structure
    clients.push(Box::new(clients::OrcaClient::new()));
    info!("- Orca client initialized.");

    clients.push(Box::new(clients::RaydiumClient::new()));
    info!("- Raydium client initialized.");

    clients.push(Box::new(clients::MeteoraClient::new()));
    info!("- Meteora client initialized.");

    clients.push(Box::new(clients::LifinityClient::new()));
    info!("- Lifinity client initialized.");

    // Phoenix client commented out until needed
    // clients.push(Box::new(clients::PhoenixClient::new()));
    // info!("- Phoenix client initialized.");

    info!(
        "Total {} DEX API clients initialized successfully.",
        clients.len()
    );
    clients
}

/// Returns all DEX clients as PoolDiscoverable trait objects, wrapped in `Arc`.
pub fn get_all_discoverable_clients(
    _cache: Arc<Cache>,
    _app_config: Arc<Config>,
) -> Vec<Arc<dyn PoolDiscoverable>> {
    vec![
        Arc::new(clients::OrcaClient::new()),
        Arc::new(clients::RaydiumClient::new()),
        Arc::new(clients::MeteoraClient::new()),
        Arc::new(clients::LifinityClient::new()),
    ]
}

/// Asynchronously initializes and returns all DEX client instances, wrapped in `Arc`.
pub async fn get_all_clients_arc(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    get_all_clients(cache, app_config)
        .into_iter()
        .map(Arc::from)
        .collect()
}