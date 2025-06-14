// src/dex/clients/mod.rs
//! DEX client implementations for various Solana DEXs.
//! Each client implements the DexClient and PoolDiscoverable traits.

pub mod orca;
pub mod raydium;
pub mod meteora;
pub mod lifinity;
pub mod phoenix;

// Re-export client structs for easier access
pub use orca::{OrcaClient, OrcaPoolParser};
pub use raydium::RaydiumClient;
pub use meteora::{MeteoraClient, MeteoraPoolParser};
pub use lifinity::{LifinityClient, LifinityPoolParser};
pub use phoenix::{PhoenixClient, PhoenixPoolParser};
