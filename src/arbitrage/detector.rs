

use crate::{
    arbitrage::{
        calculator::{calculate_multihop_profit_and_slippage, OpportunityCalculationResult },
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{PoolInfo},
};
use log::{debug, info};
use solana_sdk::pubkey::Pubkey;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug)] 
pub struct ArbitrageDetector {
    min_profit_threshold_pct: f64,
    min_profit_threshold_usd: f64,
    sol_price_usd: f64,
    _default_priority_fee_lamports: u64,
}

impl ArbitrageDetector {
    pub fn new(min_profit_pct: f64, min_profit_usd: f64, sol_price_usd: f64, priority_fee: u64) -> Self {
        info!(
            "ArbitrageDetector initialized with: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            min_profit_pct, min_profit_usd, sol_price_usd, priority_fee
        );
        Self {
            min_profit_threshold_pct: min_profit_pct,
            min_profit_threshold_usd: min_profit_usd,
            sol_price_usd,
            _default_priority_fee_lamports: priority_fee,
        }
    }

    pub fn new_from_config(config: &crate::config::settings::Config) -> Self {
        let min_profit_pct = config.min_profit_pct * 100.0; // Convert fraction to percentage
        let min_profit_usd = config.min_profit_usd_threshold.unwrap_or(0.05); // Default if not set in config
        let sol_price_usd = config.sol_price_usd.unwrap_or(150.0); // Default if not set in config
        let priority_fee = config.default_priority_fee_lamports;
        info!(
            "ArbitrageDetector initialized from config: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            min_profit_pct, min_profit_usd, sol_price_usd, priority_fee
        );
        Self::new(min_profit_pct, min_profit_usd, sol_price_usd, priority_fee)
    }

    pub fn set_min_profit_threshold(&mut self, new_threshold_pct: f64) {
        self.min_profit_threshold_pct = new_threshold_pct;
    }

    pub fn get_min_profit_threshold_pct(&self) -> f64 {
        self.min_profit_threshold_pct
    }

    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("Scanning {} pools for multi-hop arbitrage opportunities.", pools.len());
        let mut opportunities = Vec::new();
        let pool_vec: Vec<Arc<PoolInfo>> = pools.values().cloned().collect();

        if pool_vec.len() < 3 {
            return Ok(opportunities);
        }

        for i in 0..pool_vec.len() {
            for j in 0..pool_vec.len() {
                if i == j { continue; }
                for k in 0..pool_vec.len() {
                    if k == i || k == j { continue; }

                    let p1 = &pool_vec[i];
                    let p2 = &pool_vec[j];
                    let p3 = &pool_vec[k];

                    self.evaluate_three_hop_opportunity(p1, p2, p3, &mut opportunities, metrics).await;
                }
            }
        }

        opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
        info!("Detected {} profitable multi-hop arbitrage opportunities.", opportunities.len());
        Ok(opportunities)
    }

    async fn evaluate_three_hop_opportunity(
        &self,
        p1: &Arc<PoolInfo>, p2: &Arc<PoolInfo>, p3: &Arc<PoolInfo>,
        opportunities: &mut Vec<MultiHopArbOpportunity>,
        metrics: &mut Metrics,
    ) {
        let pools_path = vec![p1.as_ref(), p2.as_ref(), p3.as_ref()];
        let directions = vec![true, true, true];

        let input_amount = 100.0; 
        let last_fee_data = vec![(None, None, None); pools_path.len()];

        let (profit, slippage, calculated_fee_usd) =
            calculate_multihop_profit_and_slippage(&pools_path, input_amount, p1.token_a.decimals, &directions, &last_fee_data);

        let opp_calc_result = OpportunityCalculationResult {
            input_amount,
            output_amount: input_amount + profit,
            profit,
            profit_percentage: profit / input_amount,
        };

        let input_token_price_usd = if p1.token_a.symbol == "USDC" || p1.token_a.symbol == "USDT" { 1.0 } else { self.sol_price_usd };

        if self.is_profitable(&opp_calc_result, input_token_price_usd, calculated_fee_usd) {
            let opp_id = format!("multi-hop-{}-{}-{}", p1.address, p2.address, p3.address);
            let opportunity = MultiHopArbOpportunity { // opp_id is moved here
                id: opp_id,
                hops: vec![
                    ArbHop { 
                        dex: p1.dex_type.clone(), 
                        pool: p1.address, 
                        input_token: p1.token_a.symbol.clone(), 
                        output_token: p1.token_b.symbol.clone(), 
                        input_amount, 
                        expected_output: input_amount + profit,
                    },
                    ArbHop { 
                        dex: p2.dex_type.clone(), 
                        pool: p2.address, 
                        input_token: p2.token_a.symbol.clone(), 
                        output_token: p2.token_b.symbol.clone(), 
                        input_amount, 
                        expected_output: input_amount + profit,
                    },
                    ArbHop { 
                        dex: p3.dex_type.clone(), 
                        pool: p3.address, 
                        input_token: p3.token_a.symbol.clone(), 
                        output_token: p3.token_b.symbol.clone(), 
                        input_amount, 
                        expected_output: input_amount + profit,
                    },
                ],
                total_profit: profit,
                profit_pct: opp_calc_result.profit_percentage * 100.0,
                input_token: p1.token_a.symbol.clone(),
                output_token: p3.token_b.symbol.clone(),
                input_amount,
                expected_output: input_amount + profit,
                dex_path: vec![p1.dex_type.clone(), p2.dex_type.clone(), p3.dex_type.clone()],
                pool_path: vec![p1.address, p2.address, p3.address],
                risk_score: Some(slippage),
                notes: Some(format!("Multi-hop arbitrage opportunity with slippage {:.2}%", slippage * 100.0)),
                estimated_profit_usd: Some(profit * input_token_price_usd),
                input_amount_usd: Some(input_amount * input_token_price_usd),
                output_amount_usd: Some((input_amount + profit) * input_token_price_usd),
                intermediate_tokens: vec![p1.token_b.symbol.clone(), p2.token_b.symbol.clone()],
                // Add missing fields
                source_pool: p1.clone(),
                target_pool: p3.clone(),
                input_token_mint: p1.token_a.mint,
                output_token_mint: p3.token_b.mint, // Assuming p3.token_b is the final output token
                intermediate_token_mint: Some(p1.token_b.mint), // Token after the first hop
            };

            info!("Found profitable arbitrage opportunity: {}", opportunity.id); // Use the id from the struct
            opportunities.push(opportunity);
            // The old call `metrics.log_opportunities_detected(1)` is replaced by the more detailed call below.
            let last_opportunity = opportunities.last().unwrap(); // Get the opportunity we just pushed
            if let Err(e) = metrics.record_opportunity_detected(
                &last_opportunity.input_token,
                &last_opportunity.intermediate_tokens.get(0).cloned().unwrap_or_default(), // First intermediate token
                last_opportunity.profit_pct, // Already in percentage form (e.g., 1.5 for 1.5%)
                last_opportunity.estimated_profit_usd,
                last_opportunity.input_amount_usd,
                last_opportunity.dex_path.iter().map(|d| format!("{:?}", d)).collect(),
            ) {
                log::error!("Failed to record opportunity metric: {}", e);
            }
        }
    }

    pub fn is_profitable(
        &self,
        opp_calc_result: &OpportunityCalculationResult,
        input_token_price_usd: f64,
        transaction_cost_usd: f64) -> bool {

        // Convert profit to USD before comparing with USD threshold
        let gross_profit_usd = opp_calc_result.profit * input_token_price_usd;
        let net_profit_usd = gross_profit_usd - transaction_cost_usd;

        let meets_pct_threshold = opp_calc_result.profit_percentage * 100.0 >= self.min_profit_threshold_pct;
        let meets_usd_threshold = net_profit_usd >= self.min_profit_threshold_usd;

        debug!(
            "Profitability check: InputTokenProfit: {:.6}, GrossProfitUSD: {:.2}, NetProfitUSD: {:.2}, ProfitPct: {:.2}%, TxCostUSD: {:.2}, MinProfitUSDThreshold: {:.2}, MeetsPct: {}, MeetsUSD: {}",
            opp_calc_result.profit,
            gross_profit_usd,
            net_profit_usd,
            opp_calc_result.profit_percentage * 100.0,
            transaction_cost_usd,
            self.min_profit_threshold_usd,
            meets_pct_threshold,
            meets_usd_threshold
        );
        meets_pct_threshold && meets_usd_threshold
    }
}
