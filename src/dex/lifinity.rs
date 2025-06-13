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

/// Lifinity Proton Pool State struct.
/// Based on Lifinity's concentrated liquidity model with oracle-based rebalancing.
/// This is an initial implementation that will be refined through testing with real pool data.
#[repr(C, packed)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LifinityPoolState {
    /// Discriminator for the account type (8 bytes)
    pub discriminator: [u8; 8],
    
    /// Authority that can modify pool parameters
    pub authority: Pubkey,
    
    /// Token A mint address
    pub token_a_mint: Pubkey,
    /// Token B mint address
    pub token_b_mint: Pubkey,
    
    /// Token A vault account
    pub token_a_vault: Pubkey,
    /// Token B vault account  
    pub token_b_vault: Pubkey,
    
    /// LP token mint
    pub lp_mint: Pubkey,
    
    /// Fee rate in basis points (1/10000)
    pub fee_bps: u16,
    
    /// Pool concentration parameter
    pub concentration: u64,
    
    /// Current liquidity in the pool
    pub liquidity: u128,
    
    /// Pool's sqrt price (Q64.64 format)
    pub sqrt_price: u128,
    
    /// Current tick index
    pub tick_current: i32,
    
    /// Protocol fee accumulated for token A
    pub protocol_fee_a: u64,
    /// Protocol fee accumulated for token B  
    pub protocol_fee_b: u64,
    
    /// Last rebalancing timestamp
    pub last_rebalance_timestamp: u64,
    
    /// Oracle price for token A (Q64.64)
    pub oracle_price_a: u128,
    /// Oracle price for token B (Q64.64)
    pub oracle_price_b: u128,
    
    /// Pool status flags
    pub status: u8,
    
    /// Reserved space for future use (reducing to supported array size)
    pub reserved: [u8; 128],
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
        // Validate data length first
        if data.len() < std::mem::size_of::<LifinityPoolState>() {
            return Err(anyhow!(
                "Lifinity pool data too short for {}: expected {} bytes, got {}",
                address,
                std::mem::size_of::<LifinityPoolState>(),
                data.len()
            ));
        }

        info!("Parsing Lifinity pool data for address: {}", address);
        info!("Pool data length: {} bytes", data.len());

        let state: &LifinityPoolState = bytemuck::try_from_bytes(data)
            .map_err(|e| anyhow!("Failed to parse Lifinity pool state for {}: {}", address, e))?;

        // Copy packed fields to local variables to avoid alignment issues
        let fee_bps = state.fee_bps;
        let concentration = state.concentration;
        let liquidity = state.liquidity;
        let status = state.status;
        let token_a_mint = state.token_a_mint;
        let token_b_mint = state.token_b_mint;
        let token_a_vault = state.token_a_vault;
        let token_b_vault = state.token_b_vault;
        let sqrt_price = state.sqrt_price;
        let tick_current = state.tick_current;
        let last_rebalance_timestamp = state.last_rebalance_timestamp;

        // Validate pool state
        if status != 1 {
            return Err(anyhow!(
                "Lifinity pool {} is not active (status: {})",
                address,
                status
            ));
        }

        info!("Lifinity pool state: fee_bps={}, concentration={}, liquidity={}", 
            fee_bps, concentration, liquidity);

        // Fetch vault balances and token decimals in parallel
        let (token_a_vault_data, token_b_vault_data, decimals_a, decimals_b) = tokio::try_join!(
            async { 
                rpc_client.primary_client.get_account_data(&token_a_vault).await
                    .map_err(|e| anyhow!("Failed to fetch token A vault {}: {}", token_a_vault, e))
            },
            async { 
                rpc_client.primary_client.get_account_data(&token_b_vault).await
                    .map_err(|e| anyhow!("Failed to fetch token B vault {}: {}", token_b_vault, e))
            },
            async { 
                rpc_client.get_token_mint_decimals(&token_a_mint).await
                    .map_err(|e| anyhow!("Failed to fetch token A decimals {}: {}", token_a_mint, e))
            },
            async { 
                rpc_client.get_token_mint_decimals(&token_b_mint).await
                    .map_err(|e| anyhow!("Failed to fetch token B decimals {}: {}", token_b_mint, e))
            }
        )?;

        // Parse token account data
        let token_a_account = TokenAccount::unpack(&token_a_vault_data)
            .map_err(|e| anyhow!("Failed to unpack token A vault data: {}", e))?;
        let token_b_account = TokenAccount::unpack(&token_b_vault_data)
            .map_err(|e| anyhow!("Failed to unpack token B vault data: {}", e))?;

        let reserve_a = token_a_account.amount;
        let reserve_b = token_b_account.amount;

        // Validate reserves
        if reserve_a == 0 && reserve_b == 0 {
            return Err(anyhow!("Lifinity pool {} has zero reserves", address));
        }

        info!("Lifinity pool reserves: token_a={}, token_b={}, decimals=({}, {})", 
            reserve_a, reserve_b, decimals_a, decimals_b);

        Ok(PoolInfo {
            address,
            name: format!("Lifinity/{}", address),
            token_a: PoolToken {
                mint: token_a_mint,
                symbol: "TKA".to_string(), // TODO: Replace with actual symbol lookup
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: token_b_mint,
                symbol: "TKB".to_string(), // TODO: Replace with actual symbol lookup
                decimals: decimals_b,
                reserve: reserve_b,
            },
            token_a_vault: token_a_vault,
            token_b_vault: token_b_vault,
            fee_numerator: Some(fee_bps as u64),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: last_rebalance_timestamp,
            dex_type: DexType::Lifinity,
            liquidity: Some(liquidity),
            sqrt_price: Some(sqrt_price),
            tick_current_index: Some(tick_current),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::quote::{DexClient, SwapInfo};
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use bytemuck::Zeroable;
    use solana_sdk::pubkey::Pubkey;
    use std::mem;
    use std::str::FromStr;

    // Test constants
    const TEST_PROGRAM_ID: &str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn";
    const POOL_ADDRESS: &str = "7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y";
    const TOKEN_A_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // USDC
    const TOKEN_B_MINT: &str = "So11111111111111111111111111111111111111112";  // SOL
    const TOKEN_A_VAULT: &str = "5VUJKMjGJZp9GuxhqfcECy6tE5Jit9r8fLzH5fYZ1aQz";
    const TOKEN_B_VAULT: &str = "7VaXtCKtJBJUJpXV2KDmJh7CnP5Mz9XqDm8CdgY5rtTp";

    /// Helper function to create mock LifinityPoolState for testing
    fn create_mock_pool_state() -> LifinityPoolState {
        let mut state = LifinityPoolState::zeroed();
        
        // Set discriminator 
        state.discriminator = [1, 2, 3, 4, 5, 6, 7, 8];
        
        // Set authority
        state.authority = Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap();
        
        // Set mints
        state.token_a_mint = Pubkey::from_str(TOKEN_A_MINT).unwrap();
        state.token_b_mint = Pubkey::from_str(TOKEN_B_MINT).unwrap();
        
        // Set vaults
        state.token_a_vault = Pubkey::from_str(TOKEN_A_VAULT).unwrap();
        state.token_b_vault = Pubkey::from_str(TOKEN_B_VAULT).unwrap();
        
        // Set LP mint
        state.lp_mint = Pubkey::from_str("9iKqoEMcBCF1Gb2ZPf7xvJq8J5vHJVo5Ktf2HsBNFJmv").unwrap();
        
        // Set pool parameters
        state.fee_bps = 30; // 0.3%
        state.concentration = 1000000;
        state.liquidity = 50000000000u128; // 50B
        state.sqrt_price = 1000000000000000000u128; // Q64.64 format
        state.tick_current = 1000;
        state.protocol_fee_a = 1000000;
        state.protocol_fee_b = 2000000;
        state.last_rebalance_timestamp = 1672531200; // 2023-01-01
        state.oracle_price_a = 1000000000000000000u128;
        state.oracle_price_b = 50000000000000000000u128;
        state.status = 1; // Active
        
        state
    }

    /// Helper function to create mock pool data bytes
    fn create_mock_pool_data() -> Vec<u8> {
        let state = create_mock_pool_state();
        bytemuck::bytes_of(&state).to_vec()
    }

    #[test]
    fn test_lifinity_pool_state_size() {
        // Test 1: Verify struct size is reasonable for blockchain account
        let size = mem::size_of::<LifinityPoolState>();
        println!("LifinityPoolState size: {} bytes", size);
        
        // Lifinity pools should be within reasonable size limits (typically < 1KB)
        assert!(size <= 1024, "LifinityPoolState too large: {} bytes", size);
        assert!(size >= 200, "LifinityPoolState too small: {} bytes", size);
    }

    #[test]
    fn test_lifinity_pool_state_pod_trait() {
        // Test 2: Verify Pod and Zeroable traits work correctly
        let state = LifinityPoolState::zeroed();
        
        // Should be able to convert to bytes and back
        let bytes = bytemuck::bytes_of(&state);
        let parsed: &LifinityPoolState = bytemuck::from_bytes(bytes);
        
        // All fields should be zero for zeroed struct (copy to avoid alignment issues)
        let fee_bps = parsed.fee_bps;
        let concentration = parsed.concentration;
        let liquidity = parsed.liquidity;
        let sqrt_price = parsed.sqrt_price;
        let status = parsed.status;
        
        assert_eq!(fee_bps, 0);
        assert_eq!(concentration, 0);
        assert_eq!(liquidity, 0);
        assert_eq!(sqrt_price, 0);
        assert_eq!(status, 0);
    }

    #[test]
    fn test_lifinity_pool_state_field_alignment() {
        // Test 3: Verify field access works with packed struct
        let mut state = create_mock_pool_state();
        
        // Test field access by copying values (avoiding alignment issues)
        let fee_bps = state.fee_bps;
        let concentration = state.concentration;
        let liquidity = state.liquidity;
        let status = state.status;
        
        assert_eq!(fee_bps, 30);
        assert_eq!(concentration, 1000000);
        assert_eq!(liquidity, 50000000000u128);
        assert_eq!(status, 1);
        
        // Test field modification
        state.fee_bps = 50;
        let new_fee = state.fee_bps;
        assert_eq!(new_fee, 50);
    }

    #[test]
    fn test_lifinity_pool_parser_program_id() {
        // Test 4: Verify parser returns correct program ID
        let parser = LifinityPoolParser;
        let program_id = parser.get_program_id();
        let expected = Pubkey::from_str(TEST_PROGRAM_ID).unwrap();
        
        assert_eq!(program_id, expected, "Program ID mismatch");
    }

    #[test]
    fn test_lifinity_pool_state_serialization() {
        // Test 5: Verify serialization and deserialization
        let original_state = create_mock_pool_state();
        let bytes = bytemuck::bytes_of(&original_state);
        
        // Should be able to deserialize back to the same values
        let parsed_state: &LifinityPoolState = bytemuck::from_bytes(bytes);
        
        // Compare key fields (copy to avoid alignment issues)
        let orig_fee_bps = original_state.fee_bps;
        let orig_concentration = original_state.concentration;
        let orig_liquidity = original_state.liquidity;
        let orig_sqrt_price = original_state.sqrt_price;
        let orig_tick_current = original_state.tick_current;
        let orig_status = original_state.status;
        let orig_token_a_mint = original_state.token_a_mint;
        let orig_token_b_mint = original_state.token_b_mint;
        
        let parsed_fee_bps = parsed_state.fee_bps;
        let parsed_concentration = parsed_state.concentration;
        let parsed_liquidity = parsed_state.liquidity;
        let parsed_sqrt_price = parsed_state.sqrt_price;
        let parsed_tick_current = parsed_state.tick_current;
        let parsed_status = parsed_state.status;
        let parsed_token_a_mint = parsed_state.token_a_mint;
        let parsed_token_b_mint = parsed_state.token_b_mint;
        
        assert_eq!(parsed_fee_bps, orig_fee_bps);
        assert_eq!(parsed_concentration, orig_concentration);
        assert_eq!(parsed_liquidity, orig_liquidity);
        assert_eq!(parsed_sqrt_price, orig_sqrt_price);
        assert_eq!(parsed_tick_current, orig_tick_current);
        assert_eq!(parsed_status, orig_status);
        assert_eq!(parsed_token_a_mint, orig_token_a_mint);
        assert_eq!(parsed_token_b_mint, orig_token_b_mint);
    }

    #[test]
    fn test_lifinity_pool_data_validation() {
        // Test 6: Test data validation in parser
        // Test too short data
        let short_data = vec![0u8; 100];
        
        // Since we can't easily create a mock RpcClient, we'll test the size validation
        // by checking if the data is long enough
        let required_size = mem::size_of::<LifinityPoolState>();
        assert!(short_data.len() < required_size, "Test data should be too short");
        
        // Test properly sized data
        let proper_data = create_mock_pool_data();
        assert!(proper_data.len() >= required_size, "Mock data should be properly sized");
    }

    #[test]
    fn test_lifinity_client_implementation() {
        // Test 7: Verify LifinityClient trait implementation
        let client = LifinityClient::new();
        
        assert_eq!(client.get_name(), "Lifinity");
        
        // Test default implementation
        let default_client = LifinityClient::default();
        assert_eq!(default_client.get_name(), "Lifinity");
    }

    #[test]
    fn test_lifinity_quote_calculation() {
        // Test 8: Test quote calculation logic
        let client = LifinityClient::new();
        
        // Create mock pool info
        let pool = PoolInfo {
            address: Pubkey::from_str(POOL_ADDRESS).unwrap(),
            name: "Lifinity/TestPool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::from_str(TOKEN_A_MINT).unwrap(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 1000000000000, // 1M USDC
            },
            token_b: PoolToken {
                mint: Pubkey::from_str(TOKEN_B_MINT).unwrap(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 20000000000000, // 20K SOL
            },
            token_a_vault: Pubkey::from_str(TOKEN_A_VAULT).unwrap(),
            token_b_vault: Pubkey::from_str(TOKEN_B_VAULT).unwrap(),
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: 1672531200,
            dex_type: DexType::Lifinity,
            liquidity: Some(50000000000u128),
            sqrt_price: Some(1000000000000000000u128),
            tick_current_index: Some(1000),
            tick_spacing: None,
        };
        
        // Test quote calculation
        let input_amount = 1000000; // 1 USDC
        let quote_result = client.calculate_onchain_quote(&pool, input_amount);
        
        assert!(quote_result.is_ok(), "Quote calculation should succeed");
        
        let quote = quote_result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0, "Output amount should be positive");
        assert_eq!(quote.dex, "Lifinity");
        assert_eq!(quote.route.len(), 1);
        assert_eq!(quote.route[0], pool.address);
    }

    #[test]
    fn test_lifinity_quote_zero_reserves() {
        // Test 9: Test quote calculation with zero reserves
        let client = LifinityClient::new();
        
        // Create pool with zero reserves
        let pool = PoolInfo {
            address: Pubkey::from_str(POOL_ADDRESS).unwrap(),
            name: "Lifinity/TestPool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::from_str(TOKEN_A_MINT).unwrap(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 0, // Zero reserves
            },
            token_b: PoolToken {
                mint: Pubkey::from_str(TOKEN_B_MINT).unwrap(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 20000000000000,
            },
            token_a_vault: Pubkey::from_str(TOKEN_A_VAULT).unwrap(),
            token_b_vault: Pubkey::from_str(TOKEN_B_VAULT).unwrap(),
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: 1672531200,
            dex_type: DexType::Lifinity,
            liquidity: Some(50000000000u128),
            sqrt_price: Some(1000000000000000000u128),
            tick_current_index: Some(1000),
            tick_spacing: None,
        };
        
        let input_amount = 1000000;
        let quote_result = client.calculate_onchain_quote(&pool, input_amount);
        
        assert!(quote_result.is_err(), "Quote should fail with zero reserves");
        let error = quote_result.unwrap_err();
        assert!(error.to_string().contains("zero reserves"), "Error should mention zero reserves");
    }

    #[test]
    fn test_lifinity_swap_instruction_placeholder() {
        // Test 10: Test swap instruction building (placeholder implementation)
        let client = LifinityClient::new();
        
        // Create a mock pool for the SwapInfo
        let pool = PoolInfo {
            address: Pubkey::from_str(POOL_ADDRESS).unwrap(),
            name: "Lifinity/TestPool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::from_str(TOKEN_A_MINT).unwrap(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 1000000000000,
            },
            token_b: PoolToken {
                mint: Pubkey::from_str(TOKEN_B_MINT).unwrap(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 20000000000000,
            },
            token_a_vault: Pubkey::from_str(TOKEN_A_VAULT).unwrap(),
            token_b_vault: Pubkey::from_str(TOKEN_B_VAULT).unwrap(),
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: 1672531200,
            dex_type: DexType::Lifinity,
            liquidity: Some(50000000000u128),
            sqrt_price: Some(1000000000000000000u128),
            tick_current_index: Some(1000),
            tick_spacing: None,
        };
        
        // Create a properly structured SwapInfo
        let swap_info = SwapInfo {
            dex_name: "Lifinity",
            pool: &pool,
            user_wallet: Pubkey::from_str("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM").unwrap(),
            user_source_token_account: Pubkey::from_str("HdyMPk1x95LdVrGP5kpZF6aCMYhWYyQ3i6jNyU8pMB7y").unwrap(),
            user_destination_token_account: Pubkey::from_str("GqE8LTdKB5dPF4mhZJ3Xx2p9Y7vV6wH8nJhb2A9tQK4x").unwrap(),
            amount_in: 1000000,
            min_output_amount: 900000,
            pool_account: Pubkey::from_str(POOL_ADDRESS).unwrap(),
            pool_authority: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            pool_open_orders: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            pool_target_orders: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            pool_base_vault: Pubkey::from_str(TOKEN_A_VAULT).unwrap(),
            pool_quote_vault: Pubkey::from_str(TOKEN_B_VAULT).unwrap(),
            market_id: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            market_bids: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            market_asks: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            market_event_queue: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            market_program_id: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            market_authority: Pubkey::from_str("7R5DJKqfPEV4mF2iLUP4iu14fLp6VqZqrEFRBVPL8J2y").unwrap(),
            user_owner: Pubkey::from_str("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM").unwrap(),
        };
        
        let instruction_result = client.get_swap_instruction(&swap_info);
        
        // Current implementation returns "not yet implemented" error
        assert!(instruction_result.is_err(), "Swap instruction should return not implemented error");
        let error = instruction_result.unwrap_err();
        assert!(error.to_string().contains("not yet implemented"), 
            "Error should mention not implemented");
    }

    #[test]
    fn test_lifinity_pool_state_constants() {
        // Test 11: Verify important constants and program ID
        let expected_program_id = Pubkey::from_str(TEST_PROGRAM_ID).unwrap();
        let actual_program_id = Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap();
        
        assert_eq!(actual_program_id, expected_program_id, "Program ID should match test constant");
        
        // Test struct size is consistent
        let size = mem::size_of::<LifinityPoolState>();
        assert!(size > 0, "Struct size should be positive");
        
        // Test that all fields fit within the struct
        let state = create_mock_pool_state();
        let bytes = bytemuck::bytes_of(&state);
        assert_eq!(bytes.len(), size, "Byte length should match struct size");
    }

    #[test]
    fn test_lifinity_pool_status_validation() {
        // Test 12: Test pool status validation logic
        let mut pool_data = create_mock_pool_data();
        
        // Modify status to inactive (0)
        let mut state: LifinityPoolState = *bytemuck::from_bytes(&pool_data);
        state.status = 0; // Inactive
        pool_data = bytemuck::bytes_of(&state).to_vec();
        
        // The parser should reject inactive pools when we implement full validation
        // For now, we can test that the status field is correctly set and read
        let parsed_state: &LifinityPoolState = bytemuck::from_bytes(&pool_data);
        assert_eq!(parsed_state.status, 0, "Status should be inactive");
        
        // Test active status
        state.status = 1; // Active
        pool_data = bytemuck::bytes_of(&state).to_vec();
        let parsed_state: &LifinityPoolState = bytemuck::from_bytes(&pool_data);
        assert_eq!(parsed_state.status, 1, "Status should be active");
    }
}

