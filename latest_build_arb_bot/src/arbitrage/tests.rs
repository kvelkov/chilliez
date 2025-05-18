#[cfg(test)]
mod tests {
    use super::engine::{ArbOpportunity, ArbitrageEngine};
    use crate::dex::pool::{DexType, PoolInfo, PoolToken, TokenAmount};
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_discover_and_filter() {
        let mut pools = HashMap::new();
        // Fake pool (simulate a real Raydium/Orca pool)
        let pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "FAKE/USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "FAKE".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };
        pools.insert(pool.address, Arc::new(pool));
        let pools = Arc::new(RwLock::new(pools));

        let engine = ArbitrageEngine::new(pools, 0.01, 0.1); // min_profit=0.01%

        let opps = engine.discover().await;
        // There should be two directions
        assert_eq!(opps.len(), 2);

        let filtered = engine.filter_risk(opps);
        // Should filter by min_profit
        for opp in filtered {
            assert!(opp.profit_percentage > 0.01);
        }
    }

    #[tokio::test]
    async fn test_engine_initialization() {
        let pools = Arc::new(RwLock::new(HashMap::new()));
        let min_profit_percentage = 0.5;
        let max_risk_factor = 0.2;

        let engine = ArbitrageEngine::new(pools, min_profit_percentage, max_risk_factor);

        assert_eq!(engine.min_profit_percentage, min_profit_percentage);
        assert_eq!(engine.max_risk_factor, max_risk_factor);
    }

    #[tokio::test]
    async fn test_opportunity_creation() {
        // Create two pools with different prices
        let token_a_mint = Pubkey::new_unique();
        let token_b_mint = Pubkey::new_unique();

        let mut pools = HashMap::new();

        // First pool with 1:2 ratio
        let pool1 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "FAKE/USDC Pool 1".to_string(),
            token_a: PoolToken {
                mint: token_a_mint,
                symbol: "FAKE".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: token_b_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };

        // Second pool with 1:1.9 ratio (creating an arbitrage opportunity)
        let pool2 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "FAKE/USDC Pool 2".to_string(),
            token_a: PoolToken {
                mint: token_a_mint,
                symbol: "FAKE".to_string(),
                decimals: 6,
                reserve: 950_000_000,
            },
            token_b: PoolToken {
                mint: token_b_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 1_900_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Raydium,
        };

        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));
        let pools = Arc::new(RwLock::new(pools));

        let engine = ArbitrageEngine::new(pools, 0.01, 0.1);
        let opportunities = engine.discover().await;

        // Should find opportunities in both directions
        assert!(opportunities.len() > 0);

        // At least one opportunity should be profitable
        let filtered = engine.filter_risk(opportunities);
        assert!(filtered.len() > 0);

        // Check opportunity structure
        if !filtered.is_empty() {
            let opp = &filtered[0];
            assert!(opp.profit_percentage > 0.0);
            assert!(opp.risk_factor <= 0.1);
            assert_eq!(opp.path.len(), 2); // Should have 2 pools in the path
        }
    }

    #[tokio::test]
    async fn test_risk_filtering() {
        let mut pools = HashMap::new();
        let pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "FAKE/USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "FAKE".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };
        pools.insert(pool.address, Arc::new(pool));
        let pools = Arc::new(RwLock::new(pools));

        // Create engine with strict risk parameters
        let engine = ArbitrageEngine::new(pools, 5.0, 0.01); // 5% min profit, 0.01 max risk

        // Create test opportunities with different risk profiles
        let mut opportunities = Vec::new();

        // Low profit, low risk (should be filtered out due to low profit)
        opportunities.push(ArbOpportunity {
            path: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            profit_percentage: 1.0,
            risk_factor: 0.005,
            input_amount: TokenAmount::new(1000, 6),
            expected_output_amount: TokenAmount::new(1010, 6),
            token_mint: Pubkey::new_unique(),
        });

        // High profit, high risk (should be filtered out due to high risk)
        opportunities.push(ArbOpportunity {
            path: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            profit_percentage: 10.0,
            risk_factor: 0.02,
            input_amount: TokenAmount::new(1000, 6),
            expected_output_amount: TokenAmount::new(1100, 6),
            token_mint: Pubkey::new_unique(),
        });

        // High profit, low risk (should pass)
        opportunities.push(ArbOpportunity {
            path: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            profit_percentage: 6.0,
            risk_factor: 0.005,
            input_amount: TokenAmount::new(1000, 6),
            expected_output_amount: TokenAmount::new(1060, 6),
            token_mint: Pubkey::new_unique(),
        });

        let filtered = engine.filter_risk(opportunities);

        // Only one opportunity should pass the filters
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].profit_percentage >= 5.0);
        assert!(filtered[0].risk_factor <= 0.01);
    }
}
