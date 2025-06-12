// src/dex/raydium.rs
//! Raydium client and parser for on-chain data and instruction building.

use crate::dex::quote::{DexClient, Quote, SwapInfo};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use bytemuck::{Pod, Zeroable};
use log::info;
use solana_sdk::{
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Account as TokenAccount;
use std::str::FromStr;
use std::sync::Arc;

// Official Program IDs from Raydium's documentation.
pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

// --- On-Chain Data Structures ---

// Layout for a Raydium Liquidity Pool V4 state account.
#[repr(C)]
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
    // FIX: Removed stray 's' character from the beginning of the next line.
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
    pub punishment_numerator: u64,
    pub punishment_denominator: u64,
    pub amod: u64,
    pub padding: [u64; 1],
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub open_orders: Pubkey,
    pub target_orders: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub withdraw_queue: Pubkey,
    pub lp_vault: Pubkey,
    pub market_id: Pubkey,
    pub market_program_id: Pubkey,
    pub market_authority: Pubkey,
}

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
        let pool_state: &LiquidityStateV4 = bytemuck::try_from_bytes(data)
            .map_err(|e| anyhow!("Failed to parse Raydium pool state for {}: {}", address, e))?;

        info!("Parsing Raydium AMM V4 pool data for address: {}", address);

        let (base_vault_data, quote_vault_data) = tokio::try_join!(
            async { rpc_client.primary_client.get_account_data(&pool_state.base_vault).await.map_err(anyhow::Error::from) },
            async { rpc_client.primary_client.get_account_data(&pool_state.quote_vault).await.map_err(anyhow::Error::from) }
        )?;

        let base_reserve = TokenAccount::unpack_from_slice(&base_vault_data)?.amount;
        let quote_reserve = TokenAccount::unpack_from_slice(&quote_vault_data)?.amount;

        Ok(PoolInfo {
            address,
            name: format!("Raydium AMM V4/{}", address),
            token_a: PoolToken {
                mint: pool_state.base_mint,
                symbol: "BASE".to_string(), // Placeholder
                decimals: pool_state.base_decimal as u8,
                reserve: base_reserve,
            },
            token_b: PoolToken {
                mint: pool_state.quote_mint,
                symbol: "QUOTE".to_string(), // Placeholder
                decimals: pool_state.quote_decimal as u8,
                reserve: quote_reserve,
            },
            fee_numerator: pool_state.swap_fee_numerator,
            fee_denominator: pool_state.swap_fee_denominator,
            last_update_timestamp: pool_state.pool_open_time,
            dex_type: DexType::Raydium,
        })
    }

    fn get_program_id(&self) -> Pubkey {
        Pubkey::from_str(RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID).unwrap()
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
            return Err(anyhow!("Pool has zero reserves."));
        }
        if pool.fee_denominator == 0 {
            return Err(anyhow!("Fee denominator cannot be zero."));
        }

        let fee = (input_amount as u128 * pool.fee_numerator as u128 / pool.fee_denominator as u128) as u64;
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
        _swap_info: &SwapInfo,
    ) -> AnyhowResult<Instruction> {
        // TODO: Implement the instruction building logic for a Raydium AMM V4 swap.
        let data = vec![];
        let accounts = vec![];

        if data.is_empty() || accounts.is_empty() {
            return Err(anyhow!("Raydium AMM V4 swap instruction is not yet implemented."));
        }

        Ok(Instruction {
            program_id: Pubkey::from_str(RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID).unwrap(),
            accounts,
            data,
        })
    }
}