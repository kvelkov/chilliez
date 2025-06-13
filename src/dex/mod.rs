// src/dex/mod.rs


// src/dex/mod.rs


#[cfg(any(test, debug_assertions))]
pub mod integration_test;

pub mod lifinity;
pub mod meteora;
#[cfg(test)]
pub mod meteora_test;
pub mod orca; // Ensures the orca.rs file is included as a module
#[cfg(test)]
pub mod orca_test;
// pub mod phoenix;
pub mod pool;
pub mod quote;
pub mod raydium; // Ensures the raydium.rs file is included as a module
#[cfg(test)]
pub mod raydium_test;
pub mod whirlpool;
pub mod whirlpool_parser;
pub mod banned_pairs;
// Module declarations for each DEX client and utility

// Re-export the main DexClient trait for easier access
pub use quote::DexClient;
// pub use banned_pairs::{BannedPairsManager, BannedPairFilteringDexClientDecorator}; // These are unused as top-level exports


// --- Publicly re-export concrete client types ---
// These lines make the client structs available directly under the `dex` module,
// e.g., as `crate::dex::OrcaClient`
// The following re-exports are marked as unused by the compiler.
// They are not strictly necessary if `get_all_clients` is the primary way clients are obtained,
// or if external modules use the full path like `crate::dex::orca::OrcaClient`.
// Removing them to satisfy the compiler warning.
// pub use self::lifinity::LifinityClient;
// pub use self::meteora::MeteoraClient;
// pub use self::orca::OrcaClient;
// pub use self::phoenix::PhoenixClient;
// pub use self::raydium::RaydiumClient;
// pub use self::whirlpool::WhirlpoolClient;

// (Keep your existing imports for get_all_clients, etc.)
use crate::cache::Cache;
use crate::config::settings::Config;
use log::info;
use std::sync::Arc;

// The get_all_clients function can remain as is,
// as it internally uses the full paths to the clients.

/// Initializes and returns all supported DEX API client instances.
/// Each client is configured with shared cache and application configuration.
pub fn get_all_clients(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Box<dyn DexClient>> {
    let mut clients: Vec<Box<dyn DexClient>> = Vec::new();

    info!("Initializing DEX API clients with Cache and Config integration...");

    let get_dex_ttl = |dex_name: &str| -> Option<u64> {
        app_config.dex_quote_cache_ttl_secs
            .as_ref()
            .and_then(|map| map.get(dex_name).copied())
            .or(Some(app_config.redis_default_ttl_secs))
    };

    // These instantiations will work if OrcaClient, RaydiumClient etc. are correctly defined
    // in their respective modules (e.g. orca.rs, raydium.rs)
    clients.push(Box::new(orca::OrcaClient::new()));
    info!("- Orca client initialized.");

    clients.push(Box::new(raydium::RaydiumClient::new()));
    info!("- Raydium client initialized.");

    clients.push(Box::new(meteora::MeteoraClient::new()));
    info!("- Meteora client initialized.");

    clients.push(Box::new(lifinity::LifinityClient::new()));
    info!("- Lifinity client initialized.");

    // clients.push(Box::new(phoenix::PhoenixClient::new(
    //     Arc::clone(&cache),
    //     get_dex_ttl("Phoenix"),
    // )));
    // info!("- Phoenix client initialized.");

    clients.push(Box::new(whirlpool::WhirlpoolClient::new(
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
pub async fn get_all_clients_arc(
    cache: Arc<Cache>,
    app_config: Arc<Config>,
) -> Vec<Arc<dyn DexClient>> {
    get_all_clients(cache, app_config)
        .into_iter()
        .map(Arc::from)
        .collect()
}