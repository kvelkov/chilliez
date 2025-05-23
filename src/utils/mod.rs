// src/utils/mod.rs
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    // signer::Signer, // Removed unused import
};
use std::error::Error as StdError; // Alias to avoid conflict if you have a local Error type

// Required for fern
// Ensure 'fern' is in your Cargo.toml dependencies version 0.6 may be a good choice
use log::{info, error}; // For logging

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)] // Added Serialize, Deserialize for dex/integration_test.rs
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)] // Added Serialize, Deserialize
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
                "[{}][{}] {}", // Corrected string literal
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

pub fn load_keypair(path: &str) -> Result<Keypair, Box<dyn StdError>> { // Using aliased StdError
    match read_keypair_file(path) {
        Ok(kp) => {
            info!("Successfully loaded keypair from: {}", path);
            Ok(kp)
        }
        Err(e) => {
            // The error 'e' from read_keypair_file is a Box<dyn Any + Send>.
            // We need to convert it to Box<dyn StdError>.
            // A simple way is to format its debug output into a new error.
            let err_msg = format!("Failed to load keypair from path '{}': {:?}", path, e);
            error!("{}", err_msg);
            Err(err_msg.into()) // Convert String into Box<dyn StdError> via From implemented for Box
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

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)] // Added Serialize/Deserialize
pub struct TokenAmount {
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

// Removed `Copy` because String in Unknown(String) is not Copy.
// If Copy is strictly needed, Unknown might need to store &str or a fixed-size char array,
// or be represented differently. For now, removing Copy is the simplest fix.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)] // Removed Copy
pub enum DexType {
    Orca,
    Raydium,
    Lifinity,
    Meteora,
    Phoenix,
    Whirlpool, // Added from user's previous manual edit context
    Unknown(String),
}

impl DexType {
    pub fn get_name(&self) -> String {
        match self {
            DexType::Orca => "Orca".to_string(),
            DexType::Raydium => "Raydium".to_string(),
            DexType::Lifinity => "Lifinity".to_string(),
            DexType::Meteora => "Meteora".to_string(), // Added from user's previous manual edit context
            DexType::Phoenix => "Phoenix".to_string(), // Added from user's previous manual edit context
            DexType::Whirlpool => "Whirlpool".to_string(), // Added from user's previous manual edit context
            DexType::Unknown(s) => s.clone(),
        }
    }
}


#[async_trait::async_trait]
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
    let (input_reserve_val, output_reserve_val, output_decimals_val, fee_num, fee_den) = if is_a_to_b {
        (pool.token_a.reserve, pool.token_b.reserve, pool.token_b.decimals, pool.fee_numerator, pool.fee_denominator)
    } else {
        (pool.token_b.reserve, pool.token_a.reserve, pool.token_a.decimals, pool.fee_numerator, pool.fee_denominator)
    };

    if input_reserve_val == 0 || input_amount.amount == 0 || fee_den == 0 {
        return TokenAmount::new(0, output_decimals_val);
    }

    let input_amount_after_fee = input_amount.amount as u128 * (fee_den as u128 - fee_num as u128) / fee_den as u128;

    let output_amount_u128 = (output_reserve_val as u128 * input_amount_after_fee)
        / (input_reserve_val as u128 + input_amount_after_fee);

    TokenAmount::new(output_amount_u128 as u64, output_decimals_val)
}