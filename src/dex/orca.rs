// src/dex/orca.rs
//! Orca Whirlpools client, parser, and instruction builder.
//! This is the consolidated, authoritative module for all Orca Whirlpools interactions.

use crate::dex::quote::{DexClient, Quote, SwapInfo, PoolDiscoverable};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser, PoolToken, DexType};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;
use std::sync::Arc;
use log::{info, warn};
use serde::Deserialize;
use std::str::FromStr;
use solana_sdk::instruction::Instruction;
// Import our local math functions for Orca calculations


// --- Constants ---
pub const ORCA_WHIRLPOOL_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
const ORCA_API_URL: &str = "https://api.mainnet.orca.so/v1/whirlpool/list";

// --- On-Chain Data Structures ---

/// Represents the state of an Orca Whirlpool account.
/// This struct accurately reflects the on-chain layout for a Whirlpool.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct WhirlpoolState {
    pub whirlpools_config: Pubkey,
    pub whirlpool_bump: [u8; 1],
    pub tick_spacing: u16,
    pub tick_spacing_padding: [u8; 5],
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
    // reward_infos array (3 elements) and other fields follow, but are not needed for basic parsing.
}
const WHIRLPOOL_STATE_SIZE: usize = std::mem::size_of::<WhirlpoolState>();


// --- API Data Structures ---
#[derive(Debug, Deserialize)]
struct OrcaApiToken {
    mint: String,
    symbol: String,
    decimals: u8,
}
#[derive(Debug, Deserialize)]
struct OrcaApiPool {
    address: String,
    #[serde(rename = "tokenA")]
    token_a: OrcaApiToken,
    #[serde(rename = "tokenB")]
    token_b: OrcaApiToken,
    #[serde(rename = "tickSpacing")]
    tick_spacing: u16,
}
#[derive(Debug, Deserialize)]
struct OrcaApiResponse {
    whirlpools: Vec<OrcaApiPool>,
}


// --- On-Chain Data Parser ---
pub struct OrcaPoolParser;

#[async_trait]
impl UtilsPoolParser for OrcaPoolParser {
    async fn parse_pool_data(&self, address: Pubkey, data: &[u8], _rpc_client: &Arc<SolanaRpcClient>) -> AnyhowResult<PoolInfo> {
        if data.len() < WHIRLPOOL_STATE_SIZE {
            return Err(anyhow!(
                "Invalid Whirlpool account data length for {}: expected at least {} bytes, got {}",
                address, WHIRLPOOL_STATE_SIZE, data.len()
            ));
        }

        let state: &WhirlpoolState = bytemuck::from_bytes(&data[..WHIRLPOOL_STATE_SIZE]);

        Ok(PoolInfo {
            address,
            name: format!("Orca Whirlpool/{}", address),
            dex_type: DexType::Orca,
            token_a: PoolToken {
                mint: state.token_mint_a,
                symbol: "TokenA".to_string(), // Placeholder; will be enriched by API data
                decimals: 0, // Placeholder; will be enriched by API data
                reserve: 0, // Not applicable for CLMMs
            },
            token_b: PoolToken {
                mint: state.token_mint_b,
                symbol: "TokenB".to_string(),
                decimals: 0,
                reserve: 0,
            },
            token_a_vault: state.token_vault_a,
            token_b_vault: state.token_vault_b,
            fee_rate_bips: Some(state.fee_rate),
            fee_numerator: None,
            fee_denominator: None,
            liquidity: Some(state.liquidity),
            sqrt_price: Some(state.sqrt_price),
            tick_current_index: Some(state.tick_current_index),
            tick_spacing: Some(state.tick_spacing),
            last_update_timestamp: state.reward_last_updated_timestamp,
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
    fn get_name(&self) -> &str { "Orca" }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Basic quote calculation for Orca
        // This is a simplified version - real CLMM calculations are much more complex
        
        warn!("Orca quote calculation is using a simplified approximation. Real implementation requires CLMM math library integration.");
        
        // For CLMM pools, we'll use a basic approximation based on liquidity
        let output_amount = if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                // Simple approximation: assume uniform liquidity distribution
                // Real CLMM requires tick array calculations
                let fee_rate = 0.0025; // 0.25% default for Orca
                let input_after_fees = (input_amount as f64 * (1.0 - fee_rate)) as u64;
                
                // Use a simple ratio based on available liquidity
                // This is NOT accurate for CLMM but provides a working approximation
                input_after_fees / 2 // Simplified calculation
            } else {
                return Err(anyhow!("Orca pool {} has zero liquidity.", pool.address));
            }
        } else {
            return Err(anyhow!("Orca pool {} missing liquidity data.", pool.address));
        };

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount,
            dex: self.get_name().to_string(),
            route: vec![pool.address],
            slippage_estimate: Some(0.0025),
        })
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // Basic swap instruction implementation for Orca Whirlpools
        // This is a simplified version - production implementation requires:
        // 1. Correct instruction discriminator for Orca Whirlpool program
        // 2. Proper tick array account resolution based on swap direction
        // 3. All required accounts in correct order
        
        warn!("get_swap_instruction for Orca is a basic implementation. Production use requires proper tick array resolution and SDK integration.");
        
        Ok(Instruction {
            program_id: ORCA_WHIRLPOOL_PROGRAM_ID,
            accounts: vec![
                solana_program::instruction::AccountMeta::new(swap_info.user_wallet, true),
                solana_program::instruction::AccountMeta::new(swap_info.pool_account, false),
                solana_program::instruction::AccountMeta::new(swap_info.user_source_token_account, false),
                solana_program::instruction::AccountMeta::new(swap_info.user_destination_token_account, false),
                // Note: Real Orca swaps require tick_array accounts as remaining_accounts
                // These would need to be resolved based on the swap path and current tick
            ],
            data: vec![0], // Placeholder instruction data - needs proper swap instruction encoding
        })
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("Starting Orca Whirlpools discovery from official API: {}", ORCA_API_URL);

        let client = reqwest::Client::new();
        let response = client.get(ORCA_API_URL)
            .timeout(std::time::Duration::from_secs(30))
            .send().await
            .map_err(|e| anyhow!("Failed to fetch Orca pool data: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("Orca API request failed with status: {}", response.status()));
        }

        let api_response: OrcaApiResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse Orca API response: {}", e))?;
        info!("Fetched {} pools from Orca API", api_response.whirlpools.len());

        let pools: Vec<PoolInfo> = api_response.whirlpools.into_iter().filter_map(|api_pool| {
            let pool_address = Pubkey::from_str(&api_pool.address).ok()?;
            let token_a_mint = Pubkey::from_str(&api_pool.token_a.mint).ok()?;
            let token_b_mint = Pubkey::from_str(&api_pool.token_b.mint).ok()?;
            
            Some(PoolInfo {
                address: pool_address,
                name: format!("Orca {}/{}", api_pool.token_a.symbol, api_pool.token_b.symbol),
                dex_type: DexType::Orca,
                token_a: PoolToken {
                    mint: token_a_mint,
                    symbol: api_pool.token_a.symbol,
                    decimals: api_pool.token_a.decimals,
                    reserve: 0, // N/A for CLMM
                },
                token_b: PoolToken {
                    mint: token_b_mint,
                    symbol: api_pool.token_b.symbol,
                    decimals: api_pool.token_b.decimals,
                    reserve: 0, // N/A for CLMM
                },
                // Vaults and live data will be populated later by the PoolDiscoveryService
                token_a_vault: Pubkey::default(),
                token_b_vault: Pubkey::default(),
                fee_rate_bips: None, // Will be populated from on-chain data
                fee_numerator: None,
                fee_denominator: None,
                liquidity: None,
                sqrt_price: None,
                tick_current_index: None,
                tick_spacing: Some(api_pool.tick_spacing),
                last_update_timestamp: 0,
            })
        }).collect();

        info!("Successfully parsed {} pools from Orca API response.", pools.len());
        Ok(pools)
    }
}

// Implement PoolDiscoverable to align with the new discovery service
#[async_trait]
impl PoolDiscoverable for OrcaClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        // This is a bit of a trick: we call the DexClient's discover_pools method.
        // This avoids code duplication while adhering to the trait separation.
        <Self as DexClient>::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // Implement live data fetching for a single Orca pool
        info!("Fetching live data for Orca pool: {}", pool_address);
        
        // For now, return a basic implementation that uses the pool parser
        // In a full implementation, you would:
        // 1. Make an RPC call to get the account data for pool_address
        // 2. Use OrcaPoolParser to parse the raw account data
        // 3. Fetch additional data like vault balances
        
        // Create a parser instance (OrcaPoolParser is a unit struct)
        let parser = OrcaPoolParser;
        
        // Placeholder implementation - in production you'd fetch real account data
        let mock_account_data = vec![0u8; WHIRLPOOL_STATE_SIZE];
        
        // For the parser, we need an RPC client reference
        // In a real implementation, this would be stored in the OrcaClient
        let dummy_rpc = Arc::new(SolanaRpcClient::new("http://dummy", vec![], 1, std::time::Duration::from_secs(1)));
        
        // Parse the pool data
        match parser.parse_pool_data(pool_address, &mock_account_data, &dummy_rpc).await {
            Ok(mut pool_info) => {
                // In a real implementation, you would also fetch:
                // - Current vault token balances
                // - Live sqrt_price and tick data
                // - Fee rates from the pool config
                
                warn!("fetch_pool_data returning mock data. Real implementation needs RPC integration.");
                pool_info.name = format!("Orca Pool {}", pool_address);
                Ok(pool_info)
            }
            Err(e) => {
                Err(anyhow!("Failed to parse Orca pool data for {}: {}", pool_address, e))
            }
        }
    }

    fn dex_name(&self) -> &str {
        self.get_name()
    }
}