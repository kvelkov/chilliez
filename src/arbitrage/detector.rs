// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{
            calculate_max_profit, calculate_optimal_input, // Removed unused calculate_transaction_cost, estimate_price_impact, is_profitable_calc
            OpportunityCalculationResult,
        },
        fee_manager::{FeeManager, XYKSlippageModel}, // Removed unused FeeEstimationResult
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{
        calculate_multihop_profit_and_slippage, calculate_output_amount, PoolInfo, TokenAmount, DexType, // Added DexType
        // Removed unused calculate_rebate
    },
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap, // Removed HashSet as it's unused
    fs::OpenOptions,
    io::Write,
    sync::Arc,
};
// Removed: use tokio::io::AsyncWriteExt; // Not used directly here

#[derive(Clone)]
pub struct ArbitrageDetector {
    min_profit_threshold: f64,
}

#[allow(dead_code)]
impl ArbitrageDetector {
    pub fn new(min_profit_threshold_pct: f64) -> Self {
        Self {
            min_profit_threshold: min_profit_threshold_pct,
        }
    }

    pub fn set_min_profit_threshold(&mut self, new_threshold_pct: f64) {
        self.min_profit_threshold = new_threshold_pct;
        info!(
            "ArbitrageDetector min_profit_threshold updated to: {:.4}%",
            new_threshold_pct
        );
    }

    pub fn get_min_profit_threshold(&self) -> f64 {
        self.min_profit_threshold
    }

    pub fn log_banned_pair(token_a: &str, token_b: &str, ban_type: &str, reason: &str) {
        let log_entry = format!("{},{},{},{}\n", token_a, token_b, ban_type, reason);
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
                if parts.len() >= 3 && parts[2] == "permanent" && 
                   ((parts[0] == token_a && parts[1] == token_b) || (parts[0] == token_b && parts[1] == token_a)) {
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
                            if ((parts[0] == token_a && parts[1] == token_b) || (parts[0] == token_b && parts[1] == token_a)) && expiry > now {
                                return true;
                            }
                        } else {
                            warn!("Failed to parse expiry timestamp '{}' in ban log for pair {}/{}", expiry_str, token_a, token_b);
                        }
                    }
                }
            }
        }
        false
    }
    
    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();

        for (src_id, src_pool_arc) in pools {
            let src_pool = src_pool_arc.as_ref();
            let tokens_in_src_pool = [
                (&src_pool.token_a.mint, &src_pool.token_a.symbol, src_pool.token_a.decimals),
                (&src_pool.token_b.mint, &src_pool.token_b.symbol, src_pool.token_b.decimals),
            ];

            for &(input_token_mint_for_cycle, input_token_symbol_for_cycle, input_decimals_for_cycle) in &tokens_in_src_pool {
                let (intermediate_token_mint_val, intermediate_token_symbol_val, _intermediate_decimals_val) =
                    if src_pool.token_a.mint == *input_token_mint_for_cycle {
                        (src_pool.token_b.mint, src_pool.token_b.symbol.clone(), src_pool.token_b.decimals)
                    } else {
                        (src_pool.token_a.mint, src_pool.token_a.symbol.clone(), src_pool.token_a.decimals)
                    };

                if *input_token_mint_for_cycle == intermediate_token_mint_val { continue; }
                if Self::is_permanently_banned(input_token_symbol_for_cycle, &intermediate_token_symbol_val) || Self::is_temporarily_banned(input_token_symbol_for_cycle, &intermediate_token_symbol_val) {
                    debug!("Skipping banned pair for first hop: {} <-> {}", input_token_symbol_for_cycle, intermediate_token_symbol_val);
                    continue;
                }

                for (tgt_id, tgt_pool_arc) in pools {
                    if src_id == tgt_id { continue; }
                    let tgt_pool = tgt_pool_arc.as_ref();

                    let tgt_trades_intermediate_to_input = tgt_pool.token_a.mint == intermediate_token_mint_val && tgt_pool.token_b.mint == *input_token_mint_for_cycle;
                    let tgt_trades_input_to_intermediate_reverse = tgt_pool.token_b.mint == intermediate_token_mint_val && tgt_pool.token_a.mint == *input_token_mint_for_cycle;

                    if !(tgt_trades_intermediate_to_input || tgt_trades_input_to_intermediate_reverse) { continue; }

                    let final_output_token_symbol_val = if tgt_trades_intermediate_to_input { &tgt_pool.token_b.symbol } else { &tgt_pool.token_a.symbol };
                    if Self::is_permanently_banned(&intermediate_token_symbol_val, final_output_token_symbol_val) || Self::is_temporarily_banned(&intermediate_token_symbol_val, final_output_token_symbol_val) {
                        debug!("Skipping banned pair for second hop: {} <-> {}", intermediate_token_symbol_val, final_output_token_symbol_val);
                        continue;
                    }

                    let src_swaps_input_for_intermediate = src_pool.token_a.mint == *input_token_mint_for_cycle;
                    let max_input_token_amount = TokenAmount::new(1_000_000, input_decimals_for_cycle);
                    let optimal_in = calculate_optimal_input(src_pool, tgt_pool, src_swaps_input_for_intermediate, max_input_token_amount);
                    let intermediate_amt_received = calculate_output_amount(src_pool, optimal_in.clone(), src_swaps_input_for_intermediate);
                    let tgt_swaps_intermediate_for_input = tgt_pool.token_a.mint == intermediate_token_mint_val;
                    let final_output_amt = calculate_output_amount(tgt_pool, intermediate_amt_received.clone(), tgt_swaps_intermediate_for_input);
                    let profit_pct_calc_fractional = calculate_max_profit(src_pool, tgt_pool, src_swaps_input_for_intermediate, optimal_in.clone());
                    let profit_abs_tokens = (final_output_amt.to_float() - optimal_in.to_float()).max(0.0);
                    
                    let input_token_price_usd = 1.0; 
                    let profit_usd = profit_abs_tokens * input_token_price_usd;
                    let input_amount_usd_val = optimal_in.to_float() * input_token_price_usd;
                    let output_amount_usd_val = final_output_amt.to_float() * input_token_price_usd;

                    if profit_pct_calc_fractional * 100.0 > self.min_profit_threshold {
                        let opp_id = format!("2hop-{}-{}-{}-{}", input_token_symbol_for_cycle, src_pool.address, tgt_pool.address, chrono::Utc::now().timestamp_millis());
                        let current_opportunity = MultiHopArbOpportunity {
                            id: opp_id.clone(),
                            hops: vec![
                                ArbHop {
                                    dex: src_pool.dex_type.clone(), pool: src_pool.address, // Clone DexType
                                    input_token: input_token_symbol_for_cycle.to_string(),
                                    output_token: intermediate_token_symbol_val.clone(),
                                    input_amount: optimal_in.to_float(),
                                    expected_output: intermediate_amt_received.to_float(),
                                },
                                ArbHop {
                                    dex: tgt_pool.dex_type.clone(), pool: tgt_pool.address, // Clone DexType
                                    input_token: intermediate_token_symbol_val.clone(),
                                    output_token: final_output_token_symbol_val.to_string(),
                                    input_amount: intermediate_amt_received.to_float(),
                                    expected_output: final_output_amt.to_float(),
                                },
                            ],
                            total_profit: profit_abs_tokens,
                            profit_pct: profit_pct_calc_fractional * 100.0,
                            input_token: input_token_symbol_for_cycle.to_string(),
                            output_token: final_output_token_symbol_val.to_string(),
                            input_amount: optimal_in.to_float(),
                            expected_output: final_output_amt.to_float(),
                            dex_path: vec![src_pool.dex_type.clone(), tgt_pool.dex_type.clone()], // Clone DexType
                            pool_path: vec![src_pool.address, tgt_pool.address],
                            risk_score: None, notes: Some("Direct 2-hop cyclic opportunity".to_string()),
                            estimated_profit_usd: Some(profit_usd), input_amount_usd: Some(input_amount_usd_val),
                            intermediate_tokens: vec![intermediate_token_symbol_val.clone()],
                            output_amount_usd: Some(output_amount_usd_val),
                            source_pool: Arc::clone(src_pool_arc), target_pool: Arc::clone(tgt_pool_arc),
                            input_token_mint: *input_token_mint_for_cycle, output_token_mint: *input_token_mint_for_cycle,
                            intermediate_token_mint: Some(intermediate_token_mint_val),
                        };
                        
                        info!("Potential 2-hop Arb Opp: {} -> {} (Profit: {:.4}%)", current_opportunity.input_token, current_opportunity.output_token, current_opportunity.profit_pct);
                        current_opportunity.log_summary();

                        let dex_path_strings_log: Vec<String> = current_opportunity.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                        if let Err(e) = metrics.record_opportunity_detected(
                            &current_opportunity.input_token,
                            &current_opportunity.intermediate_tokens.get(0).cloned().unwrap_or_default(),
                            current_opportunity.profit_pct, // Pass as percentage
                            current_opportunity.estimated_profit_usd,
                            current_opportunity.input_amount_usd,
                            dex_path_strings_log,
                        ) { error!("Failed to record 2-hop opportunity detection metric: {}", e); }
                        opportunities.push(current_opportunity);
                    }
                }
            }
        }
        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} direct (2-hop cyclic) arbitrage opportunities above {:.4}% threshold", opportunities.len(), self.min_profit_threshold);
        Ok(opportunities)
    }
    
    pub async fn find_two_hop_opportunities(&self, pools: &HashMap<Pubkey, Arc<PoolInfo>>, metrics: &mut Metrics) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.find_all_opportunities(pools, metrics).await
    }

    pub async fn find_all_multihop_opportunities(&self, pools: &HashMap<Pubkey, Arc<PoolInfo>>, metrics: &mut Metrics) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();
        let pool_vec: Vec<_> = pools.values().cloned().collect();
        if pool_vec.len() < 3 { return Ok(opportunities); }

        for i in 0..pool_vec.len() {
            for j in 0..pool_vec.len() {
                if i == j { continue; }
                for k in 0..pool_vec.len() {
                    if k == i || k == j { continue; }

                    let p1_arc = &pool_vec[i]; let p2_arc = &pool_vec[j]; let p3_arc = &pool_vec[k];
                    let p1 = p1_arc.as_ref(); let p2 = p2_arc.as_ref(); let p3 = p3_arc.as_ref();

                    for start_token_index_p1 in 0..2 {
                        let (start_token_mint_p1, start_token_symbol_p1, start_token_decimals_p1) = if start_token_index_p1 == 0 { (p1.token_a.mint, &p1.token_a.symbol, p1.token_a.decimals) } else { (p1.token_b.mint, &p1.token_b.symbol, p1.token_b.decimals) };
                        let (mid_token1_mint_p1, mid_token1_symbol_p1, _mid_token1_decimals_p1) = if start_token_index_p1 == 0 { (p1.token_b.mint, &p1.token_b.symbol, p1.token_b.decimals) } else { (p1.token_a.mint, &p1.token_a.symbol, p1.token_a.decimals) };

                        for mid_token1_index_p2 in 0..2 {
                            let (current_mid_token1_mint_p2, _current_mid_token1_symbol_p2, _current_mid_token1_decimals_p2) = if mid_token1_index_p2 == 0 { (p2.token_a.mint, &p2.token_a.symbol, p2.token_a.decimals) } else { (p2.token_b.mint, &p2.token_b.symbol, p2.token_b.decimals) };
                            if mid_token1_mint_p1 != current_mid_token1_mint_p2 { continue; }
                            let (mid_token2_mint_p2, mid_token2_symbol_p2, _mid_token2_decimals_p2) = if mid_token1_index_p2 == 0 { (p2.token_b.mint, &p2.token_b.symbol, p2.token_b.decimals) } else { (p2.token_a.mint, &p2.token_a.symbol, p2.token_a.decimals) };

                            for mid_token2_index_p3 in 0..2 {
                                let (current_mid_token2_mint_p3, _current_mid_token2_symbol_p3, _current_mid_token2_decimals_p3) = if mid_token2_index_p3 == 0 { (p3.token_a.mint, &p3.token_a.symbol, p3.token_a.decimals) } else { (p3.token_b.mint, &p3.token_b.symbol, p3.token_b.decimals) };
                                if mid_token2_mint_p2 != current_mid_token2_mint_p3 { continue; }
                                let (final_token_mint_p3, final_token_symbol_p3, _final_token_decimals_p3) = if mid_token2_index_p3 == 0 { (p3.token_b.mint, &p3.token_b.symbol, p3.token_b.decimals) } else { (p3.token_a.mint, &p3.token_a.symbol, p3.token_a.decimals) };
                                if final_token_mint_p3 != start_token_mint_p1 { continue; }

                                if Self::is_permanently_banned(start_token_symbol_p1, mid_token1_symbol_p1) || Self::is_temporarily_banned(start_token_symbol_p1, mid_token1_symbol_p1) ||
                                   Self::is_permanently_banned(mid_token1_symbol_p1, mid_token2_symbol_p2) || Self::is_temporarily_banned(mid_token1_symbol_p1, mid_token2_symbol_p2) ||
                                   Self::is_permanently_banned(mid_token2_symbol_p2, final_token_symbol_p3) || Self::is_temporarily_banned(mid_token2_symbol_p2, final_token_symbol_p3) { continue; }

                                let pools_arr_ref: Vec<&PoolInfo> = vec![p1, p2, p3];
                                let dir1 = p1.token_a.mint == start_token_mint_p1; let dir2 = p2.token_a.mint == mid_token1_mint_p1; let dir3 = p3.token_a.mint == mid_token2_mint_p2;
                                let directions = vec![dir1, dir2, dir3];
                                let last_fee_data = vec![(None,None,None); 3];
                                let input_amount_float = 100.0;
                                
                                let (calculated_profit_float, total_slippage_fraction, _total_fee_tokens) = calculate_multihop_profit_and_slippage(&pools_arr_ref, input_amount_float, &directions, &last_fee_data);
                                let input_tokenamount = TokenAmount::new((input_amount_float * 10f64.powi(start_token_decimals_p1 as i32)) as u64, start_token_decimals_p1);
                                let hop1_out_ta = calculate_output_amount(p1, input_tokenamount.clone(), dir1);
                                let hop2_out_ta = calculate_output_amount(p2, hop1_out_ta.clone(), dir2);
                                let hop3_out_ta = calculate_output_amount(p3, hop2_out_ta.clone(), dir3);
                                let profit_pct = if input_amount_float > 0.0 { (calculated_profit_float / input_amount_float) * 100.0 } else { 0.0 };

                                if profit_pct > self.min_profit_threshold {
                                    let opp_id = format!("3hop-{}-{}-{}-{}-{}", start_token_symbol_p1, p1.address, p2.address, p3.address, chrono::Utc::now().timestamp_millis());
                                    let hops_data = vec![
                                        ArbHop { dex: p1.dex_type.clone(), pool: p1.address, input_token: start_token_symbol_p1.to_string(), output_token: mid_token1_symbol_p1.to_string(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                                        ArbHop { dex: p2.dex_type.clone(), pool: p2.address, input_token: mid_token1_symbol_p1.to_string(), output_token: mid_token2_symbol_p2.to_string(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                                        ArbHop { dex: p3.dex_type.clone(), pool: p3.address, input_token: mid_token2_symbol_p2.to_string(), output_token: final_token_symbol_p3.to_string(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                                    ];
                                    let estimated_profit_usd = Some(calculated_profit_float * 1.0); let input_amount_usd_val = Some(input_amount_float * 1.0); let output_amount_usd_val = Some((input_amount_float + calculated_profit_float) * 1.0);

                                    let opp = MultiHopArbOpportunity {
                                        id: opp_id, hops: hops_data, total_profit: calculated_profit_float, profit_pct,
                                        input_token: start_token_symbol_p1.to_string(), output_token: final_token_symbol_p3.to_string(),
                                        input_amount: input_amount_float, expected_output: hop3_out_ta.to_float(),
                                        dex_path: vec![p1.dex_type.clone(), p2.dex_type.clone(), p3.dex_type.clone()], // Clone DexType
                                        pool_path: vec![p1.address, p2.address, p3.address],
                                        risk_score: Some(total_slippage_fraction), notes: Some(format!("3-hop: Slippage: {:.4}%", total_slippage_fraction * 100.0)),
                                        estimated_profit_usd, input_amount_usd: input_amount_usd_val,
                                        intermediate_tokens: vec![mid_token1_symbol_p1.to_string(), mid_token2_symbol_p2.to_string()],
                                        output_amount_usd: output_amount_usd_val,
                                        source_pool: Arc::clone(p1_arc), target_pool: Arc::clone(p3_arc),
                                        input_token_mint: start_token_mint_p1, output_token_mint: final_token_mint_p3,
                                        intermediate_token_mint: Some(mid_token1_mint_p1),
                                    };
                                    info!("[ANALYTICS] Potential 3-Hop Opp: {} -> {} -> {} -> {} (Profit: {:.2}%)", start_token_symbol_p1, mid_token1_symbol_p1, mid_token2_symbol_p2, final_token_symbol_p3, opp.profit_pct);
                                    opp.log_summary();
                                    
                                    let dex_path_strings_log: Vec<String> = opp.dex_path.iter().map(|d| format!("{:?}",d)).collect();
                                    if let Err(e) = metrics.record_opportunity_detected(
                                        &opp.input_token, &opp.intermediate_tokens.get(0).cloned().unwrap_or_default(),
                                        opp.profit_pct, opp.estimated_profit_usd, opp.input_amount_usd, dex_path_strings_log,
                                    ){ error!("Failed to record 3-hop opportunity metric: {}", e); }
                                    opportunities.push(opp);
                                }
                            }
                        }
                    }
                }
            }
        }
        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} multi-hop (3-hop) arbitrage opportunities above {:.4}% threshold", opportunities.len(), self.min_profit_threshold);
        Ok(opportunities)
    }

    pub async fn find_all_multihop_opportunities_with_risk(&self, pools: &HashMap<Pubkey, Arc<PoolInfo>>, metrics: &mut Metrics, max_slippage_pct: f64, tx_fee_lamports: u64) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let base_opportunities = self.find_all_multihop_opportunities(pools, metrics).await?;
        let mut risk_adjusted_opportunities = Vec::new();

        for opp in base_opportunities {
            let estimated_total_slippage_fraction = opp.risk_score.unwrap_or(1.0); // Default to high if not set
            
            // Rough gas cost estimation based on number of hops, or use a more detailed FeeManager call if available for the whole path
            let estimated_gas_cost_lamports = opp.hops.len() as u64 * FeeManager::estimate_multi_hop_with_model(
                 &opp.pool_path.iter().filter_map(|p_addr| pools.get(p_addr).map(|p| p.as_ref())).collect::<Vec<&PoolInfo>>(),
                 &opp.hops.iter().map(|h| TokenAmount::new((h.input_amount * 10f64.powi(6)) as u64, 6)).collect::<Vec<TokenAmount>>(),
                 &vec![true; opp.hops.len()],
                 &vec![(None,None,None); opp.hops.len()],
                 &XYKSlippageModel {}
            ).gas_cost / opp.hops.len().max(1) as u64; // Average per hop if complex, or sum

            if estimated_total_slippage_fraction * 100.0 <= max_slippage_pct && estimated_gas_cost_lamports <= tx_fee_lamports {
                risk_adjusted_opportunities.push(opp);
            } else {
                debug!("Skipping opportunity ID {} due to risk/fee (Slippage: {:.4}%, Fee: {} lamports)", opp.id, estimated_total_slippage_fraction * 100.0, estimated_gas_cost_lamports);
            }
        }
        
        info!("Found {} multi-hop arbitrage opportunities (risk-adjusted) above {:.4}% threshold, max slippage {:.2}%, max fee {} lamports",
            risk_adjusted_opportunities.len(), self.min_profit_threshold, max_slippage_pct, tx_fee_lamports);
        Ok(risk_adjusted_opportunities)
    }
}