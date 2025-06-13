// src/utils/mod.rs
use crate::solana::rpc::SolanaRpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
};
use std::error::Error as StdError;
use log::{info, error};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::sync::Arc;

pub mod pool_validation;
pub mod dex_routing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    // Vaults are relevant for many DEXs, including Orca
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    // Fee structure: AMMs might use numerator/denominator, CLMMs like Orca use bips
    pub fee_numerator: Option<u64>,
    pub fee_denominator: Option<u64>,
    pub fee_rate_bips: Option<u16>, // Basis points (0.01%)
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
    // CLMM specific fields (like Orca Whirlpools)
    pub liquidity: Option<u128>,
    pub sqrt_price: Option<u128>,
    pub tick_current_index: Option<i32>,
    pub tick_spacing: Option<u16>,
}

impl Default for PoolInfo {
    fn default() -> Self {
        Self {
            address: Pubkey::default(),
            name: "Default Pool".to_string(),
            token_a: PoolToken::default(),
            token_b: PoolToken::default(),
            token_a_vault: Pubkey::default(),
            token_b_vault: Pubkey::default(),
            fee_numerator: Some(0), // Default for AMMs
            fee_denominator: Some(10000), // Default for AMMs, non-zero
            fee_rate_bips: None,
            last_update_timestamp: 0,
            dex_type: DexType::Unknown("Default".to_string()),
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
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
}

impl ProgramConfig {
    pub fn new(name: String, version: String) -> Self {
        Self { name, version }
    }

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

#[allow(dead_code)]
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
    Whirlpool,
    Unknown(String),
}

#[async_trait]
pub trait PoolParser: Send + Sync {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> anyhow::Result<PoolInfo>;

    fn get_program_id(&self) -> Pubkey;
}


pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    let (input_reserve_val, output_reserve_val, output_decimals_val) = if is_a_to_b {
        (
            pool.token_a.reserve,
            pool.token_b.reserve,
            pool.token_b.decimals,
        )
    } else {
        (
            pool.token_b.reserve,
            pool.token_a.reserve,
            pool.token_a.decimals,
        )
    };

    // Use fee_numerator and fee_denominator for AMM-style calculation
    // For CLMMs, this function might need to be adapted or a different one used.
    let fee_num = pool.fee_numerator.unwrap_or(0); // Default to 0 if not present
    let fee_den = pool.fee_denominator.unwrap_or(1); // Default to 1 to avoid div by zero if not present

    if input_reserve_val == 0
        || output_reserve_val == 0
        || input_amount.amount == 0
        || fee_den == 0
    {
        return TokenAmount::new(0, output_decimals_val);
    }

    let fee_rate = fee_num as f64 / fee_den as f64;
    let input_amount_after_fee = input_amount.amount as f64 * (1.0 - fee_rate);

    if input_reserve_val as f64 + input_amount_after_fee == 0.0 {
        return TokenAmount::new(0, output_decimals_val);
    }
    let output_amount_precise = (output_reserve_val as f64 * input_amount_after_fee)
        / (input_reserve_val as f64 + input_amount_after_fee);

    TokenAmount::new(output_amount_precise.floor() as u64, output_decimals_val)
}

// Test struct to check if field definitions work
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

// ---- Original PoolInfo for comparison ----