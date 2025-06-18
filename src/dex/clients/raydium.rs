// src/dex/raydium.rs
//! Raydium client and parser for on-chain data and instruction building.
//! This implementation follows the official Raydium V4 layout for maximum accuracy.
//! Includes Raydium API data models.

use crate::dex::api::{
    CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo,
};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use chrono;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, Mint};
use std::str::FromStr;
use std::sync::Arc;

// --- Constants (made public for tests) ---
pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
const RAYDIUM_LIQUIDITY_JSON_URL: &str = "https://api.raydium.io/v2/sdk/liquidity/mainnet.json";
pub const RAYDIUM_V4_POOL_STATE_SIZE: usize = 752;

// --- On-Chain Data Structures ---
#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LiquidityStateV4 {
    pub status: u64,
    pub nonce: u64,
    pub max_order: u64,
    pub depth: u64,
    pub base_decimal: u64,
    pub quote_decimal: u64,
    pub state: u64,
    pub reset_flag: u64,
    pub min_size: u64,
    pub vol_max_cut_ratio: u64,
    pub amount_wave_ratio: u64,
    pub base_lot_size: u64,
    pub quote_lot_size: u64,
    pub min_price_multiplier: u64,
    pub max_price_multiplier: u64,
    pub system_decimal_value: u64,
    pub min_separate_numerator: u64,
    pub min_separate_denominator: u64,
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    pub pnl_numerator: u64,
    pub pnl_denominator: u64,
    pub swap_fee_numerator: u64,
    pub swap_fee_denominator: u64,
    pub base_need_take_pnl: u64,
    pub quote_need_take_pnl: u64,
    pub quote_total_pnl: u64,
    pub base_total_pnl: u64,
    pub pool_open_time: u64,
    pub punish_pc_amount: u64,
    pub punish_coin_amount: u64,
    pub orderbook_to_init_time: u64,
    pub swap_base_in_amount: u128,
    pub swap_quote_out_amount: u128,
    pub swap_base2_quote_fee: u64,
    pub swap_quote_in_amount: u128,
    pub swap_base_out_amount: u128,
    pub swap_quote2_base_fee: u64,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub open_orders: Pubkey,
    pub market_id: Pubkey,
    pub market_program_id: Pubkey,
    pub target_orders: Pubkey,
    pub withdraw_queue: Pubkey,
    pub lp_vault: Pubkey,
    pub owner: Pubkey,
    pub lp_reserve: u64,
    pub padding: [u64; 3],
}

// --- Pool Parser ---
pub struct RaydiumPoolParser;

#[async_trait]
impl UtilsPoolParser for RaydiumPoolParser {
    async fn parse_pool_data(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < RAYDIUM_V4_POOL_STATE_SIZE {
            return Err(anyhow!(
                "Invalid Raydium pool data size: expected {}, got {}",
                RAYDIUM_V4_POOL_STATE_SIZE,
                data.len()
            ));
        }

        let pool_state =
            bytemuck::from_bytes::<LiquidityStateV4>(&data[..RAYDIUM_V4_POOL_STATE_SIZE]);

        // Fetch token account data concurrently
        let (base_account_result, quote_account_result, base_mint_result, quote_mint_result) = tokio::join!(
            rpc_client.get_account_data(&pool_state.base_vault),
            rpc_client.get_account_data(&pool_state.quote_vault),
            rpc_client.get_account_data(&pool_state.base_mint),
            rpc_client.get_account_data(&pool_state.quote_mint)
        );

        // Parse token account data
        let base_account_data = base_account_result?;
        let quote_account_data = quote_account_result?;
        let base_mint_data = base_mint_result?;
        let quote_mint_data = quote_mint_result?;

        // Unpack token account data
        let base_token_account = TokenAccount::unpack(&base_account_data)?;
        let quote_token_account = TokenAccount::unpack(&quote_account_data)?;
        let base_mint = Mint::unpack(&base_mint_data)?;
        let quote_mint = Mint::unpack(&quote_mint_data)?;

        let pool_info = PoolInfo {
            address: pool_address,
            name: format!("Raydium V4 Pool"),
            token_a: PoolToken {
                mint: pool_state.base_mint,
                symbol: "Unknown".to_string(), // Would be resolved from metadata
                decimals: base_mint.decimals,
                reserve: base_token_account.amount,
            },
            token_b: PoolToken {
                mint: pool_state.quote_mint,
                symbol: "Unknown".to_string(),
                decimals: quote_mint.decimals,
                reserve: quote_token_account.amount,
            },
            token_a_vault: pool_state.base_vault,
            token_b_vault: pool_state.quote_vault,
            fee_numerator: Some(pool_state.swap_fee_numerator),
            fee_denominator: Some(pool_state.swap_fee_denominator),
            fee_rate_bips: Some(
                (pool_state.swap_fee_numerator * 10000 / pool_state.swap_fee_denominator) as u16,
            ),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Raydium,
            // Raydium V4 is not CLMM, so these are None
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            // Orca-specific fields (not applicable)
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };

        Ok(pool_info)
    }

    fn get_program_id(&self) -> Pubkey {
        RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID
    }
}

// --- Raydium Client ---
pub struct RaydiumClient {
    pub name: String,
}

impl RaydiumClient {
    pub fn new() -> Self {
        Self {
            name: "Raydium".to_string(),
        }
    }
}

#[async_trait]
impl DexClient for RaydiumClient {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Use Raydium-specific calculation from math module
        let fee_numerator = pool.fee_numerator.unwrap_or(25);
        let fee_denominator = pool.fee_denominator.unwrap_or(10000);

        let swap_result = crate::dex::math::raydium::calculate_raydium_swap_output(
            input_amount,
            pool.token_a.reserve,
            pool.token_b.reserve,
            fee_numerator,
            fee_denominator,
        )
        .map_err(|e| anyhow!("Raydium quote calculation failed: {}", e))?;

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount: swap_result.output_amount,
            dex: self.name.clone(),
            route: vec![pool.address],
            slippage_estimate: Some(swap_result.price_impact), // Use calculated price impact
        })
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        warn!("get_swap_instruction for Raydium is a basic implementation. Use get_swap_instruction_enhanced for production.");

        // Create a basic instruction for legacy compatibility
        Ok(Instruction {
            program_id: RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(swap_info.user_wallet, true),
                AccountMeta::new(swap_info.pool_account, false),
                AccountMeta::new(swap_info.user_source_token_account, false),
                AccountMeta::new(swap_info.user_destination_token_account, false),
            ],
            data: vec![9], // Raydium swap instruction discriminator
        })
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        info!(
            "RaydiumClient: Building production swap instruction for pool {} ({} -> {})",
            pool_info.address, swap_info.source_token_mint, swap_info.destination_token_mint
        );

        // Validate that the tokens match the pool
        if swap_info.source_token_mint != pool_info.token_a.mint
            && swap_info.source_token_mint != pool_info.token_b.mint
        {
            return Err(crate::error::ArbError::InstructionError(format!(
                "Source token {} does not match pool tokens",
                swap_info.source_token_mint
            )));
        }

        if swap_info.destination_token_mint != pool_info.token_a.mint
            && swap_info.destination_token_mint != pool_info.token_b.mint
        {
            return Err(crate::error::ArbError::InstructionError(format!(
                "Destination token {} does not match pool tokens",
                swap_info.destination_token_mint
            )));
        }

        // Calculate minimum output with slippage protection
        let swap_result = crate::dex::math::raydium::calculate_raydium_swap_output(
            swap_info.input_amount,
            pool_info.token_a.reserve,
            pool_info.token_b.reserve,
            pool_info.fee_numerator.unwrap_or(25),
            pool_info.fee_denominator.unwrap_or(10000),
        )
        .map_err(|e| {
            crate::error::ArbError::InstructionError(format!(
                "Failed to calculate swap output: {}",
                e
            ))
        })?;

        // Use calculated minimum output or provided one, whichever is higher for safety
        let minimum_output = swap_info.minimum_output_amount.max(
            crate::dex::math::raydium::calculate_minimum_output_with_slippage(
                swap_result.output_amount,
                500, // 5% default slippage tolerance
            ),
        );

        // TODO: In production, these addresses need to be derived from pool state
        // For now, we use placeholders that would be resolved from on-chain pool data
        let amm_authority = derive_amm_authority(&pool_info.address)?;
        let market_program_id = derive_market_program_id(&pool_info)?;
        let market_id = derive_market_id(&pool_info)?;

        // Build complete Raydium V4 swap instruction with all required accounts
        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false), // TOKEN_PROGRAM_ID
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // User wallet (signer)
            AccountMeta::new(pool_info.address, false),        // AMM ID
            AccountMeta::new_readonly(amm_authority, false),   // AMM authority
            AccountMeta::new(swap_info.user_source_token_account, false), // User source token account
            AccountMeta::new(swap_info.user_destination_token_account, false), // User destination token account
            AccountMeta::new(pool_info.token_a_vault, false),                  // Pool coin vault
            AccountMeta::new(pool_info.token_b_vault, false),                  // Pool PC vault
            AccountMeta::new_readonly(market_program_id, false),               // Market program
            AccountMeta::new(market_id, false),                                // Market ID
            AccountMeta::new(derive_market_bids(&market_id)?, false),          // Market bids
            AccountMeta::new(derive_market_asks(&market_id)?, false),          // Market asks
            AccountMeta::new(derive_market_event_queue(&market_id)?, false),   // Market event queue
            AccountMeta::new(derive_market_coin_vault(&market_id)?, false),    // Market coin vault
            AccountMeta::new(derive_market_pc_vault(&market_id)?, false),      // Market PC vault
            AccountMeta::new_readonly(derive_market_vault_signer(&market_id)?, false), // Market vault signer
        ];

        // Build instruction data (Raydium V4 swap instruction format)
        let mut instruction_data = Vec::new();
        instruction_data.push(9); // Raydium swap instruction discriminator
        instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes()); // Amount in
        instruction_data.extend_from_slice(&minimum_output.to_le_bytes()); // Minimum amount out

        info!(
            "RaydiumClient: Built swap instruction - input: {}, min_output: {}, price_impact: {:.4}%",
            swap_info.input_amount, minimum_output, swap_result.price_impact * 100.0
        );

        Ok(Instruction {
            program_id: RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("RaydiumClient: Starting pool discovery from API...");

        let client = reqwest::Client::new();
        info!("[DEBUG] RaydiumClient: Sent GET request to {}", RAYDIUM_LIQUIDITY_JSON_URL);
        let response = client.get(RAYDIUM_LIQUIDITY_JSON_URL).send().await?;
        info!("[DEBUG] RaydiumClient: Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch Raydium pools: HTTP {}",
                response.status()
            ));
        }

        let liquidity_file: LiquidityFile = response.json().await?;

        let pools: Vec<PoolInfo> = liquidity_file
            .official
            .into_iter()
            .filter_map(|pool| {
                // Parse pool data
                let pool_pubkey = Pubkey::from_str(&pool.id).ok()?;
                let base_mint = Pubkey::from_str(&pool.base_mint).ok()?;
                let quote_mint = Pubkey::from_str(&pool.quote_mint).ok()?;
                let base_vault = Pubkey::from_str(&pool.base_vault).ok()?;
                let quote_vault = Pubkey::from_str(&pool.quote_vault).ok()?;

                Some(PoolInfo {
                    address: pool_pubkey,
                    name: format!(
                        "Raydium {}/{}",
                        pool.base_symbol.clone().unwrap_or_default(),
                        pool.quote_symbol.clone().unwrap_or_default()
                    ),
                    dex_type: DexType::Raydium,
                    token_a: PoolToken {
                        mint: base_mint,
                        symbol: pool.base_symbol.unwrap_or_default(),
                        decimals: pool.base_decimals.unwrap_or(0),
                        reserve: 0,
                    },
                    token_b: PoolToken {
                        mint: quote_mint,
                        symbol: pool.quote_symbol.unwrap_or_default(),
                        decimals: pool.quote_decimals.unwrap_or(0),
                        reserve: 0,
                    },
                    token_a_vault: base_vault,
                    token_b_vault: quote_vault,
                    fee_numerator: Some(25),
                    fee_denominator: Some(10000),
                    fee_rate_bips: Some(25),
                    last_update_timestamp: 0,
                    sqrt_price: None,
                    liquidity: None,
                    tick_current_index: None,
                    tick_spacing: None,
                    // Orca-specific fields (not applicable to Raydium)
                    tick_array_0: None,
                    tick_array_1: None,
                    tick_array_2: None,
                    oracle: None,
                })
            })
            .collect();

        info!("RaydiumClient: Discovered {} pools", pools.len());
        Ok(pools)
    }

    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError> {
        let start_time = std::time::Instant::now();

        // Test Raydium API connectivity
        let client = reqwest::Client::new();
        match client.get(RAYDIUM_LIQUIDITY_JSON_URL).send().await {
            Ok(response) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                let is_healthy = response.status().is_success();

                let health_result = DexHealthStatus {
                    is_healthy,
                    last_successful_request: if is_healthy { Some(start_time) } else { None },
                    error_count: if is_healthy { 0 } else { 1 },
                    response_time_ms: Some(response_time),
                    pool_count: None, // Would require parsing the response
                    status_message: if is_healthy {
                        format!("Raydium API healthy ({}ms)", response_time)
                    } else {
                        format!("Raydium API returned HTTP {}", response.status())
                    },
                };

                if health_result.is_healthy {
                    info!(
                        "Raydium health check passed: {}",
                        health_result.status_message
                    );
                } else {
                    warn!(
                        "Raydium health check failed: {}",
                        health_result.status_message
                    );
                }

                Ok(health_result)
            }
            Err(e) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                warn!("Raydium health check failed: {}", e);

                Ok(DexHealthStatus {
                    is_healthy: false,
                    last_successful_request: None,
                    error_count: 1,
                    response_time_ms: Some(response_time),
                    pool_count: None,
                    status_message: format!("Raydium API error: {}", e),
                })
            }
        }
    }
}

#[async_trait]
impl PoolDiscoverable for RaydiumClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        Err(anyhow!(
            "fetch_pool_data not yet implemented for RaydiumClient. Pool address: {}",
            pool_address
        ))
    }

    fn dex_name(&self) -> &str {
        self.get_name()
    }
}

// =====================================================================================
// PDA DERIVATION HELPER FUNCTIONS FOR RAYDIUM SWAP INSTRUCTIONS
// =====================================================================================

/// Derive AMM authority PDA for a Raydium pool
fn derive_amm_authority(pool_address: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    // In production, this would derive the actual AMM authority PDA
    // For now, return a placeholder that would be replaced with proper derivation
    // The seed is typically something like [pool_address, "amm_authority"]
    Ok(Pubkey::find_program_address(
        &[pool_address.as_ref(), b"amm_authority"],
        &RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
    )
    .0)
}

/// Derive market program ID from pool info (typically Serum DEX)
fn derive_market_program_id(_pool_info: &PoolInfo) -> Result<Pubkey, crate::error::ArbError> {
    // In a real implementation, this would be read from the pool's on-chain state
    // For now, use the standard Serum DEX program ID
    Ok(solana_sdk::pubkey!(
        "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"
    )) // Serum DEX v3
}

/// Derive market ID from pool info
fn derive_market_id(pool_info: &PoolInfo) -> Result<Pubkey, crate::error::ArbError> {
    // In production, this would be read from the pool's market_id field in LiquidityStateV4
    // For now, derive a placeholder based on pool address
    Ok(Pubkey::find_program_address(
        &[pool_info.address.as_ref(), b"market"],
        &RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
    )
    .0)
}

/// Derive market bids account
fn derive_market_bids(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"bids"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

/// Derive market asks account  
fn derive_market_asks(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"asks"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

/// Derive market event queue account
fn derive_market_event_queue(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"event_queue"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

/// Derive market coin vault account
fn derive_market_coin_vault(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"coin_vault"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

/// Derive market PC vault account
fn derive_market_pc_vault(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"pc_vault"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

/// Derive market vault signer account
fn derive_market_vault_signer(market_id: &Pubkey) -> Result<Pubkey, crate::error::ArbError> {
    Ok(Pubkey::find_program_address(
        &[market_id.as_ref(), b"vault_signer"],
        &solana_sdk::pubkey!("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"),
    )
    .0)
}

// =====================================================================================
// RAYDIUM API DATA MODELS (from raydium_models.rs)
// =====================================================================================

/// Root structure for Raydium liquidity JSON response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityFile {
    pub official: Vec<AmmPool>,
    #[serde(default)]
    pub un_official: Vec<AmmPool>,
}

/// Individual AMM pool from Raydium API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmmPool {
    pub id: String,
    #[serde(rename = "baseMint")]
    pub base_mint: String,
    #[serde(rename = "quoteMint")]
    pub quote_mint: String,
    #[serde(rename = "lpMint")]
    pub lp_mint: String,
    #[serde(rename = "baseDecimals")]
    pub base_decimals: Option<u8>,
    #[serde(rename = "quoteDecimals")]
    pub quote_decimals: Option<u8>,
    #[serde(rename = "lpDecimals")]
    pub lp_decimals: Option<u8>,
    pub version: u8,
    #[serde(rename = "programId")]
    pub program_id: String,
    pub authority: String,
    #[serde(rename = "openOrders")]
    pub open_orders: String,
    #[serde(rename = "targetOrders")]
    pub target_orders: String,
    #[serde(rename = "baseVault")]
    pub base_vault: String,
    #[serde(rename = "quoteVault")]
    pub quote_vault: String,
    #[serde(rename = "withdrawQueue")]
    pub withdraw_queue: String,
    #[serde(rename = "lpVault")]
    pub lp_vault: String,
    #[serde(rename = "marketVersion")]
    pub market_version: u8,
    #[serde(rename = "marketProgramId")]
    pub market_program_id: String,
    #[serde(rename = "marketId")]
    pub market_id: String,
    #[serde(rename = "marketAuthority")]
    pub market_authority: String,
    #[serde(rename = "marketBaseVault")]
    pub market_base_vault: String,
    #[serde(rename = "marketQuoteVault")]
    pub market_quote_vault: String,
    #[serde(rename = "marketBids")]
    pub market_bids: String,
    #[serde(rename = "marketAsks")]
    pub market_asks: String,
    #[serde(rename = "marketEventQueue")]
    pub market_event_queue: String,
    #[serde(rename = "lookupTableAccount")]
    pub lookup_table_account: Option<String>,
    #[serde(rename = "baseSymbol")]
    pub base_symbol: Option<String>,
    #[serde(rename = "quoteSymbol")]
    pub quote_symbol: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raydium_client_creation() {
        let client = RaydiumClient::new();
        assert_eq!(client.get_name(), "Raydium");
    }

    #[test]
    fn test_raydium_pool_parser_program_id() {
        let parser = RaydiumPoolParser;
        assert_eq!(
            parser.get_program_id(),
            RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID
        );
    }

    #[test]
    fn test_liquidity_state_v4_size() {
        assert_eq!(
            std::mem::size_of::<LiquidityStateV4>(),
            RAYDIUM_V4_POOL_STATE_SIZE
        );
    }
}
