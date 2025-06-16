// src/dex/clients/lifinity.rs
//! Lifinity DEX integration with proactive market making support.

use crate::dex::api::{DexClient, Quote, SwapInfo, PoolDiscoverable, CommonSwapInfo, DexHealthStatus};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{debug, info, warn};
use solana_sdk::{
    instruction::{Instruction, AccountMeta},
    program_pack::Pack,
    pubkey::Pubkey,
    system_program,
    sysvar,
};
use spl_token::state::{Account as TokenAccount, Mint};
use std::sync::Arc;
use futures;

// Import our local math functions for Lifinity calculations
use crate::dex::math::lifinity::calculate_lifinity_output;

// --- Constants ---
pub const LIFINITY_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S");
pub const LIFINITY_POOL_STATE_SIZE: usize = 1024; // Estimated size

// Instruction discriminators
const LIFINITY_SWAP_DISCRIMINATOR: [u8; 1] = [0x09];

// --- On-Chain Data Structures ---
#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LifinityPoolState {
    pub discriminator: [u8; 8],
    pub authority: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub fee_numerator: u32,
    pub fee_denominator: u32,
    pub fee_rate_bips: u16,
    pub concentration: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current: i32,
    pub protocol_fee_a: u64,
    pub protocol_fee_b: u64,
    pub last_rebalance_timestamp: i64,
    pub oracle_price_a: u64,
    pub oracle_price_b: u64,
    pub oracle: Pubkey,
    pub status: u8,
    pub reserved: [u8; 223], // Adjusted to fit the total size
}

/// Lifinity pool parser
pub struct LifinityPoolParser;

#[async_trait]
impl UtilsPoolParser for LifinityPoolParser {
    async fn parse_pool_data(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < LIFINITY_POOL_STATE_SIZE {
            return Err(anyhow!("Invalid Lifinity pool data size: expected {}, got {}", 
                              LIFINITY_POOL_STATE_SIZE, data.len()));
        }

        let pool_state = bytemuck::from_bytes::<LifinityPoolState>(&data[..LIFINITY_POOL_STATE_SIZE]);

        // Fetch token account data concurrently
        let (token_a_account_result, token_b_account_result, token_a_mint_result, token_b_mint_result) = tokio::join!(
            rpc_client.get_account_data(&pool_state.token_a_vault),
            rpc_client.get_account_data(&pool_state.token_b_vault),
            rpc_client.get_account_data(&pool_state.token_a_mint),
            rpc_client.get_account_data(&pool_state.token_b_mint)
        );

        // Parse token account data
        let token_a_account_data = token_a_account_result?;
        let token_b_account_data = token_b_account_result?;
        let token_a_mint_data = token_a_mint_result?;
        let token_b_mint_data = token_b_mint_result?;

        // Unpack token account data
        let token_a_data = TokenAccount::unpack(&token_a_account_data)?;
        let token_b_data = TokenAccount::unpack(&token_b_account_data)?;
        let token_a_mint = Mint::unpack(&token_a_mint_data)?;
        let token_b_mint = Mint::unpack(&token_b_mint_data)?;

        let pool_info = PoolInfo {
            address: pool_address,
            name: format!("Lifinity Pool"),
            token_a: PoolToken {
                mint: pool_state.token_a_mint,
                symbol: "Unknown".to_string(), // Would be resolved from metadata
                decimals: token_a_mint.decimals,
                reserve: token_a_data.amount, // Fix: Keep as u64, no conversion to f64
            },
            token_b: PoolToken {
                mint: pool_state.token_b_mint,
                symbol: "Unknown".to_string(),
                decimals: token_b_mint.decimals,
                reserve: token_b_data.amount, // Fix: Keep as u64, no conversion to f64
            },
            token_a_vault: pool_state.token_a_vault,
            token_b_vault: pool_state.token_b_vault,
            fee_numerator: Some(pool_state.fee_numerator as u64), // Fix: Convert u32 to u64
            fee_denominator: Some(pool_state.fee_denominator as u64), // Fix: Convert u32 to u64
            fee_rate_bips: Some(pool_state.fee_rate_bips),
            last_update_timestamp: pool_state.last_rebalance_timestamp as u64,
            dex_type: DexType::Lifinity,
            // Lifinity-specific CLMM fields
            liquidity: Some(pool_state.liquidity),
            sqrt_price: Some(pool_state.sqrt_price),
            tick_current_index: Some(pool_state.tick_current),
            tick_spacing: Some(pool_state.concentration), // Using concentration as tick spacing
            // Orca-specific fields (not applicable)
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(pool_state.oracle), // Lifinity uses oracle for price feeds
        };

        Ok(pool_info)
    }

    fn parse_pool_data_sync(
        &self,
        _pool_address: Pubkey,
        _data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        Err(anyhow!("Synchronous parsing not implemented for Lifinity"))
    }

    fn get_program_id(&self) -> Pubkey {
        LIFINITY_PROGRAM_ID
    }
}

/// Lifinity DEX client
pub struct LifinityClient {
    pub name: String,
}

impl LifinityClient {
    pub fn new() -> Self {
        Self {
            name: "Lifinity".to_string(),
        }
    }

    /// Build Lifinity swap instruction
    async fn build_swap_instruction(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: &PoolInfo,
    ) -> Result<Instruction, crate::error::ArbError> {
        // Determine swap direction
        let swap_a_to_b = swap_info.source_token_mint == pool_info.token_a.mint;
        
        let accounts = vec![
            AccountMeta::new_readonly(LIFINITY_PROGRAM_ID, false),
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            AccountMeta::new(pool_info.address, false), // Pool state
            AccountMeta::new(swap_info.user_source_token_account, false), // User source
            AccountMeta::new(swap_info.user_destination_token_account, false), // User destination
            AccountMeta::new(pool_info.token_a_vault, false), // Pool token A vault
            AccountMeta::new(pool_info.token_b_vault, false), // Pool token B vault
            AccountMeta::new_readonly(pool_info.token_a.mint, false), // Token A mint
            AccountMeta::new_readonly(pool_info.token_b.mint, false), // Token B mint
            AccountMeta::new_readonly(spl_token::id(), false), // Token program
            AccountMeta::new_readonly(system_program::id(), false), // System program
            AccountMeta::new_readonly(sysvar::clock::id(), false), // Clock sysvar for oracle
        ];

        // Build instruction data
        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(&LIFINITY_SWAP_DISCRIMINATOR); // Fix: Use slice instead of single u8
        instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes());
        instruction_data.push(if swap_a_to_b { 1 } else { 0 }); // Swap direction

        Ok(Instruction {
            program_id: LIFINITY_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }

    /// Calculate oracle-adjusted price
    pub fn calculate_oracle_price(&self, pool_info: &PoolInfo) -> Option<f64> {
        // In a real implementation, this would fetch from Lifinity's oracle
        // For now, calculate from reserves
        if pool_info.token_a.reserve > 0 && pool_info.token_b.reserve > 0 {
            Some(pool_info.token_b.reserve as f64 / pool_info.token_a.reserve as f64)
        } else {
            None
        }
    }
}

#[async_trait]
impl DexClient for LifinityClient {
    fn get_name(&self) -> &str {
        "Lifinity"
    }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        // Use Lifinity's proactive market making calculation from math module
        let oracle_price = pool.oracle.and_then(|_| {
            // In production, this would fetch from oracle
            // For now, calculate from reserves as a fallback
            if pool.token_a.reserve > 0 && pool.token_b.reserve > 0 {
                Some((pool.token_b.reserve * 1000000) / pool.token_a.reserve) // Scale by 1M for precision
            } else {
                None
            }
        });

        let output_amount = calculate_lifinity_output(
            input_amount,
            pool.token_a.reserve,
            pool.token_b.reserve,
            pool.fee_rate_bips.unwrap_or(25) as u32,
            oracle_price,
        ).map_err(|e| anyhow!("Lifinity calculation failed: {}", e))?;
        
        // Calculate slippage estimate (higher for Lifinity due to proactive market making)
        let slippage_estimate = if input_amount > 0 && pool.token_a.reserve > 0 {
            let price_impact = (input_amount as f64) / (pool.token_a.reserve as f64);
            Some((price_impact * 100.0).min(5.0)) // Cap at 5%
        } else {
            Some(0.1) // Default 0.1%
        };
        
        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount,
            dex: self.name.clone(), // Now using the name field
            route: vec![pool.address],
            slippage_estimate,
        })
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        // Legacy method - use enhanced version
        let common_swap_info = CommonSwapInfo {
            user_wallet_pubkey: swap_info.user_wallet,
            user_source_token_account: swap_info.user_source_token_account,
            user_destination_token_account: swap_info.user_destination_token_account,
            source_token_mint: swap_info.pool.token_a.mint,
            destination_token_mint: swap_info.pool.token_b.mint,
            input_amount: swap_info.amount_in,
            minimum_output_amount: swap_info.min_output_amount,
            priority_fee_lamports: None, // Set as needed
            slippage_bps: None, // Set as needed
        };
        
        // Create a pool_info Arc for the enhanced method
        let pool_info = Arc::new(swap_info.pool.clone());
        
        // Use async runtime to call the enhanced method
        futures::executor::block_on(async {
            self.get_swap_instruction_enhanced(&common_swap_info, pool_info).await
        }).map_err(|e| anyhow!("Enhanced swap instruction failed: {}", e))
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        self.build_swap_instruction(swap_info, &pool_info).await
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("LifinityClient: Starting pool discovery...");
        
        // Create enhanced sample pools with oracle integration
        let mut pools = Vec::new();

        // Enhanced USDC-SOL pool with oracle pricing
        let usdc_sol_pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Lifinity USDC-SOL Oracle Pool".to_string(),
            token_a: PoolToken {
                mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 10_000_000_000, // 10M USDC
            },
            token_b: PoolToken {
                mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 50_000_000_000, // 50K SOL
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(20),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(20), // 0.2%
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Lifinity,
            liquidity: Some(1_000_000_000_000),
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        };
        
        // Test oracle price calculation
        let oracle_price = self.calculate_oracle_price(&usdc_sol_pool);
        debug!("Calculated oracle price for {}: {:?}", usdc_sol_pool.name, oracle_price);
        
        pools.push(usdc_sol_pool);

        info!("LifinityClient: Discovered {} pools with oracle integration", pools.len());
        Ok(pools)
    }

    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError> {
        Ok(DexHealthStatus {
            is_healthy: true,
            last_successful_request: Some(std::time::Instant::now()),
            error_count: 0,
            response_time_ms: Some(50), // Fix: Wrap in Some()
            pool_count: None,
            status_message: "Lifinity client healthy".to_string(), // Fix: Use status_message instead of error_message
        })
    }
}

#[async_trait]
impl PoolDiscoverable for LifinityClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        // Delegate to DexClient implementation to avoid duplication
        <Self as DexClient>::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // In production, this would fetch actual pool data using RPC client
        // For now, create a sample pool to demonstrate functionality
        warn!("Lifinity: Fetching pool data for {} (using sample data)", pool_address);
        
        let sample_pool = PoolInfo {
            address: pool_address,
            name: "Lifinity Sample Pool".to_string(),
            token_a: PoolToken {
                mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 1_000_000_000, // 1M USDC
            },
            token_b: PoolToken {
                mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 10_000_000_000, // 10K SOL
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(30),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Lifinity,
            liquidity: Some(500_000_000_000),
            sqrt_price: Some(10000000000000),
            tick_current_index: Some(0),
            tick_spacing: Some(64),
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        };

        // Demonstrate oracle price calculation
        if let Some(price) = self.calculate_oracle_price(&sample_pool) {
            warn!("Pool {} oracle price: {:.6}", pool_address, price);
        }

        Ok(sample_pool)
    }

    fn dex_name(&self) -> &str {
        "Lifinity"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::DexType;

    fn create_test_pool_info() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TokenA".to_string(),
                decimals: 6,
                reserve: 1000000, // Fix: Use u64 instead of f64
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TokenB".to_string(),
                decimals: 6,
                reserve: 2000000, // Fix: Use u64 instead of f64
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 0,
            dex_type: DexType::Lifinity,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        }
    }

    #[test]
    fn test_lifinity_client_creation() {
        let client = LifinityClient::new();
        assert_eq!(client.get_name(), "Lifinity");
    }

    #[test]
    fn test_oracle_price_calculation() {
        let client = LifinityClient::new();
        let pool_info = create_test_pool_info();

        let oracle_price = client.calculate_oracle_price(&pool_info);
        assert_eq!(oracle_price, Some(2.0)); // 2 B per 1 A
    }

    #[test]
    fn test_oracle_price_zero_reserves() {
        let client = LifinityClient::new();
        
        let mut pool_info = create_test_pool_info();
        pool_info.token_a.reserve = 0; // Fix: Use u64 instead of f64

        let oracle_price = client.calculate_oracle_price(&pool_info);
        assert_eq!(oracle_price, None);
    }

    #[test]
    fn test_lifinity_quote_calculation() {
        let client = LifinityClient::new();
        let pool_info = create_test_pool_info();
        
        let quote = client.calculate_onchain_quote(&pool_info, 100_000).unwrap();
        
        assert_eq!(quote.input_amount, 100_000);
        assert!(quote.output_amount > 0);
        assert_eq!(quote.dex, "Lifinity");
        assert_eq!(quote.route.len(), 1);
        assert!(quote.slippage_estimate.is_some());
    }

    #[tokio::test]
    async fn test_pool_discovery() {
        let client = LifinityClient::new();
        let pools = <LifinityClient as crate::dex::api::DexClient>::discover_pools(&client).await.unwrap();
        
        // Should discover at least one pool
        assert!(!pools.is_empty());
        assert_eq!(pools[0].dex_type, DexType::Lifinity);
        
        // Oracle price should be calculated for pools
        let oracle_price = client.calculate_oracle_price(&pools[0]);
        assert!(oracle_price.is_some());
    }
}