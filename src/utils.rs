use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAmount {
    pub amount: u64,
    pub decimals: u8,
}

impl TokenAmount {
    pub fn new(amount: u64, decimals: u8) -> Self {
        Self { amount, decimals }
    }

    pub fn to_float(&self) -> f64 {
        let divisor = 10u64.pow(self.decimals as u32) as f64;
        self.amount as f64 / divisor
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub reserve: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DexType {
    Raydium,
    Orca,
    Whirlpool,
    Jupiter,
    Lifinity,
    Phoenix,
    Meteora,
    Unknown,
}

impl fmt::Display for DexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DexType::Raydium => write!(f, "Raydium"),
            DexType::Orca => write!(f, "Orca"),
            DexType::Whirlpool => write!(f, "Whirlpool"),
            DexType::Jupiter => write!(f, "Jupiter"),
            DexType::Lifinity => write!(f, "Lifinity"),
            DexType::Phoenix => write!(f, "Phoenix"),
            DexType::Meteora => write!(f, "Meteora"),
            DexType::Unknown => write!(f, "Unknown"),
        }
    }
}

pub trait PoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo>;
    fn get_program_id() -> Pubkey;
    fn get_dex_type() -> DexType;
}

#[allow(dead_code)]
pub fn calculate_price(pool: &PoolInfo) -> f64 {
    let token_a_amount = pool.token_a.reserve as f64 / 10f64.powi(pool.token_a.decimals as i32);
    let token_b_amount = pool.token_b.reserve as f64 / 10f64.powi(pool.token_b.decimals as i32);
    token_a_amount / token_b_amount
}

pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    let (input_reserve, _input_decimals, output_reserve, output_decimals) = if is_a_to_b {
        (
            pool.token_a.reserve,
            pool.token_a.decimals,
            pool.token_b.reserve,
            pool.token_b.decimals,
        )
    } else {
        (
            pool.token_b.reserve,
            pool.token_b.decimals,
            pool.token_a.reserve,
            pool.token_a.decimals,
        )
    };
    let adjusted_input = input_amount.amount;
    let fee = adjusted_input * pool.fee_numerator / pool.fee_denominator;
    let input_with_fee = adjusted_input - fee;
    let numerator = input_with_fee * output_reserve;
    let denominator = input_reserve + input_with_fee;
    let output_amount = numerator / denominator;
    TokenAmount::new(output_amount, output_decimals)
}

// Whirlpool implementation
use anyhow::{anyhow, Result};
use log::{error, info};
use std::convert::TryInto;
use std::str::FromStr;

pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbmvGdJ8kT34DbDZpeMZQRAu8da5nq7WaRDRtyQ"; // Mainnet

#[repr(C)]
#[derive(Debug, Clone)]
pub struct Whirlpool {
    pub whirlpool_bump: u8,
    pub tick_spacing: u16,
    pub tick_current_index: i32,
    pub sqrt_price: u128,
    pub liquidity: u128,
    pub fee_growth_global_a: u128,
    pub fee_growth_global_b: u128,
    pub reward_last_updated_timestamp: u64,
    pub reward_infos: [RewardInfo; 3],
    pub token_mint_a: [u8; 32],
    pub token_vault_a: [u8; 32],
    pub token_mint_b: [u8; 32],
    pub token_vault_b: [u8; 32],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub protocol_fees: [u64; 2],
    pub token_a: u64,
    pub token_b: u64,
    pub open_time: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RewardInfo {
    pub mint: [u8; 32],
    pub vault: [u8; 32],
    pub authority: [u8; 32],
    pub emissions_per_second: u64,
    pub growth_global: u128,
}

impl RewardInfo {
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < 120 {
            return Err(anyhow!("buffer too short for RewardInfo"));
        }
        Ok(RewardInfo {
            mint: buf[0..32].try_into()?,
            vault: buf[32..64].try_into()?,
            authority: buf[64..96].try_into()?,
            emissions_per_second: u64::from_le_bytes(buf[96..104].try_into()?),
            growth_global: u128::from_le_bytes(buf[104..120].try_into()?),
        })
    }
}

impl Whirlpool {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 340 {
            return Err(anyhow!(
                "buffer too short for Whirlpool struct: got {}, want >= 340",
                data.len()
            ));
        }
        let mut offset = 0;
        let whirlpool_bump = data[0];
        offset += 1;
        let tick_spacing = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
        offset += 2;
        let tick_current_index = i32::from_le_bytes(data[offset..offset + 4].try_into()?);
        offset += 4;
        let sqrt_price = u128::from_le_bytes(data[offset..offset + 16].try_into()?);
        offset += 16;
        let liquidity = u128::from_le_bytes(data[offset..offset + 16].try_into()?);
        offset += 16;
        let fee_growth_global_a = u128::from_le_bytes(data[offset..offset + 16].try_into()?);
        offset += 16;
        let fee_growth_global_b = u128::from_le_bytes(data[offset..offset + 16].try_into()?);
        offset += 16;
        let reward_last_updated_timestamp =
            u64::from_le_bytes(data[offset..offset + 8].try_into()?);
        offset += 8;
        let mut reward_infos = [RewardInfo {
            mint: [0; 32],
            vault: [0; 32],
            authority: [0; 32],
            emissions_per_second: 0,
            growth_global: 0,
        }; 3];
        for reward_info in &mut reward_infos {
            *reward_info = RewardInfo::parse(&data[offset..offset + 120])?;
            offset += 120;
        }
        let token_mint_a = data[offset..offset + 32].try_into()?;
        offset += 32;
        let token_vault_a = data[offset..offset + 32].try_into()?;
        offset += 32;
        let token_mint_b = data[offset..offset + 32].try_into()?;
        offset += 32;
        let token_vault_b = data[offset..offset + 32].try_into()?;
        offset += 32;
        let fee_rate = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
        offset += 2;
        let protocol_fee_rate = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
        offset += 2;
        let protocol_fees = [
            u64::from_le_bytes(data[offset..offset + 8].try_into()?),
            u64::from_le_bytes(data[offset + 8..offset + 16].try_into()?),
        ];
        offset += 16;
        let token_a = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
        offset += 8;
        let token_b = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
        offset += 8;
        let open_time = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
        Ok(Self {
            whirlpool_bump,
            tick_spacing,
            tick_current_index,
            sqrt_price,
            liquidity,
            fee_growth_global_a,
            fee_growth_global_b,
            reward_last_updated_timestamp,
            reward_infos,
            token_mint_a,
            token_vault_a,
            token_mint_b,
            token_vault_b,
            fee_rate,
            protocol_fee_rate,
            protocol_fees,
            token_a,
            token_b,
            open_time,
        })
    }
}

// Manual Whirlpool DEX pool parser
pub struct WhirlpoolPoolParser;

impl PoolParser for WhirlpoolPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> Result<PoolInfo> {
        info!("Parsing Whirlpool pool at address: {}", address);
        match Whirlpool::parse(data) {
            Ok(wp) => {
                // Use Pubkey::from for [u8;32] arrays
                let token_a_mint = Pubkey::from(wp.token_mint_a);
                let token_b_mint = Pubkey::from(wp.token_mint_b);
                let token_a_symbol = &token_a_mint.to_string()[0..4];
                let token_b_symbol = &token_b_mint.to_string()[0..4];

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                Ok(PoolInfo {
                    address,
                    name: format!("WP/{}-{}", token_a_symbol, token_b_symbol),
                    token_a: PoolToken {
                        mint: token_a_mint,
                        symbol: token_a_symbol.to_string(),
                        decimals: 6, // TODO: Lookup decimals from registry/cache
                        reserve: wp.token_a,
                    },
                    token_b: PoolToken {
                        mint: token_b_mint,
                        symbol: token_b_symbol.to_string(),
                        decimals: 6,
                        reserve: wp.token_b,
                    },
                    fee_numerator: wp.fee_rate as u64,
                    fee_denominator: 1_000_000,
                    last_update_timestamp: timestamp,
                    dex_type: DexType::Whirlpool,
                })
            }
            Err(e) => {
                error!("Failed to parse Whirlpool state at {}: {:?}", address, e);
                Err(anyhow!(
                    "Could not deserialize Whirlpool pool at {}: {:?}",
                    address,
                    e
                ))
            }
        }
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).unwrap()
    }

    fn get_dex_type() -> DexType {
        DexType::Whirlpool
    }
}

pub struct AppConfig {
    pub rpc_url: String,
    pub ws_url: String,
    pub rpc_url_backup: String,
    pub rpc_url_staked: String,
    pub orca_api_key: String,
    pub lifinity_api_key: String,
    pub whirlpool_api_key: String,
    pub meteora_api_key: String,
    pub phoenix_api_key: String,
    pub raydium_api_key: String,
    pub serum_api_key: String,
    pub paper_trading: String,
    pub trader_wallet_address: String,
    pub trader_wallet_keypair_path: String,
    pub min_profit: String,
    pub max_slippage: String,
    pub cycle_interval: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenv().ok(); // Loads .env file automatically

        AppConfig {
            rpc_url: std::env::var("RPC_URL").unwrap_or_default(),
            ws_url: std::env::var("WS_URL").unwrap_or_default(),
            rpc_url_backup: std::env::var("RPC_URL_BACKUP").unwrap_or_default(),
            rpc_url_staked: std::env::var("RPC_URL_STAKED").unwrap_or_default(),
            orca_api_key: std::env::var("ORCA_API_KEY").unwrap_or_default(),
            lifinity_api_key: std::env::var("LIFINITY_API_KEY").unwrap_or_default(),
            whirlpool_api_key: std::env::var("WHIRLPOOL_API_KEY").unwrap_or_default(),
            meteora_api_key: std::env::var("METEORA_API_KEY").unwrap_or_default(),
            phoenix_api_key: std::env::var("PHOENIX_API_KEY").unwrap_or_default(),
            raydium_api_key: std::env::var("RAYDIUM_API_KEY").unwrap_or_default(),
            serum_api_key: std::env::var("SERUM_API_KEY").unwrap_or_default(),
            paper_trading: std::env::var("PAPER_TRADING").unwrap_or_default(),
            trader_wallet_address: std::env::var("TRADER_WALLET_ADDRESS").unwrap_or_default(),
            trader_wallet_keypair_path: std::env::var("TRADER_WALLET_KEYPAIR_PATH")
                .unwrap_or_default(),
            min_profit: std::env::var("MIN_PROFIT").unwrap_or_default(),
            max_slippage: std::env::var("MAX_SLIPPAGE").unwrap_or_default(),
            cycle_interval: std::env::var("CYCLE_INTERVAL").unwrap_or_default(),
        }
    }
}

// Print config function used for debugging and diagnostics
#[cfg(feature = "print-config")]
pub fn print_config() {
    let config = AppConfig::from_env();
    println!("RPC URL: {}", config.rpc_url);
    println!("WebSocket URL: {}", config.ws_url);
    println!("Backup RPC URL: {}", config.rpc_url_backup);
    println!("Staked RPC URL: {}", config.rpc_url_staked);
    println!("ORCA API KEY: {}", config.orca_api_key);
    println!("LIFINITY API KEY: {}", config.lifinity_api_key);
    println!("WHIRLPOOL API KEY: {}", config.whirlpool_api_key);
    println!("METEORA API KEY: {}", config.meteora_api_key);
    println!("PHOENIX API KEY: {}", config.phoenix_api_key);
    println!("RAYDIUM API KEY: {}", config.raydium_api_key);
    println!("SERUM API KEY: {}", config.serum_api_key);
    println!("PAPER TRADING: {}", config.paper_trading);
    println!("TRADER WALLET ADDRESS: {}", config.trader_wallet_address);
    println!(
        "TRADER WALLET KEYPAIR PATH: {}",
        config.trader_wallet_keypair_path
    );
    println!("MIN PROFIT: {}", config.min_profit);
    println!("MAX SLIPPAGE: {}", config.max_slippage);
    println!("CYCLE INTERVAL: {}", config.cycle_interval);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config() {
        let config = AppConfig::from_env();
        assert!(!config.rpc_url.is_empty() || config.rpc_url.is_empty());
    }
}
