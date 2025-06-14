// src/dex/lifinity.rs
//! Lifinity client and parser for on-chain data and instruction building.

use crate::dex::api::{DexClient, Quote, SwapInfo, PoolDiscoverable, CommonSwapInfo};
use crate::error::ArbError;
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
            // Orca-specific fields (not applicable to Lifinity)
            tick_array_0: None, tick_array_1: None, tick_array_2: None, oracle: None,
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
        warn!("LifinityClient: calculate_onchain_quote is a placeholder.");
        Err(anyhow!("calculate_onchain_quote not implemented for Lifinity"))
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
        warn!("LifinityClient: discover_pools is a placeholder.");
        // Implement discovery logic, e.g., from a Lifinity API or on-chain registry
        Ok(Vec::new())
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        info!(
            "LifinityClient: Building swap instruction for pool {} ({} -> {})",
            pool_info.address, swap_info.source_token_mint, swap_info.destination_token_mint
        );
        warn!("LifinityClient: get_swap_instruction_enhanced is a placeholder. DEX-specific logic required.");

        // PoolInfo for Lifinity should contain:
        // - Specific vault accounts, config accounts, oracle accounts if used.
        //   Example fields: pool_config, token_a_vault, token_b_vault, oracle_account

        // Example:
        // let token_a_vault = pool_info.token_a_vault.ok_or_else(|| ArbError::InstructionError("Missing Lifinity token_a_vault".to_string()))?;
        // let token_b_vault = pool_info.token_b_vault.ok_or_else(|| ArbError::InstructionError("Missing Lifinity token_b_vault".to_string()))?;
        // let oracle_account = pool_info.lifinity_oracle.ok_or_else(|| ArbError::InstructionError("Missing Lifinity oracle_account".to_string()))?;

        let accounts = vec![
            // AccountMetas depend heavily on the Lifinity program's swap instruction
            solana_sdk::instruction::AccountMeta::new_readonly(spl_token::id(), false),
            solana_sdk::instruction::AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            solana_sdk::instruction::AccountMeta::new(pool_info.address, false), // Pool address
            solana_sdk::instruction::AccountMeta::new(swap_info.user_source_token_account, false),
            solana_sdk::instruction::AccountMeta::new(swap_info.user_destination_token_account, false),
            // ... other Lifinity-specific accounts from pool_info ...
            // AccountMeta::new(token_a_vault, false),
            // AccountMeta::new(token_b_vault, false),
            // AccountMeta::new_readonly(oracle_account, false), // If oracle is used
        ];

        // Instruction data will be specific to Lifinity's swap instruction.
        let mut instruction_data = Vec::new();
        // instruction_data.push(LIFINITY_SWAP_DISCRIMINATOR); // Replace with actual
        // instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes());
        // instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes());
        // ... other Lifinity-specific instruction params ...

        if instruction_data.is_empty() {
            return Err(crate::error::ArbError::InstructionError(
                "Lifinity swap instruction data not implemented".to_string(),
            ));
        }

        Ok(Instruction {
            program_id: LIFINITY_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }
}

#[async_trait]
impl PoolDiscoverable for LifinityClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    async fn fetch_pool_data(&self, _pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        Err(anyhow!("fetch_pool_data not implemented for LifinityClient"))
    }
    
    fn dex_name(&self) -> &str { 
        self.get_name() 
    }
}