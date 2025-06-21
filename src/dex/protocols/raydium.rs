//! Raydium client and parser for on-chain data and instruction building.
//! This implementation follows the official Raydium V4 layout for maximum accuracy.
//! Includes Raydium API data models.

use crate::dex::api::{
    CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo,
};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser, PoolToken};
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
impl PoolParser for RaydiumPoolParser {
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

    fn parse_pool_data_sync(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
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
        let pool_info = PoolInfo {
            address: pool_address,
            name: format!("Raydium V4 Pool"),
            token_a: PoolToken {
                mint: pool_state.base_mint,
                symbol: "Unknown".to_string(),
                decimals: 0, // Unknown in sync context
                reserve: 0,  // Unknown in sync context
            },
            token_b: PoolToken {
                mint: pool_state.quote_mint,
                symbol: "Unknown".to_string(),
                decimals: 0,
                reserve: 0,
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
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
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

// --- Raydium Math Implementation ---
/// Raydium V4 AMM Math Implementation

use num_traits::{CheckedAdd, CheckedSub, FromPrimitive, ToPrimitive};
use std::convert::TryInto;

// --- Raydium V4 Specifics ---
const RAYDIUM_V4_PRICE_DECIMALS: u32 = 9;
const RAYDIUM_V4_PRICE_MULTIPLIER: u64 = 1_000_000_000;
const RAYDIUM_V4_FEE_DECIMALS: u32 = 6;
const RAYDIUM_V4_FEE_MULTIPLIER: u64 = 1_000_000;

// --- Math Errors ---
#[derive(Debug, thiserror::Error)]
pub enum MathError {
    #[error("Overflow error")]
    Overflow,
    #[error("Underflow error")]
    Underflow,
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Invalid input data")]
    InvalidInput,
}

// --- Raydium V4 AMM Implementation ---
#[derive(Default)]
pub struct RaydiumV4AMM;

// Stub out AMM trait and math logic for now
// impl AMM for RaydiumV4AMM {}
