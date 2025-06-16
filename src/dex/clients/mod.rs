// src/dex/clients/mod.rs
//! DEX client implementations for various Solana DEXs.
//! Each client implements the DexClient and PoolDiscoverable traits.

pub mod orca;
pub mod raydium;
pub mod meteora;
pub mod lifinity;
pub mod phoenix;
pub mod jupiter; // Jupiter aggregator for additional liquidity

// Re-export client structs for easier access
pub use orca::OrcaClient; // OrcaPoolParser only used in pool_management.rs
pub use raydium::RaydiumClient;
pub use meteora::MeteoraClient; // MeteoraPoolParser only used in pool_management.rs
pub use lifinity::LifinityClient; // LifinityPoolParser only used in pool_management.rs
// Note: PhoenixClient is implemented but not currently used in get_all_* functions
// Uncomment when Phoenix integration is activated:
// pub use phoenix::{PhoenixClient, PhoenixPoolParser};
pub use jupiter::JupiterClient;
