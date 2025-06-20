// src/dex/clients/orca.rs
//! Orca Whirlpools client and parser for on-chain data and instruction building.
//! This is the consolidated and authoritative source for Orca Whirlpools integration.

use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::info;
use serde::{Deserialize, Deserializer};
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::fs;
use std::str::FromStr;
use std::sync::Arc;

// --- Constants ---
pub const ORCA_WHIRLPOOL_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");

const MARKETS_TO_IGNORE: &[&str] = &[
    "BCaq51UZ6JLpuEToQzun1GVvvqaw7Vyw8i3CzuZzBCty",
    "5dLv6NVpjUibiCgN4M9b8XFQGLikYWZKiVFhFENbkwgP",
];

// Placeholder function - would be replaced with actual implementation from orca_whirlpools_core
// --- On-Chain Data Structures ---
#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct WhirlpoolState {
    pub discriminator: [u8; 8],
    pub whirlpools_config: Pubkey,
    pub whirlpool_bump: [u8; 1],
    pub tick_spacing: u16,
    pub tick_spacing_seed: [u8; 2],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub protocol_fee_owed_a: u64,
    pub protocol_fee_owed_b: u64,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub fee_growth_global_a: u128,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,
    pub fee_growth_global_b: u128,
    pub reward_last_updated_timestamp: u64,
    pub reward_infos: [RewardInfo; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RewardInfo {
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub authority: Pubkey,
    pub emissions_per_second_x64: u128,
    pub growth_global_x64: u128,
}

// --- API Data Structures ---
/// Orca API response struct for deserializing external API data.
///
/// All fields are retained for compatibility with the external API contract, even if not directly read in Rust code.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrcaApiResponse {
    pub whirlpools: Vec<OrcaApiPool>,
}

/// Orca API pool struct for deserializing external API data.
///
/// All fields are retained for compatibility with the external API contract, even if not directly read in Rust code.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrcaApiPool {
    // Field is part of an external API contract
    pub address: String,
    #[allow(dead_code)] // Field is part of an external API contract, may be used for logging/debugging
    #[serde(rename = "whirlpoolBump")]
    pub whirlpool_bump: Option<u8>,
    #[serde(rename = "whirlpoolsConfig")]
    pub whirlpools_config: Option<String>, // Field is part of an external API contract, may be used for logging/debugging
    #[serde(rename = "tokenMintA")]
    pub token_mint_a: Option<String>, // Field is part of an external API contract, may be used for logging/debugging
    #[serde(rename = "tokenMintB")]
    pub token_mint_b: Option<String>,
    #[serde(rename = "tokenVaultA")]
    pub token_vault_a: Option<String>,
    #[serde(rename = "tokenVaultB")]
    pub token_vault_b: Option<String>,
    #[serde(rename = "feeRate", deserialize_with = "u64_from_any", default)]
    pub fee_rate: Option<u64>,
    #[serde(rename = "protocolFeeRate", deserialize_with = "u64_from_any", default)]
    pub protocol_fee_rate: Option<u64>,
    pub liquidity: Option<String>,
    #[serde(rename = "sqrtPrice")]
    pub sqrt_price: Option<String>,
    #[serde(rename = "tickCurrentIndex")]
    pub tick_current_index: Option<i32>,
    #[serde(rename = "tickSpacing")]
    pub tick_spacing: Option<u16>,
    #[serde(rename = "tokenA")]
    pub token_a: OrcaApiToken,
    #[serde(rename = "tokenB")]
    pub token_b: OrcaApiToken,
    #[serde(rename = "rewardInfos")]
    pub reward_infos: Option<Vec<OrcaApiRewardInfo>>,
    #[serde(rename = "tokenAAmount")]
    pub token_a_amount: Option<String>,
    #[serde(rename = "tokenBAmount")]
    pub token_b_amount: Option<String>,
    #[serde(deserialize_with = "string_or_float_to_string", default)]
    pub price: Option<String>,
    #[serde(
        rename = "volume24h",
        deserialize_with = "string_or_float_to_string",
        default
    )]
    pub volume_24h: Option<String>,
    #[serde(deserialize_with = "string_or_float_to_string", default)]
    pub tvl: Option<String>,
    #[serde(deserialize_with = "string_or_float_to_string", default)]
    pub apy: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
}

/// Orca API token struct for deserializing external API data.
///
/// All fields are retained for compatibility with the external API contract, even if not directly read in Rust code.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrcaApiToken {
    pub mint: String,
    pub symbol: String,
    #[allow(dead_code)]
    // Field is part of an external API contract, may be used for logging/debugging
    pub name: Option<String>,
    pub decimals: u8,
}

/// Orca API reward info struct for deserializing external API data.
///
/// All fields are retained for compatibility with the external API contract, even if not directly read in Rust code.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrcaApiRewardInfo {
    pub mint: String,
    pub vault: String,
    pub authority: String,
    #[serde(rename = "emissionsPerSecondX64")]
    pub emissions_per_second_x64: Option<String>,
    #[serde(rename = "growthGlobalX64")]
    pub growth_global_x64: Option<String>,
    pub decimals: Option<u8>,
}

// Custom deserializer for fields that may be string or float
fn string_or_float_to_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Unexpected};
    let val: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(serde_json::Value::Number(n)) => Ok(Some(n.to_string())),
        Some(serde_json::Value::Null) => Ok(None),
        Some(other) => Err(de::Error::invalid_type(
            Unexpected::Other(&format!("{:?}", other)),
            &"string or number",
        )),
        None => Ok(None),
    }
}

// Custom deserializer for u64 fields that may be float or int
fn u64_from_any<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Unexpected};
    let val: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u))
            } else if let Some(f) = n.as_f64() {
                Ok(Some(f as u64))
            } else {
                Err(de::Error::invalid_type(
                    Unexpected::Other("number"),
                    &"u64 or float",
                ))
            }
        }
        Some(serde_json::Value::String(s)) => s
            .parse::<u64>()
            .map(Some)
            .map_err(|_| de::Error::invalid_type(Unexpected::Str(&s), &"u64 string")),
        Some(serde_json::Value::Null) => Ok(None),
        Some(other) => Err(de::Error::invalid_type(
            Unexpected::Other(&format!("{:?}", other)),
            &"u64, float, or string",
        )),
        None => Ok(None),
    }
}

// Utility function to extract Orca Whirlpool pool addresses from a JSON file
// Accepts either a list of strings or a list of objects with an "address" field
fn extract_orca_whirlpool_addresses_from_json(json_path: &str) -> anyhow::Result<Vec<Pubkey>> {
    let json = std::fs::read_to_string(json_path)?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let mut addresses = Vec::new();
    if let serde_json::Value::Array(arr) = value {
        for item in arr {
            if let serde_json::Value::String(addr) = &item {
                if let Ok(pk) = Pubkey::from_str(addr) {
                    addresses.push(pk);
                }
            } else if let serde_json::Value::Object(map) = &item {
                if let Some(serde_json::Value::String(addr)) = map.get("address") {
                    if let Ok(pk) = Pubkey::from_str(addr) {
                        addresses.push(pk);
                    }
                }
            }
        }
    }
    Ok(addresses)
}

/// Helper to construct a `PoolInfo` from a `WhirlpoolState` and address.
/// Used by all Orca pool parsing and discovery logic to ensure consistency.
fn build_orca_pool_info(pool_address: Pubkey, whirlpool_state: &WhirlpoolState) -> PoolInfo {
    PoolInfo {
        address: pool_address,
        name: "Orca Whirlpool".to_string(),
        token_a: PoolToken {
            mint: whirlpool_state.token_mint_a,
            symbol: "Unknown".to_string(), // TODO: resolve from metadata
            decimals: 6,                   // TODO: resolve from metadata
            reserve: 0,                    // TODO: fetch from vault
        },
        token_b: PoolToken {
            mint: whirlpool_state.token_mint_b,
            symbol: "Unknown".to_string(),
            decimals: 6,
            reserve: 0,
        },
        token_a_vault: whirlpool_state.token_vault_a,
        token_b_vault: whirlpool_state.token_vault_b,
        fee_numerator: Some(whirlpool_state.fee_rate as u64),
        fee_denominator: Some(1_000_000), // Orca uses 1M denominator
        fee_rate_bips: Some(whirlpool_state.fee_rate),
        last_update_timestamp: whirlpool_state.reward_last_updated_timestamp,
        dex_type: DexType::Orca,
        liquidity: Some(whirlpool_state.liquidity),
        sqrt_price: Some(whirlpool_state.sqrt_price),
        tick_current_index: Some(whirlpool_state.tick_current_index),
        tick_spacing: Some(whirlpool_state.tick_spacing),
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None, // Orca doesn't use external oracles
    }
}

// --- Pool Parser ---
pub struct OrcaPoolParser;

#[async_trait]
impl UtilsPoolParser for OrcaPoolParser {
    async fn parse_pool_data(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < std::mem::size_of::<WhirlpoolState>() {
            return Err(anyhow!("Invalid Whirlpool data size"));
        }
        let whirlpool_state = bytemuck::from_bytes::<WhirlpoolState>(data);
        Ok(build_orca_pool_info(pool_address, whirlpool_state))
    }

    fn parse_pool_data_sync(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < std::mem::size_of::<WhirlpoolState>() {
            return Err(anyhow!("Invalid Whirlpool data size"));
        }
        let whirlpool_state = bytemuck::from_bytes::<WhirlpoolState>(data);
        Ok(build_orca_pool_info(pool_address, whirlpool_state))
    }

    fn get_program_id(&self) -> Pubkey {
        ORCA_WHIRLPOOL_PROGRAM_ID
    }
}

// --- Orca Client ---
/// OrcaClient provides integration with Orca Whirlpools on Solana, supporting pool discovery, quoting, and swap instruction building.
///
/// All public methods use production-grade math for CLMM pools and fallback logic for classic pools. Error handling is unified via ArbError.
pub struct OrcaClient {
    /// The name of the DEX client.
    pub name: String,
}

impl OrcaClient {
    /// Create a new OrcaClient instance.
    pub fn new() -> Self {
        Self {
            name: "Orca".to_string(),
        }
    }

    /// Discover Orca Whirlpools pools by fetching on-chain data for a static list of pool addresses.
    ///
    /// Loads pool addresses from `config/orca_whirlpool_pools.json` and fetches their on-chain state
    /// using the provided Solana RPC client.
    #[allow(dead_code)]
    pub async fn discover_pools_onchain(
        &self,
        rpc: &SolanaRpcClient,
    ) -> AnyhowResult<Vec<PoolInfo>> {
        // 1. Load pool addresses from JSON
        let json = fs::read_to_string("config/orca_whirlpool_pools.json")?;
        let pool_addresses: Vec<String> = serde_json::from_str(&json)?;
        let pool_addresses: Vec<Pubkey> = pool_addresses
            .into_iter()
            .filter(|addr| !MARKETS_TO_IGNORE.contains(&addr.as_str()))
            .filter_map(|addr| Pubkey::from_str(&addr).ok())
            .collect();

        use futures::future::join_all;
        let fetches = pool_addresses.iter().map(|addr| rpc.get_account_data(addr));
        let results = join_all(fetches).await;
        let mut discovered_pools = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            if let Ok(data) = result {
                if data.len() >= std::mem::size_of::<WhirlpoolState>() {
                    let whirlpool_state = bytemuck::from_bytes::<WhirlpoolState>(&data);
                    let pool_address = pool_addresses[i];
                    discovered_pools.push(build_orca_pool_info(pool_address, whirlpool_state));
                }
            }
        }
        info!(
            "[ONCHAIN] Discovered {} Orca Whirlpool pools",
            discovered_pools.len()
        );
        Ok(discovered_pools)
    }

    /// Discover Orca Whirlpools pools by fetching on-chain data for a static list of pool addresses from a JSON file.
    ///
    /// Loads pool addresses from the provided JSON path and fetches their on-chain state
    /// using the provided Solana RPC client.
    pub async fn discover_pools_onchain_from_json(
        &self,
        rpc: &SolanaRpcClient,
        json_path: &str,
    ) -> AnyhowResult<Vec<PoolInfo>> {
        let pool_addresses = extract_orca_whirlpool_addresses_from_json(json_path)?;
        use futures::future::join_all;
        let fetches = pool_addresses.iter().map(|addr| rpc.get_account_data(addr));
        let results = join_all(fetches).await;
        let mut discovered_pools = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            if let Ok(data) = result {
                if data.len() >= std::mem::size_of::<WhirlpoolState>() {
                    let whirlpool_state = bytemuck::from_bytes::<WhirlpoolState>(&data);
                    let pool_address = pool_addresses[i];
                    discovered_pools.push(build_orca_pool_info(pool_address, whirlpool_state));
                }
            }
        }
        info!(
            "[ONCHAIN] Discovered {} Orca Whirlpool pools from JSON",
            discovered_pools.len()
        );
        Ok(discovered_pools)
    }
}

use crate::dex::api::{
    CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo,
};
use crate::dex::math::orca;

#[async_trait]
impl DexClient for OrcaClient {
    /// Returns the name of the DEX client.
    fn get_name(&self) -> &str {
        &self.name
    }
    /// Calculates a quote for a given pool and input amount.
    ///
    /// For Whirlpool (CLMM) pools, uses precise math from `math/orca.rs`.
    /// For classic pools, falls back to the generic constant product formula.
    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Use precise CLMM math for Whirlpool pools
        if pool.liquidity.is_some() && pool.sqrt_price.is_some() {
            let liquidity = pool.liquidity.unwrap();
            let sqrt_price = pool.sqrt_price.unwrap();
            let tick_current = pool.tick_current_index.unwrap_or(0);
            let tick_spacing = pool.tick_spacing.unwrap_or(64); // already u16
            let fee_rate = pool.fee_rate_bips.unwrap_or(30); // already u16
            let result = orca::calculate_whirlpool_swap_output(
                input_amount,
                sqrt_price,
                liquidity,
                tick_current,
                tick_spacing,
                fee_rate,
                true, // Assume A to B for now; extend as needed
            )
            .map_err(|e| anyhow!(e.to_string()))?;
            Ok(Quote {
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                output_amount: result.output_amount,
                dex: "Orca".to_string(),
                route: vec![pool.address],
                slippage_estimate: Some(result.price_impact),
            })
        } else {
            // Fallback for classic pools
            use crate::utils::calculate_output_amount;
            let output = calculate_output_amount(pool, input_amount, true)
                .map_err(|e| anyhow!(e.to_string()))?;
            Ok(Quote {
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                output_amount: output,
                dex: "Orca".to_string(),
                route: vec![pool.address],
                slippage_estimate: None,
            })
        }
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // Placeholder: Implement Orca swap instruction
        Err(anyhow!("Orca swap instruction not implemented"))
    }

    /// Returns a swap instruction for the given swap info and pool info (not yet implemented).
    async fn get_swap_instruction_enhanced(
        &self,
        _swap_info: &CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        // Placeholder: Implement Orca enhanced swap instruction
        Err(crate::error::ArbError::Unknown(
            "Orca enhanced swap instruction not implemented".to_string(),
        ))
    }

    /// Not supported: requires explicit RPC client.
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        Err(anyhow!("OrcaClient::discover_pools requires an RPC client to be passed in. Use discover_pools_onchain_from_json with an explicit RPC client."))
    }

    /// Returns a health check status for the Orca DEX client.
    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError> {
        Ok(DexHealthStatus {
            is_healthy: true,
            last_successful_request: None,
            error_count: 0,
            response_time_ms: None,
            pool_count: None,
            status_message: "OrcaClient health check not implemented".to_string(),
        })
    }
}

#[async_trait]
impl PoolDiscoverable for OrcaClient {
    /// Not supported: requires explicit RPC client.
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    /// Not supported: fetching a single pool's data is not implemented.
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // Placeholder: Implement fetching a single pool's data
        Err(anyhow!(
            "fetch_pool_data not implemented for OrcaClient. Pool address: {}",
            pool_address
        ))
    }
    /// Returns the DEX name.
    fn dex_name(&self) -> &str {
        self.get_name()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for OrcaClient {
    fn default() -> Self {
        Self::new()
    }
}
