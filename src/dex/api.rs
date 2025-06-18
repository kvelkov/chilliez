// src/dex/api.rs
//! DEX API infrastructure: DexClient trait, Quote structures, and enhanced swap interfaces.
//! Consolidates quote.rs and quoting_engine.rs for better organization.

use crate::utils::PoolInfo;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use std::sync::Arc;

// =====================================================================================
// QUOTE STRUCTURES AND TRAITS
// =====================================================================================

/// Represents a calculated quote for a token swap based on on-chain data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Quote {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    /// The market addresses involved in the swap.
    pub route: Vec<Pubkey>,
    /// Optional estimate of slippage percentage.
    pub slippage_estimate: Option<f64>,
}

/// The DexClient trait defines the interface for all on-chain DEX interactions.
#[async_trait]
pub trait DexClient: Send + Sync {
    /// Returns the name of the DEX client (e.g., "Orca", "Raydium").
    fn get_name(&self) -> &str;

    /// Calculates the expected output amount for a swap given a specific pool's on-chain state.
    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> Result<Quote>;

    /// Constructs the Solana `Instruction` required to perform the swap.
    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> Result<Instruction>;

    /// Enhanced swap instruction method with simplified interface
    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError>;

    /// Discovers all supported liquidity pools for the DEX.
    async fn discover_pools(&self) -> Result<Vec<PoolInfo>>;

    /// Health check for the DEX client
    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError>;
}

/// Health status for a DEX client
#[derive(Debug, Clone)]
pub struct DexHealthStatus {
    pub is_healthy: bool,
    pub last_successful_request: Option<std::time::Instant>,
    pub error_count: u32,
    pub response_time_ms: Option<u64>,
    pub pool_count: Option<usize>,
    pub status_message: String,
}

/// Separate trait for pool discovery to maintain object safety of DexClient
#[async_trait]
pub trait PoolDiscoverable: Send + Sync {
    /// Discovers all supported liquidity pools for the DEX.
    async fn discover_pools(&self) -> Result<Vec<PoolInfo>>;

    /// Fetches updated data for a specific pool by its address
    #[allow(dead_code)] // Method is part of the trait contract, may be used in the future
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> Result<PoolInfo>;

    /// Returns the DEX name for this discoverable client
    fn dex_name(&self) -> &str;

    /// Returns a reference to self as &dyn Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A struct to hold all the necessary information for building a swap instruction.
/// This has been expanded to support complex DEXs like Raydium.
#[derive(Debug, Clone)]
pub struct SwapInfo<'a> {
    // General info
    pub dex_name: &'a str,
    pub pool: &'a PoolInfo,

    // User accounts
    pub user_wallet: Pubkey,
    pub user_source_token_account: Pubkey,
    pub user_destination_token_account: Pubkey,
    pub user_owner: Pubkey, // Often the same as user_wallet, but required as a signer

    // Swap amounts
    pub amount_in: u64,
    pub min_output_amount: u64,

    // Pool-specific accounts
    pub pool_account: Pubkey,
    pub pool_authority: Pubkey,
    pub pool_open_orders: Pubkey,
    pub pool_target_orders: Pubkey,
    pub pool_base_vault: Pubkey,
    pub pool_quote_vault: Pubkey,

    // Market-specific accounts (for DEXs that use Serum/OpenBook)
    pub market_program_id: Pubkey,
    pub market_id: Pubkey,
    pub market_bids: Pubkey,
    pub market_asks: Pubkey,
    pub market_event_queue: Pubkey,
    pub market_base_vault: Pubkey,
    pub market_quote_vault: Pubkey,
    pub market_authority: Pubkey,
}

/// A simplified swap info structure that contains the essential information
/// needed for building swap instructions across different DEXs.
/// This provides a cleaner interface compared to the more complex SwapInfo.
#[derive(Debug, Clone)]
pub struct CommonSwapInfo {
    pub user_wallet_pubkey: Pubkey,
    pub user_source_token_account: Pubkey,
    pub user_destination_token_account: Pubkey,
    pub source_token_mint: Pubkey,
    pub destination_token_mint: Pubkey,
    pub input_amount: u64,
    pub minimum_output_amount: u64,
    pub slippage_bps: Option<u16>, // Added for Jupiter and general use
    pub priority_fee_lamports: Option<u64>, // Added for Jupiter and general use
}

// =====================================================================================
// HELPER IMPLEMENTATIONS
// =====================================================================================

impl Quote {
    /// Calculate profit from the quote
    pub fn profit(&self) -> i64 {
        self.output_amount as i64 - self.input_amount as i64
    }

    /// Calculate profit percentage
    pub fn profit_pct(&self) -> f64 {
        if self.input_amount == 0 {
            return 0.0;
        }
        ((self.output_amount as f64 - self.input_amount as f64) / self.input_amount as f64) * 100.0
    }

    /// Convert output amount to float with given decimals
    pub fn output_as_float(&self, decimals: u8) -> f64 {
        self.output_amount as f64 / 10_f64.powi(decimals as i32)
    }

    /// Convert input amount to float with given decimals
    pub fn input_as_float(&self, decimals: u8) -> f64 {
        self.input_amount as f64 / 10_f64.powi(decimals as i32)
    }
}

impl PoolInfo {
    /// Get sqrt price for CLMM pools
    pub fn get_sqrt_price(&self) -> u128 {
        self.sqrt_price.unwrap_or(0)
    }

    /// Get liquidity for CLMM pools
    pub fn get_liquidity(&self) -> u128 {
        self.liquidity.unwrap_or(0)
    }

    /// Get current tick index for CLMM pools
    pub fn get_tick_current_index(&self) -> i32 {
        self.tick_current_index.unwrap_or(0)
    }

    /// Get tick spacing for CLMM pools
    pub fn get_tick_spacing(&self) -> u16 {
        self.tick_spacing.unwrap_or(1)
    }

    /// Get fee rate in basis points
    pub fn get_fee_rate_bips(&self) -> u16 {
        self.fee_rate_bips.unwrap_or(30)
    }
}
