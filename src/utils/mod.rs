//! Common utility structures, enums, traits, and functions used throughout the arbitrage application.

pub mod timing; // Add timing module

use crate::solana::rpc::SolanaRpcClient;
use anyhow::Result;
use async_trait::async_trait;
use chrono;
use log::{error, info};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::read_keypair_file;
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

/// Represents information about a liquidity pool on a DEX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub fee_numerator: Option<u64>,
    pub fee_denominator: Option<u64>,
    pub fee_rate_bips: Option<u16>,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
    // CLMM specific fields
    pub liquidity: Option<u128>,
    pub sqrt_price: Option<u128>,
    pub tick_current_index: Option<i32>,
    pub tick_spacing: Option<u16>,
    // Orca Whirlpool specific fields
    pub tick_array_0: Option<Pubkey>,
    pub tick_array_1: Option<Pubkey>,
    pub tick_array_2: Option<Pubkey>,
    pub oracle: Option<Pubkey>,
}

impl Default for PoolInfo {
    fn default() -> Self {
        Self {
            address: Pubkey::default(),
            name: String::new(),
            token_a: PoolToken::default(),
            token_b: PoolToken::default(),
            token_a_vault: Pubkey::default(),
            token_b_vault: Pubkey::default(),
            fee_numerator: None,
            fee_denominator: None,
            fee_rate_bips: None,
            last_update_timestamp: 0,
            dex_type: DexType::Unknown("Unknown".to_string()),
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        }
    }
}

/// Represents a token within a liquidity pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub reserve: u64,
}

impl Default for PoolToken {
    fn default() -> Self {
        Self {
            mint: Pubkey::default(),
            symbol: String::new(),
            decimals: 6,
            reserve: 0,
        }
    }
}

/// Program configuration structure
#[derive(Debug, Clone)]
pub struct ProgramConfig {
    pub name: String,
    pub version: String,
}

/// Setup logging for the application
pub fn setup_logging() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                match record.level() {
                    log::Level::Error => "❌ ERROR",
                    log::Level::Warn => "⚠️  WARN ",
                    log::Level::Info => "ℹ️  INFO ",
                    log::Level::Debug => "🔍 DEBUG",
                    log::Level::Trace => "🔬 TRACE",
                },
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .level_for("solana_rbpf", log::LevelFilter::Warn)
        .level_for("solana_runtime::message_processor", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()?;
    info!("🚀 Logging system initialized with enhanced formatting");
    Ok(())
}

#[allow(dead_code)] // Utility function for development/testing
pub fn load_keypair(path: &str) -> Result<Keypair, Box<dyn StdError>> {
    match read_keypair_file(path) {
        Ok(kp) => {
            info!("Successfully loaded keypair from: {}", path);
            Ok(kp)
        }
        Err(e) => {
            let error_msg = format!("Failed to load keypair from path '{}': {}", path, e);
            error!("{}", error_msg);
            Err(error_msg.into())
        }
    }
}

/// Represents a token amount with its associated decimals.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenAmount {
    pub amount: u64,
    pub decimals: u8,
}

impl TokenAmount {
    /// Create a new TokenAmount with the specified amount and decimals
    pub fn new(amount: u64, decimals: u8) -> Self {
        Self { amount, decimals }
    }

    /// Convert the token amount to a floating-point representation
    pub fn to_float(&self) -> f64 {
        self.amount as f64 / 10_f64.powi(self.decimals as i32)
    }

    /// Create a TokenAmount from a floating-point value
    pub fn from_float(value: f64, decimals: u8) -> Self {
        let amount = (value * 10_f64.powi(decimals as i32)) as u64;
        Self { amount, decimals }
    }
}

/// Represents the type of decentralized exchange (DEX).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexType {
    Orca,
    Raydium,
    Lifinity,
    Meteora,
    Phoenix,
    Jupiter, // Jupiter aggregator
    Whirlpool,
    Unknown(String),
}

impl fmt::Display for DexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DexType::Orca => write!(f, "Orca"),
            DexType::Raydium => write!(f, "Raydium"),
            DexType::Lifinity => write!(f, "Lifinity"),
            DexType::Meteora => write!(f, "Meteora"),
            DexType::Phoenix => write!(f, "Phoenix"),
            DexType::Jupiter => write!(f, "Jupiter"),
            DexType::Whirlpool => write!(f, "Whirlpool"),
            DexType::Unknown(name) => write!(f, "Unknown({})", name),
        }
    }
}

/// Trait for parsing pool data from different DEXs.
#[async_trait]
pub trait PoolParser: Send + Sync {
    /// Parse pool data asynchronously and return a PoolInfo struct.
    /// Updated to use Arc<SolanaRpcClient> to match implementation expectations
    async fn parse_pool_data(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> Result<PoolInfo>;

    /// Parse pool data synchronously (fallback method)
    // Fallback method for synchronous parsing
    fn parse_pool_data_sync(
        &self,
        _pool_address: Pubkey,
        _data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> Result<PoolInfo> {
        // Default implementation - should be overridden by specific parsers
        Err(anyhow::anyhow!("Synchronous parsing not implemented"))
    }

    /// Get the program ID of the DEX
    fn get_program_id(&self) -> Pubkey;
}

/// Calculate output amount for a swap given pool information
// Utility function for quote calculations
pub fn calculate_output_amount(pool: &PoolInfo, input_amount: u64, is_a_to_b: bool) -> Result<u64> {
    let (input_reserve, output_reserve) = if is_a_to_b {
        (pool.token_a.reserve, pool.token_b.reserve)
    } else {
        (pool.token_b.reserve, pool.token_a.reserve)
    };

    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow::anyhow!("Pool has zero reserves"));
    }

    // Simple constant product formula: x * y = k
    // output = (output_reserve * input_amount) / (input_reserve + input_amount)
    let fee_rate = pool.fee_rate_bips.unwrap_or(25) as f64 / 10000.0;
    let input_after_fee = (input_amount as f64) * (1.0 - fee_rate);

    let output_amount =
        (output_reserve as f64 * input_after_fee) / (input_reserve as f64 + input_after_fee);

    Ok(output_amount as u64)
}

/// Test struct to verify PoolInfo field definitions
#[cfg(test)]
#[derive(Debug, Clone)]
// Used for field definition verification in tests
pub struct TestPoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub fee_numerator: Option<u64>,
    pub fee_denominator: Option<u64>,
    pub fee_rate_bips: Option<u16>,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
    pub liquidity: Option<u128>,
    pub sqrt_price: Option<u128>,
    pub tick_current_index: Option<i32>,
    pub tick_spacing: Option<u16>,
    pub tick_array_0: Option<Pubkey>,
    pub tick_array_1: Option<Pubkey>,
    pub tick_array_2: Option<Pubkey>,
    pub oracle: Option<Pubkey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_amount_conversion() {
        let amount = TokenAmount {
            amount: 1000000,
            decimals: 6,
        };
        assert_eq!(amount.to_float(), 1.0);

        let from_float = TokenAmount::from_float(1.5, 6);
        assert_eq!(from_float.amount, 1500000);
        assert_eq!(from_float.decimals, 6);
    }

    #[test]
    fn test_dex_type_equality() {
        assert_eq!(DexType::Orca, DexType::Orca);
        assert_ne!(DexType::Orca, DexType::Raydium);
        assert_eq!(
            DexType::Unknown("Test".to_string()),
            DexType::Unknown("Test".to_string())
        );
    }

    #[test]
    fn test_pool_info_default() {
        let pool = PoolInfo::default();
        assert_eq!(pool.address, Pubkey::default());
        assert_eq!(pool.name, "");
        assert_eq!(pool.token_a.decimals, 6);
        assert_eq!(pool.token_b.decimals, 6);
    }
}
