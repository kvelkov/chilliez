// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{
            calculate_max_profit_result, calculate_optimal_input, calculate_transaction_cost,
            is_profitable_calc, // Corrected import
            OpportunityCalculationResult, calculate_multihop_profit_and_slippage,
        },
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{PoolInfo, TokenAmount, DexType, calculate_output_amount}, 
};
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    sync::Arc,
};


#[derive(Clone)]
pub struct ArbitrageDetector {
    min_profit_threshold_pct: f64,    
    min_profit_threshold_usd: f64,   
    sol_price_usd: f64,              
    default_priority_fee_lamports: u64, 
}

impl ArbitrageDetector {
    pub fn new(
        min_profit_threshold_pct: f64,
        min_profit_threshold_usd: f64,
        sol_price_usd: f64,
        default_priority_fee_lamports: u64,
    ) -> Self {
        info!(
            "ArbitrageDetector initialized with: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            min_profit_threshold_pct, min_profit_threshold_usd, sol_price_usd, default_priority_fee_lamports
        );
        Self {
            min_profit_threshold_pct,
            min_profit_threshold_usd,
            sol_price_usd,
            default_priority_fee_lamports,
        }
    }

    pub fn set_min_profit_threshold(&mut self, new_threshold_pct: f64) {
        self.min_profit_threshold_pct = new_threshold_pct;
        info!(
            "ArbitrageDetector min_profit_threshold_pct updated to: {:.4}%",
            new_threshold_pct
        );
    }

    pub fn get_min_profit_threshold_pct(&self) -> f64 {
        self.min_profit_threshold_pct
    }
    
    fn is_opportunity_profitable_after_costs(
        &self,
        opp_calc_result: &OpportunityCalculationResult,
        input_token_price_usd: f64, 
        num_hops: usize,
    ) -> bool {
        let transaction_cost_usd = calculate_transaction_cost(
            num_hops * 2, 
            self.default_priority_fee_lamports,
            self.sol_price_usd,
        );

        let meets_pct_threshold = opp_calc_result.profit_percentage * 100.0 >= self.min_profit_threshold_pct;

        let meets_usd_threshold = is_profitable_calc( // Using the imported function
            opp_calc_result,
            input_token_price_usd,
            transaction_cost_usd,
            self.min_profit_threshold_usd,
        );
        
        debug!("Profitability detail: Result: {:?}, InputTokenPriceUSD: {}, TxCostUSD: {}, MinProfitUSD: {}, MeetsPct: {}, MeetsUSD: {}",
            opp_calc_result, input_token_price_usd, transaction_cost_usd, self.min_profit_threshold_usd, meets_pct_threshold, meets_usd_threshold);

        meets_pct_threshold && meets_usd_threshold
    }

    pub fn log_banned_pair(token_a_symbol: &str, token_b_symbol: &str, ban_type: &str, reason: &str) {
        let log_entry = format!("{},{},{},{}\n", token_a_symbol, token_b_symbol, ban_type, reason);
        let log_file_path = "banned_pairs_log.csv"; 
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file_path)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(log_entry.as_bytes()) {
                    error!("Failed to write to ban log file '{}': {}", log_file_path, e);
                } else {
                    info!("Logged banned pair to '{}': {} <-> {} ({}, Reason: {})", log_file_path, token_a_symbol, token_b_symbol, ban_type, reason);
                }
            }
            Err(e) => {
                error!("Cannot open ban log file '{}': {}", log_file_path, e);
            }
        }
    }

    pub fn is_permanently_banned(token_a_symbol: &str, token_b_symbol: &str) -> bool {
        let log_file_path = "banned_pairs_log.csv"; 
        if let Ok(content) = std::fs::read_to_string(log_file_path) {
            for line in content.lines().skip(1) { 
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 3 && parts[2].trim() == "permanent" && 
                   ((parts[0].trim() == token_a_symbol && parts[1].trim() == token_b_symbol) || 
                    (parts[0].trim() == token_b_symbol && parts[1].trim() == token_a_symbol)) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_temporarily_banned(token_a_symbol: &str, token_b_symbol: &str) -> bool {
        let log_file_path = "banned_pairs_log.csv"; 
        if let Ok(content) = std::fs::read_to_string(log_file_path) {
            let now = chrono::Utc::now().timestamp() as u64;
            for line in content.lines().skip(1) { 
                let parts: Vec<_> = line.split(',').collect();
                if parts.len() >= 4 && parts[2].trim() == "temporary" {
                    if let Some(expiry_str) = parts.get(3) {
                        if let Ok(expiry) = expiry_str.trim().parse::<u64>() {
                            if ((parts[0].trim() == token_a_symbol && parts[1].trim() == token_b_symbol) || 
                                (parts[0].trim() == token_b_symbol && parts[1].trim() == token_a_symbol)) && expiry > now {
                                return true;
                            }
                        } else {
                            warn!("Failed to parse expiry timestamp '{}' in ban log for pair {}/{}", expiry_str.trim(), token_a_symbol, token_b_symbol);
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
        info!("find_all_opportunities (2-hop cyclic) called with {} pools. Min Profit Pct: {:.4}%, Min Profit USD: ${:.2}", pools.len(), self.min_profit_threshold_pct, self.min_profit_threshold_usd);
        let mut opportunities = Vec::new();

        for (_src_pool_addr, src_pool_arc) in pools.iter() {
            let src_pool = src_pool_arc.as_ref();

            // Path 1: src_pool.token_a -> src_pool.token_b (intermediate) -> find tgt_pool for intermediate -> src_pool.token_a
            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc, 
                &src_pool.token_a.mint, &src_pool.token_a.symbol, src_pool.token_a.decimals, // Leg 1 input
                &src_pool.token_b.mint, &src_pool.token_b.symbol, // Leg 1 output (intermediate)
                true, // src_pool_is_a_to_b = true
                &mut opportunities, metrics
            ).await;

            // Path 2: src_pool.token_b -> src_pool.token_a (intermediate) -> find tgt_pool for intermediate -> src_pool.token_b
            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc,
                &src_pool.token_b.mint, &src_pool.token_b.symbol, src_pool.token_b.decimals, // Leg 1 input
                &src_pool.token_a.mint, &src_pool.token_a.symbol, // Leg 1 output (intermediate)
                false, // src_pool_is_a_to_b = false
                &mut opportunities, metrics
            ).await;
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} direct (2-hop cyclic) arbitrage opportunities.", opportunities.len());
        if opportunities.is_empty() && pools.len() >= 2 {
             debug!("No 2-hop opportunities found. Pool states might not allow profitable cycles at current thresholds, or test pool setup needs review for mint consistency.");
        }
        Ok(opportunities)
    }

    async fn find_cyclic_pair_for_src_hop(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        src_pool_arc: &Arc<PoolInfo>,
        input_mint_leg1: &Pubkey, // Mint of the token input to src_pool
        input_symbol_leg1: &str,
        input_decimals_leg1: u8,
        intermediate_mint_leg1: &Pubkey, // Mint of the token output from src_pool
        intermediate_symbol_leg1: &str,
        src_pool_is_a_to_b: bool, 
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        let src_pool = src_pool_arc.as_ref();
        if *input_mint_leg1 == *intermediate_mint_leg1 { // Should not happen if pool is valid
            return;
        }
        if Self::is_permanently_banned(input_symbol_leg1, intermediate_symbol_leg1) || 
           Self::is_temporarily_banned(input_symbol_leg1, intermediate_symbol_leg1) {
            debug!("Skipping banned pair for first hop: {} ({}) <-> {} ({}) in pool {}", input_symbol_leg1, input_mint_leg1, intermediate_symbol_leg1, intermediate_mint_leg1, src_pool.name);
            return;
        }
        
        debug!("Attempting 2-hop from Pool1='{}'({}), Input: {} ({}), Intermediate: {} ({})", src_pool.name, src_pool.address, input_symbol_leg1, input_mint_leg1, intermediate_symbol_leg1, intermediate_mint_leg1);

        for (_tgt_pool_addr, tgt_pool_arc) in pools.iter() {
            if src_pool.address == tgt_pool_arc.address { continue; } 
            let tgt_pool = tgt_pool_arc.as_ref();

            // Leg 2: Must trade intermediate_mint_leg1 -> input_mint_leg1 (to complete the cycle)
            let tgt_pool_can_trade_intermediate_for_input = 
                (tgt_pool.token_a.mint == *intermediate_mint_leg1 && tgt_pool.token_b.mint == *input_mint_leg1) ||
                (tgt_pool.token_b.mint == *intermediate_mint_leg1 && tgt_pool.token_a.mint == *input_mint_leg1);

            if !tgt_pool_can_trade_intermediate_for_input {
                continue; 
            }
            
            // Determine tgt_pool's swap direction for intermediate_mint_leg1 -> input_mint_leg1
            // This variable tgt_pool_is_a_to_b_for_leg2 was unused, but its logic is implicitly handled
            // by calculate_max_profit_result when it matches mints.
            // let _tgt_pool_is_a_to_b_for_leg2 = tgt_pool.token_a.mint == *intermediate_mint_leg1;


            debug!("  Candidate Pair: Pool1='{}', Pool2='{}'({}). Checking for cycle.", src_pool.name, tgt_pool.name, tgt_pool.address);

            if Self::is_permanently_banned(intermediate_symbol_leg1, input_symbol_leg1) || 
               Self::is_temporarily_banned(intermediate_symbol_leg1, input_symbol_leg1) {
                debug!("Skipping banned pair for second hop: {} <-> {}", intermediate_symbol_leg1, input_symbol_leg1);
                continue;
            }
            
            // Max input for calculation, e.g., 1000 units of the input token
            let base_units = if input_decimals_leg1 >= 6 { 1_000 * 10u64.pow(input_decimals_leg1 as u32) } else { 1_000_000 * 10u64.pow(input_decimals_leg1 as u32) };
            let max_input_token_amount = TokenAmount::new(base_units, input_decimals_leg1);
            
            let optimal_in_atomic = calculate_optimal_input(src_pool, tgt_pool, src_pool_is_a_to_b, max_input_token_amount);
            if optimal_in_atomic.amount == 0 {
                debug!("Optimal input amount is 0, skipping for src_pool {}, tgt_pool {}", src_pool.name, tgt_pool.name);
                continue;
            }

            let opp_calc_result = calculate_max_profit_result(src_pool, tgt_pool, src_pool_is_a_to_b, optimal_in_atomic.clone());
            
            let input_token_price_usd = if input_symbol_leg1 == "USDC" || input_symbol_leg1 == "USDT" { 1.0 } else { self.sol_price_usd };

            if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 2) {
                 let input_amount_float = opp_calc_result.input_amount;
                 // Recalculate intermediate amount based on the actual input_amount_float used in opp_calc_result
                 let intermediate_token_from_src_pool = crate::utils::calculate_output_amount(src_pool, TokenAmount::from_float(input_amount_float, input_decimals_leg1), src_pool_is_a_to_b);
                 let expected_intermediate_float = intermediate_token_from_src_pool.to_float();
                 let final_output_float = opp_calc_result.output_amount; // This is from opp_calc_result

                let opp_id = format!("2hop-{}-{}-{}-{}", input_symbol_leg1, src_pool.address, tgt_pool.address, chrono::Utc::now().timestamp_millis());
                let current_opportunity = MultiHopArbOpportunity {
                    id: opp_id.clone(),
                    hops: vec![
                        ArbHop {
                            dex: src_pool.dex_type.clone(), pool: src_pool.address,
                            input_token: input_symbol_leg1.to_string(),
                            output_token: intermediate_symbol_leg1.to_string(),
                            input_amount: input_amount_float,
                            expected_output: expected_intermediate_float,
                        },
                        ArbHop {
                            dex: tgt_pool.dex_type.clone(), pool: tgt_pool.address,
                            input_token: intermediate_symbol_leg1.to_string(),
                            output_token: input_symbol_leg1.to_string(), 
                            input_amount: expected_intermediate_float,
                            expected_output: final_output_float,
                        },
                    ],
                    total_profit: opp_calc_result.profit,
                    profit_pct: opp_calc_result.profit_percentage * 100.0, 
                    input_token: input_symbol_leg1.to_string(),
                    output_token: input_symbol_leg1.to_string(), 
                    input_amount: input_amount_float,
                    expected_output: final_output_float,
                    dex_path: vec![src_pool.dex_type.clone(), tgt_pool.dex_type.clone()],
                    pool_path: vec![src_pool.address, tgt_pool.address],
                    risk_score: Some(opp_calc_result.price_impact),
                    notes: Some("Direct 2-hop cyclic opportunity".to_string()),
                    estimated_profit_usd: Some(opp_calc_result.profit * input_token_price_usd),
                    input_amount_usd: Some(input_amount_float * input_token_price_usd),
                    output_amount_usd: Some(final_output_float * input_token_price_usd),
                    intermediate_tokens: vec![intermediate_symbol_leg1.to_string()],
                    source_pool: Arc::clone(src_pool_arc),
                    target_pool: Arc::clone(tgt_pool_arc),
                    input_token_mint: *input_mint_leg1,
                    output_token_mint: *input_mint_leg1, 
                    intermediate_token_mint: Some(*intermediate_mint_leg1),
                };
                info!("Found Profitable 2-hop Arb Opp ID: {}", opp_id);
                current_opportunity.log_summary();
                
                let dex_path_strings_log: Vec<String> = current_opportunity.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                 if let Err(e) = metrics.record_opportunity_detected(
                    &current_opportunity.input_token,
                    current_opportunity.intermediate_tokens.get(0).map_or("", |s| s.as_str()),
                    current_opportunity.profit_pct,
                    current_opportunity.estimated_profit_usd,
                    current_opportunity.input_amount_usd,
                    dex_path_strings_log,
                ) { error!("Failed to record 2-hop opportunity detection metric: {}", e); }
                opportunities.push(current_opportunity);
            } else {
                 debug!("  Skipped 2-hop cycle (src='{}', tgt='{}', input_token='{}'): profit_pct {:.4}% (raw result profit: {:.6}, price impact: {:.4}) below threshold or too costly.",
                       src_pool.name, tgt_pool.name, input_symbol_leg1, opp_calc_result.profit_percentage * 100.0, opp_calc_result.profit, opp_calc_result.price_impact);
            }
        }
    }
    
    #[deprecated(note = "Use find_all_opportunities for 2-hop cycles. This is a passthrough.")]
    pub async fn find_two_hop_opportunities(&self, pools: &HashMap<Pubkey, Arc<PoolInfo>>, metrics: &mut Metrics) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        warn!("Deprecated find_two_hop_opportunities called, redirecting to find_all_opportunities.");
        self.find_all_opportunities(pools, metrics).await
    }

    pub async fn find_all_multihop_opportunities(
        &self, 
        pools: &HashMap<Pubkey, Arc<PoolInfo>>, 
        metrics: &mut Metrics
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("find_all_multihop_opportunities (3-hop cyclic) called with {} pools. Min Profit Pct: {:.4}%, Min Profit USD: ${:.2}", pools.len(), self.min_profit_threshold_pct, self.min_profit_threshold_usd);
        let mut opportunities = Vec::new();
        let pool_vec: Vec<Arc<PoolInfo>> = pools.values().cloned().collect(); 

        if pool_vec.len() < 3 { return Ok(opportunities); }

        for i in 0..pool_vec.len() {
            for j in 0..pool_vec.len() {
                if i == j { continue; }
                for k in 0..pool_vec.len() {
                    if k == i || k == j { continue; }

                    let p1_arc = &pool_vec[i]; let p2_arc = &pool_vec[j]; let p3_arc = &pool_vec[k];
                    
                    // Try Path P1(A->B), P2(B->C), P3(C->A)
                    self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1_arc.token_a, &p1_arc.token_b, 
                        &p2_arc.token_a, &p2_arc.token_b, 
                        &p3_arc.token_a, &p3_arc.token_b, 
                        &mut opportunities, metrics,
                    ).await;
                    // Try Path P1(B->A), P2(A->C), P3(C->B) 
                     self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1_arc.token_b, &p1_arc.token_a, 
                        &p2_arc.token_a, &p2_arc.token_b, 
                        &p3_arc.token_a, &p3_arc.token_b,
                        &mut opportunities, metrics,
                    ).await;
                }
            }
        }
        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} multi-hop (3-hop) arbitrage opportunities.", opportunities.len());
        Ok(opportunities)
    }

    async fn check_specific_3_hop_path(
        &self,
        p1_arc: &Arc<PoolInfo>, p2_arc: &Arc<PoolInfo>, p3_arc: &Arc<PoolInfo>,
        p1_input_tk: &crate::utils::PoolToken, p1_output_tk: &crate::utils::PoolToken, // Leg 1
        p2_input_tk: &crate::utils::PoolToken, p2_output_tk: &crate::utils::PoolToken, // Leg 2
        p3_input_tk: &crate::utils::PoolToken, p3_output_tk: &crate::utils::PoolToken, // Leg 3
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        if !(p1_output_tk.mint == p2_input_tk.mint &&
             p2_output_tk.mint == p3_input_tk.mint &&
             p3_output_tk.mint == p1_input_tk.mint) {
            return; 
        }
        // Ensure all chosen tokens are distinct within their original pools
        if p1_input_tk.mint == p1_output_tk.mint || p2_input_tk.mint == p2_output_tk.mint || p3_input_tk.mint == p3_output_tk.mint {
            return;
        }


        debug!("Checking 3-hop: {}->{} (P1:{}), {}->{} (P2:{}), {}->{} (P3:{})",
            p1_input_tk.symbol, p1_output_tk.symbol, p1_arc.name,
            p2_input_tk.symbol, p2_output_tk.symbol, p2_arc.name,
            p3_input_tk.symbol, p3_output_tk.symbol, p3_arc.name);
            
        if Self::is_permanently_banned(&p1_input_tk.symbol, &p1_output_tk.symbol) || Self::is_temporarily_banned(&p1_input_tk.symbol, &p1_output_tk.symbol) ||
           Self::is_permanently_banned(&p2_input_tk.symbol, &p2_output_tk.symbol) || Self::is_temporarily_banned(&p2_input_tk.symbol, &p2_output_tk.symbol) ||
           Self::is_permanently_banned(&p3_input_tk.symbol, &p3_output_tk.symbol) || Self::is_temporarily_banned(&p3_input_tk.symbol, &p3_output_tk.symbol) {
            debug!("Skipping 3-hop due to banned pair in path.");
            return;
        }

        let pools_path_refs: Vec<&PoolInfo> = vec![p1_arc.as_ref(), p2_arc.as_ref(), p3_arc.as_ref()];
        let directions = vec![
            p1_arc.token_a.mint == p1_input_tk.mint, 
            p2_arc.token_a.mint == p2_input_tk.mint, 
            p3_arc.token_a.mint == p3_input_tk.mint, 
        ];
        
        let last_fee_data = vec![(None,None,None); 3]; 
        let input_amount_float = 100.0; // Example input
        
        let (calculated_profit_float, total_slippage_fraction, _total_fee_tokens_equivalent) =
            calculate_multihop_profit_and_slippage(
                &pools_path_refs, input_amount_float, p1_input_tk.decimals, &directions, &last_fee_data,
            );

        let opp_calc_result = OpportunityCalculationResult {
            input_amount: input_amount_float,
            output_amount: input_amount_float + calculated_profit_float, // Approx output
            profit: calculated_profit_float,
            profit_percentage: if input_amount_float > 1e-9 { calculated_profit_float / input_amount_float } else {0.0},
            price_impact: total_slippage_fraction,
        };
        
        let input_token_price_usd = if p1_input_tk.symbol == "USDC" || p1_input_tk.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

        if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 3) {
            let initial_ta = TokenAmount::from_float(input_amount_float, p1_input_tk.decimals);
            let hop1_out_ta = calculate_output_amount(p1_arc.as_ref(), initial_ta.clone(), directions[0]);
            // Adjust hop2_in_ta decimals to match p2_input_tk.decimals
            let hop2_in_ta_adjusted = TokenAmount::from_float(hop1_out_ta.to_float(), p2_input_tk.decimals);
            let hop2_out_ta = calculate_output_amount(p2_arc.as_ref(), hop2_in_ta_adjusted, directions[1]);
            // Adjust hop3_in_ta decimals
            let hop3_in_ta_adjusted = TokenAmount::from_float(hop2_out_ta.to_float(), p3_input_tk.decimals);
            let hop3_out_ta = calculate_output_amount(p3_arc.as_ref(), hop3_in_ta_adjusted, directions[2]);

            let opp_id = format!("3hop-{}-{}-{}-{}-{}", p1_input_tk.symbol, p1_arc.address, p2_arc.address, p3_arc.address, chrono::Utc::now().timestamp_millis());
            let current_opportunity = MultiHopArbOpportunity {
                id: opp_id.clone(),
                hops: vec![
                    ArbHop { dex: p1_arc.dex_type.clone(), pool: p1_arc.address, input_token: p1_input_tk.symbol.clone(), output_token: p1_output_tk.symbol.clone(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                    ArbHop { dex: p2_arc.dex_type.clone(), pool: p2_arc.address, input_token: p2_input_tk.symbol.clone(), output_token: p2_output_tk.symbol.clone(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                    ArbHop { dex: p3_arc.dex_type.clone(), pool: p3_arc.address, input_token: p3_input_tk.symbol.clone(), output_token: p3_output_tk.symbol.clone(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                ],
                total_profit: calculated_profit_float, // from multihop calc
                profit_pct: opp_calc_result.profit_percentage * 100.0,
                input_token: p1_input_tk.symbol.clone(),
                output_token: p3_output_tk.symbol.clone(), 
                input_amount: input_amount_float,
                expected_output: hop3_out_ta.to_float(), // More precise from step-by-step
                dex_path: vec![p1_arc.dex_type.clone(), p2_arc.dex_type.clone(), p3_arc.dex_type.clone()],
                pool_path: vec![p1_arc.address, p2_arc.address, p3_arc.address],
                risk_score: Some(total_slippage_fraction),
                notes: Some(format!("3-hop cyclic: Slippage: {:.4}%", total_slippage_fraction * 100.0)),
                estimated_profit_usd: Some(calculated_profit_float * input_token_price_usd),
                input_amount_usd: Some(input_amount_float * input_token_price_usd),
                output_amount_usd: Some(hop3_out_ta.to_float() * input_token_price_usd), 
                intermediate_tokens: vec![p1_output_tk.symbol.clone(), p2_output_tk.symbol.clone()],
                source_pool: Arc::clone(p1_arc),
                target_pool: Arc::clone(p3_arc), 
                input_token_mint: p1_input_tk.mint,
                output_token_mint: p3_output_tk.mint, 
                intermediate_token_mint: Some(p1_output_tk.mint), 
            };
            info!("Found Profitable 3-hop Arb Opp ID: {}", opp_id);
            current_opportunity.log_summary();
            
            let dex_path_strings_log: Vec<String> = current_opportunity.dex_path.iter().map(|d| format!("{:?}",d)).collect();
            if let Err(e) = metrics.record_opportunity_detected(
                &current_opportunity.input_token, current_opportunity.intermediate_tokens.get(0).map_or("", |s|s.as_str()),
                current_opportunity.profit_pct, current_opportunity.estimated_profit_usd, current_opportunity.input_amount_usd, dex_path_strings_log,
            ){ error!("Failed to record 3-hop opportunity metric: {}", e); }
            opportunities.push(current_opportunity);
        } else {
            debug!("  Skipped 3-hop path for P1:{}, P2:{}, P3:{}: Profit {:.4}% (raw result profit: {:.6}, price impact: {:.4}) below threshold or too costly.", 
                    p1_arc.name, p2_arc.name, p3_arc.name,
                    opp_calc_result.profit_percentage*100.0, opp_calc_result.profit, opp_calc_result.price_impact);
        }
    }

    pub async fn find_all_multihop_opportunities_with_risk(
        &self, 
        pools: &HashMap<Pubkey, Arc<PoolInfo>>, 
        metrics: &mut Metrics, 
        max_slippage_pct_config: f64, // Expecting percentage e.g. 0.5 for 0.5%
        max_tx_fee_lamports_for_acceptance: u64 
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("find_all_multihop_opportunities_with_risk called. Max Slippage Pct: {:.2}%, Max Tx Fee (lamports): {}", max_slippage_pct_config, max_tx_fee_lamports_for_acceptance);
        let base_opportunities = self.find_all_multihop_opportunities(pools, metrics).await?;
        let mut risk_adjusted_opportunities = Vec::new();

        for opp in base_opportunities {
            let estimated_total_slippage_fraction = opp.risk_score.unwrap_or(1.0); // Default to high if not set
            
            let estimated_instructions_per_hop = 2; 
            let total_instructions = opp.hops.len() * estimated_instructions_per_hop;
            
            let transaction_cost_usd = calculate_transaction_cost(
                total_instructions, 
                self.default_priority_fee_lamports, 
                self.sol_price_usd
            );
            
            let max_tx_fee_usd_for_acceptance = (max_tx_fee_lamports_for_acceptance as f64 / 1_000_000_000.0) * self.sol_price_usd;

            // Compare slippage fraction (0.0 to 1.0) with config (e.g. 0.5 for 0.5% means 0.005 fraction)
            if estimated_total_slippage_fraction <= (max_slippage_pct_config / 100.0) && 
               transaction_cost_usd <= max_tx_fee_usd_for_acceptance {
                info!("Opportunity ID {} meets risk/fee criteria. Slippage: {:.4}%, TxCostUSD: {:.4}", opp.id, estimated_total_slippage_fraction * 100.0, transaction_cost_usd);
                risk_adjusted_opportunities.push(opp);
            } else {
                debug!("Skipping opportunity ID {} due to risk/fee. Slippage: {:.4}% (Max: {:.2}%), TxCostUSD: {:.4} (MaxAcceptable: {:.4})", 
                    opp.id, estimated_total_slippage_fraction * 100.0, max_slippage_pct_config, transaction_cost_usd, max_tx_fee_usd_for_acceptance);
            }
        }
        
        info!("Found {} multi-hop arbitrage opportunities (risk-adjusted).", risk_adjusted_opportunities.len());
        Ok(risk_adjusted_opportunities)
    }
}