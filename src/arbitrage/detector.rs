use crate::arbitrage::calculator::{
    calculate_max_profit, calculate_multihop_profit_and_slippage, calculate_optimal_input,
    calculate_rebate, calculate_transaction_cost, estimate_price_impact, is_profitable,
};
use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
use crate::metrics::Metrics;
use crate::utils::{PoolInfo, TokenAmount};
use log::{debug, info};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;

#[derive(Clone)]
pub struct ArbitrageOpportunity {
    pub source_pool: Arc<PoolInfo>,
    pub target_pool: Arc<PoolInfo>,
    pub profit_percentage: f64,
    pub input_amount: TokenAmount,
    pub expected_output: TokenAmount,
}

/// ArbitrageDetector now only includes supported DEX clients
#[derive(Clone)]
pub struct ArbitrageDetector {
    min_profit_threshold: f64,
}

#[allow(dead_code)]
impl ArbitrageDetector {
    pub fn new(min_profit_threshold: f64) -> Self {
        Self {
            min_profit_threshold,
        }
    }

    /// Log a banned pair (permanent or temporary) to CSV for review.
    pub fn log_banned_pair(token_a: &str, token_b: &str, ban_type: &str, reason: &str) {
        let log_entry = format!("{},{},{},{}\n", token_a, token_b, ban_type, reason);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("banned_pairs_log.csv")
            .expect("Cannot open ban log file");
        file.write_all(log_entry.as_bytes())
            .expect("Failed to write ban log");
    }

    /// Check if a pair is permanently banned (CSV-based, fast lookup for small sets).
    pub fn is_permanently_banned(token_a: &str, token_b: &str) -> bool {
        // TODO: For large sets, use a HashSet loaded at startup.
        if let Ok(content) = std::fs::read_to_string("banned_pairs_log.csv") {
            for line in content.lines() {
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 3 && parts[2] == "permanent" {
                    if (parts[0] == token_a && parts[1] == token_b)
                        || (parts[0] == token_b && parts[1] == token_a)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a pair is temporarily banned (CSV-based, checks expiry).
    pub fn is_temporarily_banned(token_a: &str, token_b: &str) -> bool {
        if let Ok(content) = std::fs::read_to_string("banned_pairs_log.csv") {
            let now = chrono::Utc::now().timestamp() as u64;
            for line in content.lines() {
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 4 && parts[2] == "temporary" {
                    // parts[3] = reason, which can include expiry timestamp
                    if let Some(expiry_str) = parts.get(3) {
                        if let Ok(expiry) = expiry_str.parse::<u64>() {
                            if ((parts[0] == token_a && parts[1] == token_b)
                                || (parts[0] == token_b && parts[1] == token_a))
                                && expiry > now
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Find all arbitrage opportunities using robust profit/risk calculations
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
                    if Self::is_permanently_banned(&token_a_str, &token_b_str) {
                        debug!(
                            "Skipping permanently blacklisted pair: {} <-> {}",
                            token_a_str, token_b_str
                        );
                        continue;
                    }

                    if Self::is_temporarily_banned(&token_a_str, &token_b_str) {
                        debug!(
                            "Skipping temporarily blacklisted pair: {} <-> {}",
                            token_a_str, token_b_str
                        );
                        continue;
                    }

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

                        // Use calculator for optimal input sizing (safeguards larger/slippy arbs)
                        let max_input = TokenAmount::new(1_000_000, src_pool.token_a.decimals); // Example
                                                                                                // Fix: move optimal_in only once, then clone as needed for calculations
                        let optimal_in =
                            calculate_optimal_input(src_pool, tgt_pool, true, max_input);
                        let price_impact = estimate_price_impact(src_pool, 0, optimal_in.clone());
                        let intermediate_amt = crate::dex::pool::calculate_output_amount(
                            src_pool,
                            optimal_in.clone(),
                            true,
                        );
                        let output_amt = crate::dex::pool::calculate_output_amount(
                            tgt_pool,
                            intermediate_amt.clone(),
                            true,
                        );
                        let profit_pct =
                            calculate_max_profit(src_pool, tgt_pool, true, optimal_in.clone());
                        let profit = (output_amt.to_float() - optimal_in.to_float()).max(0.0);

                        // Assume 1:1 value vs. SOL for simplification or fetch from oracle
                        let token_price_in_sol = 1.0;

                        // Model transaction cost (real value would fetch tx size, fee config)
                        let transaction_cost = calculate_transaction_cost(1024, 0);

                        // Should we take this trade? Run canonical logic
                        let should_execute = is_profitable(
                            profit,
                            token_price_in_sol,
                            transaction_cost,
                            self.min_profit_threshold,
                        );

                        debug!(
                            "Checking arbitrage path: {} -> {}",
                            token_a_str, token_b_str
                        );

                        if should_execute {
                            info!(
                                "Arb opp: Pool {} ({}->{}) -> Pool {} (profit {:.2}%, impact {:.2}%)",
                                src_pool.address,
                                token_a_str,
                                token_b_str,
                                tgt_pool.address,
                                profit_pct * 100.0,
                                price_impact * 100.0
                            );
                        }

                        // Record opportunity to metrics/logging
                        metrics.record_arbitrage_opportunity(
                            profit_pct * 100.0,
                            &token_a_str,
                            &token_b_str,
                            optimal_in.to_float(),
                            output_amt.to_float(),
                        );

                        // Save opportunity result with all calculator details
                        opportunities.push(ArbitrageOpportunity {
                            source_pool: Arc::clone(src_pool),
                            target_pool: Arc::clone(tgt_pool),
                            profit_percentage: profit_pct * 100.0,
                            input_amount: optimal_in,
                            expected_output: output_amt,
                        });
                    }
                }
            }
        }

        // Sort best-to-worst (profitable, non-profitable at end)
        opportunities.sort_by(|a, b| {
            b.profit_percentage
                .partial_cmp(&a.profit_percentage)
                .unwrap()
        });
        info!(
            "Found {} arbitrage opportunities (calc-enhanced, pure on-chain)",
            opportunities.len()
        );
        opportunities
    }

    /// Find all multi-hop, cross-DEX arbitrage opportunities (2- and 3-hop supported)
    pub async fn find_all_multihop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        // 2-hop and 3-hop paths (TODO: generalize to N-hop)
        for (i, pool1) in pool_vec.iter().enumerate() {
            for (j, pool2) in pool_vec.iter().enumerate() {
                if i == j {
                    continue;
                }
                // Try all token directions for pool1 -> pool2
                for &start_token in &[pool1.token_a.mint, pool1.token_b.mint] {
                    for &mid_token in &[pool2.token_a.mint, pool2.token_b.mint] {
                        if start_token == mid_token {
                            continue;
                        }
                        // Find a 3rd pool for 3-hop cycles
                        for (k, pool3) in pool_vec.iter().enumerate() {
                            if k == i || k == j {
                                continue;
                            }
                            for &end_token in &[pool3.token_a.mint, pool3.token_b.mint] {
                                if end_token != start_token {
                                    continue;
                                }
                                // Check blacklist for any hop
                                let s = start_token.to_string();
                                let m = mid_token.to_string();
                                let e = end_token.to_string();
                                if Self::is_permanently_banned(&s, &m)
                                    || Self::is_permanently_banned(&m, &e)
                                    || Self::is_permanently_banned(&e, &s)
                                {
                                    continue;
                                }

                                if Self::is_temporarily_banned(&s, &m)
                                    || Self::is_temporarily_banned(&m, &e)
                                    || Self::is_temporarily_banned(&e, &s)
                                {
                                    continue;
                                }
                                // --- Use new calculator for multi-hop analytics ---
                                let pools_arr: Vec<&PoolInfo> = vec![&*pool1, &*pool2, &*pool3];
                                let directions = vec![true, true, true]; // TODO: dynamic direction
                                let last_fee_data = vec![(None, None, None); 3];
                                let input_amount = 1000.0; // TODO: dynamic sizing
                                let (total_profit, total_slippage, total_fee) =
                                    calculate_multihop_profit_and_slippage(
                                        &pools_arr,
                                        input_amount,
                                        &directions,
                                        &last_fee_data,
                                    );
                                // --- FeeManager analytics ---
                                let amounts = vec![
                                    TokenAmount::new(input_amount as u64, pool1.token_a.decimals),
                                    TokenAmount::new(
                                        (input_amount * 0.98) as u64,
                                        pool2.token_a.decimals,
                                    ),
                                    TokenAmount::new(
                                        (input_amount * 0.98 * 0.98) as u64,
                                        pool3.token_a.decimals,
                                    ),
                                ];
                                let slippage_model = XYKSlippageModel;
                                let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
                                    &pools_arr,
                                    &amounts,
                                    &directions,
                                    &last_fee_data,
                                    &slippage_model,
                                );
                                let abnormal_fee = FeeManager::is_fee_abnormal(
                                    pool1.fee_numerator,
                                    pool1.fee_denominator,
                                    FeeManager::get_last_fee_for_pool(pool1)
                                        .map_or(pool1.fee_numerator, |f| f.0),
                                    FeeManager::get_last_fee_for_pool(pool1)
                                        .map_or(pool1.fee_denominator, |f| f.1),
                                    1.5,
                                );
                                FeeManager::record_fee_observation(
                                    pool1,
                                    pool1.fee_numerator,
                                    pool1.fee_denominator,
                                );
                                let risk_score =
                                    if abnormal_fee || fee_breakdown.expected_slippage > 0.05 {
                                        Some(1.0)
                                    } else {
                                        Some(0.0)
                                    };
                                let rebate = calculate_rebate(&pools_arr, &[]);
                                let profit_pct = if input_amount > 0.0 {
                                    total_profit / input_amount * 100.0
                                } else {
                                    0.0
                                };
                                let hop1_out = input_amount * 0.98; // placeholder for swap math
                                let hop2_out = hop1_out * 0.98;
                                let hop3_out = hop2_out * 0.98;
                                let hops = vec![
                                    ArbHop {
                                        dex: pool1.dex_type,
                                        pool: pool1.address,
                                        input_token: s.clone(),
                                        output_token: m.clone(),
                                        input_amount,
                                        expected_output: hop1_out,
                                    },
                                    ArbHop {
                                        dex: pool2.dex_type,
                                        pool: pool2.address,
                                        input_token: m.clone(),
                                        output_token: e.clone(),
                                        input_amount: hop1_out,
                                        expected_output: hop2_out,
                                    },
                                    ArbHop {
                                        dex: pool3.dex_type,
                                        pool: pool3.address,
                                        input_token: e.clone(),
                                        output_token: s.clone(),
                                        input_amount: hop2_out,
                                        expected_output: hop3_out,
                                    },
                                ];
                                let notes = Some(format!(
                                    "3-hop cycle (fee: {:.5}, slippage: {:.5}, gas: {}, rebate: {:.5}, abnormal_fee: {}, explanation: {})",
                                    total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                                ));
                                let opp = MultiHopArbOpportunity {
                                    hops,
                                    total_profit,
                                    profit_pct,
                                    input_token: s.clone(),
                                    output_token: s.clone(),
                                    input_amount,
                                    expected_output: hop3_out,
                                    dex_path: vec![pool1.dex_type, pool2.dex_type, pool3.dex_type],
                                    pool_path: vec![pool1.address, pool2.address, pool3.address],
                                    risk_score,
                                    notes,
                                };
                                if !abnormal_fee && opp.is_profitable(self.min_profit_threshold) {
                                    metrics.record_arbitrage_opportunity(
                                        profit_pct,
                                        &s,
                                        &s,
                                        input_amount,
                                        hop3_out,
                                    );
                                    info!("[ANALYTICS] Fee: {:.5}, Slippage: {:.5}, Gas: {}, Abnormal Fee: {}, Risk: {:?}, Notes: {}", total_fee, total_slippage, fee_breakdown.gas_cost, abnormal_fee, risk_score, opp.notes.as_deref().unwrap_or(""));
                                    opportunities.push(opp);
                                }
                            }
                        }
                    }
                }
            }
        }
        info!(
            "Found {} multi-hop arbitrage opportunities",
            opportunities.len()
        );
        opportunities
    }

    /// Find all multi-hop, cross-DEX arbitrage opportunities (2- and 3-hop supported), with risk parameters
    pub async fn find_all_multihop_opportunities_with_risk(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
        max_slippage: f64,
        tx_fee_lamports: u64,
    ) -> Vec<MultiHopArbOpportunity> {
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        // 2-hop and 3-hop paths (TODO: generalize to N-hop)
        for (i, pool1) in pool_vec.iter().enumerate() {
            for (j, pool2) in pool_vec.iter().enumerate() {
                if i == j {
                    continue;
                }
                for &start_token in &[pool1.token_a.mint, pool1.token_b.mint] {
                    for &mid_token in &[pool2.token_a.mint, pool2.token_b.mint] {
                        if start_token == mid_token {
                            continue;
                        }
                        for (k, pool3) in pool_vec.iter().enumerate() {
                            if k == i || k == j {
                                continue;
                            }
                            for &end_token in &[pool3.token_a.mint, pool3.token_b.mint] {
                                if end_token != start_token {
                                    continue;
                                }
                                let s = start_token.to_string();
                                let m = mid_token.to_string();
                                let e = end_token.to_string();
                                if Self::is_permanently_banned(&s, &m)
                                    || Self::is_permanently_banned(&m, &e)
                                    || Self::is_permanently_banned(&e, &s)
                                {
                                    continue;
                                }
                                if Self::is_temporarily_banned(&s, &m)
                                    || Self::is_temporarily_banned(&m, &e)
                                    || Self::is_temporarily_banned(&e, &s)
                                {
                                    continue;
                                }
                                let pools_arr: Vec<&PoolInfo> = vec![&*pool1, &*pool2, &*pool3];
                                let directions = vec![true, true, true];
                                let last_fee_data = vec![(None, None, None); 3];
                                let input_amount = 1000.0;
                                let (total_profit, total_slippage, total_fee) =
                                    calculate_multihop_profit_and_slippage(
                                        &pools_arr,
                                        input_amount,
                                        &directions,
                                        &last_fee_data,
                                    );
                                let amounts = vec![
                                    TokenAmount::new(input_amount as u64, pool1.token_a.decimals),
                                    TokenAmount::new(
                                        (input_amount * 0.98) as u64,
                                        pool2.token_a.decimals,
                                    ),
                                    TokenAmount::new(
                                        (input_amount * 0.98 * 0.98) as u64,
                                        pool3.token_a.decimals,
                                    ),
                                ];
                                let slippage_model = XYKSlippageModel;
                                let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
                                    &pools_arr,
                                    &amounts,
                                    &directions,
                                    &last_fee_data,
                                    &slippage_model,
                                );
                                let abnormal_fee = FeeManager::is_fee_abnormal(
                                    pool1.fee_numerator,
                                    pool1.fee_denominator,
                                    FeeManager::get_last_fee_for_pool(pool1)
                                        .map_or(pool1.fee_numerator, |f| f.0),
                                    FeeManager::get_last_fee_for_pool(pool1)
                                        .map_or(pool1.fee_denominator, |f| f.1),
                                    1.5,
                                );
                                FeeManager::record_fee_observation(
                                    pool1,
                                    pool1.fee_numerator,
                                    pool1.fee_denominator,
                                );
                                let risk_score =
                                    if abnormal_fee || fee_breakdown.expected_slippage > 0.05 {
                                        Some(1.0)
                                    } else {
                                        Some(0.0)
                                    };
                                let rebate = calculate_rebate(&pools_arr, &[]);
                                let profit_pct = if input_amount > 0.0 {
                                    total_profit / input_amount * 100.0
                                } else {
                                    0.0
                                };
                                let hop1_out = input_amount * 0.98;
                                let hop2_out = hop1_out * 0.98;
                                let hop3_out = hop2_out * 0.98;
                                let hops = vec![
                                    ArbHop {
                                        dex: pool1.dex_type,
                                        pool: pool1.address,
                                        input_token: s.clone(),
                                        output_token: m.clone(),
                                        input_amount,
                                        expected_output: hop1_out,
                                    },
                                    ArbHop {
                                        dex: pool2.dex_type,
                                        pool: pool2.address,
                                        input_token: m.clone(),
                                        output_token: e.clone(),
                                        input_amount: hop1_out,
                                        expected_output: hop2_out,
                                    },
                                    ArbHop {
                                        dex: pool3.dex_type,
                                        pool: pool3.address,
                                        input_token: e.clone(),
                                        output_token: s.clone(),
                                        input_amount: hop2_out,
                                        expected_output: hop3_out,
                                    },
                                ];
                                let notes = Some(format!(
                                    "3-hop cycle (fee: {:.5}, slippage: {:.5}, gas: {}, rebate: {:.5}, abnormal_fee: {}, explanation: {})",
                                    total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                                ));
                                let opp = MultiHopArbOpportunity {
                                    hops,
                                    total_profit,
                                    profit_pct,
                                    input_token: s.clone(),
                                    output_token: s.clone(),
                                    input_amount,
                                    expected_output: hop3_out,
                                    dex_path: vec![pool1.dex_type, pool2.dex_type, pool3.dex_type],
                                    pool_path: vec![pool1.address, pool2.address, pool3.address],
                                    risk_score,
                                    notes,
                                };
                                if !abnormal_fee
                                    && fee_breakdown.expected_slippage <= max_slippage
                                    && fee_breakdown.gas_cost <= tx_fee_lamports
                                    && opp.is_profitable(self.min_profit_threshold)
                                {
                                    metrics.record_arbitrage_opportunity(
                                        profit_pct,
                                        &s,
                                        &s,
                                        input_amount,
                                        hop3_out,
                                    );
                                    info!(
                                        "[ANALYTICS] Fee: {:.5}, Slippage: {:.5}, Gas: {}, Abnormal Fee: {}, Risk: {:?}, Notes: {}",
                                        total_fee, total_slippage, fee_breakdown.gas_cost, abnormal_fee, risk_score, opp.notes.as_deref().unwrap_or("")
                                    );
                                    opportunities.push(opp);
                                }
                            }
                        }
                    }
                }
            }
        }
        info!(
            "Found {} multi-hop arbitrage opportunities",
            opportunities.len()
        );
        opportunities
    }
}
