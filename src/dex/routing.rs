// src/dex/routing.rs
//! DEX routing utilities for finding appropriate clients and grouping operations

use crate::utils::{PoolInfo, DexType};
use crate::dex::quote::DexClient;
use std::sync::Arc;

/// Finds the appropriate DEX client for a given pool based on its dex_type
/// 
/// This function matches pools to their corresponding DEX clients for operations
/// like refreshing pool data or executing trades.
pub fn find_dex_client_for_pool(
    pool: &PoolInfo, 
    dex_clients: &[Arc<dyn DexClient>]
) -> Option<Arc<dyn DexClient>> {
    for dex_client in dex_clients {
        let dex_name = dex_client.get_name();
        let matches = match &pool.dex_type {
            DexType::Orca => dex_name == "Orca",
            DexType::Raydium => dex_name == "Raydium",
            DexType::Lifinity => dex_name == "Lifinity",
            DexType::Meteora => dex_name == "Meteora",
            DexType::Whirlpool => dex_name == "Whirlpool",
            DexType::Unknown(name) => dex_name == name,
        };
        
        if matches {
            return Some(dex_client.clone());
        }
    }
    None
}

/// Groups pools by their DEX type for batch operations
pub fn group_pools_by_dex(pools: &[PoolInfo]) -> std::collections::HashMap<String, Vec<&PoolInfo>> {
    let mut grouped = std::collections::HashMap::new();
    
    for pool in pools {
        let dex_name = match &pool.dex_type {
            DexType::Orca => "Orca",
            DexType::Raydium => "Raydium", 
            DexType::Lifinity => "Lifinity",
            DexType::Meteora => "Meteora",
            DexType::Whirlpool => "Whirlpool",
            DexType::Unknown(name) => name.as_str(),
        };
        
        grouped.entry(dex_name.to_string()).or_insert_with(Vec::new).push(pool);
    }
    
    grouped
}

/// Finds all DEX clients that support a specific token pair
pub fn find_dex_clients_for_token_pair(
    token_a_mint: &solana_sdk::pubkey::Pubkey,
    token_b_mint: &solana_sdk::pubkey::Pubkey,
    pools: &[PoolInfo],
    dex_clients: &[Arc<dyn DexClient>]
) -> Vec<Arc<dyn DexClient>> {
    let mut matching_clients = Vec::new();
    let mut seen_dex_types = std::collections::HashSet::new();
    
    // Find pools that match the token pair
    for pool in pools {
        if (pool.token_a.mint == *token_a_mint && pool.token_b.mint == *token_b_mint) ||
           (pool.token_a.mint == *token_b_mint && pool.token_b.mint == *token_a_mint) {
            
            // Avoid duplicate DEX types
            if seen_dex_types.insert(pool.dex_type.clone()) {
                if let Some(client) = find_dex_client_for_pool(pool, dex_clients) {
                    matching_clients.push(client);
                }
            }
        }
    }
    
    matching_clients
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolToken};
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    fn create_test_pool(dex_type: DexType, token_a_mint: Pubkey, token_b_mint: Pubkey) -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: token_a_mint,
                decimals: 6,
                symbol: "TOKENA".to_string(),
                reserve: 1000000,
            },
            token_b: PoolToken {
                mint: token_b_mint,
                decimals: 6,
                symbol: "TOKENB".to_string(),
                reserve: 2000000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(30),
            last_update_timestamp: 1234567890,
            dex_type,
            sqrt_price: Some(1000000),
            liquidity: Some(500000),
            tick_current_index: Some(0),
            tick_spacing: Some(64),
        }
    }

    #[test]
    fn test_group_pools_by_dex() {
        let pools = vec![
            create_test_pool(DexType::Orca, Pubkey::new_unique(), Pubkey::new_unique()),
            create_test_pool(DexType::Raydium, Pubkey::new_unique(), Pubkey::new_unique()),
            create_test_pool(DexType::Orca, Pubkey::new_unique(), Pubkey::new_unique()),
            create_test_pool(DexType::Unknown("Custom".to_string()), Pubkey::new_unique(), Pubkey::new_unique()),
        ];

        let grouped = group_pools_by_dex(&pools);
        assert_eq!(grouped.len(), 3);
        assert_eq!(grouped.get("Orca").unwrap().len(), 2);
        assert_eq!(grouped.get("Raydium").unwrap().len(), 1);
        assert_eq!(grouped.get("Custom").unwrap().len(), 1);
    }

    #[test]
    fn test_find_dex_client_for_pool_no_clients() {
        let pool = create_test_pool(DexType::Orca, Pubkey::new_unique(), Pubkey::new_unique());
        let clients: Vec<Arc<dyn DexClient>> = vec![];
        let result = find_dex_client_for_pool(&pool, &clients);
        assert!(result.is_none()); // No clients provided, so should return None
    }

    #[test]
    fn test_find_dex_clients_for_token_pair() {
        let token_a = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // SOL
        let token_b = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(); // USDC
        let token_c = Pubkey::new_unique(); // Random token

        let pools = vec![
            create_test_pool(DexType::Orca, token_a, token_b),
            create_test_pool(DexType::Raydium, token_a, token_b),
            create_test_pool(DexType::Meteora, token_a, token_c), // Different pair
            create_test_pool(DexType::Orca, token_b, token_a), // Same pair, reversed
        ];

        let clients: Vec<Arc<dyn DexClient>> = vec![];
        let matching_clients = find_dex_clients_for_token_pair(&token_a, &token_b, &pools, &clients);
        
        // Should return empty since no clients are provided, but the function should not panic
        assert_eq!(matching_clients.len(), 0);
        
        // Test with different token pair that has no matches
        let matching_clients_none = find_dex_clients_for_token_pair(&token_c, &Pubkey::new_unique(), &pools, &clients);
        assert_eq!(matching_clients_none.len(), 0);
    }

    #[test]
    fn test_dex_type_matching() {
        let pool_orca = create_test_pool(DexType::Orca, Pubkey::new_unique(), Pubkey::new_unique());
        let pool_custom = create_test_pool(DexType::Unknown("MyDEX".to_string()), Pubkey::new_unique(), Pubkey::new_unique());
        
        // Test that grouping works correctly with custom DEX names
        let pools = vec![pool_orca, pool_custom];
        let grouped = group_pools_by_dex(&pools);
        
        assert!(grouped.contains_key("Orca"));
        assert!(grouped.contains_key("MyDEX"));
        assert_eq!(grouped.len(), 2);
    }
}
