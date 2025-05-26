// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{
            calculate_max_profit_result, calculate_optimal_input, calculate_transaction_cost,
            is_profitable as is_profitable_calc, // Renamed to avoid conflict
            OpportunityCalculationResult, calculate_multihop_profit_and_slippage,
        },
        // Removed unused: fee_manager::{FeeManager, XYKSlippageModel, FeeEstimationResult},
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{PoolInfo, TokenAmount, DexType, calculate_output_amount}, // Added calculate_output_amount, DexType
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
    min_profit_threshold_pct: f64,    // Store as percentage, e.g., 0.5 for 0.5%
    min_profit_threshold_usd: f64,   // Absolute USD profit threshold
    sol_price_usd: f64,              // For cost calculation
    default_priority_fee_lamports: u64, // For cost calculation
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
    
    // Helper to check profitability considering costs
    fn is_opportunity_profitable_after_costs(
        &self,
        opp_calc_result: &OpportunityCalculationResult,
        input_token_price_usd: f64, // Price of the specific input token for the opportunity
        num_hops: usize,
    ) -> bool {
        let transaction_cost_usd = calculate_transaction_cost(
            num_hops * 2, // Rough estimate of instructions per hop (e.g., budget + swap)
            self.default_priority_fee_lamports,
            self.sol_price_usd,
        );

        // Check against percentage threshold (on token profit)
        let meets_pct_threshold = opp_calc_result.profit_percentage * 100.0 >= self.min_profit_threshold_pct;

        // Check against absolute USD threshold (after costs)
        let meets_usd_threshold = is_profitable_calc(
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
        // Standardize on using the CSV file in the `src` directory if no other path is configured.
        // For consistency, ensure this path matches where the test might expect it or where the bot runs.
        // Assuming "banned_pairs_log.csv" is in the current working directory or src.
        // For robustness, this should ideally use a path from Config.
        let log_file_path = "banned_pairs_log.csv"; // TODO: Make this configurable
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
        let log_file_path = "banned_pairs_log.csv"; // TODO: Make this configurable
        if let Ok(content) = std::fs::read_to_string(log_file_path) {
            for line in content.lines().skip(1) { // Skip header
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
        let log_file_path = "banned_pairs_log.csv"; // TODO: Make this configurable
        if let Ok(content) = std::fs::read_to_string(log_file_path) {
            let now = chrono::Utc::now().timestamp() as u64;
            for line in content.lines().skip(1) { // Skip header
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

            // Try starting with token_a from src_pool
            let input_mint_leg1 = src_pool.token_a.mint;
            let input_symbol_leg1 = &src_pool.token_a.symbol;
            let input_decimals_leg1 = src_pool.token_a.decimals;
            let intermediate_mint_leg1 = src_pool.token_b.mint;
            let intermediate_symbol_leg1 = &src_pool.token_b.symbol;

            if Self::is_permanently_banned(input_symbol_leg1, intermediate_symbol_leg1) || 
               Self::is_temporarily_banned(input_symbol_leg1, intermediate_symbol_leg1) {
                debug!("Skipping banned pair for first hop (A->B): {} ({}) <-> {} ({}) in pool {}", input_symbol_leg1, input_mint_leg1, intermediate_symbol_leg1, intermediate_mint_leg1, src_pool.name);
                continue;
            }
            
            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc, &input_mint_leg1, input_symbol_leg1, input_decimals_leg1,
                &intermediate_mint_leg1, intermediate_symbol_leg1, true, // is_a_to_b for src_pool = true
                &mut opportunities, metrics
            ).await;

            // Try starting with token_b from src_pool
            let input_mint_leg1_alt = src_pool.token_b.mint;
            let input_symbol_leg1_alt = &src_pool.token_b.symbol;
            let input_decimals_leg1_alt = src_pool.token_b.decimals;
            let intermediate_mint_leg1_alt = src_pool.token_a.mint;
            let intermediate_symbol_leg1_alt = &src_pool.token_a.symbol;

             if Self::is_permanently_banned(input_symbol_leg1_alt, intermediate_symbol_leg1_alt) || 
                Self::is_temporarily_banned(input_symbol_leg1_alt, intermediate_symbol_leg1_alt) {
                debug!("Skipping banned pair for first hop (B->A): {} ({}) <-> {} ({}) in pool {}", input_symbol_leg1_alt, input_mint_leg1_alt, intermediate_symbol_leg1_alt, intermediate_mint_leg1_alt, src_pool.name);
                continue;
            }

            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc, &input_mint_leg1_alt, input_symbol_leg1_alt, input_decimals_leg1_alt,
                &intermediate_mint_leg1_alt, intermediate_symbol_leg1_alt, false, // is_a_to_b for src_pool = false
                &mut opportunities, metrics
            ).await;
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} direct (2-hop cyclic) arbitrage opportunities.", opportunities.len());
        if opportunities.is_empty() && pools.len() >= 2 { // Added a check to see if pools exist for opportunities
             debug!("No 2-hop opportunities found. Pool states might not allow profitable cycles at current thresholds, or test pool setup needs review for mint consistency.");
        }
        Ok(opportunities)
    }

    async fn find_cyclic_pair_for_src_hop(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        src_pool_arc: &Arc<PoolInfo>,
        input_mint_leg1: &Pubkey,
        input_symbol_leg1: &str,
        input_decimals_leg1: u8,
        intermediate_mint_leg1: &Pubkey,
        intermediate_symbol_leg1: &str,
        src_pool_is_a_to_b: bool, // Direction of trade in src_pool (input_mint_leg1 -> intermediate_mint_leg1)
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        let src_pool = src_pool_arc.as_ref();
        debug!("Attempting 2-hop: Pool1='{}'({}), {} ({}) -> {} ({})", src_pool.name, src_pool.address, input_symbol_leg1, input_mint_leg1, intermediate_symbol_leg1, intermediate_mint_leg1);

        for (_tgt_pool_addr, tgt_pool_arc) in pools.iter() {
            if src_pool.address == tgt_pool_arc.address { continue; } // Skip same pool
            let tgt_pool = tgt_pool_arc.as_ref();

            // Leg 2: Must trade intermediate_mint_leg1 -> input_mint_leg1 (to complete the cycle)
            let tgt_pool_trades_intermediate_to_input_a_to_b = 
                tgt_pool.token_a.mint == *intermediate_mint_leg1 && tgt_pool.token_b.mint == *input_mint_leg1;
            let tgt_pool_trades_intermediate_to_input_b_to_a = 
                tgt_pool.token_b.mint == *intermediate_mint_leg1 && tgt_pool.token_a.mint == *input_mint_leg1;

            if !(tgt_pool_trades_intermediate_to_input_a_to_b || tgt_pool_trades_intermediate_to_input_b_to_a) {
                continue; // tgt_pool doesn't complete the cycle
            }
            let tgt_pool_is_a_to_b = tgt_pool_trades_intermediate_to_input_a_to_b;
            
            debug!("  Candidate Pair: Pool1='{}', Pool2='{}'({}). Checking for cycle.", src_pool.name, tgt_pool.name, tgt_pool.address);

            if Self::is_permanently_banned(intermediate_symbol_leg1, input_symbol_leg1) || 
               Self::is_temporarily_banned(intermediate_symbol_leg1, input_symbol_leg1) {
                debug!("Skipping banned pair for second hop: {} <-> {}", intermediate_symbol_leg1, input_symbol_leg1);
                continue;
            }

            // Max input for calculation, e.g., 1000 units of the input token (adjust based on typical values or config)
            // Assuming input token is SOL or USDC-like for this example value
            let base_units = if input_decimals_leg1 >= 6 { 1_000 * 10u64.pow(input_decimals_leg1 as u32) } else { 1_000_000 * 10u64.pow(input_decimals_leg1 as u32) };
            let max_input_token_amount = TokenAmount::new(base_units, input_decimals_leg1);
            
            let optimal_in_atomic = calculate_optimal_input(src_pool, tgt_pool, src_pool_is_a_to_b, max_input_token_amount);
            if optimal_in_atomic.amount == 0 {
                debug!("Optimal input amount is 0, skipping for src_pool {}, tgt_pool {}", src_pool.name, tgt_pool.name);
                continue;
            }

            let opp_calc_result = calculate_max_profit_result(src_pool, tgt_pool, src_pool_is_a_to_b, optimal_in_atomic.clone());
            
            // Price of input token (e.g. SOL or USDC) in USD. For now, assume 1.0 if it's stablecoin-like, or use self.sol_price_usd
            // This needs to be more robust, ideally from a price feed or config.
            let input_token_price_usd = if input_symbol_leg1 == "USDC" || input_symbol_leg1 == "USDT" { 1.0 } else { self.sol_price_usd };

            if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 2) {
                 let input_amount_float = opp_calc_result.input_amount;
                 let expected_intermediate_float = calculate_output_amount(src_pool, TokenAmount::from_float(input_amount_float, input_decimals_leg1), src_pool_is_a_to_b).to_float();
                 let final_output_float = opp_calc_result.output_amount;

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
                            output_token: input_symbol_leg1.to_string(), // output is original input for cycle
                            input_amount: expected_intermediate_float,
                            expected_output: final_output_float,
                        },
                    ],
                    total_profit: opp_calc_result.profit,
                    profit_pct: opp_calc_result.profit_percentage * 100.0, // Convert fraction to percentage
                    input_token: input_symbol_leg1.to_string(),
                    output_token: input_symbol_leg1.to_string(), // Cyclic
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
                    output_token_mint: *input_mint_leg1, // Cyclic
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
                 debug!("  Skipped 2-hop cycle (src='{}', tgt='{}', input_token='{}'): profit_pct {:.4}% (raw result: {:?}) below threshold or too costly.",
                       src_pool.name, tgt_pool.name, input_symbol_leg1, opp_calc_result.profit_percentage * 100.0, opp_calc_result);
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
        let pool_vec: Vec<Arc<PoolInfo>> = pools.values().cloned().collect(); // Collect Arcs

        if pool_vec.len() < 3 { return Ok(opportunities); }

        for i in 0..pool_vec.len() {
            for j in 0..pool_vec.len() {
                if i == j { continue; }
                for k in 0..pool_vec.len() {
                    if k == i || k == j { continue; }

                    let p1_arc = &pool_vec[i]; let p2_arc = &pool_vec[j]; let p3_arc = &pool_vec[k];
                    let p1 = p1_arc.as_ref(); let p2 = p2_arc.as_ref(); let p3 = p3_arc.as_ref();

                    // Try P1(A->B), P2(B->C), P3(C->A)
                    self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1.token_a, &p1.token_b, // Leg 1: p1.A -> p1.B
                        &p2.token_a, &p2.token_b, // Leg 2: p2.A -> p2.B (hoping p2.A == p1.B)
                        &p3.token_a, &p3.token_b, // Leg 3: p3.A -> p3.B (hoping p3.A == p2.B AND p3.B == p1.A)
                        &mut opportunities, metrics, pools,
                    ).await;
                    
                    // Try P1(B->A), P2(A->C), P3(C->B) - one example of reversing first leg
                     self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1.token_b, &p1.token_a, // Leg 1: p1.B -> p1.A
                        &p2.token_a, &p2.token_b, // Leg 2: p2.A -> p2.B (hoping p2.A == p1.A)
                        &p3.token_a, &p3.token_b, // Leg 3: p3.A -> p3.B (hoping p3.A == p2.B AND p3.B == p1.B)
                        &mut opportunities, metrics, pools,
                    ).await;
                    // ... Add more permutations or a more systematic path generation if needed
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
        // Leg 1 (Pool 1)
        p1_input_token: &crate::utils::PoolToken, p1_output_token: &crate::utils::PoolToken,
        // Leg 2 (Pool 2)
        p2_input_token: &crate::utils::PoolToken, p2_output_token: &crate::utils::PoolToken,
        // Leg 3 (Pool 3)
        p3_input_token: &crate::utils::PoolToken, p3_output_token: &crate::utils::PoolToken,
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
        _all_pools: &HashMap<Pubkey, Arc<PoolInfo>>, // Keep for potential future use with FeeManager advanced features
    ) {
        // Check for valid path:
        // Output of Leg 1 (p1_output_token) must be Input of Leg 2 (p2_input_token)
        // Output of Leg 2 (p2_output_token) must be Input of Leg 3 (p3_input_token)
        // Output of Leg 3 (p3_output_token) must be Input of Leg 1 (p1_input_token) - cyclic
        if !(p1_output_token.mint == p2_input_token.mint &&
             p2_output_token.mint == p3_input_token.mint &&
             p3_output_token.mint == p1_input_token.mint) {
            return; // Not a valid 3-hop cycle with these token choices
        }

        debug!("Checking 3-hop: {}->{} (P1:{}), {}->{} (P2:{}), {}->{} (P3:{})",
            p1_input_token.symbol, p1_output_token.symbol, p1_arc.name,
            p2_input_token.symbol, p2_output_token.symbol, p2_arc.name,
            p3_input_token.symbol, p3_output_token.symbol, p3_arc.name);
            
        if Self::is_permanently_banned(&p1_input_token.symbol, &p1_output_token.symbol) || Self::is_temporarily_banned(&p1_input_token.symbol, &p1_output_token.symbol) ||
           Self::is_permanently_banned(&p2_input_token.symbol, &p2_output_token.symbol) || Self::is_temporarily_banned(&p2_input_token.symbol, &p2_output_token.symbol) ||
           Self::is_permanently_banned(&p3_input_token.symbol, &p3_output_token.symbol) || Self::is_temporarily_banned(&p3_input_token.symbol, &p3_output_token.symbol) {
            debug!("Skipping 3-hop due to banned pair in path.");
            return;
        }

        let pools_path_refs: Vec<&PoolInfo> = vec![p1_arc.as_ref(), p2_arc.as_ref(), p3_arc.as_ref()];
        let directions = vec![
            p1_arc.token_a.mint == p1_input_token.mint, // dir for p1
            p2_arc.token_a.mint == p2_input_token.mint, // dir for p2
            p3_arc.token_a.mint == p3_input_token.mint, // dir for p3
        ];
        
        // Placeholder for historical fee data - FeeManager needs this
        let last_fee_data = vec![(None,None,None); 3]; 

        // Example input amount for calculation - make this configurable or dynamic
        let input_amount_float = 100.0; // e.g., 100 units of the starting token (p1_input_token)
        
        let (calculated_profit_float, total_slippage_fraction, _total_fee_tokens_equivalent) =
            calculate_multihop_profit_and_slippage(
                &pools_path_refs,
                input_amount_float,
                p1_input_token.decimals,
                &directions,
                &last_fee_data,
            );

        let opp_calc_result = OpportunityCalculationResult {
            input_amount: input_amount_float,
            output_amount: input_amount_float + calculated_profit_float,
            profit: calculated_profit_float,
            profit_percentage: if input_amount_float > 0.0 { calculated_profit_float / input_amount_float } else {0.0},
            price_impact: total_slippage_fraction,
        };
        
        let input_token_price_usd = if p1_input_token.symbol == "USDC" || p1_input_token.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

        if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 3) {
            // Simulate outputs for ArbHop details (simplified, real calc from multihop used for profit)
            let initial_ta = TokenAmount::from_float(input_amount_float, p1_input_token.decimals);
            let hop1_out_ta = calculate_output_amount(p1_arc.as_ref(), initial_ta.clone(), directions[0]);
            let hop2_out_ta = calculate_output_amount(p2_arc.as_ref(), hop1_out_ta.clone(), directions[1]);
            // hop3_out_ta is effectively opp_calc_result.output_amount converted to TokenAmount
            let hop3_out_ta = TokenAmount::from_float(opp_calc_result.output_amount, p3_output_token.decimals);


            let opp_id = format!("3hop-{}-{}-{}-{}-{}", p1_input_token.symbol, p1_arc.address, p2_arc.address, p3_arc.address, chrono::Utc::now().timestamp_millis());
            let current_opportunity = MultiHopArbOpportunity {
                id: opp_id.clone(),
                hops: vec![
                    ArbHop { dex: p1_arc.dex_type.clone(), pool: p1_arc.address, input_token: p1_input_token.symbol.clone(), output_token: p1_output_token.symbol.clone(), input_amount: input_amount_float, expected_output: hop1_out_ta.to_float() },
                    ArbHop { dex: p2_arc.dex_type.clone(), pool: p2_arc.address, input_token: p2_input_token.symbol.clone(), output_token: p2_output_token.symbol.clone(), input_amount: hop1_out_ta.to_float(), expected_output: hop2_out_ta.to_float() },
                    ArbHop { dex: p3_arc.dex_type.clone(), pool: p3_arc.address, input_token: p3_input_token.symbol.clone(), output_token: p3_output_token.symbol.clone(), input_amount: hop2_out_ta.to_float(), expected_output: hop3_out_ta.to_float() },
                ],
                total_profit: calculated_profit_float,
                profit_pct: opp_calc_result.profit_percentage * 100.0,
                input_token: p1_input_token.symbol.clone(),
                output_token: p3_output_token.symbol.clone(), // Should be same as p1_input_token.symbol for cyclic
                input_amount: input_amount_float,
                expected_output: hop3_out_ta.to_float(),
                dex_path: vec![p1_arc.dex_type.clone(), p2_arc.dex_type.clone(), p3_arc.dex_type.clone()],
                pool_path: vec![p1_arc.address, p2_arc.address, p3_arc.address],
                risk_score: Some(total_slippage_fraction),
                notes: Some(format!("3-hop cyclic: Slippage: {:.4}%", total_slippage_fraction * 100.0)),
                estimated_profit_usd: Some(calculated_profit_float * input_token_price_usd),
                input_amount_usd: Some(input_amount_float * input_token_price_usd),
                output_amount_usd: Some(hop3_out_ta.to_float() * input_token_price_usd), // Assuming output token is same type for USD price
                intermediate_tokens: vec![p1_output_token.symbol.clone(), p2_output_token.symbol.clone()],
                source_pool: Arc::clone(p1_arc),
                target_pool: Arc::clone(p3_arc), // Last pool in the chain
                input_token_mint: p1_input_token.mint,
                output_token_mint: p3_output_token.mint, // Should be same as p1_input_token.mint
                intermediate_token_mint: Some(p1_output_token.mint), // First intermediate token
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
            debug!("  Skipped 3-hop path: Profit {:.4}% (raw result: {:?}) below threshold or too costly.", opp_calc_result.profit_percentage*100.0, opp_calc_result);
        }
    }


    pub async fn find_all_multihop_opportunities_with_risk(
        &self, 
        pools: &HashMap<Pubkey, Arc<PoolInfo>>, 
        metrics: &mut Metrics, 
        max_slippage_pct: f64, 
        max_tx_fee_lamports_for_acceptance: u64 // Max fee we are willing to pay for this opp
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("find_all_multihop_opportunities_with_risk called. Max Slippage Pct: {:.2}%, Max Tx Fee (lamports): {}", max_slippage_pct, max_tx_fee_lamports_for_acceptance);
        let base_opportunities = self.find_all_multihop_opportunities(pools, metrics).await?;
        let mut risk_adjusted_opportunities = Vec::new();

        for opp in base_opportunities {
            let estimated_total_slippage_fraction = opp.risk_score.unwrap_or(1.0); // Default to high if not set

            // Use a fixed number of instructions per hop for cost estimation for now
            let estimated_instructions_per_hop = 2; 
            let total_instructions = opp.hops.len() * estimated_instructions_per_hop;
            
            // Use the detector's default_priority_fee_lamports as the base for this specific opportunity's fee check.
            // If the opportunity itself carries a specific suggested priority fee, that could be used too.
            let transaction_cost_usd = calculate_transaction_cost(
                total_instructions, 
                self.default_priority_fee_lamports, // Use the executor's configured base priority fee
                self.sol_price_usd
            );
            
            // Convert max_tx_fee_lamports_for_acceptance to USD for comparison
            let max_tx_fee_usd_for_acceptance = (max_tx_fee_lamports_for_acceptance as f64 / 1_000_000_000.0) * self.sol_price_usd;

            if estimated_total_slippage_fraction * 100.0 <= max_slippage_pct && transaction_cost_usd <= max_tx_fee_usd_for_acceptance {
                info!("Opportunity ID {} meets risk/fee criteria. Slippage: {:.4}%, TxCostUSD: {:.4}", opp.id, estimated_total_slippage_fraction * 100.0, transaction_cost_usd);
                risk_adjusted_opportunities.push(opp);
            } else {
                debug!("Skipping opportunity ID {} due to risk/fee. Slippage: {:.4}% (Max: {:.2}%), TxCostUSD: {:.4} (MaxAcceptable: {:.4})", 
                    opp.id, estimated_total_slippage_fraction * 100.0, max_slippage_pct, transaction_cost_usd, max_tx_fee_usd_for_acceptance);
            }
        }
        
        info!("Found {} multi-hop arbitrage opportunities (risk-adjusted).", risk_adjusted_opportunities.len());
        Ok(risk_adjusted_opportunities)
    }
}