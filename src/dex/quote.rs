// src/dex/quote.rs

use crate::utils::PoolInfo;
use serde::{Deserialize, Serialize};
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

/// Represents a calculated quote for a token swap based on on-chain data.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// The new DexClient trait defines the interface for all on-chain DEX interactions.
/// Implementing clients must provide methods to calculate quotes from on-chain data
/// and build the raw transaction instructions for a swap.
pub trait DexClient: Send + Sync {
    /// Returns the name of the DEX client (e.g., "Orca", "Raydium").
    fn get_name(&self) -> &str;

    /// Calculates the expected output amount for a swap given a specific pool's on-chain state.
    ///
    /// # Arguments
    /// * `pool` - A `PoolInfo` struct containing the live on-chain data for the liquidity pool.
    /// * `input_amount` - The amount of the input token to be swapped.
    ///
    /// # Returns
    /// A `Result` containing the calculated `Quote` or an error.
    fn calculate_onchain_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> anyhow::Result<Quote>;

    /// Constructs the Solana `Instruction` required to perform the swap.
    ///
    /// # Arguments
    /// * `swap_info` - A struct containing all necessary Pubkeys and data for the swap.
    ///
    /// # Returns
    /// A `Result` containing the composed `Instruction` or an error.
    fn get_swap_instruction(
        &self,
        swap_info: &SwapInfo,
    ) -> anyhow::Result<Instruction>;
}

/// A struct to hold all the necessary information for building a swap instruction.
#[derive(Debug, Clone)]
pub struct SwapInfo<'a> {
    pub dex_name: &'a str,
    pub pool: &'a PoolInfo,
    pub user_wallet: &'a Pubkey,
    pub user_source_token_account: &'a Pubkey,
    pub user_destination_token_account: &'a Pubkey,
    pub amount_in: u64,
    // Add any other fields that might be universally required, like slippage settings.
    // pub slippage_bps: u16,
}


// ---- Helper Methods for Quote ----

impl Quote {
    /// Calculates the profit (output minus input) as a signed integer, assuming same decimals.
    pub fn profit(&self) -> i64 {
        self.output_amount as i64 - self.input_amount as i64
    }

    /// Calculates the profit percentage, returning 0.0 if input_amount is zero.
    pub fn profit_pct(&self) -> f64 {
        if self.input_amount == 0 {
            0.0
        } else {
            (self.output_amount as f64 - self.input_amount as f64) / self.input_amount as f64 * 100.0
        }
    }

    /// Returns the output amount converted to a float, given the token's decimal precision.
    pub fn output_as_float(&self, decimals: u8) -> f64 {
        self.output_amount as f64 / 10f64.powi(decimals as i32)
    }

    /// Returns the input amount converted to a float, given the token's decimal precision.
    pub fn input_as_float(&self, decimals: u8) -> f64 {
        self.input_amount as f64 / 10f64.powi(decimals as i32)
    }
}