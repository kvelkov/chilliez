//! Wallet Management Module
//! 
//! This module provides wallet management functionality including:
//! - Ephemeral wallet pools for arbitrage operations
//! - Helper utilities for token account management
//! - Secure key management and fund sweeping
//! - Integration with Jito for MEV-protected execution

pub mod helper;
pub mod wallet_pool;
pub mod integration;

#[cfg(test)]
mod tests;

// Re-export main components
pub use wallet_pool::{
    WalletPool, 
    WalletPoolConfig, 
    EphemeralWallet, 
    WalletPoolStats
};
pub use helper::get_or_create_ata;
pub use integration::{
    WalletJitoIntegration,
    WalletJitoConfig,
    IntegratedStats,
    IntegratedSystemStats,
};