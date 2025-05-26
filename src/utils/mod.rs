// src/utils/mod.rs
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    // signer::Signer, // Still marked as unused by compiler if not used locally
};
use std::error::Error as StdError; // Aliased to avoid conflict with local Error
use log::{info, error};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
}

// Default implementation for PoolInfo for easier testing or default states
impl Default for PoolInfo {
    fn default() -> Self {
        Self {
            address: Pubkey::default(),
            name: "Default Pool".to_string(),
            token_a: PoolToken::default(),
            token_b: PoolToken::default(),
            fee_numerator: 0,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Unknown("Default".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub reserve: u64,
}

// Default implementation for PoolToken
impl Default for PoolToken {
    fn default() -> Self {
        Self {
            mint: Pubkey::default(),
            symbol: "DEFAULT".to_string(),
            decimals: 0,
            reserve: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgramConfig {
    pub name: String,
    pub version: String,
    // Add other fields for program configuration as they become necessary
}

impl ProgramConfig {
    // Constructor for ProgramConfig
    pub fn new(name: String, version: String) -> Self {
        Self { name, version }
    }

    // Method to log config, ensuring it's "used"
    pub fn log_details(&self) {
        info!("ProgramConfig Details: Name={}, Version={}", self.name, self.version);
    }
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

pub fn load_keypair(path: &str) -> Result<Keypair, Box<dyn StdError>> {
    match read_keypair_file(path) {
        Ok(kp) => {
            info!("Successfully loaded keypair from: {}", path);
            Ok(kp)
        }
        Err(e) => {
            let error_msg = format!("Failed to load keypair from path '{}': {}", path, e.to_string());
            error!("{}", error_msg);
            Err(error_msg.into())
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
     // Adding a from_float constructor for convenience
    pub fn from_float(float_amount: f64, decimals: u8) -> Self {
        Self {
            amount: (float_amount * 10f64.powi(decimals as i32)) as u64,
            decimals,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    if input_reserve_val == 0 || output_reserve_val == 0 || input_amount.amount == 0 || pool.fee_denominator == 0 {
        return TokenAmount::new(0, output_decimals_val);
    }
    
    let fee_rate = pool.fee_numerator as f64 / pool.fee_denominator as f64;
    let input_amount_after_fee = input_amount.amount as f64 * (1.0 - fee_rate);

    if input_reserve_val as f64 + input_amount_after_fee == 0.0 {
        return TokenAmount::new(0, output_decimals_val);
    }

    let output_amount_precise = (output_reserve_val as f64 * input_amount_after_fee)
        / (input_reserve_val as f64 + input_amount_after_fee);

    TokenAmount::new(output_amount_precise.floor() as u64, output_decimals_val)
}


// calculate_multihop_profit_and_slippage and calculate_rebate were moved to arbitrage::calculator
// as they are more specific to arbitrage calculations than general utilities.
// This also helps resolve circular dependency issues if utils needs to be used by calculator.