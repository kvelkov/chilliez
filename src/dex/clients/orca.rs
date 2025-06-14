// src/dex/clients/orca.rs
//! Orca Whirlpools client, parser, and instruction builder.
//! This is the consolidated, authoritative module for all Orca Whirlpools interactions.

use crate::dex::api::{DexClient, Quote, SwapInfo, PoolDiscoverable, CommonSwapInfo};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser, PoolToken, DexType};
use crate::error::ArbError;
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;
use solana_sdk::instruction::{AccountMeta, Instruction};
use std::sync::Arc;
use log::{info, warn};
use rust_decimal::Decimal;
use num_traits::ToPrimitive;
// use orca_whirlpools_core::math::price_math::price_to_sqrt_price_x64; // TODO: Add dependency
// use orca_whirlpools_core::constants::{MIN_SQRT_PRICE, MAX_SQRT_PRICE}; // TODO: Add dependency
use serde::Deserialize;
use std::str::FromStr; 
// Import our local math functions for Orca calculations


// Placeholder function until orca_whirlpools_core is added
fn price_to_sqrt_price_x64(_price: f64, _token_a_decimals: u8, _token_b_decimals: u8) -> Option<u128> {
    // This is a placeholder - real implementation would convert price to sqrt_price
    // For now, just return a reasonable default
    Some(79228162514264337593543950336u128) // sqrt(2^128) approximation
}

// Placeholder constants until orca_whirlpools_core is added
const MIN_SQRT_PRICE: u128 = 4295048016;
const MAX_SQRT_PRICE: u128 = 79226673515401279992447579055;

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
    #[serde(rename = "feeRate")]
    fee_rate: u16,
    #[serde(rename = "liquidity")]
    liquidity: String,
    #[serde(rename = "sqrtPrice")]
    sqrt_price: String,
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
            // Orca-specific fields (will need to be resolved/calculated)
            tick_array_0: None, // TODO: Derive based on current tick and swap direction
            tick_array_1: None, // TODO: Derive based on current tick and swap direction
            tick_array_2: None, // TODO: Derive based on current tick and swap direction
            oracle: None,       // TODO: Derive oracle PDA if needed
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
                token_a_vault: Pubkey::default(), // Would need to be fetched from on-chain data
                token_b_vault: Pubkey::default(),
                fee_rate_bips: Some(api_pool.fee_rate),
                fee_numerator: None,
                fee_denominator: None,
                liquidity: api_pool.liquidity.parse().ok(),
                sqrt_price: api_pool.sqrt_price.parse().ok(),
                tick_current_index: None, // Would need on-chain data
                tick_spacing: Some(api_pool.tick_spacing),
                last_update_timestamp: 0,
                // Initialize Orca-specific optional fields to None - these need to be resolved
                tick_array_0: None, // TODO: Derive based on current tick and swap direction
                tick_array_1: None, // TODO: Derive based on current tick and swap direction
                tick_array_2: None, // TODO: Derive based on current tick and swap direction
                oracle: None,       // TODO: Derive oracle PDA if needed
            })
        }).collect();

        info!("Successfully parsed {} Orca pools", pools.len());
        Ok(pools)
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, ArbError> {
        info!(
            "OrcaClient: Building enhanced swap instruction for Whirlpool {} ({} -> {})",
            pool_info.address, swap_info.source_token_mint, swap_info.destination_token_mint
        );

        // 1. Validate that pool_info is for Orca Whirlpool and has necessary fields
        // 2. Determine swap direction (a_to_b)
        let a_to_b = if pool_info.token_a.mint == swap_info.source_token_mint && pool_info.token_b.mint == swap_info.destination_token_mint {
            true
        } else if pool_info.token_b.mint == swap_info.source_token_mint && pool_info.token_a.mint == swap_info.destination_token_mint {
            false
        } else {
            return Err(ArbError::InstructionError(
                format!("Mismatched token mints for Orca Whirlpool {} and swap info. Pool A: {}, Pool B: {}, Swap Source: {}, Swap Dest: {}",
                    pool_info.address, pool_info.token_a.mint, pool_info.token_b.mint, swap_info.source_token_mint, swap_info.destination_token_mint
                )));
        };

        // 3. Calculate sqrt_price_limit based on direction and minimum output
        let sqrt_price_limit: u128 = {
            if swap_info.minimum_output_amount == 0 {
                // If any output is acceptable, use the widest possible price limit.
                if a_to_b { MIN_SQRT_PRICE } else { MAX_SQRT_PRICE }
            } else if swap_info.input_amount == 0 {
                // This should ideally be caught before this stage.
                // If input is zero, but min_output is non-zero, it's an impossible scenario.
                // Default to safety: widest possible price limit.
                warn!("Input amount is zero during sqrt_price_limit calculation for Orca. Using default bounds.");
                if a_to_b { MIN_SQRT_PRICE } else { MAX_SQRT_PRICE }
            } else {
                let token_a_decimals = pool_info.token_a.decimals;
                let token_b_decimals = pool_info.token_b.decimals;

                // Convert raw u64 amounts to Decimal, adjusting for token decimals for price calculation
                let input_amount_decimal = Decimal::from(swap_info.input_amount) 
                    / Decimal::from(10u64.pow(if a_to_b { token_a_decimals } else { token_b_decimals } as u32));
                let min_output_amount_decimal = Decimal::from(swap_info.minimum_output_amount)
                    / Decimal::from(10u64.pow(if a_to_b { token_b_decimals } else { token_a_decimals } as u32));

                if input_amount_decimal.is_zero() || min_output_amount_decimal.is_zero() {
                    warn!("Decimal input or min_output is zero for sqrt_price_limit calculation. Using default bounds.");
                    if a_to_b { MIN_SQRT_PRICE } else { MAX_SQRT_PRICE }
                } else {
                    // Calculate the limit price of token_a in terms of token_b
                    // Price = Amount of Token B / Amount of Token A
                    let limit_price_of_a_in_b = if a_to_b { // Input is A, Output is B
                        min_output_amount_decimal / input_amount_decimal
                    } else { // Input is B, Output is A
                        input_amount_decimal / min_output_amount_decimal // This is price of B in A, invert for price of A in B
                    };

                    if limit_price_of_a_in_b.is_sign_negative() || limit_price_of_a_in_b.is_zero() {
                        warn!("Calculated zero or negative price limit ({}), using default bounds for sqrt_price_limit.", limit_price_of_a_in_b);
                        if a_to_b { MIN_SQRT_PRICE } else { MAX_SQRT_PRICE }
                    } else {
                        match price_to_sqrt_price_x64(limit_price_of_a_in_b.to_f64().unwrap_or(0.0), token_a_decimals, token_b_decimals) {
                            Some(val) => val,
                            None => {
                                warn!("Failed to convert price {} to sqrt_price_x64. Using default bounds.", limit_price_of_a_in_b);
                                if a_to_b { MIN_SQRT_PRICE } else { MAX_SQRT_PRICE }
                            }
                        }
                    }
                }
            }
        };

        // 4. Resolve tick array accounts (critical for Orca Whirlpool swaps) - Renumbered from 3.
        let tick_array_0 = pool_info.tick_array_0.ok_or_else(|| {
            ArbError::InstructionError(format!("Missing Orca tick_array_0 for pool {}", pool_info.address))
        })?;
        let tick_array_1 = pool_info.tick_array_1.ok_or_else(|| {
            ArbError::InstructionError(format!("Missing Orca tick_array_1 for pool {}", pool_info.address))
        })?;
        let tick_array_2 = pool_info.tick_array_2.ok_or_else(|| {
            ArbError::InstructionError(format!("Missing Orca tick_array_2 for pool {}", pool_info.address))
        })?;
        let oracle_pda = pool_info.oracle.ok_or_else(|| {
            ArbError::InstructionError(format!("Missing Orca oracle PDA for pool {}", pool_info.address))
        })?;

        // 5. Build accounts array for Orca Whirlpool swap instruction
        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // token_authority (signer)
            AccountMeta::new(pool_info.address, false), // whirlpool
            AccountMeta::new(swap_info.user_source_token_account, false), // token_owner_account_a or b
            AccountMeta::new(pool_info.token_a_vault, false), // token_vault_a
            AccountMeta::new(swap_info.user_destination_token_account, false), // token_owner_account_b or a
            AccountMeta::new(pool_info.token_b_vault, false), // token_vault_b
            AccountMeta::new(tick_array_0, false), // tick_array_0
            AccountMeta::new(tick_array_1, false), // tick_array_1
            AccountMeta::new(tick_array_2, false), // tick_array_2
            AccountMeta::new_readonly(oracle_pda, false), // oracle
        ];

        // 6. Build instruction data for Orca Whirlpool swap
        // Placeholder data - THIS MUST BE REPLACED with actual Orca swap instruction data
        let mut instruction_data = Vec::new();
        
        if instruction_data.is_empty() {
             warn!("OrcaClient: Instruction data is a placeholder and needs to be implemented using Anchor serialization!");
             return Err(ArbError::InstructionError(
                "Orca swap instruction data not implemented".to_string()
            ));
        }

        Ok(Instruction {
            program_id: ORCA_WHIRLPOOL_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }
}

#[async_trait]
impl PoolDiscoverable for OrcaClient {
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
                token_a_vault: Pubkey::default(), // Would need to be fetched from on-chain data
                token_b_vault: Pubkey::default(),
                fee_rate_bips: Some(api_pool.fee_rate),
                fee_numerator: None,
                fee_denominator: None,
                liquidity: api_pool.liquidity.parse().ok(),
                sqrt_price: api_pool.sqrt_price.parse().ok(),
                tick_current_index: None, // Would need on-chain data
                tick_spacing: Some(api_pool.tick_spacing),
                // Initialize Orca-specific optional fields to None
                tick_array_0: None,
                tick_array_1: None,
                tick_array_2: None,
                oracle: None,
                last_update_timestamp: 0,
            })
        }).collect();

        info!("Successfully parsed {} pools from Orca API response.", pools.len());
        Ok(pools)
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        info!("Fetching detailed data for Orca pool: {}", pool_address);
        
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
        "Orca"
    }
}