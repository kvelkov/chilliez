// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{calculate_max_profit, calculate_optimal_input, estimate_price_impact, is_profitable, OpportunityCalculationResult, calculate_transaction_cost},
        fee_manager::{FeeEstimationResult, FeeManager, XYKSlippageModel},
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{calculate_output_amount, DexType, PoolInfo, TokenAmount, calculate_multihop_profit_and_slippage, calculate_rebate},
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    sync::Arc,
};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub source_pool: Arc<PoolInfo>,
    pub target_pool: Arc<PoolInfo>,
    pub profit_percentage: f64,
    pub input_amount: TokenAmount,
    pub expected_output: TokenAmount,
    pub id: String,
    pub estimated_profit_usd: Option<f64>,
    pub input_token_mint: Pubkey,
    pub intermediate_token_mint: Option<Pubkey>,
    pub output_token_mint: Pubkey,
    pub dex_path: Vec<String>,
    pub input_amount_usd: Option<f64>,
    pub output_amount_usd: Option<f64>,
}

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

    pub fn set_min_profit_threshold(&mut self, new_threshold: f64) {
        self.min_profit_threshold = new_threshold;
        info!("ArbitrageDetector min_profit_threshold updated to: {:.4}%", new_threshold * 100.0);
    }

    pub fn get_min_profit_threshold(&self) -> f64 {
        self.min_profit_threshold
    }

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

    pub fn is_permanently_banned(token_a: &str, token_b: &str) -> bool {
        if let Ok(content) = std::fs::read_to_string("banned_pairs_log.csv") {
            for line in content.lines() {
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 3
                    && parts[2] == "permanent"
                    && ((parts[0] == token_a && parts[1] == token_b)
                        || (parts[0] == token_b && parts[1] == token_a))
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_temporarily_banned(token_a: &str, token_b: &str) -> bool {
        if let Ok(content) = std::fs::read_to_string("banned_pairs_log.csv") {
            let now = chrono::Utc::now().timestamp() as u64;
            for line in content.lines() {
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 4 && parts[2] == "temporary" {
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

    #[allow(dead_code)]
    pub fn is_profitable_with_threshold(
        calc_result: &OpportunityCalculationResult,
        fee_result: &FeeEstimationResult,
        min_profit_threshold_pct: f64,
    ) -> bool {
        let net_profit = calc_result.profit - fee_result.total_cost;
        let min_profit_threshold_abs = calc_result.input_amount * (min_profit_threshold_pct / 100.0);

        net_profit > min_profit_threshold_abs
            && calc_result.profit_percentage > (min_profit_threshold_pct / 100.0)
    }

    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics, // Changed to &Metrics from &Arc<Mutex<Metrics>>
    ) -> Result<Vec<ArbitrageOpportunity>, ArbError> {
        let mut opportunities = Vec::new();

        for (src_id, src_pool) in pools {
            let tokens_in_src_pool = [src_pool.token_a.mint, src_pool.token_b.mint];
            for &input_token_mint_for_cycle in &tokens_in_src_pool {
                let intermediate_token_mint_val = if src_pool.token_a.mint == input_token_mint_for_cycle {
                    src_pool.token_b.mint
                } else {
                    src_pool.token_a.mint
                };

                if input_token_mint_for_cycle == intermediate_token_mint_val {
                    continue;
                }

                let input_token_str = input_token_mint_for_cycle.to_string();
                let intermediate_token_str = intermediate_token_mint_val.to_string();

                if Self::is_permanently_banned(&input_token_str, &intermediate_token_str)
                    || Self::is_temporarily_banned(&input_token_str, &intermediate_token_str)
                {
                    debug!(
                        "Skipping banned pair for first hop: {} <-> {}",
                        input_token_str, intermediate_token_str
                    );
                    continue;
                }

                for (tgt_id, tgt_pool) in pools {
                    if src_id == tgt_id {
                        continue;
                    }

                    let tgt_trades_intermediate_to_input = tgt_pool.token_a.mint == intermediate_token_mint_val && tgt_pool.token_b.mint == input_token_mint_for_cycle;
                    let tgt_trades_input_to_intermediate_reverse = tgt_pool.token_b.mint == intermediate_token_mint_val && tgt_pool.token_a.mint == input_token_mint_for_cycle;

                    if !(tgt_trades_intermediate_to_input || tgt_trades_input_to_intermediate_reverse) {
                        continue;
                    }

                    if Self::is_permanently_banned(&intermediate_token_str, &input_token_str)
                        || Self::is_temporarily_banned(&intermediate_token_str, &input_token_str)
                    {
                        debug!(
                            "Skipping banned pair for second hop: {} <-> {}",
                            intermediate_token_str, input_token_str
                        );
                        continue;
                    }

                    let src_swaps_input_for_intermediate = src_pool.token_a.mint == input_token_mint_for_cycle;

                    let input_decimals = if src_swaps_input_for_intermediate {
                        src_pool.token_a.decimals
                    } else {
                        src_pool.token_b.decimals
                    };

                    let max_input_token_amount = TokenAmount::new(1_000_000, input_decimals);

                    let optimal_in = calculate_optimal_input(src_pool, tgt_pool, src_swaps_input_for_intermediate, max_input_token_amount);

                    let price_impact = estimate_price_impact(src_pool, if src_swaps_input_for_intermediate {0} else {1}, optimal_in.clone());

                    let intermediate_amt_received = calculate_output_amount(
                        src_pool,
                        optimal_in.clone(),
                        src_swaps_input_for_intermediate,
                    );

                    let tgt_swaps_intermediate_for_input = tgt_pool.token_a.mint == intermediate_token_mint_val;
                    let final_output_amt = calculate_output_amount(
                        tgt_pool,
                        intermediate_amt_received.clone(),
                        tgt_swaps_intermediate_for_input,
                    );

                    let profit_pct_calc = calculate_max_profit(src_pool, tgt_pool, src_swaps_input_for_intermediate, optimal_in.clone());
                    let profit_abs = (final_output_amt.to_float() - optimal_in.to_float()).max(0.0);

                    let token_price_in_sol = 1.0; // Placeholder
                    let transaction_cost_sol = calculate_transaction_cost(1024, 0); // Placeholder

                    // Use self.min_profit_threshold for checking profitability
                    let is_profitable_check = is_profitable(
                        profit_abs, // Profit in token terms
                        token_price_in_sol, // Price of token in SOL (or USD / SOL price)
                        transaction_cost_sol, // Cost in SOL
                        self.min_profit_threshold, // This should be a threshold in SOL or normalized
                    );

                    // For the is_profitable function, the min_profit_threshold should be in the same unit as (profit_in_token * token_price_in_sol - transaction_cost_in_sol)
                    // If self.min_profit_threshold is a percentage, it needs conversion.
                    // Let's assume for now `is_profitable` expects the threshold in SOL terms.
                    // A more direct check against self.min_profit_threshold (which is a percentage):
                    let should_execute = profit_pct_calc > self.min_profit_threshold && (profit_abs * token_price_in_sol > transaction_cost_sol);


                    if should_execute {
                        info!(
                            "Arb opp: Pool {} ({}->{}) -> Pool {} ({}->{}) (profit {:.2}%, impact {:.2}%)",
                            src_pool.address, input_token_str, intermediate_token_str,
                            tgt_pool.address, intermediate_token_str, input_token_mint_for_cycle.to_string(),
                            profit_pct_calc * 100.0,
                            price_impact * 100.0
                        );
                    }

                    let current_opportunity = ArbitrageOpportunity {
                        source_pool: Arc::clone(src_pool),
                        target_pool: Arc::clone(tgt_pool),
                        profit_percentage: profit_pct_calc * 100.0, // Storing as percentage
                        input_amount: optimal_in.clone(),
                        expected_output: final_output_amt.clone(),
                        id: format!("{}-{}", src_pool.address, tgt_pool.address),
                        estimated_profit_usd: Some(profit_abs * token_price_in_sol), // Example placeholder for USD
                        input_token_mint: input_token_mint_for_cycle,
                        intermediate_token_mint: Some(intermediate_token_mint_val),
                        output_token_mint: input_token_mint_for_cycle,
                        dex_path: vec![format!("{:?}", src_pool.dex_type), format!("{:?}", tgt_pool.dex_type)],
                        input_amount_usd: Some(optimal_in.to_float() * token_price_in_sol), // Placeholder
                        output_amount_usd: Some(final_output_amt.to_float() * token_price_in_sol), // Placeholder
                    };

                    // Logging to file
                    if let Some(log_file_mutex) = metrics.get_log_file() {
                        let mut file_guard = log_file_mutex.lock().await; // Tokio Mutex
                        let log_entry_for_file = format!(
                            "{}Z,{:.6},{:.6},{},{},{},{},{:.2},{:.2}\n",
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                            current_opportunity.estimated_profit_usd.unwrap_or(0.0),
                            current_opportunity.profit_percentage / 100.0,
                            current_opportunity.input_token_mint,
                            current_opportunity.intermediate_token_mint.map_or_else(|| "N/A".to_string(), |pk| pk.to_string()),
                            current_opportunity.output_token_mint,
                            current_opportunity.dex_path.join("->"),
                            current_opportunity.input_amount_usd.unwrap_or(0.0),
                            current_opportunity.output_amount_usd.unwrap_or(0.0)
                        );
                        if let Err(e) = file_guard.write_all(log_entry_for_file.as_bytes()).await {
                            // Don't return error for logging failure, just log it
                            error!("Failed to write to metrics log file (async): {}", e);
                        }
                    }

                    // Profitability check based on the detector's threshold
                    if profit_pct_calc > self.min_profit_threshold {
                        opportunities.push(current_opportunity);
                    }
                }
            }
        }

        opportunities.sort_by(|a, b| {
            b.profit_percentage
                .partial_cmp(&a.profit_percentage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(
            "Found {} direct arbitrage opportunities above {:.4}% threshold",
            opportunities.len(),
            self.min_profit_threshold * 100.0
        );
        Ok(opportunities)
    }

    pub async fn find_all_multihop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics, // Changed to &Metrics
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Added Result
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        if pool_vec.len() < 2 { return Ok(opportunities); } // Return Ok

        // Simplified 3-hop logic for demonstration
        if pool_vec.len() >= 3 {
            for i in 0..pool_vec.len() {
                for j in 0..pool_vec.len() {
                    if i == j { continue; }
                    for k in 0..pool_vec.len() {
                        if k == i || k == j { continue; }

                        let p1 = pool_vec[i].as_ref();
                        let p2 = pool_vec[j].as_ref();
                        let p3 = pool_vec[k].as_ref();

                        // Try all combinations of (TokenA -> TokenB -> TokenC -> TokenA)
                        for start_token_mint_p1_hop1 in [p1.token_a.mint, p1.token_b.mint] {
                            let mid_token_mint_p1_hop1 = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_b.mint } else { p1.token_a.mint };

                            if !(p2.token_a.mint == mid_token_mint_p1_hop1 || p2.token_b.mint == mid_token_mint_p1_hop1) { continue; }
                            let mid_token_mint_p2_hop2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { p2.token_b.mint } else { p2.token_a.mint };

                            if !((p3.token_a.mint == mid_token_mint_p2_hop2 && p3.token_b.mint == start_token_mint_p1_hop1) ||
                                 (p3.token_b.mint == mid_token_mint_p2_hop2 && p3.token_a.mint == start_token_mint_p1_hop1)) { continue; }

                            let s_tk_str = start_token_mint_p1_hop1.to_string();
                            let m1_tk_str = mid_token_mint_p1_hop1.to_string();
                            let m2_tk_str = mid_token_mint_p2_hop2.to_string();

                            if Self::is_permanently_banned(&s_tk_str, &m1_tk_str) || Self::is_temporarily_banned(&s_tk_str, &m1_tk_str) ||
                               Self::is_permanently_banned(&m1_tk_str, &m2_tk_str) || Self::is_temporarily_banned(&m1_tk_str, &m2_tk_str) ||
                               Self::is_permanently_banned(&m2_tk_str, &s_tk_str) || Self::is_temporarily_banned(&m2_tk_str, &s_tk_str) {
                                continue;
                            }

                            let pools_arr_ref: Vec<&PoolInfo> = vec![p1, p2, p3];
                            let dir1 = p1.token_a.mint == start_token_mint_p1_hop1;
                            let dir2 = p2.token_a.mint == mid_token_mint_p1_hop1;
                            let dir3 = p3.token_a.mint == mid_token_mint_p2_hop2;
                            let directions = vec![dir1, dir2, dir3];

                            let last_fee_data = vec![(None, None, None); 3]; // Placeholder
                            let input_amount_float = 1000.0; // Example input

                            let input_decimals = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_a.decimals } else { p1.token_b.decimals };
                            let input_token_amount_u64 = (input_amount_float * 10f64.powi(input_decimals as i32)) as u64;

                            let (calculated_profit_float, total_slippage, total_fee) =
                                calculate_multihop_profit_and_slippage(
                                    &pools_arr_ref,
                                    input_amount_float,
                                    &directions,
                                    &last_fee_data,
                                );

                            let amounts_for_fee_est = vec![
                                TokenAmount::new(input_token_amount_u64, input_decimals),
                                TokenAmount::new(0, if mid_token_mint_p1_hop1 == p2.token_a.mint {p2.token_a.decimals} else {p2.token_b.decimals}),
                                TokenAmount::new(0, if mid_token_mint_p2_hop2 == p3.token_a.mint {p3.token_a.decimals} else {p3.token_b.decimals}),
                            ];
                            let slippage_model = XYKSlippageModel;
                            let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
                                &pools_arr_ref, &amounts_for_fee_est, &directions, &last_fee_data, &slippage_model,
                            );
                            let abnormal_fee = FeeManager::is_fee_abnormal(
                                p1.fee_numerator, p1.fee_denominator,
                                FeeManager::get_last_fee_for_pool(p1).map_or(p1.fee_numerator, |f|f.0),
                                FeeManager::get_last_fee_for_pool(p1).map_or(p1.fee_denominator, |f|f.1), 1.5);
                            FeeManager::record_fee_observation(p1, p1.fee_numerator, p1.fee_denominator); // Example

                            let risk_score = if abnormal_fee || fee_breakdown.expected_slippage > 0.05 { Some(1.0) } else { Some(0.0) }; // Example risk
                            let rebate = calculate_rebate(&pools_arr_ref, &[]); // Placeholder

                            let initial_ta = TokenAmount::new(input_token_amount_u64, input_decimals);
                            let hop1_out_ta = calculate_output_amount(p1, initial_ta.clone(), dir1);
                            let hop2_out_ta = calculate_output_amount(p2, hop1_out_ta.clone(), dir2);
                            let hop3_out_ta = calculate_output_amount(p3, hop2_out_ta.clone(), dir3);

                            let final_output_float = hop3_out_ta.to_float();
                            let net_profit_float = final_output_float - input_amount_float; // This is raw profit, not considering fees from fee_breakdown yet
                            let profit_pct = if input_amount_float > 0.0 { (calculated_profit_float / input_amount_float) * 100.0 } else { 0.0 }; // Using profit from multihop calc

                            let hops_data = vec![
                                ArbHop { dex: p1.dex_type, pool: p1.address, input_token: s_tk_str.clone(), output_token: m1_tk_str.clone(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                                ArbHop { dex: p2.dex_type, pool: p2.address, input_token: m1_tk_str.clone(), output_token: m2_tk_str.clone(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                                ArbHop { dex: p3.dex_type, pool: p3.address, input_token: m2_tk_str.clone(), output_token: s_tk_str.clone(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                            ];
                            let notes = Some(format!(
                                "3-hop: raw_profit {:.5}, calc_profit {:.5}, fee {:.5}, slip {:.5}, gas {}, rebate {:.5}, abnormal_fee {}, expl: {}",
                                net_profit_float, calculated_profit_float, total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                            ));
                            let opp_id = format!("3hop-{}-{}-{}-{}", s_tk_str, p1.address, p2.address, p3.address);


                            let opp = MultiHopArbOpportunity {
                                id: opp_id.clone(),
                                hops: hops_data,
                                total_profit: calculated_profit_float, // Use profit from multihop calculation
                                profit_pct,
                                input_token: s_tk_str.clone(),
                                output_token: s_tk_str.clone(),
                                input_amount: input_amount_float,
                                expected_output: input_amount_float + calculated_profit_float, // Expected output considering profit
                                dex_path: vec![p1.dex_type, p2.dex_type, p3.dex_type],
                                pool_path: vec![p1.address, p2.address, p3.address],
                                risk_score,
                                notes,
                                estimated_profit_usd: Some(calculated_profit_float * 1.0), // Placeholder
                                input_amount_usd: Some(input_amount_float * 1.0),    // Placeholder
                                intermediate_tokens: vec![m1_tk_str.clone(), m2_tk_str.clone()],
                                output_amount_usd: Some((input_amount_float + calculated_profit_float) * 1.0), // Placeholder
                            };

                            if !abnormal_fee && opp.is_profitable(self.min_profit_threshold) {
                                let dex_path_strings: Vec<String> = opp.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                                if let Err(e) = metrics.record_opportunity_detected(
                                    &s_tk_str,
                                    &s_tk_str, // For cyclic, output is same as input token symbol for logging
                                    opp.profit_pct,
                                    opp.estimated_profit_usd,
                                    opp.input_amount_usd,
                                    dex_path_strings,
                                ) {
                                    error!("Failed to record 3-hop opportunity: {:?}", e);
                                }
                                info!("[ANALYTICS] 3-Hop Opp: ID {} Profit {:.2}%, Slippage: {:.2}%, Fee: {:.5}", opp.id, opp.profit_pct, total_slippage * 100.0, total_fee);
                                opportunities.push(opp);
                            }
                        }
                    }
                }
            }
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!(
            "Found {} multi-hop arbitrage opportunities above {:.4}% threshold",
            opportunities.len(),
            self.min_profit_threshold * 100.0
        );
        Ok(opportunities) // Return Ok
    }

    pub async fn find_all_multihop_opportunities_with_risk(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics, // Changed to &Metrics
        max_slippage_pct: f64,
        tx_fee_lamports: u64,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> { // Added Result
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        if pool_vec.len() < 2 { return Ok(opportunities); } // Return Ok

        if pool_vec.len() >= 3 {
            for i in 0..pool_vec.len() {
                for j in 0..pool_vec.len() {
                    if i == j { continue; }
                    for k in 0..pool_vec.len() {
                        if k == i || k == j { continue; }

                        let p1 = pool_vec[i].as_ref();
                        let p2 = pool_vec[j].as_ref();
                        let p3 = pool_vec[k].as_ref();

                        for start_token_mint_p1_hop1 in [p1.token_a.mint, p1.token_b.mint] {
                            let mid_token_mint_p1_hop1 = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_b.mint } else { p1.token_a.mint };
                            if !(p2.token_a.mint == mid_token_mint_p1_hop1 || p2.token_b.mint == mid_token_mint_p1_hop1) { continue; }
                            let mid_token_mint_p2_hop2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { p2.token_b.mint } else { p2.token_a.mint };
                            if !((p3.token_a.mint == mid_token_mint_p2_hop2 && p3.token_b.mint == start_token_mint_p1_hop1) ||
                                 (p3.token_b.mint == mid_token_mint_p2_hop2 && p3.token_a.mint == start_token_mint_p1_hop1)) { continue; }

                            let s_tk_str = start_token_mint_p1_hop1.to_string();
                            let m1_tk_str = mid_token_mint_p1_hop1.to_string();
                            let m2_tk_str = mid_token_mint_p2_hop2.to_string();

                            if Self::is_permanently_banned(&s_tk_str, &m1_tk_str) || Self::is_temporarily_banned(&s_tk_str, &m1_tk_str) ||
                               Self::is_permanently_banned(&m1_tk_str, &m2_tk_str) || Self::is_temporarily_banned(&m1_tk_str, &m2_tk_str) ||
                               Self::is_permanently_banned(&m2_tk_str, &s_tk_str) || Self::is_temporarily_banned(&m2_tk_str, &s_tk_str) {
                                continue;
                            }

                            let pools_arr_ref: Vec<&PoolInfo> = vec![p1, p2, p3];
                            let dir1 = p1.token_a.mint == start_token_mint_p1_hop1;
                            let dir2 = p2.token_a.mint == mid_token_mint_p1_hop1;
                            let dir3 = p3.token_a.mint == mid_token_mint_p2_hop2;
                            let directions = vec![dir1, dir2, dir3];

                            let last_fee_data = vec![(None, None, None); 3];
                            let input_amount_float = 1000.0; // Example input
                            let input_decimals = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_a.decimals } else { p1.token_b.decimals };
                            let input_token_amount_u64 = (input_amount_float * 10f64.powi(input_decimals as i32)) as u64;

                            let (calculated_profit_float, total_slippage, total_fee) =
                                calculate_multihop_profit_and_slippage(
                                    &pools_arr_ref, input_amount_float, &directions, &last_fee_data,
                                );

                            let amounts_for_fee_est = vec![
                                TokenAmount::new(input_token_amount_u64, input_decimals),
                                TokenAmount::new(0, if mid_token_mint_p1_hop1 == p2.token_a.mint {p2.token_a.decimals} else {p2.token_b.decimals}),
                                TokenAmount::new(0, if mid_token_mint_p2_hop2 == p3.token_a.mint {p3.token_a.decimals} else {p3.token_b.decimals}),
                            ];
                            let slippage_model = XYKSlippageModel;
                            let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
                                &pools_arr_ref, &amounts_for_fee_est, &directions, &last_fee_data, &slippage_model,
                            );
                            let abnormal_fee = FeeManager::is_fee_abnormal(
                                p1.fee_numerator, p1.fee_denominator,
                                FeeManager::get_last_fee_for_pool(p1).map_or(p1.fee_numerator, |f|f.0),
                                FeeManager::get_last_fee_for_pool(p1).map_or(p1.fee_denominator, |f|f.1), 1.5);
                            FeeManager::record_fee_observation(p1, p1.fee_numerator, p1.fee_denominator);

                            let risk_score = if abnormal_fee || fee_breakdown.expected_slippage > 0.05 { Some(1.0) } else { Some(0.0) };
                            let rebate = calculate_rebate(&pools_arr_ref, &[]);

                            let initial_ta = TokenAmount::new(input_token_amount_u64, input_decimals);
                            let hop1_out_ta = calculate_output_amount(p1, initial_ta.clone(), dir1);
                            let hop2_out_ta = calculate_output_amount(p2, hop1_out_ta.clone(), dir2);
                            let hop3_out_ta = calculate_output_amount(p3, hop2_out_ta.clone(), dir3);

                            let final_output_float = hop3_out_ta.to_float();
                            let net_profit_float = final_output_float - input_amount_float;
                            let profit_pct = if input_amount_float > 0.0 { (calculated_profit_float / input_amount_float) * 100.0 } else { 0.0 };

                            let hops_data = vec![
                                ArbHop { dex: p1.dex_type, pool: p1.address, input_token: s_tk_str.clone(), output_token: m1_tk_str.clone(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                                ArbHop { dex: p2.dex_type, pool: p2.address, input_token: m1_tk_str.clone(), output_token: m2_tk_str.clone(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                                ArbHop { dex: p3.dex_type, pool: p3.address, input_token: m2_tk_str.clone(), output_token: s_tk_str.clone(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                            ];
                            let notes = Some(format!(
                                "3-hop risk: fee {:.5}, slip {:.5}, gas {}, rebate {:.5}, abnormal_fee {}, expl: {}",
                                total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                            ));
                            let opp_id = format!("3hop-risk-{}-{}-{}-{}", s_tk_str, p1.address, p2.address, p3.address);

                            let opp = MultiHopArbOpportunity {
                                id: opp_id.clone(),
                                hops: hops_data,
                                total_profit: calculated_profit_float,
                                profit_pct,
                                input_token: s_tk_str.clone(),
                                output_token: s_tk_str.clone(),
                                input_amount: input_amount_float,
                                expected_output: input_amount_float + calculated_profit_float,
                                dex_path: vec![p1.dex_type, p2.dex_type, p3.dex_type],
                                pool_path: vec![p1.address, p2.address, p3.address],
                                risk_score,
                                notes,
                                estimated_profit_usd: Some(calculated_profit_float * 1.0),
                                input_amount_usd: Some(input_amount_float * 1.0),
                                intermediate_tokens: vec![m1_tk_str.clone(), m2_tk_str.clone()],
                                output_amount_usd: Some((input_amount_float + calculated_profit_float) * 1.0),
                            };

                            if !abnormal_fee
                                && fee_breakdown.expected_slippage <= max_slippage_pct
                                && fee_breakdown.gas_cost <= tx_fee_lamports
                                && opp.is_profitable(self.min_profit_threshold)
                            {
                                let dex_path_strings: Vec<String> = opp.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                                if let Err(e) = metrics.record_opportunity_detected(
                                    &s_tk_str,
                                    &s_tk_str, // For cyclic, output is same as input token symbol for logging
                                    opp.profit_pct,
                                    opp.estimated_profit_usd,
                                    opp.input_amount_usd,
                                    dex_path_strings, // Changed from Option<Vec<String>> to Vec<String>
                                ) {
                                    error!("Failed to record 3-hop opportunity with risk: {:?}", e);
                                }
                                info!("[ANALYTICS] 3-Hop Opp (Risk Checked): ID {} Profit {:.2}%, Slippage: {:.2}%, Fee: {:.5}", opp.id, opp.profit_pct, total_slippage*100.0, total_fee);
                                opportunities.push(opp);
                            }
                        }
                    }
                }
            }
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!(
            "Found {} multi-hop arbitrage opportunities (with risk) above {:.4}% threshold, slippage {:.2}%, fee {} lamports",
            opportunities.len(),
            self.min_profit_threshold * 100.0,
            max_slippage_pct * 100.0,
            tx_fee_lamports
        );
        Ok(opportunities) // Return Ok
    }

    #[allow(dead_code)]
    pub fn is_profitable(
        calc_result: &OpportunityCalculationResult,
        fee_result: &FeeEstimationResult,
    ) -> bool {
        let net_profit = calc_result.profit - fee_result.total_cost;
        // Ensure min_profit_threshold is correctly interpreted (e.g., as absolute value or percentage of input)
        // Assuming calc_result.input_amount is in the same unit as net_profit (e.g. token amount)
        // And 0.005 is a percentage (0.5%)
        let min_profit_abs = calc_result.input_amount * (0.005 / 100.0) ; // If 0.005 is bps or needs division
        // If 0.005 is already the fractional profit (e.g. 0.5% = 0.005)
        // let min_profit_abs = calc_result.input_amount * 0.005;

        net_profit > min_profit_abs // Check absolute profit
            // && calc_result.profit_percentage > 0.005 // Check percentage profit
    }
}

// These structs and functions (Peer, generate_id, Point) seem unused in this file
// and might be remnants or for a different context.
// Consider removing them if they are not needed for ArbitrageDetector.
/*
struct Peer {
    pub address: String,
    // ... other peer information
}

fn generate_id(p1: &Peer, p2: &Peer, p3: &Peer) -> String {
    // ...existing code...
    let id = format!("{}-{}-{}", p1.address, p2.address, p3.address);
    // ...existing code...
    id
}

struct Point {
    pub address: String, // Or whatever type address is
    // ... other fields
}

// ... or if address is a method
impl Point {
    pub fn address(&self) -> &String { // Or &str, or String
        &self.address_field // Assuming an internal field
    }
}
*/