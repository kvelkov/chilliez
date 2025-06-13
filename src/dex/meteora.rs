// src/dex/meteora.rs
//! Meteora DEX integration supporting both Dynamic AMM and DLMM pool types.
//! This module provides a unified client and specialized parsers for Meteora's distinct pool models.

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
use std::str::FromStr;
use std::sync::Arc;

// --- Constants ---
pub const METEORA_DYNAMIC_AMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB");
pub const METEORA_DLMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");


// --- On-Chain Data Structures ---

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
    // ... other fields that may not be needed for basic parsing
}
const DYNAMIC_AMM_STATE_SIZE: usize = std::mem::size_of::<DynamicAmmPoolState>();

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
    // ... other fields
}
const DLMM_LB_PAIR_STATE_SIZE: usize = std::mem::size_of::<DlmmLbPairState>();


// --- On-Chain Data Parser ---
pub struct MeteoraPoolParser;

impl MeteoraPoolParser {
    /// Helper to identify pool type based on program ID.
    fn identify_pool_type(&self, program_id: &Pubkey) -> Option<DexType> {
        if *program_id == METEORA_DYNAMIC_AMM_PROGRAM_ID {
            Some(DexType::Meteora) // Could be refined to a sub-type if needed
        } else if *program_id == METEORA_DLMM_PROGRAM_ID {
            Some(DexType::Meteora) // Could be refined to a sub-type if needed
        } else {
            None
        }
    }
}

#[async_trait]
impl UtilsPoolParser for MeteoraPoolParser {
    async fn parse_pool_data(&self, address: Pubkey, data: &[u8], rpc_client: &Arc<SolanaRpcClient>) -> AnyhowResult<PoolInfo> {
        // This parser needs the program_id to dispatch correctly.
        // The PoolDiscoveryService should fetch the account owner (program_id) and pass it here.
        // For now, we'll assume a way to get it or add it to this function's signature later.
        // Let's simulate fetching it for now.
        let owner = rpc_client.primary_client.get_account(&address).await?.owner;

        match self.identify_pool_type(&owner) {
            Some(_) if owner == METEORA_DYNAMIC_AMM_PROGRAM_ID => {
                if data.len() < DYNAMIC_AMM_STATE_SIZE {
                    return Err(anyhow!("Meteora Dynamic AMM data too short for {}", address));
                }
                let state: &DynamicAmmPoolState = bytemuck::from_bytes(&data[..DYNAMIC_AMM_STATE_SIZE]);
                
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
                    token_a_vault: state.a_vault, token_b_vault: state.b_vault,
                    ..Default::default()
                })
            }
            Some(_) if owner == METEORA_DLMM_PROGRAM_ID => {
                 if data.len() < DLMM_LB_PAIR_STATE_SIZE {
                    return Err(anyhow!("Meteora DLMM data too short for {}", address));
                }
                let state: &DlmmLbPairState = bytemuck::from_bytes(&data[..DLMM_LB_PAIR_STATE_SIZE]);

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
                    token_a_vault: state.reserve_x, token_b_vault: state.reserve_y,
                    tick_current_index: Some(state.active_id as i32), tick_spacing: Some(state.bin_step),
                    ..Default::default()
                })
            }
            _ => Err(anyhow!("Unknown Meteora pool type for address: {}", address)),
        }
    }

    fn get_program_id(&self) -> Pubkey {
        // This is tricky as Meteora has multiple. The parser itself dispatches based on the
        // actual owner of the account. Returning the AMM one as a default.
        METEORA_DYNAMIC_AMM_PROGRAM_ID
    }
}

// --- DEX Client Implementation ---
#[derive(Debug, Clone, Default)]
pub struct MeteoraClient;

impl MeteoraClient {
    pub fn new() -> Self { Self::default() }
}

#[async_trait]
impl DexClient for MeteoraClient {
    fn get_name(&self) -> &str { "Meteora" }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Dispatch quote calculation based on pool type
        if pool.tick_current_index.is_some() {
            // This is a DLMM pool
            // TODO: Implement precise quote calculation for Meteora DLMM pools.
            // This requires complex math involving the active bin, bin step, and liquidity distribution.
            Err(anyhow!("calculate_onchain_quote not yet implemented for Meteora DLMM pools."))
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
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                output_amount: output_amount as u64,
                dex: self.get_name().to_string(),
                route: vec![pool.address],
                slippage_estimate: Some(fee_rate),
            })
        }
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // TODO: Implement instruction builders for both Meteora pool types.
        // This requires studying Meteora's official CPI examples on GitHub to ensure
        // all accounts, including PDAs and remaining_accounts for DLMM bin arrays, are correct.
        if swap_info.pool.tick_current_index.is_some() {
            Err(anyhow!("get_swap_instruction not yet implemented for Meteora DLMM pools."))
        } else {
            Err(anyhow!("get_swap_instruction not yet implemented for Meteora Dynamic AMM pools."))
        }
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        warn!("Meteora pool discovery is using a placeholder. Real implementation needed.");
        // TODO: Implement real pool discovery for Meteora.
        // This is the most complex discovery. It will require two `getProgramAccounts` calls,
        // one for each program ID (Dynamic AMM and DLMM), with appropriate memory layout filters.
        Ok(vec![])
    }
}

#[async_trait]
impl PoolDiscoverable for MeteoraClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        Err(anyhow!("fetch_pool_data not yet implemented for MeteoraClient. Pool address: {}", pool_address))
    }
    fn dex_name(&self) -> &str {
        self.get_name()
    }
}