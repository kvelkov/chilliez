// src/utils/mod.rs
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    signer::Signer,
};
use std::error::Error;
use log::{info, error};

// Define PoolInfo, ProgramConfig, etc. as needed by the rest of the application
// Placeholder structs - these should be defined based on their actual usage.

#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub address: Pubkey,
    // Add other fields that define a pool
    // Added based on other files, these are common fields for PoolInfo
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
}

#[derive(Debug, Clone)]
pub struct PoolToken { // Added this struct based on PoolInfo usage
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
        .level(log::LevelFilter::Info) // Set default log level
        .level_for("solana_rbpf", log::LevelFilter::Warn) // Example: Quieter RBPF logs
        .level_for("solana_runtime::message_processor", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        // .chain(fern::log_file("output.log")?) // Optionally log to a file
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

// Add other utility functions that were previously missing, for example:
// calculate_multihop_profit_and_slippage, calculate_rebate, etc.
// For now, these will be placeholders.

pub fn calculate_multihop_profit_and_slippage(
    _pools: &[&PoolInfo], // Added type hint based on detector.rs
    _input_amount: f64, // Added type hint based on detector.rs
    _directions: &[bool], // Added type hint based on detector.rs
    _last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)], // Added type hint based on detector.rs
) -> (f64, f64, f64) { // Added return type based on detector.rs
    // Implementation
    (0.0, 0.0, 0.0) // Placeholder return
}

pub fn calculate_rebate(
    _pools: &[&PoolInfo], // Added type hint based on detector.rs
    _amounts: &[TokenAmount] // Added type hint based on detector.rs
) -> f64 { // Added return type based on detector.rs
    // Implementation
    0.0 // Placeholder return
}

// Placeholder for TokenAmount if it's a simple type alias or struct used in utils
#[derive(Debug, Clone, Copy)] // Added Copy based on usage
pub struct TokenAmount{ // Changed from simple u64 based on detector.rs usage
    pub amount: u64,
    pub decimals: u8,
}

impl TokenAmount {
    pub fn new(amount: u64, decimals: u8) -> Self {
        Self { amount, decimals }
    }

    pub fn to_float(&self) -> f64 { // Added based on detector.rs usage
        self.amount as f64 / 10f64.powi(self.decimals as i32)
    }
}


// Placeholder for DexType enum if it's used within utils or passed around
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DexType {
    Orca,
    Raydium,
    Lifinity,
    Meteora, // Added based on project_map.md
    Phoenix, // Added based on project_map.md
    Whirlpool, // Added based on project_map.md
    Unknown(String), // Added based on arbitrage/tests.rs
}

// Added PoolParser trait definition based on its usage in other dex modules
#[async_trait::async_trait] // Assuming it might need to be async later
pub trait PoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo>;
    fn get_program_id() -> Pubkey;
    fn get_dex_type() -> DexType;
}

// Added calculate_output_amount function based on its usage in detector.rs
pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    // Placeholder implementation.
    // This should reflect the actual AMM logic (e.g., constant product).
    let (input_reserve_val, output_reserve_val, output_decimals_val) = if is_a_to_b {
        (pool.token_a.reserve, pool.token_b.reserve, pool.token_b.decimals)
    } else {
        (pool.token_b.reserve, pool.token_a.reserve, pool.token_a.decimals)
    };

    if input_reserve_val == 0 || input_amount.amount == 0 {
        return TokenAmount::new(0, output_decimals_val);
    }

    // Simple constant product formula: (X * Y = K)
    // Output = (Y * input_X) / (X + input_X)
    // This ignores fees and slippage for simplicity here.
    let output_amount_u64 = (output_reserve_val as u128 * input_amount.amount as u128)
        / (input_reserve_val as u128 + input_amount.amount as u128);

    TokenAmount::new(output_amount_u64 as u64, output_decimals_val)
}