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
use spl_token::state::Account as TokenAccount;
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

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Basic quote calculation for Lifinity
        // This is a simplified version using constant product formula
        // Real Lifinity implementation would use their proactive market making model
        
        warn!("Lifinity quote calculation is using a simplified constant product formula. Real implementation requires their PMM model.");
        
        if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
            return Err(anyhow!("Lifinity pool {} has zero reserves.", pool.address));
        }
        
        let fee_rate = 0.003; // 0.3% default fee
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

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // Basic swap instruction implementation for Lifinity
        warn!("get_swap_instruction for Lifinity is a basic implementation. Production use requires proper instruction building.");
        
        Ok(Instruction {
            program_id: LIFINITY_PROGRAM_ID,
            accounts: vec![
                solana_program::instruction::AccountMeta::new(swap_info.user_wallet, true),
                solana_program::instruction::AccountMeta::new(swap_info.pool_account, false),
                solana_program::instruction::AccountMeta::new(swap_info.user_source_token_account, false),
                solana_program::instruction::AccountMeta::new(swap_info.user_destination_token_account, false),
            ],
            data: vec![0], // Placeholder instruction data
        })
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        warn!("Lifinity pool discovery is using a placeholder implementation.");
        
        // Return a mock pool for testing purposes
        let mock_pools = vec![
            PoolInfo {
                address: Pubkey::new_unique(),
                name: "Lifinity SOL/USDC".to_string(),
                dex_type: DexType::Lifinity,
                token_a: PoolToken {
                    mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
                    symbol: "SOL".to_string(),
                    decimals: 9,
                    reserve: 2_000_000_000_000, // 2000 SOL
                },
                token_b: PoolToken {
                    mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
                    symbol: "USDC".to_string(),
                    decimals: 6,
                    reserve: 150_000_000_000, // 150k USDC
                },
                token_a_vault: Pubkey::new_unique(),
                token_b_vault: Pubkey::new_unique(),
                fee_numerator: Some(30),
                fee_denominator: Some(10000),
                fee_rate_bips: Some(30),
                last_update_timestamp: 0,
                liquidity: Some(15_000_000_000),
                sqrt_price: None,
                tick_current_index: None,
                tick_spacing: None,
            }
        ];
        
        Ok(mock_pools)
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