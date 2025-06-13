// src/dex/lifinity.rs
//! Lifinity client and parser for on-chain data and instruction building.

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
pub const LIFINITY_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn");

// --- On-Chain Data Structures ---

/// Lifinity Pool State struct, designed to match the on-chain layout.
/// This structure needs to be verified against Lifinity's official documentation or SDK.
#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LifinityPoolState {
    pub discriminator: [u8; 8],
    pub authority: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub fee_bps: u16,
    pub concentration: u64,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current: i32,
    pub protocol_fee_a: u64,
    pub protocol_fee_b: u64,
    pub last_rebalance_timestamp: u64,
    pub oracle_price_a: u128,
    pub oracle_price_b: u128,
    pub status: u8,
    pub reserved: [u8; 128],
}
const LIFINITY_POOL_STATE_SIZE: usize = std::mem::size_of::<LifinityPoolState>();


// --- On-Chain Data Parser ---

pub struct LifinityPoolParser;

#[async_trait]
impl UtilsPoolParser for LifinityPoolParser {
    async fn parse_pool_data(&self, address: Pubkey, data: &[u8], rpc_client: &Arc<SolanaRpcClient>) -> AnyhowResult<PoolInfo> {
        if data.len() < LIFINITY_POOL_STATE_SIZE {
            return Err(anyhow!("Lifinity pool data too short for {}: expected {} bytes, got {}", address, LIFINITY_POOL_STATE_SIZE, data.len()));
        }

        info!("Parsing Lifinity pool data for address: {}", address);
        let state: &LifinityPoolState = bytemuck::from_bytes(&data[..LIFINITY_POOL_STATE_SIZE]);

        if state.status != 1 {
            return Err(anyhow!("Lifinity pool {} is not active (status: {})", address, state.status));
        }

        // --- OPTIMIZATION: Fetch all required accounts concurrently ---
        let (
            token_a_vault_data_res,
            token_b_vault_data_res,
            decimals_a_res,
            decimals_b_res
        ) = tokio::join!(
            rpc_client.primary_client.get_account_data(&state.token_a_vault),
            rpc_client.primary_client.get_account_data(&state.token_b_vault),
            rpc_client.get_token_mint_decimals(&state.token_a_mint),
            rpc_client.get_token_mint_decimals(&state.token_b_mint)
        );

        let token_a_vault_data = token_a_vault_data_res.map_err(|e| anyhow!("Failed to fetch token A vault {}: {}", state.token_a_vault, e))?;
        let token_b_vault_data = token_b_vault_data_res.map_err(|e| anyhow!("Failed to fetch token B vault {}: {}", state.token_b_vault, e))?;
        let decimals_a = decimals_a_res.map_err(|e| anyhow!("Failed to fetch token A decimals {}: {}", state.token_a_mint, e))?;
        let decimals_b = decimals_b_res.map_err(|e| anyhow!("Failed to fetch token B decimals {}: {}", state.token_b_mint, e))?;
        // --- End Optimization ---

        let reserve_a = TokenAccount::unpack(&token_a_vault_data)?.amount;
        let reserve_b = TokenAccount::unpack(&token_b_vault_data)?.amount;

        Ok(PoolInfo {
            address,
            name: format!("Lifinity/{}", address),
            dex_type: DexType::Lifinity,
            token_a: PoolToken {
                mint: state.token_a_mint,
                symbol: "TKA".to_string(), // Placeholder
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: state.token_b_mint,
                symbol: "TKB".to_string(), // Placeholder
                decimals: decimals_b,
                reserve: reserve_b,
            },
            token_a_vault: state.token_a_vault,
            token_b_vault: state.token_b_vault,
            fee_numerator: Some(state.fee_bps as u64),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(state.fee_bps),
            last_update_timestamp: state.last_rebalance_timestamp,
            liquidity: Some(state.liquidity),
            sqrt_price: Some(state.sqrt_price),
            tick_current_index: Some(state.tick_current),
            tick_spacing: None, // Not explicitly in this struct
        })
    }

    fn get_program_id(&self) -> Pubkey {
        LIFINITY_PROGRAM_ID
    }
}


// --- DEX Client Implementation ---

#[derive(Debug, Clone, Default)]
pub struct LifinityClient;

impl LifinityClient {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DexClient for LifinityClient {
    fn get_name(&self) -> &str { "Lifinity" }

    fn calculate_onchain_quote(&self, _pool: &PoolInfo, _input_amount: u64) -> AnyhowResult<Quote> {
        // TODO: Implement the precise quote calculation for Lifinity's model.
        // The simple AMM formula is INCORRECT.
        // This requires a deep understanding of their proactive market making and oracle-based rebalancing.
        // The `lb_clmm` crate might be a starting point, but Lifinity's model is unique.
        Err(anyhow!("calculate_onchain_quote not yet implemented for Lifinity. Requires specialized math model."))
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // TODO: Implement the Lifinity swap instruction builder.
        // This will require analyzing on-chain transactions or their official SDK to determine
        // the correct instruction data layout and required accounts.
        Err(anyhow!("get_swap_instruction not yet implemented for Lifinity."))
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        warn!("Lifinity pool discovery is using a placeholder. Real implementation needed.");
        // TODO: Implement real pool discovery for Lifinity.
        // Strategy 1: Check if Lifinity offers an official JSON API for their pools. This is ideal.
        // Strategy 2: If no API exists, a one-time `getProgramAccounts` call against the Lifinity
        // program ID, filtered by account size or discriminator, will be necessary to fetch all pool addresses.
        // For now, returning an empty list to avoid providing stale demo data.
        Ok(vec![])
    }
}

#[async_trait]
impl PoolDiscoverable for LifinityClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        Err(anyhow!("fetch_pool_data not yet implemented for LifinityClient. Pool address: {}", pool_address))
    }
    fn dex_name(&self) -> &str {
        self.get_name()
    }
}