// src/dex/meteora.rs
//! Meteora DEX integration supporting both Dynamic AMM and DLMM pool types.

use crate::dex::quote::{DexClient, Quote, SwapInfo, PoolDiscoverable};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{info, warn};
use solana_sdk::{
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, Mint};
use std::sync::Arc;
// Import our local math functions for Meteora calculations
use crate::dex::math::meteora::calculate_dlmm_output;

// --- Constants (Made Public for tests) ---
pub const METEORA_DYNAMIC_AMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB");
pub const METEORA_DLMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
pub const DYNAMIC_AMM_POOL_STATE_SIZE: usize = 520;
pub const DLMM_LB_PAIR_STATE_SIZE: usize = 304;

// --- On-Chain Data Structures (Made Public for tests) ---
#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct DynamicAmmPoolState {
    pub enabled: u8,
    pub bump: u8,
    pub pool_type: u8,
    pub padding1: [u8; 5],
    pub lp_mint: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub a_vault: Pubkey,
    pub b_vault: Pubkey,
    pub lp_vault: Pubkey,
    pub a_vault_lp: Pubkey,
    pub b_vault_lp: Pubkey,
    pub a_vault_lp_mint: Pubkey,
    pub b_vault_lp_mint: Pubkey,
    pub pool_authority: Pubkey,
    pub token_a_fees: Pubkey,
    pub token_b_fees: Pubkey,
    pub oracle: Pubkey,
    pub fee_rate: u64,
    pub protocol_fee_rate: u64,
    pub lp_fee_rate: u64,
    pub curve_type: u8,
    pub padding2: [u8; 7],
    pub padding3: [u8; 32],
}

#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct DlmmLbPairState {
    pub active_id: u32,
    pub bin_step: u16,
    pub status: u8,
    pub padding1: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee_x: Pubkey,
    pub protocol_fee_y: Pubkey,
    pub fee_bps: u16,
    pub protocol_share: u16,
    pub pair_type: u8,
    pub padding2: [u8; 3],
    pub oracle: Pubkey,
    pub padding3: [u8; 64],
}

// Enum for pool type identification (Made Public for tests)
#[derive(Debug, Clone, PartialEq)]
pub enum MeteoraPoolType {
    DynamicAmm,
    Dlmm,
}

// --- On-Chain Data Parser ---
pub struct MeteoraPoolParser;

impl MeteoraPoolParser {
    // Made public for test access
    pub fn identify_pool_type(&self, program_id: &Pubkey) -> Option<MeteoraPoolType> {
        if *program_id == METEORA_DYNAMIC_AMM_PROGRAM_ID { Some(MeteoraPoolType::DynamicAmm) }
        else if *program_id == METEORA_DLMM_PROGRAM_ID { Some(MeteoraPoolType::Dlmm) }
        else { None }
    }
}

#[async_trait]
impl UtilsPoolParser for MeteoraPoolParser {
    // ... parse_pool_data remains the same ...
    async fn parse_pool_data(&self, address: Pubkey, data: &[u8], rpc_client: &Arc<SolanaRpcClient>) -> AnyhowResult<PoolInfo> {
        // This parser needs the program_id to dispatch correctly.
        let owner = rpc_client.primary_client.get_account(&address).await?.owner;

        match self.identify_pool_type(&owner) {
            Some(MeteoraPoolType::DynamicAmm) => {
                let state: &DynamicAmmPoolState = bytemuck::try_from_bytes(&data[..DYNAMIC_AMM_POOL_STATE_SIZE])
                    .map_err(|e| anyhow!("Failed to parse Dynamic AMM pool state: {}", e))?;
                let (a_vault_res, b_vault_res, a_mint_res, b_mint_res) = tokio::join!(
                    rpc_client.primary_client.get_account_data(&state.a_vault),
                    rpc_client.primary_client.get_account_data(&state.b_vault),
                    rpc_client.primary_client.get_account_data(&state.token_a_mint),
                    rpc_client.primary_client.get_account_data(&state.token_b_mint)
                );
                let reserve_a = TokenAccount::unpack(&a_vault_res?)?.amount;
                let reserve_b = TokenAccount::unpack(&b_vault_res?)?.amount;
                let decimals_a = Mint::unpack(&a_mint_res?)?.decimals;
                let decimals_b = Mint::unpack(&b_mint_res?)?.decimals;
                Ok(PoolInfo {
                    address, name: format!("Meteora-AMM/{}", address), dex_type: DexType::Meteora,
                    token_a: PoolToken { mint: state.token_a_mint, symbol: "TKA".into(), decimals: decimals_a, reserve: reserve_a },
                    token_b: PoolToken { mint: state.token_b_mint, symbol: "TKB".into(), decimals: decimals_b, reserve: reserve_b },
                    token_a_vault: state.a_vault, token_b_vault: state.b_vault, fee_numerator: Some(state.fee_rate), fee_denominator: Some(10000), fee_rate_bips: Some((state.fee_rate / 100) as u16),
                    ..Default::default()
                })
            }
            Some(MeteoraPoolType::Dlmm) => {
                let state: &DlmmLbPairState = bytemuck::try_from_bytes(&data[..DLMM_LB_PAIR_STATE_SIZE])
                    .map_err(|e| anyhow!("Failed to parse DLMM LB pair state: {}", e))?;
                let (x_vault_res, y_vault_res, x_mint_res, y_mint_res) = tokio::join!(
                    rpc_client.primary_client.get_account_data(&state.reserve_x),
                    rpc_client.primary_client.get_account_data(&state.reserve_y),
                    rpc_client.primary_client.get_account_data(&state.token_x_mint),
                    rpc_client.primary_client.get_account_data(&state.token_y_mint)
                );
                let reserve_x = TokenAccount::unpack(&x_vault_res?)?.amount;
                let reserve_y = TokenAccount::unpack(&y_vault_res?)?.amount;
                let decimals_x = Mint::unpack(&x_mint_res?)?.decimals;
                let decimals_y = Mint::unpack(&y_mint_res?)?.decimals;
                Ok(PoolInfo {
                    address, name: format!("Meteora-DLMM/{}", address), dex_type: DexType::Meteora,
                    token_a: PoolToken { mint: state.token_x_mint, symbol: "TKX".into(), decimals: decimals_x, reserve: reserve_x },
                    token_b: PoolToken { mint: state.token_y_mint, symbol: "TKY".into(), decimals: decimals_y, reserve: reserve_y },
                    token_a_vault: state.reserve_x, token_b_vault: state.reserve_y, fee_rate_bips: Some(state.fee_bps),
                    tick_current_index: Some(state.active_id as i32), tick_spacing: Some(state.bin_step),
                    ..Default::default()
                })
            }
            None => Err(anyhow!("Unknown Meteora program ID: {}", owner)),
        }
    }

    fn get_program_id(&self) -> Pubkey { METEORA_DYNAMIC_AMM_PROGRAM_ID }
}

// --- DEX Client Implementation ---
#[derive(Debug, Clone, Default)]
pub struct MeteoraClient;

impl MeteoraClient { pub fn new() -> Self { Self::default() } }

#[async_trait]
impl DexClient for MeteoraClient {
    fn get_name(&self) -> &str { "Meteora" }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        info!("Calculating Meteora quote for pool {}", pool.address);

        // --- <<< NEW: Dispatch calculation based on pool type ---
        if pool.tick_current_index.is_some() {
            // This is a DLMM pool, use CLMM math
            let liquidity = pool.liquidity.ok_or_else(|| anyhow!("Liquidity not available for Meteora DLMM pool {}", pool.address))?;
            let _sqrt_price = pool.sqrt_price.ok_or_else(|| anyhow!("Sqrt_price not available for Meteora DLMM pool {}", pool.address))?;
            
            let output_amount = calculate_dlmm_output(
                input_amount,
                0, // active_bin_id - placeholder
                10, // bin_step - placeholder
                liquidity,
                30, // fee_rate - placeholder (0.3%)
            )?;
            
            warn!("Meteora DLMM quote is an estimate. For perfect quotes, bin_array fetching must be implemented.");
            
            Ok(Quote {
                output_amount,
                // ... other fields
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                dex: self.get_name().to_string(),
                route: vec![pool.address],
                slippage_estimate: None,
            })
        } else {
            // This is a Dynamic AMM pool, use constant product formula
            if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
                return Err(anyhow!("Meteora AMM pool {} has zero reserves.", pool.address));
            }
            let fee_rate = pool.fee_rate_bips.unwrap_or(0) as f64 / 10000.0;
            let input_after_fees = (input_amount as f64 * (1.0 - fee_rate)) as u64;
            
            let output_amount = (input_after_fees as u128 * pool.token_b.reserve as u128)
                .saturating_div(pool.token_a.reserve as u128 + input_after_fees as u128);

            Ok(Quote {
                output_amount: output_amount as u64,
                // ... other fields
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                dex: self.get_name().to_string(),
                route: vec![pool.address],
                slippage_estimate: Some(fee_rate),
            })
        }
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        if swap_info.pool.tick_current_index.is_some() {
            Err(anyhow!("get_swap_instruction not yet implemented for Meteora DLMM pools."))
        } else {
            Err(anyhow!("get_swap_instruction not yet implemented for Meteora Dynamic AMM pools."))
        }
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        warn!("Meteora pool discovery is using a placeholder. Real implementation needed.");
        // TODO: Implement real pool discovery for Meteora.
        // This requires two `getProgramAccounts` calls, one for each program ID.
        Ok(vec![])
    }
}

#[async_trait]
impl PoolDiscoverable for MeteoraClient {
    // ... trait implementation remains the same ...
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> { <Self as DexClient>::discover_pools(self).await }
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> { Err(anyhow!("fetch_pool_data not yet implemented for MeteoraClient. Pool address: {}", pool_address)) }
    fn dex_name(&self) -> &str { self.get_name() }
}