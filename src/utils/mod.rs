// src/utils/mod.rs
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    signer::Signer,
};
use std::error::Error;
use log::{info, error};
use async_trait::async_trait; // Added for PoolParser

// Define PoolInfo, ProgramConfig, etc. as needed by the rest of the application
// Placeholder structs - these should be defined based on their actual usage.

#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
    pub last_update_timestamp: u64,
    pub dex_type: DexType, // Now uses the cloneable DexType
}

#[derive(Debug, Clone)]
pub struct PoolToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub reserve: u64,
}


#[derive(Debug, Clone)]
pub struct ProgramConfig {
    // Add fields for program configuration
}

pub fn setup_logging() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .level_for("solana_rbpf", log::LevelFilter::Warn)
        .level_for("solana_runtime::message_processor", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()?;
    info!("Logging initialized.");
    Ok(())
}

pub fn load_keypair(path: &str) -> Result<Keypair, Box<dyn Error>> {
    match read_keypair_file(path) {
        Ok(kp) => {
            info!("Successfully loaded keypair from: {}", path);
            Ok(kp)
        }
        Err(e) => {
            error!("Failed to load keypair from path '{}': {}", path, e);
            Err(Box::new(e))
        }
    }
}

pub fn calculate_multihop_profit_and_slippage(
    _pools: &[&PoolInfo],
    _input_amount: f64,
    _directions: &[bool],
    _last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> (f64, f64, f64) {
    (0.0, 0.0, 0.0)
}

pub fn calculate_rebate(
    _pools: &[&PoolInfo],
    _amounts: &[TokenAmount]
) -> f64 {
    0.0
}

#[derive(Debug, Clone, Copy)]
pub struct TokenAmount{
    pub amount: u64,
    pub decimals: u8,
}

impl TokenAmount {
    pub fn new(amount: u64, decimals: u8) -> Self {
        Self { amount, decimals }
    }

    pub fn to_float(&self) -> f64 {
        self.amount as f64 / 10f64.powi(self.decimals as i32)
    }
}

// Made DexType Cloneable
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DexType {
    Orca,
    Raydium,
    Lifinity,
    Meteora,
    Phoenix,
    Whirlpool,
    Unknown(String),
}

#[async_trait]
pub trait PoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo>;
    fn get_program_id() -> Pubkey;
    fn get_dex_type() -> DexType;
}

pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    let (input_reserve_val, output_reserve_val, output_decimals_val) = if is_a_to_b {
        (pool.token_a.reserve, pool.token_b.reserve, pool.token_b.decimals)
    } else {
        (pool.token_b.reserve, pool.token_a.reserve, pool.token_a.decimals)
    };

    if input_reserve_val == 0 || input_amount.amount == 0 {
        return TokenAmount::new(0, output_decimals_val);
    }

    let output_amount_u64 = (output_reserve_val as u128 * input_amount.amount as u128)
        / (input_reserve_val as u128 + input_amount.amount as u128);

    TokenAmount::new(output_amount_u64 as u64, output_decimals_val)
}