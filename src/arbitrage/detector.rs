use crate::arbitrage::pair_blacklist::PairBlacklist;
use crate::dex::pool::{PoolInfo, TokenAmount};
use crate::metrics::Metrics;
use log::{debug, info};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct ArbitrageOpportunity {
    /// Source pool for the arbitrage opportunity (kept for analysis/debugging)
    #[allow(dead_code)]
    pub source_pool: Arc<PoolInfo>,
    /// Target pool for the arbitrage opportunity (kept for analysis/debugging)
    #[allow(dead_code)]
    pub target_pool: Arc<PoolInfo>,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub profit_percentage: f64,
    pub route: Vec<Pubkey>,
    pub input_amount: TokenAmount,
    pub expected_output: TokenAmount,
}

/// ArbitrageDetector now only includes supported DEX clients
#[derive(Clone)]
pub struct ArbitrageDetector {
    min_profit_threshold: f64,
    pub blacklist: PairBlacklist,
}

impl ArbitrageDetector {
    pub fn new(min_profit_threshold: f64) -> Self {
        Self {
            min_profit_threshold,
            blacklist: PairBlacklist::new(),
        }
    }

    pub fn new_with_blacklist(min_profit_threshold: f64, blacklist: PairBlacklist) -> Self {
        Self {
            min_profit_threshold,
            blacklist,
        }
    }

    /// Find all arbitrage opportunities using on-chain pools
    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // For each pair of pools (src, tgt), scan all possible token pairs using direct price simulation
        for (src_id, src_pool) in pools {
            let tokens = vec![src_pool.token_a.mint, src_pool.token_b.mint];
            for &token_a in &tokens {
                for &token_b in &tokens {
                    if token_a == token_b {
                        continue;
                    }
                    
                    let token_a_str = token_a.to_string();
                    let token_b_str = token_b.to_string();
                    
                    // Skip blacklisted token pairs
                    if self.blacklist.contains(&token_a_str, &token_b_str) {
                        debug!("Skipping blacklisted pair: {} <-> {}", token_a_str, token_b_str);
                        continue;
                    }
                    
                    let test_amount = 1_000_000; // Example input

                    for (tgt_id, tgt_pool) in pools {
                        if src_id == tgt_id {
                            continue;
                        }
                        let matches_tokens = (tgt_pool.token_a.mint == token_a
                            && tgt_pool.token_b.mint == token_b)
                            || (tgt_pool.token_a.mint == token_b
                                && tgt_pool.token_b.mint == token_a);
                        if !matches_tokens {
                            continue;
                        }

                        // AMM simulation for output from source pool
                        let intermediate_amt = crate::dex::pool::calculate_output_amount(
                            src_pool,
                            TokenAmount::new(test_amount, src_pool.token_a.decimals),
                            true,
                        );

                        // AMM simulation for output from target pool
                        let output_amt = crate::dex::pool::calculate_output_amount(
                            tgt_pool,
                            intermediate_amt,
                            true,
                        );

                        // Calculate profit percent
                        let profit = output_amt.to_float() / (test_amount as f64) - 1.0;
                        let profit_percent = profit * 100.0;

                        // Threshold filter
                        if profit_percent > self.min_profit_threshold {
                            debug!(
                                "Checking arbitrage path: {} -> {}",
                                token_a_str, token_b_str
                            );
                            info!(
                                "Arb opp: Pool {} ({}->{}) -> Pool {} (profit {:.2}%)",
                                src_pool.address,
                                token_a_str,
                                token_b_str,
                                tgt_pool.address,
                                profit_percent
                            );
                            metrics.record_arbitrage_opportunity(
                                profit_percent,
                                &token_a_str,
                                &token_b_str,
                                test_amount as f64,
                                output_amt.to_float(),
                            );
                            opportunities.push(ArbitrageOpportunity {
                                source_pool: Arc::clone(src_pool),
                                target_pool: Arc::clone(tgt_pool),
                                token_a,
                                token_b,
                                profit_percentage: profit_percent,
                                route: vec![token_a, token_b],
                                input_amount: TokenAmount::new(
                                    test_amount,
                                    src_pool.token_a.decimals,
                                ),
                                expected_output: output_amt,
                            });
                        }
                    }
                }
            }
        }

        opportunities.sort_by(|a, b| {
            b.profit_percentage
                .partial_cmp(&a.profit_percentage)
                .unwrap()
        });
        info!(
            "Found {} arbitrage opportunities (pure on-chain)",
            opportunities.len()
        );
        opportunities
    }
}
