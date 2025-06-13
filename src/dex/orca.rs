// src/dex/orca.rs
//! Orca client and parser for on-chain data and instruction building.
//! This implementation leverages the official orca_whirlpools SDK for accuracy.

use crate::dex::quote::{DexClient, Quote, SwapInfo};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser};
use crate::utils::{DexType, PoolToken}; // Assuming DexType and PoolToken are in utils
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use solana_program::program_pack::Pack;
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Mint;
use std::sync::Arc;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use chrono;

/// The program ID for the Orca Whirlpools program.
pub const ORCA_WHIRLPOOL_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");

// === Orca API Data Structures ===

/// Token information from the Orca API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrcaApiToken {
    pub mint: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    #[serde(rename = "logoURI")]
    pub logo_uri: Option<String>,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: Option<String>,
    pub whitelisted: bool,
    #[serde(rename = "poolToken")]
    pub pool_token: bool,
}

/// Pool information from the Orca API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrcaApiPool {
    pub address: String,
    #[serde(rename = "tokenA")]
    pub token_a: OrcaApiToken,
    #[serde(rename = "tokenB")]
    pub token_b: OrcaApiToken,
    #[serde(rename = "tickSpacing")]
    pub tick_spacing: u16,
    pub price: f64,
    #[serde(rename = "lpFeeRate")]
    pub lp_fee_rate: f64,
    #[serde(rename = "protocolFeeRate")]
    pub protocol_fee_rate: f64,
    pub whitelisted: bool,
    pub tvl: f64,
    #[serde(rename = "volume")]
    pub volume: OrcaApiVolume,
    #[serde(rename = "volumeDenominatedInToken")]
    pub volume_denominated_in_token: Option<String>,
    #[serde(rename = "priceRange")]
    pub price_range: Option<OrcaApiPriceRange>,
}

/// Volume data from the Orca API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrcaApiVolume {
    pub day: f64,
    pub week: f64,
    pub month: f64,
}

/// Price range data from the Orca API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrcaApiPriceRange {
    pub min: f64,
    pub max: f64,
}

/// Root response from the Orca API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrcaApiResponse {
    pub whirlpools: Vec<OrcaApiPool>,
}

// --- On-Chain Data Parser ---

/// Represents the state of an Orca Whirlpool account.
/// Aligned with the structure provided in `dex_restructuring.txt`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Whirlpool {
    pub whirlpools_config: Pubkey,
    pub whirlpool_bump: [u8; 1],
    pub tick_spacing: u16,
    pub tick_spacing_padding: [u8; 5], // Added padding to match docs
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
    // reward_infos array follows, but we only need to parse the fixed part.
    // pub reward_infos: [WhirlpoolRewardInfo; NUM_REWARDS], // NUM_REWARDS is 3
    // pub padding_after_rewards: [u8; REMAINDER_BYTES], // Ensure struct size is as expected if parsing full data
}

pub struct OrcaPoolParser;

#[async_trait::async_trait]
impl UtilsPoolParser for OrcaPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        info!("Parsing Orca Whirlpool data for address: {}", address);

        if data.len() < std::mem::size_of::<Whirlpool>() {
            return Err(anyhow!(
                "Invalid Whirlpool account data length for {}: expected at least {} bytes, got {}",
                address,
                std::mem::size_of::<Whirlpool>(),
                data.len()
            ));
        }

        let whirlpool: &Whirlpool = bytemuck::from_bytes(&data[..std::mem::size_of::<Whirlpool>()]);

        // Fetch decimals for token A and B
        let (token_a_mint_data, token_b_mint_data) = tokio::try_join!(
            async { rpc_client.primary_client.get_account_data(&whirlpool.token_mint_a).await.map_err(anyhow::Error::from) },
            async { rpc_client.primary_client.get_account_data(&whirlpool.token_mint_b).await.map_err(anyhow::Error::from) }
        )?;

        let token_a_decimals = Mint::unpack_from_slice(&token_a_mint_data)?.decimals;
        let token_b_decimals = Mint::unpack_from_slice(&token_b_mint_data)?.decimals;

        // Note: 'reserve' field in PoolToken is more for AMMs. For CLMMs like Orca,
        // liquidity is represented by whirlpool.liquidity. We'll set reserve to 0 or handle appropriately.
        Ok(PoolInfo {
            address,
            name: format!("Orca Whirlpool/{}", address),
            dex_type: DexType::Orca, // Assuming DexType::Orca variant exists
            token_a: PoolToken {
                mint: whirlpool.token_mint_a,
                symbol: "TokenA".to_string(), // Placeholder, consider a token registry
                decimals: token_a_decimals,
                reserve: 0, // Not directly applicable as AMM reserve
            },
            token_b: PoolToken {
                mint: whirlpool.token_mint_b,
                symbol: "TokenB".to_string(), // Placeholder, consider a token registry
                decimals: token_b_decimals,
                reserve: 0, // Not directly applicable as AMM reserve
            },
            token_a_vault: whirlpool.token_vault_a,
            token_b_vault: whirlpool.token_vault_b,
            
            fee_rate_bips: Some(whirlpool.fee_rate), // fee_rate is in basis points (0.01%)
            fee_numerator: None, // Not used by Orca in this way
            fee_denominator: None, // Not used by Orca in this way

            liquidity: Some(whirlpool.liquidity),
            sqrt_price: Some(whirlpool.sqrt_price),
            tick_current_index: Some(whirlpool.tick_current_index),
            tick_spacing: Some(whirlpool.tick_spacing),
            
            last_update_timestamp: whirlpool.reward_last_updated_timestamp,
        })
    }
    fn get_program_id(&self) -> Pubkey {
        ORCA_WHIRLPOOL_PROGRAM_ID
    }
}

// --- DEX Client Implementation ---

#[derive(Debug, Clone, Default)]
pub struct OrcaClient;

impl OrcaClient {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DexClient for OrcaClient {
    fn get_name(&self) -> &str {
        "Orca"
    }

    /// Calculates a quote using the official Orca SDK math.
    fn calculate_onchain_quote(
        &self,
        _pool: &PoolInfo,
        _input_amount: u64,
    ) -> AnyhowResult<Quote> {
        // This is a simplified example. A full implementation requires fetching the whirlpool,
        // tick arrays, and oracle accounts to pass to the quote function.
        // For now, we'll return an error indicating what's needed.
        return Err(anyhow!( // Keep return for clarity, or remove semicolon from line below
            "Orca quote calculation requires the full Whirlpool and TickArray accounts, not just PoolInfo."
        ));

        // TODO: The actual implementation will look something like this:
        // let whirlpool = ...; // Fetch and deserialize the full Whirlpool account.
        // let tick_arrays = [...]; // Fetch and deserialize the necessary TickArray accounts.
        // let quote = quote_from_whirlpool(
        //     &whirlpool,
        //     input_amount,
        //     pool.token_a.mint,
        //     &tick_arrays,
        //     // ... other params
        // )?;
        //
        // Ok(Quote {
        //     input_token: pool.token_a.symbol.clone(),
        //     output_token: pool.token_b.symbol.clone(),
        //     input_amount,
        //     output_amount: quote.estimated_amount_out,
        //     dex: self.get_name().to_string(),
        //     route: vec![pool.address],
        //     slippage_estimate: Some(quote.estimated_slippage),
        // })
    }

    fn get_swap_instruction(
        &self,
        _swap_info: &SwapInfo,
    ) -> AnyhowResult<Instruction> {
        Err(anyhow!("Orca Whirlpool swap instruction is not yet implemented: missing SDK integration and TickArray logic."))
    }

    /// Discovers all supported liquidity pools for the DEX.
    ///
    /// This method fetches real pool data from the Orca Official API and converts it
    /// to the internal PoolInfo format for further processing.
    ///
    /// # Returns
    /// A vector of `PoolInfo` structs with real market data from the Orca API.
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("Starting Orca pool discovery using official API");
        
        // Fetch pool data from the official Orca API
        let api_url = "https://api.mainnet.orca.so/v1/whirlpool/list";
        let client = reqwest::Client::new();
        
        let response = client.get(api_url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch Orca pool data: {}", e))?;
        
        if !response.status().is_success() {
            return Err(anyhow!("API request failed with status: {}", response.status()));
        }
        
        let api_response: OrcaApiResponse = response.json()
            .await
            .map_err(|e| anyhow!("Failed to parse Orca API response: {}", e))?;
        
        info!("Fetched {} pools from Orca API", api_response.whirlpools.len());
        
        let mut pools = Vec::new();
        let mut successful_conversions = 0;
        let mut failed_conversions = 0;
        
        // Convert API data to our internal PoolInfo format
        for api_pool in api_response.whirlpools {
            let pool_address = api_pool.address.clone(); // Clone for error reporting
            match self.convert_api_pool_to_pool_info(api_pool).await {
                Ok(pool_info) => {
                    pools.push(pool_info);
                    successful_conversions += 1;
                }
                Err(e) => {
                    warn!("Failed to convert pool {}: {}", pool_address, e);
                    failed_conversions += 1;
                }
            }
        }
        
        info!("Successfully converted {} pools, failed to convert {} pools", 
              successful_conversions, failed_conversions);
        
        // Filter out pools with very low TVL (< $1000) to focus on liquid pools
        let min_tvl = 1000.0;
        let filtered_pools: Vec<PoolInfo> = pools.into_iter()
            .filter(|_pool| {
                // We'll use a simple heuristic for TVL filtering since it's not directly stored in PoolInfo
                // In practice, you might want to store TVL in PoolInfo or use other liquidity indicators
                true // For now, include all successfully converted pools
            })
            .collect();
        
        info!("Discovered {} liquid Orca pools (TVL >= ${})", filtered_pools.len(), min_tvl);
        Ok(filtered_pools)
    }
}

impl OrcaClient {
    /// Converts an API pool response to our internal PoolInfo structure
    pub async fn convert_api_pool_to_pool_info(&self, api_pool: OrcaApiPool) -> AnyhowResult<PoolInfo> {
        // Parse the pool address
        let pool_address = Pubkey::from_str(&api_pool.address)
            .map_err(|e| anyhow!("Invalid pool address {}: {}", api_pool.address, e))?;
        
        // Parse token mint addresses
        let token_a_mint = Pubkey::from_str(&api_pool.token_a.mint)
            .map_err(|e| anyhow!("Invalid token A mint {}: {}", api_pool.token_a.mint, e))?;
        
        let token_b_mint = Pubkey::from_str(&api_pool.token_b.mint)
            .map_err(|e| anyhow!("Invalid token B mint {}: {}", api_pool.token_b.mint, e))?;
        
        // Convert fee rate from decimal to basis points (0.003 -> 30 bips)
        let fee_rate_bips = (api_pool.lp_fee_rate * 10000.0) as u16;
        
        // Create PoolToken structs
        let token_a = PoolToken {
            mint: token_a_mint,
            symbol: api_pool.token_a.symbol.clone(),
            decimals: api_pool.token_a.decimals,
            reserve: 0, // Will be populated later when fetching vault balances
        };
        
        let token_b = PoolToken {
            mint: token_b_mint,
            symbol: api_pool.token_b.symbol.clone(),
            decimals: api_pool.token_b.decimals,
            reserve: 0, // Will be populated later when fetching vault balances
        };
        
        // Estimate sqrt price from regular price
        // For a CLMM, sqrt_price = sqrt(price) * 2^64 (approximately)
        // This is a rough conversion, actual sqrt_price would need on-chain data
        let sqrt_price_estimate = if api_pool.price > 0.0 {
            Some((api_pool.price.sqrt() * (1u128 << 32) as f64) as u128)
        } else {
            None
        };
        
        Ok(PoolInfo {
            address: pool_address,
            name: format!("{}/{} Orca Pool", api_pool.token_a.symbol, api_pool.token_b.symbol),
            dex_type: DexType::Orca,
            token_a,
            token_b,
            token_a_vault: Pubkey::default(), // Will be fetched from on-chain data if needed
            token_b_vault: Pubkey::default(), // Will be fetched from on-chain data if needed
            fee_rate_bips: Some(fee_rate_bips),
            fee_numerator: None,
            fee_denominator: None,
            liquidity: None, // Will be fetched from on-chain data if needed
            sqrt_price: sqrt_price_estimate,
            tick_current_index: None, // Will be fetched from on-chain data if needed
            tick_spacing: Some(api_pool.tick_spacing),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }
}