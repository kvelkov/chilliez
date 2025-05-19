pub mod dex_api_templates;
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
pub mod whirlpool;

pub use crate::dex::quote::DexClient;
use log::info;
use std::sync::Arc;

use crate::dex::lifinity::LifinityClient;
use crate::dex::meteora::MeteoraClient;
use crate::dex::orca::OrcaClient;
use crate::dex::phoenix::PhoenixClient;
use crate::dex::raydium::RaydiumClient;

/// Synchronously get all DEX client instances.
/// Each client is boxed as a trait object.
pub fn get_all_clients() -> Vec<Box<dyn DexClient>> {
    let mut clients: Vec<Box<dyn DexClient>> = Vec::new();

    // Add Orca Client
    clients.push(Box::new(OrcaClient::new()));
    info!("Orca client initialized.");

    // Add Raydium Client
    clients.push(Box::new(RaydiumClient::new()));
    info!("Raydium Pro client initialized.");

    // Add Meteora Client
    clients.push(Box::new(MeteoraClient::new()));
    info!("Meteora client initialized.");

    // Add Lifinity Client
    clients.push(Box::new(LifinityClient::new()));
    info!("Lifinity client initialized.");

    // Add Phoenix Client
    clients.push(Box::new(PhoenixClient::new()));
    info!("Phoenix client initialized.");

    info!(
        "Total {} DEX clients initialized for get_all_clients.",
        clients.len()
    );
    clients
}

/// Asynchronously get all DEX client instances, wrapped in Arc.
/// This is suitable for concurrent usage.
pub async fn get_all_clients_arc() -> Vec<Arc<dyn DexClient>> {
    // For now, this wraps the synchronous get_all_clients.
    // If client initialization becomes async, this function's body will change.
    get_all_clients().into_iter().map(Arc::from).collect()
}

