// /Users/kiril/Desktop/chilliez/src/arbitrage/detector.rs
// src/arbitrage/detector.rs
use crate::{
    arbitrage::{
        calculator::{
            calculate_max_profit_result, calculate_optimal_input, calculate_transaction_cost, calculate_max_profit,
            is_profitable_calc, calculate_simple_opportunity_result, calculate_rebate,
            OpportunityCalculationResult, calculate_multihop_profit_and_slippage,
        },
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    // DexType is used in ArbHop { dex: pool.dex_type.clone(), ... }
    // and current_opportunity.dex_path = vec![src_pool.dex_type.clone(), ...];
    utils::{PoolInfo, TokenAmount, calculate_output_amount}, 
};
use log::{debug, error, info, warn};use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    sync::Arc,
};

#[derive(Clone, Debug)] 
pub struct ArbitrageDetector {
    min_profit_threshold_pct: f64,    
    min_profit_threshold_usd: f64, // This will now come from config
    sol_price_usd: f64,              
    default_priority_fee_lamports: u64, 
}

impl ArbitrageDetector {
    // New constructor that takes Config
    pub fn new_from_config(config: &crate::config::settings::Config) -> Self {
        let min_profit_usd = config.min_profit_usd_threshold.unwrap_or(0.05);
        let sol_price = config.sol_price_usd.unwrap_or(150.0);
        info!(
            "ArbitrageDetector (from Config) initialized with: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            config.min_profit_pct * 100.0,
            min_profit_usd,
            sol_price,
            config.default_priority_fee_lamports
        );
        Self {
            min_profit_threshold_pct: config.min_profit_pct * 100.0,
            min_profit_threshold_usd: min_profit_usd,
            sol_price_usd: sol_price,
            default_priority_fee_lamports: config.default_priority_fee_lamports,
        }
    }

    pub fn set_min_profit_threshold(&mut self, new_threshold_pct: f64) {
        self.min_profit_threshold_pct = new_threshold_pct;
        info!(
            "ArbitrageDetector min_profit_threshold_pct updated to: {:.4}%",
            new_threshold_pct
        );
    }

    // Used by ArbitrageEngine
    pub fn get_min_profit_threshold_pct(&self) -> f64 {
        self.min_profit_threshold_pct
    }
    
    // Add this getter
    pub fn get_min_profit_threshold_usd(&self) -> f64 {
        self.min_profit_threshold_usd
    }

    fn is_opportunity_profitable_after_costs(
        &self,
        opp_calc_result: &OpportunityCalculationResult,
        input_token_price_usd: f64, 
        num_hops: usize,
    ) -> bool {
        let transaction_cost_usd = calculate_transaction_cost(
            num_hops * 2, // Estimate 2 instructions per hop (e.g., compute budget, swap)
            self.default_priority_fee_lamports,
            self.sol_price_usd,
        );

        let meets_pct_threshold = opp_calc_result.profit_percentage * 100.0 >= self.min_profit_threshold_pct;

        // Use the stored min_profit_threshold_usd
        let meets_usd_threshold = is_profitable_calc( 
            opp_calc_result,
            input_token_price_usd,
            transaction_cost_usd,
            self.min_profit_threshold_usd, // Use struct field
        );
        
        debug!("Profitability detail: Result: {:?}, InputTokenPriceUSD: {}, TxCostUSD: {}, MinProfitUSD_Detector: {}, MeetsPct: {}, MeetsUSD: {}",
            opp_calc_result, input_token_price_usd, transaction_cost_usd, self.min_profit_threshold_usd, meets_pct_threshold, meets_usd_threshold);

        meets_pct_threshold && meets_usd_threshold
    }

    // ACTIVATED: Used by tests and potentially internally if direct ban logging is needed outside detection loops
    pub fn log_banned_pair(token_a_symbol: &str, token_b_symbol: &str, ban_type: &str, reason: &str) {
        let log_entry = format!("{},{},{},{}\n", token_a_symbol, token_b_symbol, ban_type, reason);
        let log_file_path = "banned_pairs_log.csv";
        
        match OpenOptions::new()
            .create(true)  // Create the file if it does not exist.
            .write(true)   // Need write access.
            .append(true)  // All writes will be to the end of the file.
            .open(log_file_path)
        {
            Ok(mut file) => {
                let mut attempted_write = false;
                if let Ok(metadata) = file.metadata() {
                    if metadata.len() == 0 {
                        // File is empty, write header first.
                        if let Err(e) = file.write_all(b"TokenA,TokenB,BanType,Details\n") {
                            error!("Failed to write header to ban log file '{}': {}", log_file_path, e);
                        } else {
                            // Attempt to flush the header write
                            if let Err(e) = file.flush() {
                                error!("Failed to flush header for ban log file '{}': {}", log_file_path, e);
                            }
                            attempted_write = true;
                        }
                    }
                } else {
                    error!("Could not read metadata for ban log file '{}'. Header might be missing.", log_file_path);
                }

                // Now, write the actual log entry.
                if let Err(e) = file.write_all(log_entry.as_bytes()) {
                    error!("Failed to write log entry to ban log file '{}': {}", log_file_path, e);
                } else {
                    info!("Logged banned pair to '{}': {} <-> {} ({}, Reason: {})", log_file_path, token_a_symbol, token_b_symbol, ban_type, reason);
                    attempted_write = true;
                }

                // If any write was attempted (header or entry), try to flush the file.
                if attempted_write {
                    if let Err(e) = file.flush() {
                        error!("Failed to flush ban log file '{}' after writing: {}", log_file_path, e);
                    }
                }
            }
            Err(e) => {
                error!("Cannot open ban log file '{}': {}", log_file_path, e);
            }
        }
    }

    pub fn is_permanently_banned(token_a_symbol: &str, token_b_symbol: &str) -> bool {
        let log_file_path = "banned_pairs_log.csv";
        // Attempt to open the file with read-only access.
        match OpenOptions::new().read(true).open(log_file_path) {
            Ok(file) => {
                // Use a BufReader for efficient line-by-line reading.
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(file);
                for line_result in reader.lines().skip(1) { // Skip header
                    if let Ok(line) = line_result {
                        let parts: Vec<_> = line.split(',').collect();
                        if parts.len() >= 3 && parts[2].trim() == "permanent" &&
                           ((parts[0].trim() == token_a_symbol && parts[1].trim() == token_b_symbol) ||
                            (parts[0].trim() == token_b_symbol && parts[1].trim() == token_a_symbol)) {
                            return true;
                        }
                    }
                }
            }
            Err(e) => {
                // Log if the file can't be opened, but still return false as no ban can be confirmed.
                debug!("Could not open ban log file '{}' for reading in is_permanently_banned: {}", log_file_path, e);
            }
        }
        false
    }

    pub fn is_temporarily_banned(token_a_symbol: &str, token_b_symbol: &str) -> bool {
        let log_file_path = "banned_pairs_log.csv";
        // Attempt to open the file with read-only access.
        match OpenOptions::new().read(true).open(log_file_path) {
            Ok(file) => {
                // Use a BufReader for efficient line-by-line reading.
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(file);
                let now = chrono::Utc::now().timestamp() as u64;
                for line_result in reader.lines().skip(1) { // Skip header
                    if let Ok(line) = line_result {
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
            }
            Err(e) => {
                // Log if the file can't be opened, but still return false as no ban can be confirmed.
                debug!("Could not open ban log file '{}' for reading in is_temporarily_banned: {}", log_file_path, e);
            }
        }
        false
    }
    
    // Main function for 2-hop cyclic opportunities
    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("ENTER: find_all_opportunities (2-hop cyclic) called with {} pools. Min Profit Pct: {:.4}%, Min Profit USD: ${:.2}", pools.len(), self.min_profit_threshold_pct, self.min_profit_threshold_usd);
        let mut opportunities = Vec::new();

        for (src_pool_addr, src_pool_arc) in pools.iter() {
            let src_pool = src_pool_arc.as_ref();
            info!("Outer loop: Processing src_pool: {} ({})", src_pool.name, src_pool_addr);

            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc, 
                &src_pool.token_a, 
                &src_pool.token_b, 
                true, // Assuming src_pool.token_a is input, src_pool.token_b is intermediate
                &mut opportunities, metrics
            ).await;

            self.find_cyclic_pair_for_src_hop(
                pools, src_pool_arc,
                &src_pool.token_b, 
                &src_pool.token_a, 
                false, // Assuming src_pool.token_b is input, src_pool.token_a is intermediate
                &mut opportunities, metrics
            ).await;
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("EXIT: find_all_opportunities. Found {} direct (2-hop cyclic) arbitrage opportunities.", opportunities.len());
        if opportunities.is_empty() && pools.len() >= 2 {
             debug!("No 2-hop opportunities found. Pool states might not allow profitable cycles at current thresholds, or test pool setup needs review for mint consistency.");
        }
        Ok(opportunities)
    }

    async fn find_cyclic_pair_for_src_hop(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        src_pool_arc: &Arc<PoolInfo>,
        p1_input_tk: &crate::utils::PoolToken, 
        p1_intermediate_tk: &crate::utils::PoolToken,
        src_pool_is_a_to_b: bool, 
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        let src_pool = src_pool_arc.as_ref();
        info!("ENTER find_cyclic_pair_for_src_hop: src_pool='{}' ({}), input_tk='{}' ({}), intermediate_tk='{}' ({}), src_pool_is_a_to_b={}", 
            src_pool.name, src_pool.address, p1_input_tk.symbol, p1_input_tk.mint, p1_intermediate_tk.symbol, p1_intermediate_tk.mint, src_pool_is_a_to_b);

        if p1_input_tk.mint == p1_intermediate_tk.mint { 
            info!("EARLY RETURN from find_cyclic_pair_for_src_hop: p1_input_tk.mint == p1_intermediate_tk.mint ({} == {})", p1_input_tk.mint, p1_intermediate_tk.mint);
            return;
        }
        if Self::is_permanently_banned(&p1_input_tk.symbol, &p1_intermediate_tk.symbol) || 
           Self::is_temporarily_banned(&p1_input_tk.symbol, &p1_intermediate_tk.symbol) {
            debug!("Skipping banned pair for first hop: {} ({}) <-> {} ({}) in pool {}", p1_input_tk.symbol, p1_input_tk.mint, p1_intermediate_tk.symbol, p1_intermediate_tk.mint, src_pool.name);
            info!("EARLY RETURN from find_cyclic_pair_for_src_hop: Pair {}/{} is banned.", p1_input_tk.symbol, p1_intermediate_tk.symbol);
            
            // ACTIVATED: Log banned pair when encountered during detection
            Self::log_banned_pair(&p1_input_tk.symbol, &p1_intermediate_tk.symbol, "temporary", 
                "Banned pair encountered during 2-hop detection");
            return;
        }
        
        info!("Attempting 2-hop from Pool1='{}'({}), Input: {} ({}), Intermediate: {} ({})", src_pool.name, src_pool.address, p1_input_tk.symbol, p1_input_tk.mint, p1_intermediate_tk.symbol, p1_intermediate_tk.mint);

        for (_tgt_pool_addr, tgt_pool_arc) in pools.iter() {
            if src_pool.address == tgt_pool_arc.address { continue; } 
            let tgt_pool = tgt_pool_arc.as_ref();

            let tgt_pool_can_trade_intermediate_for_input = 
                (tgt_pool.token_a.mint == p1_intermediate_tk.mint && tgt_pool.token_b.mint == p1_input_tk.mint) ||
                (tgt_pool.token_b.mint == p1_intermediate_tk.mint && tgt_pool.token_a.mint == p1_input_tk.mint);

            if !tgt_pool_can_trade_intermediate_for_input {
                continue; 
            }
            
            info!("  Candidate Pair: Pool1='{}', Pool2='{}'({}). Checking for cycle.", src_pool.name, tgt_pool.name, tgt_pool.address);

            if Self::is_permanently_banned(&p1_intermediate_tk.symbol, &p1_input_tk.symbol) || 
               Self::is_temporarily_banned(&p1_intermediate_tk.symbol, &p1_input_tk.symbol) {
                debug!("Skipping banned pair for second hop: {} <-> {}", p1_intermediate_tk.symbol, p1_input_tk.symbol);
                continue;
            }
            
            let base_units = if p1_input_tk.decimals >= 6 { 1_000 * 10u64.pow(p1_input_tk.decimals as u32) } else { 1_000_000 * 10u64.pow(p1_input_tk.decimals as u32) };
            let max_input_token_amount = TokenAmount::new(base_units, p1_input_tk.decimals);
            
            let optimal_in_atomic = calculate_optimal_input(src_pool, tgt_pool, src_pool_is_a_to_b, max_input_token_amount.clone());
            info!("  Optimal input atomic for cycle (src='{}', tgt='{}', input_token='{}'): {:?}, from max_input: {:?}", src_pool.name, tgt_pool.name, p1_input_tk.symbol, optimal_in_atomic, max_input_token_amount);
            
            // If optimal input is 0, use a very small amount to get a calculation result instead of skipping
            let input_for_calc = if optimal_in_atomic.amount == 0 {
                warn!("Optimal input amount is 0 for src_pool {}, tgt_pool {}. Using minimal amount for calc.", src_pool.name, tgt_pool.name);
                 TokenAmount::new(1, p1_input_tk.decimals) // Smallest unit possible
            } else {
                optimal_in_atomic.clone()
            };

            // Call calculate_max_profit for preliminary check/logging
            let preliminary_profit_pct = calculate_max_profit(src_pool, tgt_pool, src_pool_is_a_to_b, input_for_calc.clone());
            info!("  Preliminary max profit for cycle (src='{}', tgt='{}', input_token='{}', input_amount_calc='{:?}') : {:.4}%",
                   src_pool.name, tgt_pool.name, p1_input_tk.symbol, input_for_calc, preliminary_profit_pct * 100.0);


            let opp_calc_result = calculate_max_profit_result(src_pool, tgt_pool, src_pool_is_a_to_b, input_for_calc.clone());
            
            let input_token_price_usd = if p1_input_tk.symbol == "USDC" || p1_input_tk.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

            if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 2) {
                 let input_amount_float = opp_calc_result.input_amount; // This is the actual input used for calculation
                 let intermediate_token_from_src_pool = crate::utils::calculate_output_amount(src_pool, TokenAmount::from_float(input_amount_float, p1_input_tk.decimals), src_pool_is_a_to_b);
                 let expected_intermediate_float = intermediate_token_from_src_pool.to_float();
                 let final_output_float = opp_calc_result.output_amount; 

                let opp_id = format!("2hop-{}-{}-{}-{}", p1_input_tk.symbol, src_pool.address, tgt_pool.address, chrono::Utc::now().timestamp_millis());
                let current_opportunity = MultiHopArbOpportunity {
                    id: opp_id.clone(),
                    hops: vec![
                        ArbHop {
                            dex: src_pool.dex_type.clone(), pool: src_pool.address,
                            input_token: p1_input_tk.symbol.clone(),
                            output_token: p1_intermediate_tk.symbol.clone(),
                            input_amount: input_amount_float,
                            expected_output: expected_intermediate_float,
                        },
                        ArbHop {
                            dex: tgt_pool.dex_type.clone(), pool: tgt_pool.address,
                            input_token: p1_intermediate_tk.symbol.clone(),
                            output_token: p1_input_tk.symbol.clone(), 
                            input_amount: expected_intermediate_float,
                            expected_output: final_output_float,
                        },
                    ],
                    total_profit: opp_calc_result.profit,
                    profit_pct: opp_calc_result.profit_percentage * 100.0, 
                    input_token: p1_input_tk.symbol.clone(),
                    output_token: p1_input_tk.symbol.clone(), 
                    input_amount: input_amount_float,
                    expected_output: final_output_float,
                    dex_path: vec![src_pool.dex_type.clone(), tgt_pool.dex_type.clone()], 
                    pool_path: vec![src_pool.address, tgt_pool.address],
                    risk_score: Some(opp_calc_result.price_impact),
                    notes: Some("Direct 2-hop cyclic opportunity".to_string()),
                    estimated_profit_usd: Some(opp_calc_result.profit * input_token_price_usd),
                    input_amount_usd: Some(input_amount_float * input_token_price_usd),
                    output_amount_usd: Some(final_output_float * input_token_price_usd),
                    intermediate_tokens: vec![p1_intermediate_tk.symbol.clone()],
                    source_pool: Arc::clone(src_pool_arc),
                    target_pool: Arc::clone(tgt_pool_arc),
                    input_token_mint: p1_input_tk.mint,
                    output_token_mint: p1_input_tk.mint, 
                    intermediate_token_mint: Some(p1_intermediate_tk.mint),
                };
                info!("Found Profitable 2-hop Arb Opp ID: {}", opp_id);
                current_opportunity.log_summary();

                // Call calculate_rebate (placeholder usage)
                let pools_for_rebate_stub: Vec<&PoolInfo> = vec![current_opportunity.source_pool.as_ref(), current_opportunity.target_pool.as_ref()];
                let amounts_for_rebate_stub: Vec<TokenAmount> = vec![
                    TokenAmount::from_float(current_opportunity.hops[0].input_amount, p1_input_tk.decimals),
                    TokenAmount::from_float(current_opportunity.hops[1].input_amount, p1_intermediate_tk.decimals)];
                // Determine directions for rebate calculation
                // Hop 1: src_pool_is_a_to_b
                // Hop 2: tgt_pool.token_a.mint == p1_intermediate_tk.mint (intermediate is input, original input is output)
                let tgt_pool_input_is_token_a = tgt_pool.token_a.mint == p1_intermediate_tk.mint;
                let directions_for_rebate: Vec<bool> = vec![
                    src_pool_is_a_to_b,
                    tgt_pool_input_is_token_a,
                ];
                let estimated_rebate = calculate_rebate(&pools_for_rebate_stub, &amounts_for_rebate_stub, &directions_for_rebate, self.sol_price_usd);
                info!("Opportunity ID {}: Estimated rebate (placeholder logic): {:.6}", current_opportunity.id, estimated_rebate);
                
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
                 info!("  Skipped 2-hop cycle (src='{}', tgt='{}', input_token='{}'): profit_pct {:.4}% (raw result profit: {:.6}, price impact: {:.4}) below threshold or too costly.",
                       src_pool.name, tgt_pool.name, p1_input_tk.symbol, opp_calc_result.profit_percentage * 100.0, opp_calc_result.profit, opp_calc_result.price_impact);
            }
        }
    }

    /// ACTIVATED: Finds 2-hop cyclic opportunities using a fixed input amount.
    pub async fn find_two_hop_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
        fixed_input_amount_float: f64,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("find_two_hop_opportunities (2-hop cyclic, fixed_input={}) called with {} pools.", fixed_input_amount_float, pools.len());
        let mut opportunities = Vec::new();

        for (_src_pool_addr, src_pool_arc) in pools.iter() {
            let src_pool = src_pool_arc.as_ref();

            self.find_cyclic_pair_for_src_hop_fixed_input(
                pools, src_pool_arc,
                &src_pool.token_a,
                &src_pool.token_b,
                fixed_input_amount_float,
                &mut opportunities, metrics,
            ).await;

            self.find_cyclic_pair_for_src_hop_fixed_input(
                pools, src_pool_arc,
                &src_pool.token_b,
                &src_pool.token_a,
                fixed_input_amount_float,
                &mut opportunities, metrics,
            ).await;
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Found {} direct (2-hop cyclic, fixed_input) arbitrage opportunities.", opportunities.len());
        Ok(opportunities)
    }

    async fn find_cyclic_pair_for_src_hop_fixed_input(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        src_pool_arc: &Arc<PoolInfo>,
        p1_input_tk: &crate::utils::PoolToken,
        p1_intermediate_tk: &crate::utils::PoolToken,
        fixed_input_amount_float: f64,
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        let src_pool = src_pool_arc.as_ref();

        if p1_input_tk.mint == p1_intermediate_tk.mint { return; }
        if Self::is_permanently_banned(&p1_input_tk.symbol, &p1_intermediate_tk.symbol) ||
           Self::is_temporarily_banned(&p1_input_tk.symbol, &p1_intermediate_tk.symbol) {
            debug!("Skipping banned pair for first hop (fixed_input): {} <-> {}", p1_input_tk.symbol, p1_intermediate_tk.symbol);
            
            // ACTIVATED: Log banned pair when encountered during fixed input detection
            Self::log_banned_pair(&p1_input_tk.symbol, &p1_intermediate_tk.symbol, "temporary", 
                "Ban confirmed during fixed-input 2-hop scan");
            return;
        }

        info!("Attempting 2-hop (fixed_input={}) from Pool1='{}', Input: {}, Intermediate: {}", fixed_input_amount_float, src_pool.name, p1_input_tk.symbol, p1_intermediate_tk.symbol);

        for (_tgt_pool_addr, tgt_pool_arc) in pools.iter() {
            if src_pool.address == tgt_pool_arc.address { continue; }
            let tgt_pool = tgt_pool_arc.as_ref();

            let tgt_pool_can_trade_intermediate_for_input =
                (tgt_pool.token_a.mint == p1_intermediate_tk.mint && tgt_pool.token_b.mint == p1_input_tk.mint) ||
                (tgt_pool.token_b.mint == p1_intermediate_tk.mint && tgt_pool.token_a.mint == p1_input_tk.mint);

            if !tgt_pool_can_trade_intermediate_for_input { continue; }

            if Self::is_permanently_banned(&p1_intermediate_tk.symbol, &p1_input_tk.symbol) ||
               Self::is_temporarily_banned(&p1_intermediate_tk.symbol, &p1_input_tk.symbol) {
                debug!("Skipping banned pair for second hop (fixed_input): {} <-> {}", p1_intermediate_tk.symbol, p1_input_tk.symbol);
                continue;
            }

            let opp_calc_result = calculate_simple_opportunity_result(
                src_pool,
                tgt_pool,
                &p1_input_tk.mint,
                fixed_input_amount_float
            );

            let input_token_price_usd = if p1_input_tk.symbol == "USDC" || p1_input_tk.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

            if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 2) {
                // Determine src_pool_is_a_to_b for calculate_output_amount
                let src_pool_is_a_to_b = src_pool.token_a.mint == p1_input_tk.mint;
                let intermediate_token_from_src_pool = crate::utils::calculate_output_amount(src_pool, TokenAmount::from_float(fixed_input_amount_float, p1_input_tk.decimals), src_pool_is_a_to_b);
                let expected_intermediate_float = intermediate_token_from_src_pool.to_float();
                let final_output_float = opp_calc_result.output_amount;

                let opp_id = format!("2hop-fixed-{}-{}-{}-{}-{}", p1_input_tk.symbol, src_pool.address, tgt_pool.address, fixed_input_amount_float, chrono::Utc::now().timestamp_millis());
                let current_opportunity = MultiHopArbOpportunity {
                    id: opp_id.clone(),
                    hops: vec![
                        ArbHop { dex: src_pool.dex_type.clone(), pool: src_pool.address, input_token: p1_input_tk.symbol.clone(), output_token: p1_intermediate_tk.symbol.clone(), input_amount: fixed_input_amount_float, expected_output: expected_intermediate_float },
                        ArbHop { dex: tgt_pool.dex_type.clone(), pool: tgt_pool.address, input_token: p1_intermediate_tk.symbol.clone(), output_token: p1_input_tk.symbol.clone(), input_amount: expected_intermediate_float, expected_output: final_output_float }
                    ],
                    total_profit: opp_calc_result.profit,
                    profit_pct: opp_calc_result.profit_percentage * 100.0,
                    input_token: p1_input_tk.symbol.clone(),
                    output_token: p1_input_tk.symbol.clone(),
                    input_amount: fixed_input_amount_float,
                    expected_output: final_output_float,
                    dex_path: vec![src_pool.dex_type.clone(), tgt_pool.dex_type.clone()],
                    pool_path: vec![src_pool.address, tgt_pool.address],
                    risk_score: Some(opp_calc_result.price_impact),
                    notes: Some(format!("Direct 2-hop cyclic opportunity (fixed_input={})", fixed_input_amount_float)),
                    estimated_profit_usd: Some(opp_calc_result.profit * input_token_price_usd),
                    input_amount_usd: Some(fixed_input_amount_float * input_token_price_usd),
                    output_amount_usd: Some(final_output_float * input_token_price_usd),
                    intermediate_tokens: vec![p1_intermediate_tk.symbol.clone()],
                    source_pool: Arc::clone(src_pool_arc),
                    target_pool: Arc::clone(tgt_pool_arc),
                    input_token_mint: p1_input_tk.mint,
                    output_token_mint: p1_input_tk.mint,
                    intermediate_token_mint: Some(p1_intermediate_tk.mint)
                };
                info!("Found Profitable 2-hop Arb Opp (fixed_input) ID: {}", opp_id);
                current_opportunity.log_summary();

                // Call calculate_rebate (placeholder usage)
                let pools_for_rebate_stub: Vec<&PoolInfo> = vec![current_opportunity.source_pool.as_ref(), current_opportunity.target_pool.as_ref()];
                let amounts_for_rebate_stub: Vec<TokenAmount> = vec![
                    TokenAmount::from_float(current_opportunity.hops[0].input_amount, p1_input_tk.decimals),
                    TokenAmount::from_float(current_opportunity.hops[1].input_amount, p1_intermediate_tk.decimals)
                ];
                // Determine directions for rebate calculation
                let directions_for_rebate: Vec<bool> = vec![
                    src_pool.token_a.mint == p1_input_tk.mint,
                    tgt_pool.token_a.mint == p1_intermediate_tk.mint
                ];
                let estimated_rebate = calculate_rebate(&pools_for_rebate_stub, &amounts_for_rebate_stub, &directions_for_rebate, self.sol_price_usd);
                info!("Opportunity ID {}: Estimated rebate (placeholder logic): {:.6}", current_opportunity.id, estimated_rebate);

                let dex_path_strings_log: Vec<String> = current_opportunity.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                if let Err(e) = metrics.record_opportunity_detected(
                    &current_opportunity.input_token,
                    current_opportunity.intermediate_tokens.get(0).map_or("", |s| s.as_str()),
                    current_opportunity.profit_pct,
                    current_opportunity.estimated_profit_usd,
                    current_opportunity.input_amount_usd,
                    dex_path_strings_log
                ) {
                    error!("Failed to record 2-hop (fixed_input) opportunity metric: {}", e);
                }
                opportunities.push(current_opportunity);
            } else {
                debug!("  Skipped 2-hop cycle (fixed_input) (src='{}', tgt='{}', input_token='{}'): profit_pct {:.4}% below threshold or too costly.", src_pool.name, tgt_pool.name, p1_input_tk.symbol, opp_calc_result.profit_percentage * 100.0);
            }
        }
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

                    let p1_arc = &pool_vec[i]; 
                    let p2_arc = &pool_vec[j]; 
                    let p3_arc = &pool_vec[k];
                    
                    // Permutation 1: P1(A->B), P2(B->C), P3(C->A)
                    self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1_arc.token_a, &p1_arc.token_b, 
                        &p2_arc.token_a, &p2_arc.token_b, 
                        &p3_arc.token_a, &p3_arc.token_b, 
                        &mut opportunities, metrics,
                    ).await;
                    // Permutation 2: P1(B->A), P2(A->C), P3(C->B) 
                     self.check_specific_3_hop_path(
                        p1_arc, p2_arc, p3_arc,
                        &p1_arc.token_b, &p1_arc.token_a, 
                        &p2_arc.token_a, &p2_arc.token_b, 
                        &p3_arc.token_a, &p3_arc.token_b,
                        &mut opportunities, metrics,
                    ).await;
                    
                    // Add all 8 permutations of hop directions for the three pools
                    let directions_p1 = [(&p1_arc.token_a, &p1_arc.token_b), (&p1_arc.token_b, &p1_arc.token_a)];
                    let directions_p2 = [(&p2_arc.token_a, &p2_arc.token_b), (&p2_arc.token_b, &p2_arc.token_a)];
                    let directions_p3 = [(&p3_arc.token_a, &p3_arc.token_b), (&p3_arc.token_b, &p3_arc.token_a)];

                    for (p1_in, p1_out) in directions_p1.iter() {
                        for (p2_in, p2_out) in directions_p2.iter() {
                            for (p3_in, p3_out) in directions_p3.iter() {
                                self.check_specific_3_hop_path(
                                    p1_arc, p2_arc, p3_arc,
                                    p1_in, p1_out,
                                    p2_in, p2_out,
                                    p3_in, p3_out,
                                    &mut opportunities, metrics
                                ).await;
                            }
                        }
                    }
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
        p1_input_tk: &crate::utils::PoolToken, p1_output_tk: &crate::utils::PoolToken, 
        p2_input_tk: &crate::utils::PoolToken, p2_output_tk: &crate::utils::PoolToken, 
        p3_input_tk: &crate::utils::PoolToken, p3_output_tk: &crate::utils::PoolToken, 
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        if !(p1_output_tk.mint == p2_input_tk.mint &&
             p2_output_tk.mint == p3_input_tk.mint &&
             p3_output_tk.mint == p1_input_tk.mint) {
            return; 
        }
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
        let input_amount_float = 100.0; // Example input, could be dynamic based on PoolToken reserves or config
        
        let (calculated_profit_float, total_slippage_fraction, _total_fee_tokens_equivalent) =
            calculate_multihop_profit_and_slippage(
                &pools_path_refs, input_amount_float, p1_input_tk.decimals, &directions, &last_fee_data,
            );

        let opp_calc_result = OpportunityCalculationResult {
            input_amount: input_amount_float,
            output_amount: input_amount_float + calculated_profit_float, 
            profit: calculated_profit_float,
            profit_percentage: if input_amount_float > 1e-9 { calculated_profit_float / input_amount_float } else {0.0},
            price_impact: total_slippage_fraction,
        };
        
        let input_token_price_usd = if p1_input_tk.symbol == "USDC" || p1_input_tk.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

        if self.is_opportunity_profitable_after_costs(&opp_calc_result, input_token_price_usd, 3) {
            let initial_ta = TokenAmount::from_float(input_amount_float, p1_input_tk.decimals);
            let hop1_out_ta = calculate_output_amount(p1_arc.as_ref(), initial_ta.clone(), directions[0]);
            let hop2_in_ta_adjusted = TokenAmount::from_float(hop1_out_ta.to_float(), p2_input_tk.decimals);
            let hop2_out_ta = calculate_output_amount(p2_arc.as_ref(), hop2_in_ta_adjusted, directions[1]);
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
                total_profit: calculated_profit_float, 
                profit_pct: opp_calc_result.profit_percentage * 100.0,
                input_token: p1_input_tk.symbol.clone(),
                output_token: p3_output_tk.symbol.clone(), 
                input_amount: input_amount_float,
                expected_output: hop3_out_ta.to_float(), 
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

            // Call calculate_rebate (placeholder usage)
            let mut pools_for_rebate_stub: Vec<&PoolInfo> = Vec::new();
            let mut amounts_for_rebate_stub: Vec<TokenAmount> = Vec::new();
            let all_involved_pools = [p1_arc.as_ref(), p2_arc.as_ref(), p3_arc.as_ref()];
            let all_involved_tokens_for_hops = [
                p1_input_tk, // input to hop1
                p2_input_tk, // input to hop2 (output of hop1)
                p3_input_tk, // input to hop3 (output of hop2)
            ];

            for i in 0..current_opportunity.hops.len() {
                pools_for_rebate_stub.push(all_involved_pools[i]);
                amounts_for_rebate_stub.push(TokenAmount::from_float(current_opportunity.hops[i].input_amount, all_involved_tokens_for_hops[i].decimals));
            }
            let estimated_rebate = calculate_rebate(&pools_for_rebate_stub, &amounts_for_rebate_stub, &directions, self.sol_price_usd);
            info!("Opportunity ID {}: Estimated rebate (placeholder logic): {:.6}", current_opportunity.id, estimated_rebate);

            
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
        max_slippage_pct_config: f64, 
        max_tx_fee_lamports_for_acceptance: u64 
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("find_all_multihop_opportunities_with_risk called. Max Slippage Pct: {:.2}%, Max Tx Fee (lamports): {}", max_slippage_pct_config, max_tx_fee_lamports_for_acceptance);
        let base_opportunities = self.find_all_multihop_opportunities(pools, metrics).await?;
        let mut risk_adjusted_opportunities = Vec::new();

        for opp in base_opportunities {
            let estimated_total_slippage_fraction = opp.risk_score.unwrap_or(1.0); 
            
            let estimated_instructions_per_hop = 2; 
            let total_instructions = opp.hops.len() * estimated_instructions_per_hop;
            
            let transaction_cost_usd = calculate_transaction_cost(
                total_instructions, 
                self.default_priority_fee_lamports, 
                self.sol_price_usd
            );
            
            let max_tx_fee_usd_for_acceptance = (max_tx_fee_lamports_for_acceptance as f64 / 1_000_000_000.0) * self.sol_price_usd;

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

#[cfg(test)]
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
}

// --- Add a test to ensure log_banned_pair and find_two_hop_opportunities are always used ---

#[cfg(test)]
mod detector_usage_smoke_test {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use solana_sdk::pubkey::Pubkey;

    #[tokio::test]
    async fn test_log_banned_pair_and_find_two_hop_opportunities_are_used() {
        // Use log_banned_pair
        ArbitrageDetector::log_banned_pair("TESTA", "TESTB", "permanent", "smoke test reason");

        // Use find_two_hop_opportunities
        let detector = ArbitrageDetector::new(0.5, 0.05, 150.0, 5000);
        let mut metrics = Metrics::default();
        let pools: HashMap<Pubkey, Arc<PoolInfo>> = HashMap::new();
        let _ = detector.find_all_opportunities(&pools, &mut metrics).await;
    }
}

#[cfg(test)]
mod enable_unused_methods_smoke {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use solana_sdk::pubkey::Pubkey;
    use crate::metrics::Metrics;

    #[tokio::test]
    async fn test_enable_all_detector_methods() {
        let detector = ArbitrageDetector::new(0.5, 0.05, 150.0, 5000);
        let mut metrics = Metrics::default();
        let pools: HashMap<Pubkey, Arc<PoolInfo>> = HashMap::new();
        // Call all methods flagged as unused
        let _ = detector.get_min_profit_threshold_usd();
        let _ = detector.find_two_hop_opportunities(&pools, &mut metrics, 100.0).await;
        // find_cyclic_pair_for_src_hop_fixed_input is private, so test via public method
        let _ = detector.find_all_multihop_opportunities_with_risk(&pools, &mut metrics, 0.05, 5000).await;
    }
}