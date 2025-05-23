// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{
            calculate_max_profit, calculate_optimal_input, calculate_transaction_cost,
            estimate_price_impact, is_profitable as is_profitable_calc, // aliased to avoid conflict
            OpportunityCalculationResult,
        },
        fee_manager::{FeeEstimationResult, FeeManager, XYKSlippageModel},
        opportunity::{ArbHop, MultiHopArbOpportunity}, // Using the unified struct
    },
    error::ArbError,
    metrics::Metrics,
    utils::{
        calculate_multihop_profit_and_slippage, calculate_output_amount, calculate_rebate,
        DexType, PoolInfo, TokenAmount,
    },
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    sync::Arc,
};
use tokio::io::AsyncWriteExt; // For async file writes

// Removed ArbitrageOpportunity struct, using MultiHopArbOpportunity instead

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
        info!(
            "ArbitrageDetector min_profit_threshold updated to: {:.4}%",
            new_threshold * 100.0
        );
    }

    pub fn get_min_profit_threshold(&self) -> f64 {
        self.min_profit_threshold
    }

    pub fn log_banned_pair(token_a: &str, token_b: &str, ban_type: &str, reason: &str) {
        let log_entry = format!("{},{},{},{}\n", token_a, token_b, ban_type, reason);
        // Consider making this async if it becomes a bottleneck
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open("banned_pairs_log.csv")
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(log_entry.as_bytes()) {
                    error!("Failed to write to ban log file: {}", e);
                }
            }
            Err(e) => {
                error!("Cannot open ban log file: {}", e);
            }
        }
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
        let min_profit_threshold_abs =
            calc_result.input_amount * (min_profit_threshold_pct / 100.0);

        net_profit > min_profit_threshold_abs
            && calc_result.profit_percentage > (min_profit_threshold_pct / 100.0)
    }

    // find_all_opportunities now returns Vec<MultiHopArbOpportunity>
    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();

        for (src_id, src_pool_arc) in pools {
            let src_pool = src_pool_arc.as_ref();
            let tokens_in_src_pool = [src_pool.token_a.mint, src_pool.token_b.mint];
            for &input_token_mint_for_cycle in &tokens_in_src_pool {
                let intermediate_token_mint_val =
                    if src_pool.token_a.mint == input_token_mint_for_cycle {
                        src_pool.token_b.mint
                    } else {
                        src_pool.token_a.mint
                    };

                if input_token_mint_for_cycle == intermediate_token_mint_val {
                    continue;
                }

                let input_token_symbol = if src_pool.token_a.mint == input_token_mint_for_cycle {
                    src_pool.token_a.symbol.clone()
                } else {
                    src_pool.token_b.symbol.clone()
                };
                let intermediate_token_symbol = if src_pool.token_a.mint == intermediate_token_mint_val {
                     src_pool.token_a.symbol.clone()
                } else {
                     src_pool.token_b.symbol.clone()
                };


                if Self::is_permanently_banned(&input_token_symbol, &intermediate_token_symbol)
                    || Self::is_temporarily_banned(&input_token_symbol, &intermediate_token_symbol)
                {
                    debug!(
                        "Skipping banned pair for first hop: {} <-> {}",
                        input_token_symbol, intermediate_token_symbol
                    );
                    continue;
                }

                for (tgt_id, tgt_pool_arc) in pools {
                    if src_id == tgt_id {
                        continue;
                    }
                    let tgt_pool = tgt_pool_arc.as_ref();

                    let tgt_trades_intermediate_to_input = tgt_pool.token_a.mint == intermediate_token_mint_val && tgt_pool.token_b.mint == input_token_mint_for_cycle;
                    let tgt_trades_input_to_intermediate_reverse = tgt_pool.token_b.mint == intermediate_token_mint_val && tgt_pool.token_a.mint == input_token_mint_for_cycle;

                    if !(tgt_trades_intermediate_to_input || tgt_trades_input_to_intermediate_reverse) {
                        continue;
                    }

                     let final_output_token_symbol = if tgt_pool.token_a.mint == input_token_mint_for_cycle { // Assuming cycle ends with input_token_mint_for_cycle
                        tgt_pool.token_a.symbol.clone()
                    } else {
                        tgt_pool.token_b.symbol.clone()
                    };


                    if Self::is_permanently_banned(&intermediate_token_symbol, &final_output_token_symbol)
                        || Self::is_temporarily_banned(&intermediate_token_symbol, &final_output_token_symbol)
                    {
                        debug!(
                            "Skipping banned pair for second hop: {} <-> {}",
                            intermediate_token_symbol, final_output_token_symbol
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

                    let should_execute = is_profitable_calc( // Using aliased function
                        profit_abs,
                        token_price_in_sol,
                        transaction_cost_sol,
                        self.min_profit_threshold, // Assuming threshold is in SOL terms for this function
                    );

                    if should_execute {
                        info!(
                            "Arb opp: Pool {} ({}->{}) -> Pool {} ({}->{}) (profit {:.2}%, impact {:.2}%)",
                            src_pool.address, input_token_symbol, intermediate_token_symbol,
                            tgt_pool.address, intermediate_token_symbol, final_output_token_symbol,
                            profit_pct_calc * 100.0,
                            price_impact * 100.0
                        );
                    }

                    let opp_id = format!(
                        "2hop-{}-{}-{}-{}",
                        input_token_symbol,
                        src_pool.address,
                        tgt_pool.address,
                        chrono::Utc::now().timestamp_millis() // Add timestamp for uniqueness
                    );

                    let current_opportunity = MultiHopArbOpportunity {
                        id: opp_id.clone(),
                        hops: vec![ // Simplified hops for a 2-hop
                            ArbHop {
                                dex: src_pool.dex_type,
                                pool: src_pool.address,
                                input_token: input_token_symbol.clone(),
                                output_token: intermediate_token_symbol.clone(),
                                input_amount: optimal_in.to_float(),
                                expected_output: intermediate_amt_received.to_float(),
                            },
                            ArbHop {
                                dex: tgt_pool.dex_type,
                                pool: tgt_pool.address,
                                input_token: intermediate_token_symbol.clone(),
                                output_token: final_output_token_symbol.clone(), // Should be same as input_token_symbol for cycle
                                input_amount: intermediate_amt_received.to_float(),
                                expected_output: final_output_amt.to_float(),
                            },
                        ],
                        total_profit: profit_abs,
                        profit_pct: profit_pct_calc * 100.0,
                        input_token: input_token_symbol.clone(),
                        output_token: final_output_token_symbol.clone(), // Should match input_token for cyclic
                        input_amount: optimal_in.to_float(),
                        expected_output: final_output_amt.to_float(),
                        dex_path: vec![src_pool.dex_type, tgt_pool.dex_type],
                        pool_path: vec![src_pool.address, tgt_pool.address],
                        risk_score: None,
                        notes: Some("Direct 2-hop opportunity".to_string()),
                        estimated_profit_usd: Some(profit_abs * token_price_in_sol),
                        input_amount_usd: Some(optimal_in.to_float() * token_price_in_sol),
                        intermediate_tokens: vec![intermediate_token_symbol.clone()],
                        output_amount_usd: Some(final_output_amt.to_float() * token_price_in_sol),
                        source_pool: Arc::clone(src_pool_arc),
                        target_pool: Arc::clone(tgt_pool_arc),
                        input_token_mint: input_token_mint_for_cycle,
                        output_token_mint: input_token_mint_for_cycle, // Assuming cyclic
                        intermediate_token_mint: Some(intermediate_token_mint_val),
                    };

                    if let Some(log_file_mutex) = metrics.get_log_file() {
                        let mut file_guard = log_file_mutex.lock().await;
                        let log_entry_for_file = format!(
                             "{}Z,{},{},{},{},{},{:.6},{:.2},{:.2}\n", // Matched metrics call
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                            current_opportunity.id, // opp_identifier
                            current_opportunity.dex_path.iter().map(|d| format!("{:?}",d)).collect::<Vec<_>>().join("->"), // dex_path
                            current_opportunity.input_token, // input_token
                            current_opportunity.output_token, // output_token
                            current_opportunity.input_amount, // input_amount
                            current_opportunity.expected_output, // expected_output_amount
                            current_opportunity.profit_pct, // estimated_profit_pct
                            current_opportunity.estimated_profit_usd.unwrap_or(0.0), // estimated_profit_usd
                        );
                        if let Err(e) = file_guard.write_all(log_entry_for_file.as_bytes()).await {
                            error!("Failed to write to metrics log file (async): {}", e);
                        }
                    }

                    if profit_pct_calc * 100.0 > self.min_profit_threshold { // Ensure min_profit_threshold is percentage
                        opportunities.push(current_opportunity);
                    }
                }
            }
        }

        opportunities.sort_by(|a, b| {
            b.profit_pct
                .partial_cmp(&a.profit_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(
            "Found {} direct arbitrage opportunities above {:.4}% threshold",
            opportunities.len(),
            self.min_profit_threshold // Assuming threshold is stored as percentage value
        );
        Ok(opportunities)
    }
    
    // find_two_hop_opportunities is a specific case of find_all_opportunities
    // For now, let's assume find_all_opportunities covers 2-hop.
    // If a dedicated method is needed, it can be a filtered version of find_all_opportunities.
    pub async fn find_two_hop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // This can call find_all_opportunities and then filter for those that have exactly 2 hops,
        // or be a more specialized implementation. For now, using the general one.
        self.find_all_opportunities(pools, metrics).await
    }

    // find_three_hop_opportunities
    pub async fn find_three_hop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        // This can call find_all_multihop_opportunities and then filter,
        // or be a specific implementation for 3 hops.
        self.find_all_multihop_opportunities(pools, metrics).await
    }


    pub async fn find_all_multihop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        if pool_vec.len() < 2 {
            return Ok(opportunities);
        }

        // Simplified 3-hop logic for demonstration
        if pool_vec.len() >= 3 {
            for i in 0..pool_vec.len() {
                for j in 0..pool_vec.len() {
                    if i == j {
                        continue;
                    }
                    for k in 0..pool_vec.len() {
                        if k == i || k == j {
                            continue;
                        }

                        let p1_arc = &pool_vec[i];
                        let p2_arc = &pool_vec[j];
                        let p3_arc = &pool_vec[k];
                        let p1 = p1_arc.as_ref();
                        let p2 = p2_arc.as_ref();
                        let p3 = p3_arc.as_ref();


                        for start_token_mint_p1_hop1 in [p1.token_a.mint, p1.token_b.mint] {
                            let mid_token_mint_p1_hop1 = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_b.mint } else { p1.token_a.mint };
                            let start_token_symbol_p1 = if start_token_mint_p1_hop1 == p1.token_a.mint { &p1.token_a.symbol } else { &p1.token_b.symbol };
                            let mid_token_symbol_p1 = if mid_token_mint_p1_hop1 == p1.token_a.mint { &p1.token_a.symbol } else { &p1.token_b.symbol };


                            if !(p2.token_a.mint == mid_token_mint_p1_hop1 || p2.token_b.mint == mid_token_mint_p1_hop1) { continue; }
                            let mid_token_mint_p2_hop2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { p2.token_b.mint } else { p2.token_a.mint };
                            let mid_token_symbol_p2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { &p2.token_b.symbol } else { &p2.token_a.symbol };


                            if !((p3.token_a.mint == mid_token_mint_p2_hop2 && p3.token_b.mint == start_token_mint_p1_hop1) ||
                                 (p3.token_b.mint == mid_token_mint_p2_hop2 && p3.token_a.mint == start_token_mint_p1_hop1)) { continue; }
                            let final_token_symbol_p3 = if start_token_mint_p1_hop1 == p3.token_a.mint { &p3.token_a.symbol } else { &p3.token_b.symbol };


                            if Self::is_permanently_banned(start_token_symbol_p1, mid_token_symbol_p1) || Self::is_temporarily_banned(start_token_symbol_p1, mid_token_symbol_p1) ||
                               Self::is_permanently_banned(mid_token_symbol_p1, mid_token_symbol_p2) || Self::is_temporarily_banned(mid_token_symbol_p1, mid_token_symbol_p2) ||
                               Self::is_permanently_banned(mid_token_symbol_p2, final_token_symbol_p3) || Self::is_temporarily_banned(mid_token_symbol_p2, final_token_symbol_p3) {
                                continue;
                            }

                            let pools_arr_ref: Vec<&PoolInfo> = vec![p1, p2, p3];
                            let dir1 = p1.token_a.mint == start_token_mint_p1_hop1;
                            let dir2 = p2.token_a.mint == mid_token_mint_p1_hop1;
                            let dir3 = p3.token_a.mint == mid_token_mint_p2_hop2;
                            let directions = vec![dir1, dir2, dir3];

                            let last_fee_data = vec![(None, None, None); 3];
                            let input_amount_float = 1000.0;

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
                            FeeManager::record_fee_observation(p1, p1.fee_numerator, p1.fee_denominator);

                            let risk_score = if abnormal_fee || fee_breakdown.expected_slippage > 0.05 { Some(1.0) } else { Some(0.0) };
                            let rebate = calculate_rebate(&pools_arr_ref, &[]);

                            let initial_ta = TokenAmount::new(input_token_amount_u64, input_decimals);
                            let hop1_out_ta = calculate_output_amount(p1, initial_ta.clone(), dir1);
                            let hop2_out_ta = calculate_output_amount(p2, hop1_out_ta.clone(), dir2);
                            let hop3_out_ta = calculate_output_amount(p3, hop2_out_ta.clone(), dir3);

                            let final_output_float = hop3_out_ta.to_float();
                            let _net_profit_float = final_output_float - input_amount_float;
                            let profit_pct = if input_amount_float > 0.0 { (calculated_profit_float / input_amount_float) * 100.0 } else { 0.0 };

                            let hops_data = vec![
                                ArbHop { dex: p1.dex_type, pool: p1.address, input_token: start_token_symbol_p1.to_string(), output_token: mid_token_symbol_p1.to_string(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                                ArbHop { dex: p2.dex_type, pool: p2.address, input_token: mid_token_symbol_p1.to_string(), output_token: mid_token_symbol_p2.to_string(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                                ArbHop { dex: p3.dex_type, pool: p3.address, input_token: mid_token_symbol_p2.to_string(), output_token: final_token_symbol_p3.to_string(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                            ];
                            let notes = Some(format!(
                                "3-hop: calc_profit {:.5}, fee {:.5}, slip {:.5}, gas {}, rebate {:.5}, abnormal_fee {}, expl: {}",
                                calculated_profit_float, total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                            ));
                            let opp_id = format!("3hop-{}-{}-{}-{}", start_token_symbol_p1, p1.address, p2.address, p3.address);


                            let opp = MultiHopArbOpportunity {
                                id: opp_id.clone(),
                                hops: hops_data,
                                total_profit: calculated_profit_float,
                                profit_pct,
                                input_token: start_token_symbol_p1.to_string(),
                                output_token: final_token_symbol_p3.to_string(),
                                input_amount: input_amount_float,
                                expected_output: input_amount_float + calculated_profit_float,
                                dex_path: vec![p1.dex_type, p2.dex_type, p3.dex_type],
                                pool_path: vec![p1.address, p2.address, p3.address],
                                risk_score,
                                notes,
                                estimated_profit_usd: Some(calculated_profit_float * 1.0),
                                input_amount_usd: Some(input_amount_float * 1.0),
                                intermediate_tokens: vec![mid_token_symbol_p1.to_string(), mid_token_symbol_p2.to_string()],
                                output_amount_usd: Some((input_amount_float + calculated_profit_float) * 1.0),
                                source_pool: Arc::clone(p1_arc), // First pool in multi-hop
                                target_pool: Arc::clone(p3_arc), // Last pool in multi-hop
                                input_token_mint: start_token_mint_p1_hop1,
                                output_token_mint: start_token_mint_p1_hop1, // Assuming cyclic
                                intermediate_token_mint: Some(mid_token_mint_p1_hop1), // First intermediate
                            };

                            if !abnormal_fee && opp.is_profitable(self.min_profit_threshold / 100.0) { // Ensure threshold is fractional
                                let dex_path_strings: Vec<String> = opp.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                                // Directly use metrics, assuming it's &Metrics
                                if let Err(e) = metrics.record_opportunity_detected( // record_opportunity_detected is not async
                                    &opp.input_token,
                                    &opp.intermediate_tokens.get(0).cloned().unwrap_or_default(), // Example for intermediate
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
            self.min_profit_threshold // Assuming threshold is stored as percentage value
        );
        Ok(opportunities)
    }

    pub async fn find_all_multihop_opportunities_with_risk(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &Metrics,
        max_slippage_pct: f64,
        tx_fee_lamports: u64,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();

        if pool_vec.len() < 2 { return Ok(opportunities); }

        if pool_vec.len() >= 3 {
            for i in 0..pool_vec.len() {
                for j in 0..pool_vec.len() {
                    if i == j { continue; }
                    for k in 0..pool_vec.len() {
                        if k == i || k == j { continue; }

                        let p1_arc = &pool_vec[i];
                        let p2_arc = &pool_vec[j];
                        let p3_arc = &pool_vec[k];
                        let p1 = p1_arc.as_ref();
                        let p2 = p2_arc.as_ref();
                        let p3 = p3_arc.as_ref();


                        for start_token_mint_p1_hop1 in [p1.token_a.mint, p1.token_b.mint] {
                            let mid_token_mint_p1_hop1 = if start_token_mint_p1_hop1 == p1.token_a.mint { p1.token_b.mint } else { p1.token_a.mint };
                            let start_token_symbol_p1 = if start_token_mint_p1_hop1 == p1.token_a.mint { &p1.token_a.symbol } else { &p1.token_b.symbol };
                            let mid_token_symbol_p1 = if mid_token_mint_p1_hop1 == p1.token_a.mint { &p1.token_a.symbol } else { &p1.token_b.symbol };

                            if !(p2.token_a.mint == mid_token_mint_p1_hop1 || p2.token_b.mint == mid_token_mint_p1_hop1) { continue; }
                            let mid_token_mint_p2_hop2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { p2.token_b.mint } else { p2.token_a.mint };
                            let mid_token_symbol_p2 = if mid_token_mint_p1_hop1 == p2.token_a.mint { &p2.token_b.symbol } else { &p2.token_a.symbol };

                            if !((p3.token_a.mint == mid_token_mint_p2_hop2 && p3.token_b.mint == start_token_mint_p1_hop1) ||
                                 (p3.token_b.mint == mid_token_mint_p2_hop2 && p3.token_a.mint == start_token_mint_p1_hop1)) { continue; }
                            let final_token_symbol_p3 = if start_token_mint_p1_hop1 == p3.token_a.mint { &p3.token_a.symbol } else { &p3.token_b.symbol };


                            if Self::is_permanently_banned(start_token_symbol_p1, mid_token_symbol_p1) || Self::is_temporarily_banned(start_token_symbol_p1, mid_token_symbol_p1) ||
                               Self::is_permanently_banned(mid_token_symbol_p1, mid_token_symbol_p2) || Self::is_temporarily_banned(mid_token_symbol_p1, mid_token_symbol_p2) ||
                               Self::is_permanently_banned(mid_token_symbol_p2, final_token_symbol_p3) || Self::is_temporarily_banned(mid_token_symbol_p2, final_token_symbol_p3) {
                                continue;
                            }

                            let pools_arr_ref: Vec<&PoolInfo> = vec![p1, p2, p3];
                            let dir1 = p1.token_a.mint == start_token_mint_p1_hop1;
                            let dir2 = p2.token_a.mint == mid_token_mint_p1_hop1;
                            let dir3 = p3.token_a.mint == mid_token_mint_p2_hop2;
                            let directions = vec![dir1, dir2, dir3];

                            let last_fee_data = vec![(None, None, None); 3];
                            let input_amount_float = 1000.0;
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
                            let _net_profit_float = final_output_float - input_amount_float;
                            let profit_pct = if input_amount_float > 0.0 { (calculated_profit_float / input_amount_float) * 100.0 } else { 0.0 };

                            let hops_data = vec![
                                ArbHop { dex: p1.dex_type, pool: p1.address, input_token: start_token_symbol_p1.to_string(), output_token: mid_token_symbol_p1.to_string(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                                ArbHop { dex: p2.dex_type, pool: p2.address, input_token: mid_token_symbol_p1.to_string(), output_token: mid_token_symbol_p2.to_string(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                                ArbHop { dex: p3.dex_type, pool: p3.address, input_token: mid_token_symbol_p2.to_string(), output_token: final_token_symbol_p3.to_string(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                            ];
                            let notes = Some(format!(
                                "3-hop risk: fee {:.5}, slip {:.5}, gas {}, rebate {:.5}, abnormal_fee {}, expl: {}",
                                total_fee, total_slippage, fee_breakdown.gas_cost, rebate, abnormal_fee, fee_breakdown.explanation
                            ));
                            let opp_id = format!("3hop-risk-{}-{}-{}-{}", start_token_symbol_p1, p1.address, p2.address, p3.address);

                            let opp = MultiHopArbOpportunity {
                                id: opp_id.clone(),
                                hops: hops_data,
                                total_profit: calculated_profit_float,
                                profit_pct,
                                input_token: start_token_symbol_p1.to_string(),
                                output_token: final_token_symbol_p3.to_string(),
                                input_amount: input_amount_float,
                                expected_output: input_amount_float + calculated_profit_float,
                                dex_path: vec![p1.dex_type, p2.dex_type, p3.dex_type],
                                pool_path: vec![p1.address, p2.address, p3.address],
                                risk_score,
                                notes,
                                estimated_profit_usd: Some(calculated_profit_float * 1.0),
                                input_amount_usd: Some(input_amount_float * 1.0),
                                intermediate_tokens: vec![mid_token_symbol_p1.to_string(), mid_token_symbol_p2.to_string()],
                                output_amount_usd: Some((input_amount_float + calculated_profit_float) * 1.0),
                                source_pool: Arc::clone(p1_arc),
                                target_pool: Arc::clone(p3_arc),
                                input_token_mint: start_token_mint_p1_hop1,
                                output_token_mint: start_token_mint_p1_hop1, // Assuming cyclic
                                intermediate_token_mint: Some(mid_token_mint_p1_hop1),
                            };

                            if !abnormal_fee
                                && fee_breakdown.expected_slippage <= max_slippage_pct
                                && fee_breakdown.gas_cost <= tx_fee_lamports
                                && opp.is_profitable(self.min_profit_threshold / 100.0) // Ensure threshold is fractional
                            {
                                let dex_path_strings: Vec<String> = opp.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                                if let Err(e) = metrics.record_opportunity_detected( // record_opportunity_detected is not async
                                    &opp.input_token,
                                    &opp.intermediate_tokens.get(0).cloned().unwrap_or_default(),
                                    opp.profit_pct,
                                    opp.estimated_profit_usd,
                                    opp.input_amount_usd,
                                    dex_path_strings,
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
            self.min_profit_threshold, // Assuming threshold is stored as percentage value
            max_slippage_pct * 100.0,
            tx_fee_lamports
        );
        Ok(opportunities)
    }

    #[allow(dead_code)] // This seems to be a duplicate or alternative profitability check.
                       // The one in calculator.rs is more commonly used. Keep for reference or remove if redundant.
    pub fn is_profitable(
        _calc_result: &OpportunityCalculationResult, // Underscore to silence warning if not used
        _fee_result: &FeeEstimationResult,           // Underscore to silence warning if not used
    ) -> bool {
        // Example placeholder logic
        // let net_profit = calc_result.profit - fee_result.total_cost;
        // net_profit > (calc_result.input_amount * (0.005 / 100.0)) // e.g., 0.005%
        true // Placeholder
    }
}