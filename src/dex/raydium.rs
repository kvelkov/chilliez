// src/dex/raydium.rs
//! Raydium client and parser for on-chain data and instruction building.
//! This implementation follows the official Raydium V4 layout for maximum accuracy.

use crate::dex::quote::{DexClient, Quote, SwapInfo, PoolDiscoverable};
use crate::dex::raydium_models::LiquidityFile;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{info, warn};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, Mint};
use std::sync::Arc;
use std::str::FromStr;

// Official Program ID from Raydium's documentation.
pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");

// Raydium API endpoint for pool discovery
const RAYDIUM_LIQUIDITY_JSON_URL: &str = "https://api.raydium.io/v2/sdk/liquidity/mainnet.json";

// Expected account size for Raydium V4 pool state (verified from official implementations)
pub const RAYDIUM_V4_POOL_STATE_SIZE: usize = 752;

// --- On-Chain Data Structures ---

/// Raydium Liquidity Pool V4 state account layout.
/// This matches the official LIQUIDITY_STATE_LAYOUT_V4 from Raydium SDK exactly.
/// Total size: 752 bytes
#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LiquidityStateV4 {
    // Basic pool state (8 * 24 = 192 bytes)
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
    
    // Fee configuration (8 * 8 = 64 bytes)
    pub min_separate_numerator: u64,
    pub min_separate_denominator: u64,
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    pub pnl_numerator: u64,
    pub pnl_denominator: u64,
    pub swap_fee_numerator: u64,
    pub swap_fee_denominator: u64,
    
    // PnL and timing data (8 * 8 = 64 bytes)
    pub base_need_take_pnl: u64,
    pub quote_need_take_pnl: u64,
    pub quote_total_pnl: u64,
    pub base_total_pnl: u64,
    pub pool_open_time: u64,
    pub punish_pc_amount: u64,
    pub punish_coin_amount: u64,
    pub orderbook_to_init_time: u64,
    
    // Swap statistics (16 + 16 + 8 + 16 + 16 + 8 = 80 bytes)
    pub swap_base_in_amount: u128,     // u128 field
    pub swap_quote_out_amount: u128,   // u128 field
    pub swap_base_2_quote_fee: u64,
    pub swap_quote_in_amount: u128,    // u128 field
    pub swap_base_out_amount: u128,    // u128 field
    pub swap_quote_2_base_fee: u64,
    
    // Account addresses (32 * 12 = 384 bytes)
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
    
    // Final fields (8 + 8*3 = 32 bytes)
    pub lp_reserve: u64,
    pub padding: [u64; 3],
}

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<LiquidityStateV4>() == RAYDIUM_V4_POOL_STATE_SIZE);

// --- On-Chain Data Parser ---

pub struct RaydiumPoolParser;

#[async_trait::async_trait]
impl UtilsPoolParser for RaydiumPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        info!("Parsing Raydium AMM V4 pool data for address: {}", address);

        // Validate account data size
        if data.len() < RAYDIUM_V4_POOL_STATE_SIZE {
            return Err(anyhow!(
                "Invalid Raydium V4 pool account data length for {}: expected {} bytes, got {}",
                address,
                RAYDIUM_V4_POOL_STATE_SIZE,
                data.len()
            ));
        }

        // Parse the pool state using bytemuck for safe memory casting
        let pool_state: &LiquidityStateV4 = bytemuck::from_bytes(&data[..RAYDIUM_V4_POOL_STATE_SIZE]);
        
        // Validate that the pool is active (status should be non-zero for active pools)
        if pool_state.status == 0 {
            return Err(anyhow!(
                "Raydium pool {} appears to be inactive (status = 0)",
                address
            ));
        }

        // Fetch token vault account data to get current reserves
        let (base_vault_data, quote_vault_data) = tokio::try_join!(
            async { 
                rpc_client.primary_client.get_account_data(&pool_state.base_vault).await
                    .map_err(|e| anyhow!("Failed to fetch base vault data for {}: {}", pool_state.base_vault, e))
            },
            async { 
                rpc_client.primary_client.get_account_data(&pool_state.quote_vault).await
                    .map_err(|e| anyhow!("Failed to fetch quote vault data for {}: {}", pool_state.quote_vault, e))
            }
        )?;

        // Parse token vault accounts to get reserves
        let base_reserve = TokenAccount::unpack_from_slice(&base_vault_data)
            .map_err(|e| anyhow!("Failed to parse base vault token account: {}", e))?
            .amount;
        let quote_reserve = TokenAccount::unpack_from_slice(&quote_vault_data)
            .map_err(|e| anyhow!("Failed to parse quote vault token account: {}", e))?
            .amount;

        // Fetch token mint data to get decimals
        let (base_mint_data, quote_mint_data) = tokio::try_join!(
            async { 
                rpc_client.primary_client.get_account_data(&pool_state.base_mint).await
                    .map_err(|e| anyhow!("Failed to fetch base mint data for {}: {}", pool_state.base_mint, e))
            },
            async { 
                rpc_client.primary_client.get_account_data(&pool_state.quote_mint).await
                    .map_err(|e| anyhow!("Failed to fetch quote mint data for {}: {}", pool_state.quote_mint, e))
            }
        )?;

        let base_decimals = Mint::unpack_from_slice(&base_mint_data)
            .map_err(|e| anyhow!("Failed to parse base mint: {}", e))?
            .decimals;
        let quote_decimals = Mint::unpack_from_slice(&quote_mint_data)
            .map_err(|e| anyhow!("Failed to parse quote mint: {}", e))?
            .decimals;

        // Validate that decimals match what's stored in the pool state
        let base_decimal_from_pool = pool_state.base_decimal;
        let quote_decimal_from_pool = pool_state.quote_decimal;
        
        if base_decimals != base_decimal_from_pool as u8 {
            return Err(anyhow!(
                "Base token decimals mismatch for pool {}: mint={}, pool={}",
                address, base_decimals, base_decimal_from_pool
            ));
        }
        if quote_decimals != quote_decimal_from_pool as u8 {
            return Err(anyhow!(
                "Quote token decimals mismatch for pool {}: mint={}, pool={}",
                address, quote_decimals, quote_decimal_from_pool
            ));
        }

        Ok(PoolInfo {
            address,
            name: format!("Raydium AMM V4/{}", address),
            dex_type: DexType::Raydium,
            token_a: PoolToken {
                mint: pool_state.base_mint,
                symbol: "BASE".to_string(), // Placeholder - could be enhanced with token registry
                decimals: base_decimals,
                reserve: base_reserve,
            },
            token_b: PoolToken {
                mint: pool_state.quote_mint,
                symbol: "QUOTE".to_string(), // Placeholder - could be enhanced with token registry
                decimals: quote_decimals,
                reserve: quote_reserve,
            },
            token_a_vault: pool_state.base_vault,
            token_b_vault: pool_state.quote_vault,
            fee_numerator: Some(pool_state.swap_fee_numerator),
            fee_denominator: Some(pool_state.swap_fee_denominator),
            fee_rate_bips: None, // Raydium uses numerator/denominator, not basis points
            last_update_timestamp: pool_state.pool_open_time,
            // AMM-specific fields (not applicable to constant product pools)
            liquidity: None,      
            sqrt_price: None,     
            tick_current_index: None, 
            tick_spacing: None,   
        })
    }

    fn get_program_id(&self) -> Pubkey {
        RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID
    }
}

// --- DEX Client Implementation ---

#[derive(Debug, Clone, Default)]
pub struct RaydiumClient;

impl RaydiumClient {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DexClient for RaydiumClient {
    fn get_name(&self) -> &str {
        "Raydium"
    }

    /// Calculates a quote using the Constant Product formula for AMM V4 pools.
    fn calculate_onchain_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> AnyhowResult<Quote> {
        if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
            return Err(anyhow!("Raydium pool {} has zero reserves.", pool.address));
        }

        let fee_num = pool.fee_numerator.ok_or_else(|| anyhow!("Raydium pool {} fee_numerator is None", pool.address))?;
        let fee_den = pool.fee_denominator.ok_or_else(|| anyhow!("Raydium pool {} fee_denominator is None", pool.address))?;

        if fee_den == 0 {
            return Err(anyhow!("Raydium pool {} fee denominator cannot be zero.", pool.address));
        }
        let fee = (input_amount as u128 * fee_num as u128 / fee_den as u128) as u64;
        let input_amount_after_fee = input_amount.saturating_sub(fee);

        let k = pool.token_a.reserve as u128 * pool.token_b.reserve as u128;
        let output_amount = (pool.token_b.reserve as u128)
            .saturating_sub(k / (pool.token_a.reserve as u128 + input_amount_after_fee as u128));
        
        if output_amount == 0 {
            return Err(anyhow!("Output amount is zero, likely due to high fee or low input."));
        }

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount: output_amount as u64,
            dex: self.get_name().to_string(),
            route: vec![pool.address],
            slippage_estimate: None,
        })
    }

    fn get_swap_instruction(
        &self,
        swap_info: &SwapInfo,
    ) -> AnyhowResult<Instruction> {
        let instruction_data = RaydiumSwapInstruction {
            instruction: 9, // Swap instruction index for Raydium AMM V4
            amount_in: swap_info.amount_in, // fixed field name
            min_amount_out: swap_info.min_output_amount, // fixed field name
        };

        let data = instruction_data_to_bytes(&instruction_data)?;

        let accounts = vec![
            AccountMeta::new(swap_info.user_source_token_account, false),
            AccountMeta::new(swap_info.user_destination_token_account, false),
            AccountMeta::new(swap_info.pool_account, false),
            AccountMeta::new(swap_info.pool_authority, false),
            AccountMeta::new(swap_info.pool_open_orders, false),
            AccountMeta::new(swap_info.pool_target_orders, false),
            AccountMeta::new(swap_info.pool_base_vault, false),
            AccountMeta::new(swap_info.pool_quote_vault, false),
            AccountMeta::new_readonly(swap_info.market_id, false),
            AccountMeta::new_readonly(swap_info.market_bids, false),
            AccountMeta::new_readonly(swap_info.market_asks, false),
            AccountMeta::new_readonly(swap_info.market_event_queue, false),
            AccountMeta::new_readonly(swap_info.market_program_id, false),
            AccountMeta::new_readonly(swap_info.market_authority, false),
            AccountMeta::new_readonly(swap_info.user_owner, true),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];

        Ok(Instruction {
            program_id: RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            accounts,
            data,
        })
    }

    /// Discovers all supported liquidity pools for the DEX.
    ///
    /// This method is responsible for fetching the addresses and static data of all pools.
    /// It should prioritize efficient methods like fetching a JSON list over broad RPC calls.
    ///
    /// # Returns
    /// A vector of `PoolInfo` structs, potentially with live market data missing,
    /// which will be fetched later in a batched call.
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("Starting Raydium pool discovery using official API");
        
        // 1. Fetch the JSON file
        let response = reqwest::get(RAYDIUM_LIQUIDITY_JSON_URL).await
            .map_err(|e| anyhow!("Failed to fetch Raydium liquidity data: {}", e))?;
        
        let liquidity_file: LiquidityFile = response.json().await
            .map_err(|e| anyhow!("Failed to parse Raydium liquidity JSON: {}", e))?;

        info!("Successfully fetched Raydium liquidity data with {} official pools", 
              liquidity_file.official.len());

        // 2. Transform the JSON data into placeholder PoolInfo structs
        let pools: Vec<PoolInfo> = liquidity_file.official.into_iter().filter_map(|pool| {
            // We use filter_map to easily skip any pools that fail to parse
            let pool_pubkey = Pubkey::from_str(&pool.id).ok()?;
            let base_mint = Pubkey::from_str(&pool.base_mint).ok()?;
            let quote_mint = Pubkey::from_str(&pool.quote_mint).ok()?;
            let base_vault = Pubkey::from_str(&pool.base_vault).ok()?;
            let quote_vault = Pubkey::from_str(&pool.quote_vault).ok()?;

            // Extract symbols to avoid ownership issues  
            let base_symbol = pool.base_symbol.unwrap_or_else(|| "BASE".to_string());
            let quote_symbol = pool.quote_symbol.unwrap_or_else(|| "QUOTE".to_string());

            // NOTE: At this stage, we only populate static data.
            // Live data like reserves will be fetched later.
            Some(PoolInfo {
                address: pool_pubkey,
                name: format!("Raydium {}/{}", base_symbol, quote_symbol),
                dex_type: DexType::Raydium,
                token_a: PoolToken {
                    mint: base_mint,
                    symbol: base_symbol,
                    decimals: pool.base_decimals.unwrap_or(9),
                    reserve: 0, // Will be fetched later with live data
                },
                token_b: PoolToken {
                    mint: quote_mint,
                    symbol: quote_symbol,
                    decimals: pool.quote_decimals.unwrap_or(6),
                    reserve: 0, // Will be fetched later with live data
                },
                token_a_vault: base_vault,
                token_b_vault: quote_vault,
                fee_numerator: Some(25),    // Default Raydium AMM fee (0.25%)
                fee_denominator: Some(10000),
                last_update_timestamp: 0, // Will be updated with live data
                sqrt_price: None,         // Will be fetched later
                liquidity: None,          // Will be fetched later
                tick_current_index: None,
                tick_spacing: None,
                fee_rate_bips: Some(25),  // 0.25% in basis points
            })
        }).collect();

        info!("Successfully parsed {} Raydium pools from official API", pools.len());
        
        if pools.is_empty() {
            warn!("No valid Raydium pools found in API response");
        }

        Ok(pools)
    }
}

// Define RaydiumSwapInstruction explicitly
#[repr(C)]
#[derive(Clone, Debug)]
pub struct RaydiumSwapInstruction {
    pub instruction: u8, // 9 for swap instruction in Raydium AMM V4
    pub amount_in: u64,
    pub min_amount_out: u64,
}

// Helper function to serialize instruction data
fn instruction_data_to_bytes(instruction: &RaydiumSwapInstruction) -> AnyhowResult<Vec<u8>> {
    let mut data = Vec::with_capacity(17);
    data.push(instruction.instruction);
    data.extend_from_slice(&instruction.amount_in.to_le_bytes());
    data.extend_from_slice(&instruction.min_amount_out.to_le_bytes());
    Ok(data)
}

// --- Pool Discovery Implementation ---

/// Raydium client implementation of pool discovery functionality.
/// This follows the strategy of using official Raydium pool lists for efficiency.
#[async_trait]
impl PoolDiscoverable for RaydiumClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("Starting RaydiumClient pool discovery using official API");
        
        // 1. Fetch the JSON file
        let response = reqwest::get(RAYDIUM_LIQUIDITY_JSON_URL).await
            .map_err(|e| anyhow!("Failed to fetch Raydium liquidity data: {}", e))?;
        
        let liquidity_file: LiquidityFile = response.json().await
            .map_err(|e| anyhow!("Failed to parse Raydium liquidity JSON: {}", e))?;

        info!("Successfully fetched Raydium liquidity data with {} official pools", 
              liquidity_file.official.len());

        // 2. Transform the JSON data into placeholder PoolInfo structs
        let pools: Vec<PoolInfo> = liquidity_file.official.into_iter().filter_map(|pool| {
            // We use filter_map to easily skip any pools that fail to parse
            let pool_pubkey = Pubkey::from_str(&pool.id).ok()?;
            let base_mint = Pubkey::from_str(&pool.base_mint).ok()?;
            let quote_mint = Pubkey::from_str(&pool.quote_mint).ok()?;
            let base_vault = Pubkey::from_str(&pool.base_vault).ok()?;
            let quote_vault = Pubkey::from_str(&pool.quote_vault).ok()?;

            // Extract symbols to avoid ownership issues  
            let base_symbol = pool.base_symbol.unwrap_or_else(|| "BASE".to_string());
            let quote_symbol = pool.quote_symbol.unwrap_or_else(|| "QUOTE".to_string());

            // NOTE: At this stage, we only populate static data.
            // Live data like reserves will be fetched later.
            Some(PoolInfo {
                address: pool_pubkey,
                name: format!("Raydium {}/{}", base_symbol, quote_symbol),
                dex_type: DexType::Raydium,
                token_a: PoolToken {
                    mint: base_mint,
                    symbol: base_symbol,
                    decimals: pool.base_decimals.unwrap_or(9),
                    reserve: 0, // Will be fetched later with live data
                },
                token_b: PoolToken {
                    mint: quote_mint,
                    symbol: quote_symbol,
                    decimals: pool.quote_decimals.unwrap_or(6),
                    reserve: 0, // Will be fetched later with live data
                },
                token_a_vault: base_vault,
                token_b_vault: quote_vault,
                fee_numerator: Some(25),    // Default Raydium AMM fee (0.25%)
                fee_denominator: Some(10000),
                last_update_timestamp: 0, // Will be updated with live data
                sqrt_price: None,         // Will be fetched later
                liquidity: None,          // Will be fetched later
                tick_current_index: None,
                tick_spacing: None,
                fee_rate_bips: Some(25),  // 0.25% in basis points
            })
        }).collect();

        info!("Successfully parsed {} Raydium pools from official API", pools.len());
        
        if pools.is_empty() {
            warn!("No valid Raydium pools found in API response");
        }

        Ok(pools)
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        info!("Fetching pool data for Raydium pool: {}", pool_address);
        
        // For foundational implementation, return demo data
        // TODO: Implement actual RPC call to fetch pool account data and parse it
        // This would use the RaydiumPoolParser to parse the on-chain data
        
        Ok(create_demo_raydium_pool(pool_address))
    }

    fn dex_name(&self) -> &str {
        "Raydium"
    }
}



/// Creates a demo PoolInfo struct for testing the discovery system.
/// This provides realistic test data that matches the expected PoolInfo structure.
fn create_demo_raydium_pool(pool_address: Pubkey) -> PoolInfo {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    // Create different demo pools based on the address
    let pool_name = format!("Raydium Pool {}", &pool_address.to_string()[..8]);
    
    // Use deterministic but realistic values based on the pool address
    let address_bytes = pool_address.to_bytes();
    let base_reserve = u64::from_le_bytes([
        address_bytes[0], address_bytes[1], address_bytes[2], address_bytes[3],
        address_bytes[4], address_bytes[5], address_bytes[6], address_bytes[7],
    ]) % 10_000_000_000; // Max 10B tokens
    
    let quote_reserve = u64::from_le_bytes([
        address_bytes[8], address_bytes[9], address_bytes[10], address_bytes[11],
        address_bytes[12], address_bytes[13], address_bytes[14], address_bytes[15],
    ]) % 1_000_000_000; // Max 1B tokens
    
    PoolInfo {
        address: pool_address,
        name: pool_name,
        token_a: PoolToken {
            mint: Pubkey::new_unique(), // Would be the actual token mint in production
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: base_reserve.max(100_000), // Ensure minimum liquidity
        },
        token_b: PoolToken {
            mint: Pubkey::new_unique(), // Would be the actual token mint in production  
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: quote_reserve.max(100_000), // Ensure minimum liquidity
        },
        token_a_vault: Pubkey::new_unique(), // Would be actual vault addresses
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(25), // 0.25% fee
        fee_denominator: Some(10000),
        fee_rate_bips: None, // Raydium uses numerator/denominator
        last_update_timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        dex_type: DexType::Raydium,
        // AMM pools don't use these CLMM fields
        liquidity: None,
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
    }
}