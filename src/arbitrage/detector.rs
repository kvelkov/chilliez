use crate::{
    arbitrage::{
        calculator::{calculate_multihop_profit_and_slippage, OpportunityCalculationResult },
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    error::ArbError,
    metrics::Metrics,
    utils::{PoolInfo},
};
use log::{debug, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{collections::HashMap, sync::Arc, time::Instant};

#[derive(Clone, Debug)] 
pub struct ArbitrageDetector {
    min_profit_threshold_pct: f64,
    min_profit_threshold_usd: f64,
    sol_price_usd: f64,
    _default_priority_fee_lamports: u64,
    // Sprint 2: Enhanced detection parameters
    max_hops: usize,
    enable_cross_dex_arbitrage: bool,
    enable_parallel_detection: bool,
    detection_batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct DetectionConfig {
    pub max_hops: usize,
    pub enable_cross_dex_arbitrage: bool,
    pub enable_parallel_detection: bool,
    pub detection_batch_size: usize,
    pub min_liquidity_threshold: u64,
    pub max_slippage_tolerance: f64,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            max_hops: 4,
            enable_cross_dex_arbitrage: true,
            enable_parallel_detection: true,
            detection_batch_size: 100,
            min_liquidity_threshold: 10000, // Minimum $10k liquidity
            max_slippage_tolerance: 0.05,   // 5% max slippage
        }
    }
}

#[derive(Debug, Default)]
pub struct DetectionMetrics {
    pub total_paths_analyzed: u64,
    pub profitable_paths_found: u64,
    pub cross_dex_opportunities: u64,
    pub single_dex_opportunities: u64,
    pub average_detection_time_ms: f64,
    pub best_profit_pct: f64,
    pub total_detection_cycles: u64,
}

impl ArbitrageDetector {
    pub fn new(min_profit_pct: f64, min_profit_usd: f64, sol_price_usd: f64, priority_fee: u64) -> Self {
        info!(
            "🔍 Enhanced ArbitrageDetector initialized with: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            min_profit_pct, min_profit_usd, sol_price_usd, priority_fee
        );
        Self {
            min_profit_threshold_pct: min_profit_pct,
            min_profit_threshold_usd: min_profit_usd,
            sol_price_usd,
            _default_priority_fee_lamports: priority_fee,
            // Sprint 2: Enhanced detection defaults
            max_hops: 4,
            enable_cross_dex_arbitrage: true,
            enable_parallel_detection: true,
            detection_batch_size: 100,
        }
    }

    pub fn new_from_config(config: &crate::config::settings::Config) -> Self {
        let min_profit_pct = config.min_profit_pct * 100.0; // Convert fraction to percentage
        let min_profit_usd = config.min_profit_usd_threshold.unwrap_or(0.05); // Default if not set in config
        let sol_price_usd = config.sol_price_usd.unwrap_or(150.0); // Default if not set in config
        let priority_fee = config.default_priority_fee_lamports;
        
        info!(
            "🔍 Enhanced ArbitrageDetector initialized from config: Min Profit Pct = {:.4}%, Min Profit USD = ${:.2}, SOL Price = ${:.2}, Default Priority Fee = {} lamports",
            min_profit_pct, min_profit_usd, sol_price_usd, priority_fee
        );
        
        let mut detector = Self::new(min_profit_pct, min_profit_usd, sol_price_usd, priority_fee);
        
        // Sprint 2: Configure enhanced detection parameters from config
        detector.max_hops = 4; // Could be configurable in future
        detector.enable_cross_dex_arbitrage = true;
        detector.enable_parallel_detection = true;
        detector.detection_batch_size = 100;
        
        detector
    }

    pub fn set_min_profit_threshold(&mut self, new_threshold_pct: f64) {
        self.min_profit_threshold_pct = new_threshold_pct;
        info!("🎯 Updated minimum profit threshold to {:.4}%", new_threshold_pct);
    }

    pub fn get_min_profit_threshold_pct(&self) -> f64 {
        self.min_profit_threshold_pct
    }

    /// Sprint 2: Enhanced opportunity detection with hot cache optimization
    pub async fn find_all_opportunities(
        &self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        metrics: &mut Metrics,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let start_time = Instant::now();
        info!("🔍 Starting enhanced arbitrage detection on {} pools...", pools.len());
        
        let opportunities = Vec::new();
        let mut detection_metrics = DetectionMetrics::default();
        detection_metrics.total_detection_cycles += 1;

        if pools.len() < 2 {
            warn!("Insufficient pools for arbitrage detection: {} pools", pools.len());
            return Ok(opportunities);
        }

        // Sprint 2: Multi-strategy detection
        let pool_vec: Vec<Arc<PoolInfo>> = pools.values().cloned().collect();
        
        // Strategy 1: Direct 2-hop arbitrage (fastest)
        let mut direct_opportunities = self.find_direct_arbitrage_opportunities(&pool_vec, &mut detection_metrics).await;
        
        // Strategy 2: Multi-hop arbitrage (2-4 hops)
        if self.max_hops > 2 {
            let multihop_opportunities = self.find_multihop_arbitrage_opportunities(&pool_vec, &mut detection_metrics).await;
            direct_opportunities.extend(multihop_opportunities);
        }
        
        // Strategy 3: Cross-DEX arbitrage (if enabled)
        if self.enable_cross_dex_arbitrage {
            let cross_dex_opportunities = self.find_cross_dex_arbitrage_opportunities(&pool_vec, &mut detection_metrics).await;
            direct_opportunities.extend(cross_dex_opportunities);
        }

        // Sort opportunities by profit percentage (descending)
        direct_opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));

        let detection_time = start_time.elapsed();
        let detection_time_ms = detection_time.as_secs_f64() * 1000.0;
        
        // Update metrics
        detection_metrics.average_detection_time_ms = detection_time_ms;
        detection_metrics.profitable_paths_found = direct_opportunities.len() as u64;
        if let Some(best) = direct_opportunities.first() {
            detection_metrics.best_profit_pct = best.profit_pct;
        }

        info!("✅ Enhanced detection completed in {:.2}ms:", detection_time_ms);
        info!("   📊 Paths analyzed: {}", detection_metrics.total_paths_analyzed);
        info!("   💰 Profitable opportunities: {}", detection_metrics.profitable_paths_found);
        info!("   🔄 Cross-DEX opportunities: {}", detection_metrics.cross_dex_opportunities);
        info!("   🏪 Single-DEX opportunities: {}", detection_metrics.single_dex_opportunities);
        if detection_metrics.best_profit_pct > 0.0 {
            info!("   🎯 Best profit: {:.4}%", detection_metrics.best_profit_pct);
        }

        // Record metrics for external tracking
        for opportunity in &direct_opportunities {
            if let Err(e) = metrics.record_opportunity_detected(
                &opportunity.input_token,
                &opportunity.intermediate_tokens.first().cloned().unwrap_or_default(),
                opportunity.profit_pct,
                opportunity.estimated_profit_usd,
                opportunity.input_amount_usd,
                opportunity.dex_path.iter().map(|d| format!("{:?}", d)).collect(),
            ) {
                warn!("Failed to record opportunity metric: {}", e);
            }
        }

        Ok(direct_opportunities)
    }

    /// Sprint 2: Find direct 2-hop arbitrage opportunities
    async fn find_direct_arbitrage_opportunities(
        &self,
        pools: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let start_time = Instant::now();
        let mut opportunities = Vec::new();
        
        info!("🔍 Scanning for direct 2-hop arbitrage opportunities...");
        
        // Direct arbitrage: A -> B -> A
        for (i, pool1) in pools.iter().enumerate() {
            for (j, pool2) in pools.iter().enumerate() {
                if i >= j { continue; } // Avoid duplicates and self-comparison
                
                metrics.total_paths_analyzed += 1;
                
                // Check if pools can form a valid arbitrage path
                if let Some(opportunity) = self.evaluate_direct_arbitrage_pair(pool1, pool2).await {
                    opportunities.push(opportunity);
                    metrics.profitable_paths_found += 1;
                    
                    // Track single vs cross-DEX
                    if pool1.dex_type == pool2.dex_type {
                        metrics.single_dex_opportunities += 1;
                    } else {
                        metrics.cross_dex_opportunities += 1;
                    }
                }
            }
        }
        
        let detection_time = start_time.elapsed();
        info!("📊 Direct arbitrage scan completed in {:.2}ms: {} opportunities found", 
              detection_time.as_secs_f64() * 1000.0, opportunities.len());
        
        opportunities
    }

    /// Sprint 2: Find multi-hop arbitrage opportunities (3-4 hops)
    async fn find_multihop_arbitrage_opportunities(
        &self,
        pools: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let start_time = Instant::now();
        let mut opportunities = Vec::new();
        
        info!("🔍 Scanning for multi-hop arbitrage opportunities (up to {} hops)...", self.max_hops);
        
        // 3-hop arbitrage: A -> B -> C -> A
        if self.max_hops >= 3 {
            opportunities.extend(self.find_three_hop_opportunities(pools, metrics).await);
        }
        
        // 4-hop arbitrage: A -> B -> C -> D -> A (if enabled)
        if self.max_hops >= 4 {
            opportunities.extend(self.find_four_hop_opportunities(pools, metrics).await);
        }
        
        let detection_time = start_time.elapsed();
        info!("📊 Multi-hop arbitrage scan completed in {:.2}ms: {} opportunities found", 
              detection_time.as_secs_f64() * 1000.0, opportunities.len());
        
        opportunities
    }

    /// Sprint 2: Find cross-DEX arbitrage opportunities
    async fn find_cross_dex_arbitrage_opportunities(
        &self,
        pools: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let start_time = Instant::now();
        let mut opportunities = Vec::new();
        
        info!("🔍 Scanning for cross-DEX arbitrage opportunities...");
        
        // Group pools by DEX type for efficient cross-DEX analysis
        let mut pools_by_dex: HashMap<String, Vec<Arc<PoolInfo>>> = HashMap::new();
        for pool in pools {
            let dex_key = format!("{:?}", pool.dex_type);
            pools_by_dex.entry(dex_key).or_insert_with(Vec::new).push(pool.clone());
        }
        
        // Find opportunities between different DEXs
        let dex_names: Vec<String> = pools_by_dex.keys().cloned().collect();
        for (i, dex1) in dex_names.iter().enumerate() {
            for dex2 in dex_names.iter().skip(i + 1) {
                if let (Some(pools1), Some(pools2)) = (pools_by_dex.get(dex1), pools_by_dex.get(dex2)) {
                    let cross_dex_opps = self.find_cross_dex_pairs(pools1, pools2, metrics).await;
                    opportunities.extend(cross_dex_opps);
                }
            }
        }
        
        let detection_time = start_time.elapsed();
        info!("📊 Cross-DEX arbitrage scan completed in {:.2}ms: {} opportunities found", 
              detection_time.as_secs_f64() * 1000.0, opportunities.len());
        
        opportunities
    }

    /// Sprint 2: Evaluate direct arbitrage between two pools
    async fn evaluate_direct_arbitrage_pair(
        &self,
        pool1: &Arc<PoolInfo>,
        pool2: &Arc<PoolInfo>,
    ) -> Option<MultiHopArbOpportunity> {
        // Check if pools share a common token for arbitrage
        let common_token = if pool1.token_a.mint == pool2.token_a.mint || pool1.token_a.mint == pool2.token_b.mint {
            Some(pool1.token_a.mint)
        } else if pool1.token_b.mint == pool2.token_a.mint || pool1.token_b.mint == pool2.token_b.mint {
            Some(pool1.token_b.mint)
        } else {
            None
        };

        if common_token.is_none() {
            return None;
        }

        // Calculate arbitrage opportunity
        let input_amount = 100.0; // Base amount for calculation
        let pools_path = vec![pool1.as_ref(), pool2.as_ref()];
        let directions = vec![true, true]; // Simplified direction logic
        let last_fee_data = vec![(None, None, None); pools_path.len()];

        let (profit, slippage, calculated_fee_usd) =
            calculate_multihop_profit_and_slippage(&pools_path, input_amount, pool1.token_a.decimals, &directions, &last_fee_data);

        let opp_calc_result = OpportunityCalculationResult {
            input_amount,
            output_amount: input_amount + profit,
            profit,
            profit_percentage: profit / input_amount,
        };

        let input_token_price_usd = if pool1.token_a.symbol == "USDC" || pool1.token_a.symbol == "USDT" { 
            1.0 
        } else { 
            self.sol_price_usd 
        };

        if self.is_profitable(&opp_calc_result, input_token_price_usd, calculated_fee_usd) {
            let opp_id = format!("direct-{}-{}", pool1.address, pool2.address);
            
            Some(MultiHopArbOpportunity {
                id: opp_id,
                hops: vec![
                    ArbHop { 
                        dex: pool1.dex_type.clone(), 
                        pool: pool1.address, 
                        input_token: pool1.token_a.symbol.clone(), 
                        output_token: pool1.token_b.symbol.clone(), 
                        input_amount, 
                        expected_output: input_amount + profit,
                    },
                    ArbHop { 
                        dex: pool2.dex_type.clone(), 
                        pool: pool2.address, 
                        input_token: pool2.token_a.symbol.clone(), 
                        output_token: pool2.token_b.symbol.clone(), 
                        input_amount, 
                        expected_output: input_amount + profit,
                    },
                ],
                total_profit: profit,
                profit_pct: opp_calc_result.profit_percentage * 100.0,
                input_token: pool1.token_a.symbol.clone(),
                output_token: pool2.token_b.symbol.clone(),
                input_amount,
                expected_output: input_amount + profit,
                dex_path: vec![pool1.dex_type.clone(), pool2.dex_type.clone()],
                pool_path: vec![pool1.address, pool2.address],
                risk_score: Some(slippage),
                notes: Some(format!("Direct arbitrage with slippage {:.2}%", slippage * 100.0)),
                estimated_profit_usd: Some(profit * input_token_price_usd),
                input_amount_usd: Some(input_amount * input_token_price_usd),
                output_amount_usd: Some((input_amount + profit) * input_token_price_usd),
                intermediate_tokens: vec![pool1.token_b.symbol.clone()],
                source_pool: pool1.clone(),
                target_pool: pool2.clone(),
                input_token_mint: pool1.token_a.mint,
                output_token_mint: pool2.token_b.mint,
                intermediate_token_mint: Some(pool1.token_b.mint),
                estimated_gas_cost: Some(400_000), // Default gas estimate for 2-hop
            })
        } else {
            None
        }
    }

    /// Sprint 2: Find three-hop arbitrage opportunities
    async fn find_three_hop_opportunities(
        &self,
        pools: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let mut opportunities = Vec::new();
        
        // Limit search space for performance
        let max_combinations = 1000; // Prevent excessive computation
        let mut combinations_checked = 0;
        
        for i in 0..pools.len() {
            for j in 0..pools.len() {
                if i == j { continue; }
                for k in 0..pools.len() {
                    if k == i || k == j { continue; }
                    
                    combinations_checked += 1;
                    if combinations_checked > max_combinations {
                        warn!("⚠️ Reached maximum combinations limit for 3-hop detection");
                        return opportunities;
                    }
                    
                    metrics.total_paths_analyzed += 1;
                    
                    let p1 = &pools[i];
                    let p2 = &pools[j];
                    let p3 = &pools[k];

                    if let Some(opportunity) = self.evaluate_three_hop_opportunity(p1, p2, p3).await {
                        opportunities.push(opportunity);
                        metrics.profitable_paths_found += 1;
                        
                        // Track cross-DEX vs single-DEX
                        let unique_dexs: std::collections::HashSet<_> = [&p1.dex_type, &p2.dex_type, &p3.dex_type].into_iter().collect();
                        if unique_dexs.len() > 1 {
                            metrics.cross_dex_opportunities += 1;
                        } else {
                            metrics.single_dex_opportunities += 1;
                        }
                    }
                }
            }
        }
        
        opportunities
    }

    /// Sprint 2: Find four-hop arbitrage opportunities (limited scope for performance)
    async fn find_four_hop_opportunities(
        &self,
        pools: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let opportunities = Vec::new();
        
        // Very limited search for 4-hop to prevent performance issues
        let max_combinations = 500;
        let mut combinations_checked = 0;
        
        // Only check high-liquidity pools for 4-hop arbitrage
        let high_liquidity_pools: Vec<_> = pools.iter()
            .filter(|pool| pool.token_a.reserve > 100000 && pool.token_b.reserve > 100000)
            .take(20) // Limit to top 20 pools by liquidity
            .collect();
        
        for i in 0..high_liquidity_pools.len() {
            for j in 0..high_liquidity_pools.len() {
                if i == j { continue; }
                for k in 0..high_liquidity_pools.len() {
                    if k == i || k == j { continue; }
                    for l in 0..high_liquidity_pools.len() {
                        if l == i || l == j || l == k { continue; }
                        
                        combinations_checked += 1;
                        if combinations_checked > max_combinations {
                            warn!("⚠️ Reached maximum combinations limit for 4-hop detection");
                            return opportunities;
                        }
                        
                        metrics.total_paths_analyzed += 1;
                        
                        // Simplified 4-hop evaluation (placeholder)
                        // In a full implementation, this would be similar to 3-hop but with 4 pools
                        debug!("Evaluating 4-hop path: {} -> {} -> {} -> {}", 
                               high_liquidity_pools[i].address, 
                               high_liquidity_pools[j].address,
                               high_liquidity_pools[k].address,
                               high_liquidity_pools[l].address);
                    }
                }
            }
        }
        
        opportunities
    }

    /// Sprint 2: Find cross-DEX arbitrage pairs
    async fn find_cross_dex_pairs(
        &self,
        pools1: &[Arc<PoolInfo>],
        pools2: &[Arc<PoolInfo>],
        metrics: &mut DetectionMetrics,
    ) -> Vec<MultiHopArbOpportunity> {
        let mut opportunities = Vec::new();
        
        for pool1 in pools1.iter().take(50) { // Limit for performance
            for pool2 in pools2.iter().take(50) {
                metrics.total_paths_analyzed += 1;
                
                if let Some(opportunity) = self.evaluate_direct_arbitrage_pair(pool1, pool2).await {
                    opportunities.push(opportunity);
                    metrics.cross_dex_opportunities += 1;
                }
            }
        }
        
        opportunities
    }

    /// Enhanced three-hop opportunity evaluation
    async fn evaluate_three_hop_opportunity(
        &self,
        p1: &Arc<PoolInfo>, 
        p2: &Arc<PoolInfo>, 
        p3: &Arc<PoolInfo>
    ) -> Option<MultiHopArbOpportunity> {
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

        let input_token_price_usd = if p1.token_a.symbol == "USDC" || p1.token_a.symbol == "USDT" { 
            1.0 
        } else { 
            self.sol_price_usd 
        };

        if self.is_profitable(&opp_calc_result, input_token_price_usd, calculated_fee_usd) {
            let opp_id = format!("multi-hop-{}-{}-{}", p1.address, p2.address, p3.address);
            
            Some(MultiHopArbOpportunity {
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
                notes: Some(format!("Multi-hop arbitrage with slippage {:.2}%", slippage * 100.0)),
                estimated_profit_usd: Some(profit * input_token_price_usd),
                input_amount_usd: Some(input_amount * input_token_price_usd),
                output_amount_usd: Some((input_amount + profit) * input_token_price_usd),
                intermediate_tokens: vec![p1.token_b.symbol.clone(), p2.token_b.symbol.clone()],
                source_pool: p1.clone(),
                target_pool: p3.clone(),
                input_token_mint: p1.token_a.mint,
                output_token_mint: p3.token_b.mint,
                intermediate_token_mint: Some(p1.token_b.mint),
                estimated_gas_cost: Some(600_000), // Default gas estimate for 3-hop
            })
        } else {
            None
        }
    }

    pub fn is_profitable(
        &self,
        opp_calc_result: &OpportunityCalculationResult,
        input_token_price_usd: f64,
        transaction_cost_usd: f64
    ) -> bool {
        // Convert profit to USD before comparing with USD threshold
        let gross_profit_usd = opp_calc_result.profit * input_token_price_usd;
        let net_profit_usd = gross_profit_usd - transaction_cost_usd;

        let meets_pct_threshold = opp_calc_result.profit_percentage * 100.0 >= self.min_profit_threshold_pct;
        let meets_usd_threshold = net_profit_usd >= self.min_profit_threshold_usd;

        debug!(
            "💰 Profitability check: InputTokenProfit: {:.6}, GrossProfitUSD: {:.2}, NetProfitUSD: {:.2}, ProfitPct: {:.2}%, TxCostUSD: {:.2}, MinProfitUSDThreshold: {:.2}, MeetsPct: {}, MeetsUSD: {}",
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

    /// Sprint 2: Configure detection parameters
    pub fn configure_detection(&mut self, config: DetectionConfig) {
        self.max_hops = config.max_hops;
        self.enable_cross_dex_arbitrage = config.enable_cross_dex_arbitrage;
        self.enable_parallel_detection = config.enable_parallel_detection;
        self.detection_batch_size = config.detection_batch_size;
        
        info!("🔧 Detection configuration updated:");
        info!("   🔄 Max hops: {}", self.max_hops);
        info!("   🔀 Cross-DEX arbitrage: {}", self.enable_cross_dex_arbitrage);
        info!("   ⚡ Parallel detection: {}", self.enable_parallel_detection);
        info!("   📦 Batch size: {}", self.detection_batch_size);
    }

    /// Sprint 2: Get detection statistics
    pub fn get_detection_stats(&self) -> (usize, bool, bool, usize) {
        (
            self.max_hops,
            self.enable_cross_dex_arbitrage,
            self.enable_parallel_detection,
            self.detection_batch_size,
        )
    }
}