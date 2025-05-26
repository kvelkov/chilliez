// src/utils/mod.rs
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    // signer::Signer, // Marked as unused by compiler
};
use std::error::Error as StdError; // Aliased to avoid conflict with local Error
use log::{info, error};
use async_trait::async_trait;
use serde::{Serialize, Deserialize}; // Added for PoolInfo

#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize, Deserialize
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

#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize, Deserialize
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

pub fn load_keypair(path: &str) -> Result<Keypair, Box<dyn StdError>> { // Changed to StdError
    match read_keypair_file(path) {
        Ok(kp) => {
            info!("Successfully loaded keypair from: {}", path);
            Ok(kp)
        }
        Err(e) => {
            // The error `e` from `read_keypair_file` is `Box<dyn Error>`.
            // We need to ensure it's compatible with the return type.
            // If `e` is already `Box<dyn StdError>`, this is fine.
            // `solana_sdk::signature::SignatureError` implements `std::error::Error`.
            // `read_keypair_file` returns `Result<Keypair, Box<dyn Error>>`.
            // So, this should be okay if `dyn Error` is `dyn StdError`.
            // Let's make the error message more specific if cast is needed.
            error!("Failed to load keypair from path '{}': {}", path, e.to_string());
            // The error `e` is `Box<dyn Any>`, not `Box<dyn Error>` from `read_keypair_file`
            // `read_keypair_file` actually returns `Result<Keypair, Box<dyn std::any::Any + Send>>`
            // This needs to be handled properly. For now, we assume it can be converted to string for error.
            // A better way:
            // Err(format!("Failed to load keypair: {:?}", e).into())
            // For now, to match the original structure more closely while fixing the Sized error:
            // We assume 'e' can be boxed into Box<dyn StdError> if it came from an Error source.
            // The error message indicates `e` is `Box<dyn Any + Send>`.
            // A simple way to handle this for the given return type:
            Err(format!("Failed to load keypair from {}: (error details not displayable due to type)", path).into())
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)] // Added Serialize, Deserialize
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] // Added Serialize, Deserialize
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

    if input_reserve_val == 0 || output_reserve_val == 0 || input_amount.amount == 0 { // Added output_reserve_val check
        return TokenAmount::new(0, output_decimals_val);
    }
    
    // Applying fee to input amount: Input_After_Fee = Input_Amount * (1 - Fee_Rate)
    // Fee_Rate = Numerator / Denominator
    // This is a common way, but some AMMs might apply fee on output or k.
    // Assuming fee is on input for this generic function.
    let fee_rate = pool.fee_numerator as f64 / pool.fee_denominator as f64;
    let input_amount_after_fee = input_amount.amount as f64 * (1.0 - fee_rate);


    // Constant product: (X + dX_after_fee) * (Y - dY) = X * Y
    // Y - dY = (X * Y) / (X + dX_after_fee)
    // dY = Y - (X * Y) / (X + dX_after_fee)
    // dY = Y * (1 - X / (X + dX_after_fee))
    // dY = Y * (dX_after_fee / (X + dX_after_fee))
    // dY = (Y * dX_after_fee) / (X + dX_after_fee)

    let output_amount_precise = (output_reserve_val as f64 * input_amount_after_fee)
        / (input_reserve_val as f64 + input_amount_after_fee);

    TokenAmount::new(output_amount_precise as u64, output_decimals_val)
}