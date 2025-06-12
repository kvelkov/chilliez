// src/dex/lifinity.rs
//! Lifinity client and parser for on-chain data and instruction building.

use crate::dex::quote::{DexClient, Quote, SwapInfo};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use bytemuck::{Pod, Zeroable};
use log::info;
use solana_sdk::{
    instruction::Instruction,
    program_pack::Pack, // The trait that provides the `unpack` method.
    pubkey::Pubkey,
};
use spl_token::state::Account as TokenAccount;
use std::str::FromStr;
use std::sync::Arc;

// This struct is a starting point. It must match the on-chain data layout exactly.
#[repr(C)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LifinityPoolState {
    // Other fields from the on-chain state must be added here to ensure correct size and alignment.
    // NOTE: This is likely incomplete and needs to be verified against Lifinity's SDK or on-chain data.
    pub concentration: u64,
    pub fee_bps: u64,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
}

/// The program ID for the Lifinity DEX.
pub const LIFINITY_PROGRAM_ID: &str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn";

// --- On-Chain Data Parser ---

pub struct LifinityPoolParser;

#[async_trait::async_trait]
impl UtilsPoolParser for LifinityPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        let state: &LifinityPoolState = bytemuck::try_from_bytes(data)
            .map_err(|e| anyhow!("Failed to parse Lifinity pool state for {}: {}", address, e))?;

        info!("Parsing Lifinity pool data for address: {}", address);

        let (token_a_vault_data, token_b_vault_data, decimals_a, decimals_b) = tokio::try_join!(
            async { rpc_client.primary_client.get_account_data(&state.token_a_vault).await.map_err(anyhow::Error::from) },
            async { rpc_client.primary_client.get_account_data(&state.token_b_vault).await.map_err(anyhow::Error::from) },
            async { rpc_client.get_token_mint_decimals(&state.token_a_mint).await },
            async { rpc_client.get_token_mint_decimals(&state.token_b_mint).await }
        )?;

        let reserve_a = TokenAccount::unpack(&token_a_vault_data)?.amount;
        let reserve_b = TokenAccount::unpack(&token_b_vault_data)?.amount;

        Ok(PoolInfo {
            address,
            name: format!("Lifinity/{}", address),
            token_a: PoolToken {
                mint: state.token_a_mint,
                symbol: "TKA".to_string(), // TODO: Replace with actual symbol lookup
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: state.token_b_mint,
                symbol: "TKB".to_string(), // TODO: Replace with actual symbol lookup
                decimals: decimals_b,
                reserve: reserve_b,
            },
            token_a_vault: state.token_a_vault,
            token_b_vault: state.token_b_vault,
            fee_numerator: Some(state.fee_bps),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: 0,
            dex_type: DexType::Lifinity,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
        })
    }

    fn get_program_id(&self) -> Pubkey {
        Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap()
    }
}


// --- DEX Client Implementation ---

#[derive(Debug, Clone)]
pub struct LifinityClient;

impl LifinityClient {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LifinityClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DexClient for LifinityClient {
    fn get_name(&self) -> &str {
        "Lifinity"
    }

    fn calculate_onchain_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> AnyhowResult<Quote> {
        // TODO: Implement the precise quote calculation for Lifinity's concentrated liquidity model.
        if pool.token_a.reserve == 0 {
            return Err(anyhow!("Pool has zero reserves for token A"));
        }
        let output_amount = (input_amount as u128 * pool.token_b.reserve as u128 / pool.token_a.reserve as u128) as u64;

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount,
            dex: self.get_name().to_string(),
            route: vec![pool.address],
            slippage_estimate: None,
        })
    }

    fn get_swap_instruction(
        &self,
        _swap_info: &SwapInfo,
    ) -> AnyhowResult<Instruction> {
        // TODO: Implement the actual instruction building logic for Lifinity.
        let data = vec![];
        let accounts = vec![];

        if data.is_empty() || accounts.is_empty() {
             return Err(anyhow!("Lifinity swap instruction is not yet implemented."));
        }

        Ok(Instruction {
            // FIX: Use the constant directly, as get_program_id belongs to the parser, not the client.
            program_id: Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap(),
            accounts,
            data,
        })
    }
}