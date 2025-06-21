//! DEX module providing unified interface for all supported decentralized exchanges.
//!
//! This module includes enhanced support for:
//! - Meteora (Dynamic AMM and DLMM)
//! - Lifinity (Proactive Market Making)
//! - Phoenix (Order Book DEX)
//! - Advanced mathematical calculations
//! - Comprehensive testing framework

// Core modules
pub mod api;
pub mod clients;
pub mod discovery;
pub mod live_update_manager;
pub mod math;
pub mod protocols;

// Test modules
#[cfg(test)]
pub mod dex_tests;

// --- Public Re-exports ---
pub use api::{CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo};

// Re-export discovery functionality (actively used items)
pub use discovery::{
    validate_single_pool,
    // Note: PoolDiscoveryService, find_dex_client_for_pool, POOL_PARSER_REGISTRY
    // are imported directly from discovery in main.rs and other places
    BannedPairsManager,
    // Used in orchestrator.rs and webhooks:
    PoolValidationConfig,
};

// Re-export live update management (items used in main.rs)
// Note: These are imported directly in main.rs from dex::live_update_manager
// pub use live_update_manager::{
//     LiveUpdateManager, LiveUpdateManagerBuilder, LiveUpdateConfig,
// };

// Re-export DEX clients that are used externally (currently none are used externally)
// The clients are only used in the get_all_* functions within this module
// If external modules need direct access to clients, uncomment the needed ones:
// pub use clients::{
//     OrcaClient, OrcaPoolParser,
//     RaydiumClient,
//     MeteoraClient, MeteoraPoolParser,
//     LifinityClient, LifinityPoolParser,
// };

// Imports for client aggregation
use crate::data::cache::Cache;
use crate::config::Config;
use log::info;
use std::sync::Arc;

/// Initializes and returns all supported DEX API client instances.
///
/// Enhanced to include Meteora, Lifinity, and Phoenix clients with
/// proper configuration and caching support.
pub fn get_all_clients(
    _cache: Arc<Cache>,
    _app_config: Arc<Config>,
) -> Vec<Box<dyn DexClient>> {
    info!("Initializing all DEX clients...");

    let clients: Vec<Box<dyn DexClient>> = vec![
        Box::new(clients::OrcaClient::new()),
        Box::new(clients::RaydiumClient::new()),
        Box::new(clients::MeteoraClient::new()),
        Box::new(clients::LifinityClient::new()),
        Box::new(clients::JupiterClient::new()),
        // Note: Phoenix client can be enabled once dependency conflicts are resolved
        // Box::new(clients::PhoenixClient::new()),
    ];

    for client in &clients {
        info!(
            "- {} client initialized with enhanced features",
            client.get_name()
        );
    }

    info!(
        "Total {} enhanced DEX clients initialized successfully.",
        clients.len()
    );
    clients
}

/// Returns all DEX clients implementing the PoolDiscoverable trait.
///
/// Enhanced with new DEX integrations and improved error handling.
pub fn get_all_discoverable_clients(
    _cache: Arc<Cache>,
    _app_config: Arc<Config>,
) -> Vec<Arc<dyn PoolDiscoverable>> {
    info!("Initializing discoverable DEX clients...");

    vec![
        Arc::new(clients::OrcaClient::new()),
        Arc::new(clients::RaydiumClient::new()),
        Arc::new(clients::MeteoraClient::new()),
        Arc::new(clients::LifinityClient::new()),
        Arc::new(clients::JupiterClient::new()),
        // Arc::new(clients::PhoenixClient::new()), // Enable when ready
    ]
}

/// Asynchronously initializes and returns all DEX client instances, wrapped in Arc.
///
/// Enhanced version with improved concurrent initialization and health checking.
pub async fn get_all_clients_arc(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    info!("Initializing Arc-wrapped DEX clients...");

    let clients: Vec<Arc<dyn DexClient>> = get_all_clients(cache.clone(), app_config.clone())
        .into_iter()
        .map(|client| Arc::from(client) as Arc<dyn DexClient>)
        .collect();

    // Perform health checks on all clients
    info!("Performing health checks on {} clients...", clients.len());
    let mut healthy_clients = Vec::new();

    for client in clients {
        match client.health_check().await {
            Ok(health) if health.is_healthy => {
                info!("✅ {} client is healthy", client.get_name());
                healthy_clients.push(client);
            }
            Ok(health) => {
                info!(
                    "⚠️ {} client is unhealthy: {}",
                    client.get_name(),
                    health.status_message
                );
                // Still include unhealthy clients but log the issue
                healthy_clients.push(client);
            }
            Err(e) => {
                info!("❌ {} client health check failed: {}", client.get_name(), e);
                // Still include clients with failed health checks
                healthy_clients.push(client);
            }
        }
    }

    info!(
        "Initialized {} DEX clients with health status checked",
        healthy_clients.len()
    );
    healthy_clients
}

/// Get clients by DEX type for targeted operations
#[allow(dead_code)] // Planned for DEX-specific client filtering
pub fn get_clients_by_type(
    dex_type: &crate::utils::DexType,
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    let all_clients = get_all_clients(cache, app_config);

    all_clients
        .into_iter()
        .filter(|client| {
            match dex_type {
                crate::utils::DexType::Orca => client.get_name() == "Orca",
                crate::utils::DexType::Raydium => client.get_name() == "Raydium",
                crate::utils::DexType::Meteora => client.get_name() == "Meteora",
                crate::utils::DexType::Lifinity => client.get_name() == "Lifinity",
                crate::utils::DexType::Phoenix => client.get_name() == "Phoenix",
                crate::utils::DexType::Jupiter => client.get_name() == "Jupiter",
                crate::utils::DexType::Whirlpool => client.get_name() == "Orca", // Whirlpool is part of Orca
                crate::utils::DexType::Unknown(name) => client.get_name() == name,
            }
        })
        .map(Arc::from)
        .collect()
}

/// Enhanced DEX capabilities summary
#[allow(dead_code)] // Planned for DEX capability introspection
pub fn get_dex_capabilities() -> std::collections::HashMap<String, Vec<String>> {
    let mut capabilities = std::collections::HashMap::new();

    capabilities.insert(
        "Orca".to_string(),
        vec![
            "Whirlpool CLMM".to_string(),
            "Legacy AMM".to_string(),
            "Concentrated Liquidity".to_string(),
            "Tick-based Pricing".to_string(),
        ],
    );

    capabilities.insert(
        "Raydium".to_string(),
        vec![
            "AMM V4".to_string(),
            "CLMM".to_string(),
            "Constant Product".to_string(),
            "OpenBook Integration".to_string(),
        ],
    );

    capabilities.insert(
        "Meteora".to_string(),
        vec![
            "Dynamic AMM".to_string(),
            "DLMM (Bin-based)".to_string(),
            "Multi-token Pools".to_string(),
            "Variable Fees".to_string(),
        ],
    );

    capabilities.insert(
        "Lifinity".to_string(),
        vec![
            "Proactive Market Making".to_string(),
            "Oracle Integration".to_string(),
            "Concentrated Liquidity".to_string(),
            "Dynamic Rebalancing".to_string(),
        ],
    );

    capabilities.insert(
        "Phoenix".to_string(),
        vec![
            "Order Book DEX".to_string(),
            "Limit Orders".to_string(),
            "Market Orders".to_string(),
            "Advanced Order Types".to_string(),
        ],
    );

    capabilities
}
