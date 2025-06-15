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
pub use orca::{OrcaClient, OrcaPoolParser};
pub use raydium::RaydiumClient;
pub use meteora::{MeteoraClient, MeteoraPoolParser};
pub use lifinity::{LifinityClient, LifinityPoolParser};
// Note: PhoenixClient is implemented but not currently used in get_all_* functions
// Uncomment when Phoenix integration is activated:
// pub use phoenix::{PhoenixClient, PhoenixPoolParser};
// Note: JupiterClient is implemented but not currently used in get_all_* functions
// Uncomment when Jupiter integration is activated:
// pub use jupiter::JupiterClient;
