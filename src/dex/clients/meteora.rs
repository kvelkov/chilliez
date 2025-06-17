// src/dex/meteora.rs
//! Meteora DEX integration supporting both Dynamic AMM and DLMM pool types.

use crate::dex::api::{
    CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo,
};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{debug, info, warn};
use num_traits::ToPrimitive;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use std::str::FromStr;
use std::sync::Arc;

// Import our math functions for Meteora calculations
use crate::dex::math::clmm::calculate_price_impact;
use crate::dex::math::meteora::{calculate_dlmm_output, calculate_dynamic_amm_output};

// --- Constants (Made Public for tests) ---
pub const METEORA_DYNAMIC_AMM_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB");
pub const METEORA_DLMM_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
pub const DYNAMIC_AMM_POOL_STATE_SIZE: usize = 520;
pub const DLMM_LB_PAIR_STATE_SIZE: usize = 304;

// Instruction discriminators for Meteora
const DYNAMIC_AMM_SWAP_DISCRIMINATOR: u8 = 0x01;
const DLMM_SWAP_DISCRIMINATOR: u8 = 0x02;

// --- On-Chain Data Structures (Made Public for tests) ---
#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct DynamicAmmPoolState {
    pub enabled: u8,
    pub bump: u8,
    pub pool_type: u8,
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
    pub token_a_fees: u64,
    pub token_b_fees: u64,
    pub oracle: Pubkey,
    pub fee_rate: u64,
    pub protocol_fee_rate: u64,
    pub lp_fee_rate: u64,
    pub curve_type: u8,
    pub _padding: [u8; 7],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct DlmmLbPairState {
    pub active_id: u32,
    pub bin_step: u16,
    pub status: u8,
    pub _padding1: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee_x: u64,
    pub protocol_fee_y: u64,
    pub fee_bps: u16,
    pub protocol_share: u16,
    pub pair_type: u8,
    pub _padding2: [u8; 3],
    pub oracle: Pubkey,
    pub _reserved: [u8; 64],
}

/// Meteora pool type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum MeteoraPoolType {
    DynamicAmm,
    Dlmm,
}

/// Meteora pool parser
pub struct MeteoraPoolParser;

impl MeteoraPoolParser {
    /// Identify the pool type based on program ID and account size
    #[allow(dead_code)] // Planned for Meteora pool type detection
    pub fn identify_pool_type(program_id: &Pubkey, data_len: usize) -> Option<MeteoraPoolType> {
        match program_id {
            &METEORA_DYNAMIC_AMM_PROGRAM_ID => {
                if data_len >= DYNAMIC_AMM_POOL_STATE_SIZE {
                    Some(MeteoraPoolType::DynamicAmm)
                } else {
                    None
                }
            }
            &METEORA_DLMM_PROGRAM_ID => {
                if data_len >= DLMM_LB_PAIR_STATE_SIZE {
                    Some(MeteoraPoolType::Dlmm)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Identify the pool type based on data length only (when program ID isn't available)
    pub fn identify_pool_type_from_data(data: &[u8]) -> Option<MeteoraPoolType> {
        match data.len() {
            DYNAMIC_AMM_POOL_STATE_SIZE => Some(MeteoraPoolType::DynamicAmm),
            DLMM_LB_PAIR_STATE_SIZE => Some(MeteoraPoolType::Dlmm),
            _ => None,
        }
    }

    /// Parse Dynamic AMM pool state
    fn parse_dynamic_amm_pool(
        pool_address: Pubkey,
        data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < DYNAMIC_AMM_POOL_STATE_SIZE {
            return Err(anyhow!("Invalid Dynamic AMM pool data size"));
        }

        let pool_state =
            bytemuck::from_bytes::<DynamicAmmPoolState>(&data[..DYNAMIC_AMM_POOL_STATE_SIZE]);

        // Create pool info with available data
        let pool_info = PoolInfo {
            address: pool_address,
            name: format!("Meteora Dynamic AMM Pool"),
            token_a: PoolToken {
                mint: pool_state.token_a_mint,
                symbol: "Unknown".to_string(), // Will be resolved later
                decimals: 6,                   // Default, will be resolved
                reserve: 0,                    // Will be fetched from vault
            },
            token_b: PoolToken {
                mint: pool_state.token_b_mint,
                symbol: "Unknown".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_a_vault: pool_state.a_vault,
            token_b_vault: pool_state.b_vault,
            fee_numerator: Some(pool_state.fee_rate),
            fee_denominator: Some(10000), // Meteora uses basis points
            fee_rate_bips: Some((pool_state.fee_rate / 100) as u16), // Convert to bips
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Meteora,
            // CLMM fields (not applicable for Dynamic AMM)
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            // Orca-specific fields
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(pool_state.oracle),
        };

        Ok(pool_info)
    }

    /// Parse DLMM pool state
    fn parse_dlmm_pool(
        pool_address: Pubkey,
        data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < DLMM_LB_PAIR_STATE_SIZE {
            return Err(anyhow!("Invalid DLMM pool data size"));
        }

        let pool_state = bytemuck::from_bytes::<DlmmLbPairState>(&data[..DLMM_LB_PAIR_STATE_SIZE]);

        let pool_info = PoolInfo {
            address: pool_address,
            name: format!("Meteora DLMM Pool"),
            token_a: PoolToken {
                mint: pool_state.token_x_mint,
                symbol: "Unknown".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_b: PoolToken {
                mint: pool_state.token_y_mint,
                symbol: "Unknown".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_a_vault: pool_state.reserve_x,
            token_b_vault: pool_state.reserve_y,
            fee_numerator: Some(pool_state.fee_bps as u64),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(pool_state.fee_bps),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Meteora,
            // DLMM-specific fields
            liquidity: None,  // DLMM uses bin-based liquidity
            sqrt_price: None, // DLMM uses different pricing model
            tick_current_index: Some(pool_state.active_id as i32),
            tick_spacing: Some(pool_state.bin_step),
            // Orca-specific fields
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(pool_state.oracle),
        };

        Ok(pool_info)
    }
}

#[async_trait]
impl UtilsPoolParser for MeteoraPoolParser {
    async fn parse_pool_data(
        &self,
        pool_address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        // Identify pool type from data length (heuristic approach)
        let pool_type = Self::identify_pool_type_from_data(data)
            .ok_or_else(|| anyhow!("Unknown Meteora pool type with data length {}", data.len()))?;

        match pool_type {
            MeteoraPoolType::DynamicAmm => {
                Self::parse_dynamic_amm_pool(pool_address, data, rpc_client)
            }
            MeteoraPoolType::Dlmm => Self::parse_dlmm_pool(pool_address, data, rpc_client),
        }
    }

    fn get_program_id(&self) -> Pubkey {
        METEORA_DYNAMIC_AMM_PROGRAM_ID // Default to Dynamic AMM
    }
}

/// Meteora DEX client
pub struct MeteoraClient {
    pub name: String,
}

impl MeteoraClient {
    pub fn new() -> Self {
        Self {
            name: "Meteora".to_string(),
        }
    }

    /// Determine pool type from PoolInfo
    pub fn get_pool_type(&self, pool_info: &PoolInfo) -> MeteoraPoolType {
        // Use tick_spacing as an indicator - DLMM pools have this field
        if pool_info.tick_spacing.is_some() {
            MeteoraPoolType::Dlmm
        } else {
            MeteoraPoolType::DynamicAmm
        }
    }

    /// Build Dynamic AMM swap instruction
    fn build_dynamic_amm_swap_instruction(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: &PoolInfo,
    ) -> Result<Instruction, crate::error::ArbError> {
        let accounts = vec![
            AccountMeta::new_readonly(METEORA_DYNAMIC_AMM_PROGRAM_ID, false),
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            AccountMeta::new(pool_info.address, false),                    // Pool
            AccountMeta::new(swap_info.user_source_token_account, false),  // User source
            AccountMeta::new(swap_info.user_destination_token_account, false), // User destination
            AccountMeta::new(pool_info.token_a_vault, false),              // Pool token A vault
            AccountMeta::new(pool_info.token_b_vault, false),              // Pool token B vault
            AccountMeta::new_readonly(spl_token::id(), false),             // Token program
            AccountMeta::new_readonly(system_program::id(), false),        // System program
        ];

        // Build instruction data
        let mut instruction_data = Vec::new();
        instruction_data.push(DYNAMIC_AMM_SWAP_DISCRIMINATOR);
        instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes());

        Ok(Instruction {
            program_id: METEORA_DYNAMIC_AMM_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }

    /// Build DLMM swap instruction
    fn build_dlmm_swap_instruction(
        &self,
        swap_info: &CommonSwapInfo,
        pool_info: &PoolInfo,
    ) -> Result<Instruction, crate::error::ArbError> {
        let accounts = vec![
            AccountMeta::new_readonly(METEORA_DLMM_PROGRAM_ID, false),
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            AccountMeta::new(pool_info.address, false),                    // LB Pair
            AccountMeta::new(swap_info.user_source_token_account, false),  // User source
            AccountMeta::new(swap_info.user_destination_token_account, false), // User destination
            AccountMeta::new(pool_info.token_a_vault, false),              // Reserve X
            AccountMeta::new(pool_info.token_b_vault, false),              // Reserve Y
            AccountMeta::new_readonly(spl_token::id(), false),             // Token program
            AccountMeta::new_readonly(system_program::id(), false),        // System program
                                                                           // Note: DLMM requires bin arrays which would need to be resolved dynamically
        ];

        // Build instruction data for DLMM
        let mut instruction_data = Vec::new();
        instruction_data.push(DLMM_SWAP_DISCRIMINATOR);
        instruction_data.extend_from_slice(&swap_info.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes());

        // Add active bin ID if available
        if let Some(active_id) = pool_info.tick_current_index {
            instruction_data.extend_from_slice(&(active_id as u32).to_le_bytes());
        }

        Ok(Instruction {
            program_id: METEORA_DLMM_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }

    /// Create sample pools for testing and demonstration
    /// In production, this would be replaced by actual on-chain scanning
    fn create_sample_pools(&self) -> Vec<PoolInfo> {
        let mut pools = Vec::new();

        // Sample Dynamic AMM pool
        let dynamic_pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Meteora USDC-SOL Dynamic AMM".to_string(),
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
                reserve: 5_000_000_000, // 5K SOL
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25), // 0.25%
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Meteora,
            liquidity: Some(500_000_000_000),
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None, // Dynamic AMM doesn't use ticks
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        };
        pools.push(dynamic_pool);

        // Sample DLMM pool
        let dlmm_pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Meteora USDT-USDC DLMM".to_string(),
            token_a: PoolToken {
                mint: solana_sdk::pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"), // USDT
                symbol: "USDT".to_string(),
                decimals: 6,
                reserve: 2_000_000_000, // 2M USDT
            },
            token_b: PoolToken {
                mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000, // 2M USDC
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(10),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(10), // 0.1%
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Meteora,
            liquidity: Some(1_000_000_000_000),
            sqrt_price: None,
            tick_current_index: Some(8388608), // Active bin ID
            tick_spacing: Some(1),             // Bin step for DLMM
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        };
        pools.push(dlmm_pool);

        // Log pool types using our identification function
        for pool in &pools {
            let pool_type = self.get_pool_type(pool);
            debug!(
                "Created sample {} pool: {} ({})",
                match pool_type {
                    MeteoraPoolType::DynamicAmm => "Dynamic AMM",
                    MeteoraPoolType::Dlmm => "DLMM",
                },
                pool.name,
                pool.address
            );
        }

        pools
    }
}

#[async_trait]
impl DexClient for MeteoraClient {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        let pool_type = self.get_pool_type(pool);

        let output_amount = match pool_type {
            MeteoraPoolType::DynamicAmm => {
                // Use Dynamic AMM calculation from math module
                calculate_dynamic_amm_output(
                    input_amount,
                    pool.token_a.reserve,
                    pool.token_b.reserve,
                    pool.fee_rate_bips.unwrap_or(25) as u32, // Base fee
                    0, // No dynamic fee for simplified calculation
                )
                .map_err(|e| anyhow!("Dynamic AMM calculation failed: {}", e))?
            }
            MeteoraPoolType::Dlmm => {
                // Use DLMM calculation from math module
                let active_id = pool.tick_current_index.unwrap_or(8388608) as u32; // 2^23 neutral bin
                let bin_step = pool.tick_spacing.unwrap_or(1) as u16;
                let liquidity_in_bin = pool.liquidity.unwrap_or(1_000_000_000_000) as u128;

                calculate_dlmm_output(
                    input_amount,
                    active_id,
                    bin_step,
                    liquidity_in_bin,
                    pool.fee_rate_bips.unwrap_or(25),
                )
                .map_err(|e| anyhow!("DLMM calculation failed: {}", e))?
            }
        };

        // Calculate price impact for better slippage estimation
        let price_impact = match pool_type {
            MeteoraPoolType::DynamicAmm => {
                calculate_price_impact(input_amount, pool.token_a.reserve, pool.token_b.reserve)
                    .unwrap_or_else(|_| rust_decimal::Decimal::from_str("0.01").unwrap())
            }
            MeteoraPoolType::Dlmm => {
                // For DLMM, use a simplified price impact estimate based on bin concentration
                rust_decimal::Decimal::from_str("0.005").unwrap() // 0.5% for concentrated DLMM
            }
        };

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount,
            dex: self.name.clone(),
            route: vec![pool.address],
            slippage_estimate: Some(price_impact.to_f64().unwrap_or(0.01)), // Use calculated price impact
        })
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        warn!("get_swap_instruction for Meteora is a basic implementation. Use get_swap_instruction_enhanced for production.");

        // Create a basic instruction for legacy compatibility
        Ok(Instruction {
            program_id: METEORA_DYNAMIC_AMM_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(swap_info.user_wallet, true),
                AccountMeta::new(swap_info.pool_account, false),
                AccountMeta::new(swap_info.user_source_token_account, false),
                AccountMeta::new(swap_info.user_destination_token_account, false),
            ],
            data: vec![DYNAMIC_AMM_SWAP_DISCRIMINATOR], // Basic discriminator
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

        // Determine pool type and build appropriate instruction
        let pool_type = self.get_pool_type(&pool_info);

        match pool_type {
            MeteoraPoolType::DynamicAmm => {
                debug!("Building Dynamic AMM swap instruction");
                self.build_dynamic_amm_swap_instruction(swap_info, &pool_info)
            }
            MeteoraPoolType::Dlmm => {
                debug!("Building DLMM swap instruction");
                self.build_dlmm_swap_instruction(swap_info, &pool_info)
            }
        }
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("MeteoraClient: Starting pool discovery...");

        // Create a mock pool for demonstration - in production this would scan on-chain
        let mock_pools = self.create_sample_pools();

        info!("MeteoraClient: Discovered {} pools", mock_pools.len());
        Ok(mock_pools)
    }

    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError> {
        let start_time = std::time::Instant::now();

        // Basic health check - in production this would ping Meteora's API
        let is_healthy = true; // Placeholder
        let response_time = start_time.elapsed().as_millis() as u64;

        Ok(DexHealthStatus {
            is_healthy,
            last_successful_request: Some(start_time),
            error_count: 0,
            response_time_ms: Some(response_time),
            pool_count: Some(0), // Would be populated from actual discovery
            status_message: "Meteora client operational".to_string(),
        })
    }
}

#[async_trait]
impl PoolDiscoverable for MeteoraClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        DexClient::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, _pool_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // In production, this would fetch actual pool data from the blockchain
        // For now, return a sample pool to demonstrate functionality
        info!("MeteoraClient: Fetching pool data for {}", _pool_address);

        // Create a sample pool that uses our parsing functionality
        let sample_pool = PoolInfo {
            address: _pool_address,
            name: "Meteora Sample Pool".to_string(),
            token_a: PoolToken {
                mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 5_000_000_000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Meteora,
            liquidity: Some(500_000_000_000),
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        };

        // Demonstrate pool type identification
        let pool_type = self.get_pool_type(&sample_pool);
        info!("Identified pool type: {:?}", pool_type);

        Ok(sample_pool)
    }

    fn dex_name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::DexType;

    #[test]
    fn test_meteora_pool_type_identification() {
        // Test Dynamic AMM identification
        let dynamic_type = MeteoraPoolParser::identify_pool_type(
            &METEORA_DYNAMIC_AMM_PROGRAM_ID,
            DYNAMIC_AMM_POOL_STATE_SIZE,
        );
        assert_eq!(dynamic_type, Some(MeteoraPoolType::DynamicAmm));

        // Test DLMM identification
        let dlmm_type = MeteoraPoolParser::identify_pool_type(
            &METEORA_DLMM_PROGRAM_ID,
            DLMM_LB_PAIR_STATE_SIZE,
        );
        assert_eq!(dlmm_type, Some(MeteoraPoolType::Dlmm));

        // Test unknown program ID
        let unknown_type = MeteoraPoolParser::identify_pool_type(&Pubkey::new_unique(), 100);
        assert_eq!(unknown_type, None);
    }

    #[test]
    fn test_meteora_pool_parser_integration() {
        // Test that the parser can handle different data types
        let parser = MeteoraPoolParser;

        // Test Dynamic AMM data identification
        let dynamic_data = vec![0u8; DYNAMIC_AMM_POOL_STATE_SIZE];
        let dynamic_type = MeteoraPoolParser::identify_pool_type_from_data(&dynamic_data);
        assert_eq!(dynamic_type, Some(MeteoraPoolType::DynamicAmm));

        // Test DLMM data identification
        let dlmm_data = vec![0u8; DLMM_LB_PAIR_STATE_SIZE];
        let dlmm_type = MeteoraPoolParser::identify_pool_type_from_data(&dlmm_data);
        assert_eq!(dlmm_type, Some(MeteoraPoolType::Dlmm));

        // Test parser program ID
        assert_eq!(parser.get_program_id(), METEORA_DYNAMIC_AMM_PROGRAM_ID);
    }

    #[test]
    fn test_meteora_client_creation() {
        let client = MeteoraClient::new();
        assert_eq!(client.get_name(), "Meteora");
    }

    #[test]
    fn test_pool_type_detection() {
        let client = MeteoraClient::new();

        // Test Dynamic AMM pool (no tick_spacing)
        let dynamic_pool = PoolInfo {
            tick_spacing: None,
            dex_type: DexType::Meteora,
            ..Default::default()
        };
        assert_eq!(
            client.get_pool_type(&dynamic_pool),
            MeteoraPoolType::DynamicAmm
        );

        // Test DLMM pool (has tick_spacing)
        let dlmm_pool = PoolInfo {
            tick_spacing: Some(64),
            dex_type: DexType::Meteora,
            ..Default::default()
        };
        assert_eq!(client.get_pool_type(&dlmm_pool), MeteoraPoolType::Dlmm);
    }

    #[test]
    fn test_sample_pools_creation() {
        let client = MeteoraClient::new();
        let pools = client.create_sample_pools();

        // Should create 2 sample pools
        assert_eq!(pools.len(), 2);

        // First should be Dynamic AMM (no tick_spacing)
        assert_eq!(client.get_pool_type(&pools[0]), MeteoraPoolType::DynamicAmm);
        assert!(pools[0].tick_spacing.is_none());

        // Second should be DLMM (has tick_spacing)
        assert_eq!(client.get_pool_type(&pools[1]), MeteoraPoolType::Dlmm);
        assert!(pools[1].tick_spacing.is_some());

        // Both should be Meteora DEX type
        assert_eq!(pools[0].dex_type, DexType::Meteora);
        assert_eq!(pools[1].dex_type, DexType::Meteora);
    }
}
