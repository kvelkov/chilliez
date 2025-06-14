// src/dex/meteora.rs
//! Meteora DEX integration supporting both Dynamic AMM and DLMM pool types.

use crate::dex::api::{DexClient, Quote, SwapInfo, PoolDiscoverable, CommonSwapInfo};
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

    fn calculate_onchain_quote(&self, _pool: &PoolInfo, _input_amount: u64) -> AnyhowResult<Quote> {
        warn!("MeteoraClient: calculate_onchain_quote is a placeholder.");
        Err(anyhow!("calculate_onchain_quote not implemented for Meteora"))
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        warn!("get_swap_instruction for Meteora is a basic implementation. Production use requires proper pool type identification and account resolution.");
        
        // This is the legacy method that uses the complex SwapInfo structure
        // For now, return a placeholder instruction
        Ok(Instruction {
            program_id: METEORA_DYNAMIC_AMM_PROGRAM_ID,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(swap_info.user_wallet, true),
                solana_sdk::instruction::AccountMeta::new(swap_info.pool_account, false),
                solana_sdk::instruction::AccountMeta::new(swap_info.user_source_token_account, false),
                solana_sdk::instruction::AccountMeta::new(swap_info.user_destination_token_account, false),
            ],
            data: vec![0], // Placeholder instruction data
        })
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        info!(
            "MeteoraClient: Building swap instruction for pool {} ({} -> {})",
            pool_info.address, swap_info.source_token_mint, swap_info.destination_token_mint
        );
        warn!("MeteoraClient: get_swap_instruction_enhanced is a placeholder. DEX-specific logic required.");

        // PoolInfo for Meteora should contain:
        // - Pool type identifier (e.g., Dynamic AMM, DLMM)
        // - Relevant accounts for that pool type (e.g., vaults, config accounts, bin arrays for DLMM)
        //   Example fields: pool_config_account, token_a_vault, token_b_vault, admin_authority etc.

        // Example:
        // let pool_config_account = pool_info.meteora_config_account.ok_or_else(|| ArbError::InstructionError("Missing Meteora pool_config_account".to_string()))?;
        // let token_a_vault = pool_info.token_a_vault.ok_or_else(|| ArbError::InstructionError("Missing Meteora token_a_vault".to_string()))?;
        // let token_b_vault = pool_info.token_b_vault.ok_or_else(|| ArbError::InstructionError("Missing Meteora token_b_vault".to_string()))?;

        let accounts = vec![
            // AccountMetas depend heavily on the specific Meteora pool type and instruction
            solana_sdk::instruction::AccountMeta::new_readonly(spl_token::id(), false),
            solana_sdk::instruction::AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            solana_sdk::instruction::AccountMeta::new(pool_info.address, false), // Pool address
            solana_sdk::instruction::AccountMeta::new(swap_info.user_source_token_account, false),
            solana_sdk::instruction::AccountMeta::new(swap_info.user_destination_token_account, false),
            // ... other Meteora-specific accounts from pool_info ...
            // AccountMeta::new(token_a_vault, false),
            // AccountMeta::new(token_b_vault, false),
            // AccountMeta::new_readonly(pool_config_account, false),
        ];

        // Instruction data will be specific to Meteora's swap instruction for the given pool type.
        // This will likely involve an instruction discriminator and serialized arguments.
        let mut instruction_data = Vec::new();
        // instruction_data.push(METEORA_SWAP_DISCRIMINATOR); // Replace with actual
        // instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes());
        // instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes());
        // ... other Meteora-specific instruction params ...

        if instruction_data.is_empty() {
            return Err(crate::error::ArbError::InstructionError(
                "Meteora swap instruction data not implemented".to_string(),
            ));
        }

        Ok(Instruction {
            program_id: METEORA_DYNAMIC_AMM_PROGRAM_ID, // This might vary based on pool type
            accounts,
            data: instruction_data,
        })
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        warn!("MeteoraClient: discover_pools is a placeholder.");
        // Implement discovery logic, e.g., from a Meteora API or on-chain registry
        Ok(Vec::new())
    }
}

#[async_trait]
impl PoolDiscoverable for MeteoraClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        <Self as DexClient>::discover_pools(self).await
    }
    
    async fn fetch_pool_data(&self, _pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        Err(anyhow!("fetch_pool_data not implemented for MeteoraClient"))
    }
    
    fn dex_name(&self) -> &str { 
        self.get_name() 
    }
}
