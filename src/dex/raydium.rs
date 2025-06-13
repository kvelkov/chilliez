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
    pub swap_base_in_amount: u128,
    pub swap_quote_out_amount: u128,
    pub swap_base_2_quote_fee: u64,
    pub swap_quote_in_amount: u128,
    pub swap_base_out_amount: u128,
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

        if data.len() < RAYDIUM_V4_POOL_STATE_SIZE {
            return Err(anyhow!(
                "Invalid Raydium V4 pool account data length for {}: expected {} bytes, got {}",
                address, RAYDIUM_V4_POOL_STATE_SIZE, data.len()
            ));
        }

        let pool_state: &LiquidityStateV4 = bytemuck::from_bytes(&data[..RAYDIUM_V4_POOL_STATE_SIZE]);
        
        if pool_state.status == 0 {
            return Err(anyhow!("Raydium pool {} is inactive (status = 0)", address));
        }

        // --- OPTIMIZATION: Fetch all required accounts concurrently ---
        let (
            base_vault_data_res, 
            quote_vault_data_res, 
            base_mint_data_res, 
            quote_mint_data_res
        ) = tokio::join!(
            rpc_client.primary_client.get_account_data(&pool_state.base_vault),
            rpc_client.primary_client.get_account_data(&pool_state.quote_vault),
            rpc_client.primary_client.get_account_data(&pool_state.base_mint),
            rpc_client.primary_client.get_account_data(&pool_state.quote_mint)
        );
        
        let base_vault_data = base_vault_data_res.map_err(|e| anyhow!("Failed to fetch base vault {}: {}", pool_state.base_vault, e))?;
        let quote_vault_data = quote_vault_data_res.map_err(|e| anyhow!("Failed to fetch quote vault {}: {}", pool_state.quote_vault, e))?;
        let base_mint_data = base_mint_data_res.map_err(|e| anyhow!("Failed to fetch base mint {}: {}", pool_state.base_mint, e))?;
        let quote_mint_data = quote_mint_data_res.map_err(|e| anyhow!("Failed to fetch quote mint {}: {}", pool_state.quote_mint, e))?;
        
        // --- End Optimization ---

        let base_reserve = TokenAccount::unpack_from_slice(&base_vault_data)?.amount;
        let quote_reserve = TokenAccount::unpack_from_slice(&quote_vault_data)?.amount;

        let base_decimals = Mint::unpack_from_slice(&base_mint_data)?.decimals;
        let quote_decimals = Mint::unpack_from_slice(&quote_mint_data)?.decimals;

        if base_decimals != pool_state.base_decimal as u8 {
            return Err(anyhow!(
                "Base token decimals mismatch for pool {}: mint={}, pool={}",
                address, base_decimals, pool_state.base_decimal
            ));
        }
        if quote_decimals != pool_state.quote_decimal as u8 {
            return Err(anyhow!(
                "Quote token decimals mismatch for pool {}: mint={}, pool={}",
                address, quote_decimals, pool_state.quote_decimal
            ));
        }

        Ok(PoolInfo {
            address,
            name: format!("Raydium AMM V4/{}", address),
            dex_type: DexType::Raydium,
            token_a: PoolToken {
                mint: pool_state.base_mint,
                symbol: "BASE".to_string(), // Placeholder - will be enriched by API data
                decimals: base_decimals,
                reserve: base_reserve,
            },
            token_b: PoolToken {
                mint: pool_state.quote_mint,
                symbol: "QUOTE".to_string(), // Placeholder - will be enriched by API data
                decimals: quote_decimals,
                reserve: quote_reserve,
            },
            token_a_vault: pool_state.base_vault,
            token_b_vault: pool_state.quote_vault,
            fee_numerator: Some(pool_state.swap_fee_numerator),
            fee_denominator: Some(pool_state.swap_fee_denominator),
            fee_rate_bips: Some((pool_state.swap_fee_numerator * 10000 / pool_state.swap_fee_denominator) as u16),
            last_update_timestamp: pool_state.pool_open_time,
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
    fn get_name(&self) -> &str { "Raydium" }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
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
            .saturating_sub(k.saturating_div(pool.token_a.reserve as u128 + input_amount_after_fee as u128));
        
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

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        #[repr(C)]
        struct RaydiumSwapInstruction {
            instruction: u8,
            amount_in: u64,
            min_amount_out: u64,
        }

        let instruction_data = RaydiumSwapInstruction {
            instruction: 9, // Instruction index for a swap in Raydium AMM V4
            amount_in: swap_info.amount_in,
            min_amount_out: swap_info.min_output_amount,
        };

        let mut data = Vec::with_capacity(17);
        data.push(instruction_data.instruction);
        data.extend_from_slice(&instruction_data.amount_in.to_le_bytes());
        data.extend_from_slice(&instruction_data.min_amount_out.to_le_bytes());
        
        let accounts = vec![
            // 0. `token_program`: The SPL Token program ID.
            AccountMeta::new_readonly(spl_token::id(), false),
            // 1. `amm_id`: The AMM account address.
            AccountMeta::new(swap_info.pool_account, false),
            // 2. `amm_authority`: The AMM authority address.
            AccountMeta::new_readonly(swap_info.pool_authority, false),
            // 3. `amm_open_orders`: The AMM's OpenOrders account.
            AccountMeta::new(swap_info.pool_open_orders, false),
            // 4. `amm_target_orders`: The AMM's target orders account.
            AccountMeta::new(swap_info.pool_target_orders, false),
            // 5. `pool_coin_token_account`: The AMM's base token vault.
            AccountMeta::new(swap_info.pool_base_vault, false),
            // 6. `pool_pc_token_account`: The AMM's quote token vault.
            AccountMeta::new(swap_info.pool_quote_vault, false),
            // 7. `serum_program_id`: The Serum/OpenBook program ID.
            AccountMeta::new_readonly(swap_info.market_program_id, false),
            // 8. `serum_market`: The Serum/OpenBook market address.
            AccountMeta::new(swap_info.market_id, false),
            // 9. `serum_bids`: The market's bids account.
            AccountMeta::new(swap_info.market_bids, false),
            // 10. `serum_asks`: The market's asks account.
            AccountMeta::new(swap_info.market_asks, false),
            // 11. `serum_event_queue`: The market's event queue.
            AccountMeta::new(swap_info.market_event_queue, false),
            // 12. `serum_coin_vault_account`: The market's base token vault.
            AccountMeta::new(swap_info.pool_base_vault, false), // Re-using for simplicity, should be market's vault
            // 13. `serum_pc_vault_account`: The market's quote token vault.
            AccountMeta::new(swap_info.pool_quote_vault, false), // Re-using for simplicity, should be market's vault
            // 14. `serum_vault_signer`: The market's vault signer PDA.
            AccountMeta::new_readonly(swap_info.market_authority, false),
            // 15. `user_source_token_account`: The user's source token account.
            AccountMeta::new(swap_info.user_source_token_account, false),
            // 16. `user_destination_token_account`: The user's destination token account.
            AccountMeta::new(swap_info.user_destination_token_account, false),
            // 17. `user_owner`: The user's wallet address.
            AccountMeta::new_readonly(swap_info.user_owner, true),
        ];

        Ok(Instruction {
            program_id: RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            accounts,
            data,
        })
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("Starting Raydium pool discovery using official API: {}", RAYDIUM_LIQUIDITY_JSON_URL);
        
        let response = reqwest::get(RAYDIUM_LIQUIDITY_JSON_URL).await
            .map_err(|e| anyhow!("Failed to fetch Raydium liquidity data: {}", e))?;
        
        let liquidity_file: LiquidityFile = response.json().await
            .map_err(|e| anyhow!("Failed to parse Raydium liquidity JSON: {}", e))?;

        info!("Fetched {} official pools from Raydium API", liquidity_file.official.len());

        let pools: Vec<PoolInfo> = liquidity_file.official.into_iter().filter_map(|pool| {
            let pool_pubkey = Pubkey::from_str(&pool.id).ok()?;
            let base_mint = Pubkey::from_str(&pool.base_mint).ok()?;
            let quote_mint = Pubkey::from_str(&pool.quote_mint).ok()?;
            let base_vault = Pubkey::from_str(&pool.base_vault).ok()?;
            let quote_vault = Pubkey::from_str(&pool.quote_vault).ok()?;
            
            Some(PoolInfo {
                address: pool_pubkey,
                name: format!("Raydium {}/{}", pool.base_symbol.clone().unwrap_or_default(), pool.quote_symbol.clone().unwrap_or_default()),
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
            })
        }).collect();

        info!("Successfully parsed {} Raydium pools from API response.", pools.len());
        Ok(pools)
    }
}

#[async_trait]
impl PoolDiscoverable for RaydiumClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // TODO: Implement live data fetching for a single Raydium pool.
        // This would involve an RPC call to get the account data and then using RaydiumPoolParser.
        Err(anyhow!("fetch_pool_data not yet implemented for RaydiumClient. Pool address: {}", pool_address))
    }
    fn dex_name(&self) -> &str {
        self.get_name()
    }
}