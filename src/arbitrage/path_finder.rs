// src/arbitrage/path_finder.rs
use crate::{
    arbitrage::opportunity::{EnhancedArbHop, AdvancedMultiHopOpportunity},
    error::ArbError,
    utils::{DexType, PoolInfo},
};
use log::{info};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};

#[derive(Debug, Clone)]
pub struct ArbitragePath {
    pub hops: Vec<EnhancedArbHop>,
    pub dex_sequence: Vec<DexType>,
    pub estimated_profit_usd: f64,
    pub estimated_profit_pct: f64,
    pub confidence_score: f64,
    pub total_liquidity: f64,
    pub execution_complexity: u8, // 1-10 scale
}

impl From<ArbitragePath> for AdvancedMultiHopOpportunity {
    fn from(path: ArbitragePath) -> Self {
        let id = format!(
            "adv_{}_{}_{}",
            path.dex_sequence.len(),
            chrono::Utc::now().timestamp_millis(),
            fastrand::u32(..)
        );
        
        let execution_priority = if path.estimated_profit_pct > 5.0 {
            10
        } else if path.estimated_profit_pct > 2.0 {
            8
        } else if path.estimated_profit_pct > 1.0 {
            6
        } else {
            4
        };

        let estimated_gas_cost = (path.hops.len() as u64) * 50_000 + 100_000; // Base + per hop
        let max_execution_time_ms = (path.hops.len() as u64) * 500 + 2000; // Base + per hop
        let requires_batch = path.hops.len() > 2 || path.execution_complexity > 5;

        Self {
            id,
            path: path.hops,
            dex_sequence: path.dex_sequence,
            expected_profit_usd: path.estimated_profit_usd,
            profit_pct: path.estimated_profit_pct,
            confidence_score: path.confidence_score,
            execution_priority,
            estimated_gas_cost,
            slippage_tolerance: 0.5, // 0.5% default
            max_execution_time_ms,
            requires_batch,
            total_liquidity: path.total_liquidity,
            path_complexity: path.execution_complexity,
        }
    }
}

pub struct AdvancedPathFinder {
    max_hops: usize,
    min_liquidity_threshold: f64,
    max_slippage_pct: f64,
    min_profit_threshold_pct: f64,
    #[allow(dead_code)] // Will be used in future gas estimation features
    gas_price_estimator: GasEstimator,
}

#[derive(Debug, Clone)]
pub struct GasEstimator {
    base_gas_cost: u64,
    per_hop_gas_cost: u64,
    current_gas_price: u64,
}

impl Default for GasEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl GasEstimator {
    pub fn new() -> Self {
        Self {
            base_gas_cost: 100_000,     // Base transaction cost
            per_hop_gas_cost: 50_000,   // Additional cost per hop
            current_gas_price: 5_000,   // Current gas price in micro-lamports
        }
    }

    pub fn estimate_gas_cost(&self, hop_count: usize) -> u64 {
        self.base_gas_cost + (self.per_hop_gas_cost * hop_count as u64)
    }

    pub fn estimate_gas_fee(&self, hop_count: usize) -> u64 {
        self.estimate_gas_cost(hop_count) * self.current_gas_price / 1_000_000
    }
}

impl AdvancedPathFinder {
    pub fn new(
        max_hops: usize,
        min_liquidity_threshold: f64,
        max_slippage_pct: f64,
        min_profit_threshold_pct: f64,
    ) -> Self {
        info!(
            "AdvancedPathFinder initialized: max_hops={}, min_liquidity=${:.2}, max_slippage={:.2}%, min_profit={:.2}%",
            max_hops, min_liquidity_threshold, max_slippage_pct, min_profit_threshold_pct
        );
        
        Self {
            max_hops,
            min_liquidity_threshold,
            max_slippage_pct,
            min_profit_threshold_pct,
            gas_price_estimator: GasEstimator::new(),
        }
    }

    /// Find ALL profitable arbitrage paths between token pairs using BFS
    pub async fn find_all_profitable_paths(
        &self,
        start_token: Pubkey,
        pools_by_dex: &HashMap<DexType, Vec<Arc<PoolInfo>>>,
        initial_amount: f64,
    ) -> Result<Vec<ArbitragePath>, ArbError> {
        let start_time = Instant::now();
        let mut profitable_paths = Vec::new();
        
        info!(
            "Starting comprehensive path search from token {} with ${:.2}",
            start_token, initial_amount
        );

        // Create unified pool map for efficient lookups
        let token_to_pools = self.build_token_pool_map(pools_by_dex);
        
        // Use BFS to explore all possible paths
        let mut queue = VecDeque::new();
        let mut visited_paths = HashSet::new();
        
        // Initialize with direct pools from start token
        if let Some(pools) = token_to_pools.get(&start_token) {
            for (pool, dex_type) in pools {
                let initial_path = ArbitragePath {
                    hops: vec![EnhancedArbHop {
                        pool_address: pool.address,
                        input_token: start_token,
                        output_token: if pool.token_a.mint == start_token { pool.token_b.mint } else { pool.token_a.mint },
                        input_amount: initial_amount,
                        expected_output: self.estimate_output_amount(pool, start_token, initial_amount)?,
                        pool_info: Arc::clone(pool),
                    }],
                    dex_sequence: vec![dex_type.clone()],
                    estimated_profit_usd: 0.0,
                    estimated_profit_pct: 0.0,
                    confidence_score: 0.8,
                    total_liquidity: (pool.token_a.reserve + pool.token_b.reserve) as f64, // Simplified liquidity calc
                    execution_complexity: 1,
                };
                
                queue.push_back(initial_path);
            }
        }

        // BFS exploration
        while let Some(current_path) = queue.pop_front() {
            if current_path.hops.len() >= self.max_hops {
                continue;
            }

            let current_output_token = current_path.hops.last().unwrap().output_token;
            let current_amount = current_path.hops.last().unwrap().expected_output;

            // Check if we can return to start token (complete arbitrage cycle)
            if current_path.hops.len() >= 2 {
                if let Some(closing_pools) = token_to_pools.get(&current_output_token) {
                    for (pool, dex_type) in closing_pools {
                        if self.can_trade_to_token(pool, current_output_token, start_token) {
                            let final_amount = self.estimate_output_amount(pool, current_output_token, current_amount)?;
                            
                            if final_amount > initial_amount {
                                let profit_pct = ((final_amount - initial_amount) / initial_amount) * 100.0;
                                
                                if profit_pct >= self.min_profit_threshold_pct {
                                    let complete_path = self.complete_arbitrage_path(
                                        current_path.clone(),
                                        pool,
                                        dex_type.clone(),
                                        current_output_token,
                                        start_token,
                                        current_amount,
                                        final_amount,
                                        profit_pct,
                                    );
                                    
                                    profitable_paths.push(complete_path);
                                }
                            }
                        }
                    }
                }
            }

            // Extend path to explore more hops
            if let Some(next_pools) = token_to_pools.get(&current_output_token) {
                for (pool, dex_type) in next_pools {
                    let next_token = if pool.token_a.mint == current_output_token { 
                        pool.token_b.mint 
                    } else { 
                        pool.token_a.mint 
                    };

                    // Avoid circular paths (except back to start for arbitrage completion)
                    if self.creates_invalid_cycle(&current_path, next_token, start_token) {
                        continue;
                    }

                    // Liquidity check
                    let pool_liquidity = (pool.token_a.reserve + pool.token_b.reserve) as f64;
                    if pool_liquidity < self.min_liquidity_threshold {
                        continue;
                    }

                    let estimated_output = self.estimate_output_amount(pool, current_output_token, current_amount)?;
                    
                    // Slippage check
                    let slippage_pct = self.calculate_slippage_pct(current_amount, estimated_output)?;
                    if slippage_pct > self.max_slippage_pct {
                        continue;
                    }

                    let mut extended_path = current_path.clone();
                    extended_path.hops.push(EnhancedArbHop {
                        pool_address: pool.address,
                        input_token: current_output_token,
                        output_token: next_token,
                        input_amount: current_amount,
                        expected_output: estimated_output,
                        pool_info: Arc::clone(pool),
                    });
                    extended_path.dex_sequence.push(dex_type.clone());
                    extended_path.total_liquidity += pool_liquidity;
                    extended_path.execution_complexity += 1;

                    // Create path signature to avoid duplicate exploration
                    let path_signature = self.create_path_signature(&extended_path);
                    if !visited_paths.contains(&path_signature) {
                        visited_paths.insert(path_signature);
                        queue.push_back(extended_path);
                    }
                }
            }
        }

        // Sort by profitability
        profitable_paths.sort_by(|a, b| b.estimated_profit_pct.partial_cmp(&a.estimated_profit_pct).unwrap());
        
        let elapsed = start_time.elapsed();
        info!(
            "Path finding completed: found {} profitable paths in {:.2}ms",
            profitable_paths.len(),
            elapsed.as_secs_f64() * 1000.0
        );

        Ok(profitable_paths)
    }

    /// Optimize DEX sequence for maximum profit and minimum gas
    pub async fn optimize_dex_sequence(&self, path: &ArbitragePath) -> ArbitragePath {
        // For now, return the original path
        // In the future, implement smart DEX ordering based on:
        // - Current liquidity depth
        // - Gas costs per DEX
        // - Historical execution success rates
        path.clone()
    }

    // Helper methods

    fn build_token_pool_map(&self, pools_by_dex: &HashMap<DexType, Vec<Arc<PoolInfo>>>) 
        -> HashMap<Pubkey, Vec<(Arc<PoolInfo>, DexType)>> {
        let mut token_to_pools: HashMap<Pubkey, Vec<(Arc<PoolInfo>, DexType)>> = HashMap::new();
        
        for (dex_type, pools) in pools_by_dex {
            for pool in pools {
                token_to_pools.entry(pool.token_a.mint).or_default().push((Arc::clone(pool), dex_type.clone()));
                token_to_pools.entry(pool.token_b.mint).or_default().push((Arc::clone(pool), dex_type.clone()));
            }
        }
        
        token_to_pools
    }

    fn can_trade_to_token(&self, pool: &PoolInfo, from_token: Pubkey, to_token: Pubkey) -> bool {
        (pool.token_a.mint == from_token && pool.token_b.mint == to_token) ||
        (pool.token_b.mint == from_token && pool.token_a.mint == to_token)
    }

    fn creates_invalid_cycle(&self, current_path: &ArbitragePath, next_token: Pubkey, start_token: Pubkey) -> bool {
        // Allow returning to start token only for final hop
        if next_token == start_token {
            return false; // This is valid for completing arbitrage
        }
        
        // Check if we've visited this token before (except start token)
        for hop in &current_path.hops {
            if hop.output_token == next_token && next_token != start_token {
                return true;
            }
        }
        
        false
    }

    fn estimate_output_amount(&self, pool: &PoolInfo, input_token: Pubkey, input_amount: f64) -> Result<f64, ArbError> {
        // Simplified constant product formula estimation
        // In production, this should use the actual DEX-specific formulas
        
        let (reserve_in, reserve_out) = if pool.token_a.mint == input_token {
            (pool.token_a.reserve as f64, pool.token_b.reserve as f64)
        } else {
            (pool.token_b.reserve as f64, pool.token_a.reserve as f64)
        };

        if reserve_in <= 0.0 || reserve_out <= 0.0 {
            return Err(ArbError::InvalidPoolState("Zero reserves".to_string()));
        }

        // Constant product formula with 0.3% fee
        let fee_adjusted_input = input_amount * 0.997;
        let numerator = fee_adjusted_input * reserve_out;
        let denominator = reserve_in + fee_adjusted_input;
        
        Ok(numerator / denominator)
    }

    fn calculate_slippage_pct(&self, input_amount: f64, output_amount: f64) -> Result<f64, ArbError> {
        if input_amount <= 0.0 {
            return Err(ArbError::InvalidAmount("Input amount must be positive".to_string()));
        }
        
        // For simplicity, assume 1:1 expected rate and calculate deviation
        // In production, use historical average rates
        let expected_output = input_amount; // Simplified
        let slippage = ((expected_output - output_amount) / expected_output).abs() * 100.0;
        
        Ok(slippage)
    }

    fn complete_arbitrage_path(
        &self,
        mut current_path: ArbitragePath,
        closing_pool: &Arc<PoolInfo>,
        closing_dex: DexType,
        from_token: Pubkey,
        to_token: Pubkey,
        input_amount: f64,
        final_amount: f64,
        profit_pct: f64,
    ) -> ArbitragePath {
        // Add the final hop that completes the arbitrage
        current_path.hops.push(EnhancedArbHop {
            pool_address: closing_pool.address,
            input_token: from_token,
            output_token: to_token,
            input_amount,
            expected_output: final_amount,
            pool_info: Arc::clone(closing_pool),
        });
        current_path.dex_sequence.push(closing_dex);
        current_path.estimated_profit_pct = profit_pct;
        current_path.estimated_profit_usd = (final_amount - current_path.hops[0].input_amount) * 150.0; // Assume $150 SOL
        current_path.total_liquidity += (closing_pool.token_a.reserve + closing_pool.token_b.reserve) as f64;
        current_path.execution_complexity += 1;
        
        // Calculate confidence score based on liquidity and complexity
        current_path.confidence_score = self.calculate_confidence_score(&current_path);
        
        current_path
    }

    fn calculate_confidence_score(&self, path: &ArbitragePath) -> f64 {
        let mut score = 1.0;
        
        // Reduce confidence for longer paths
        score *= 1.0 - (path.hops.len() as f64 * 0.1);
        
        // Reduce confidence for low liquidity
        let avg_liquidity = path.total_liquidity / path.hops.len() as f64;
        if avg_liquidity < self.min_liquidity_threshold * 2.0 {
            score *= 0.8;
        }
        
        // Reduce confidence for high complexity
        score *= 1.0 - (path.execution_complexity as f64 * 0.05);
        
        score.clamp(0.1, 1.0)
    }

    fn create_path_signature(&self, path: &ArbitragePath) -> String {
        let tokens: Vec<String> = path.hops.iter()
            .map(|hop| format!("{}→{}", hop.input_token, hop.output_token))
            .collect();
        let dexs: Vec<String> = path.dex_sequence.iter()
            .map(|dex| format!("{:?}", dex))
            .collect();
        
        format!("{}|{}", tokens.join("→"), dexs.join("→"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_path_finder_initialization() {
        let path_finder = AdvancedPathFinder::new(
            5,     // max_hops
            1000.0, // min_liquidity
            2.0,   // max_slippage_pct
            0.5,   // min_profit_threshold_pct
        );
        
        assert_eq!(path_finder.max_hops, 5);
        assert_eq!(path_finder.min_liquidity_threshold, 1000.0);
    }

    #[test]
    fn test_gas_estimator() {
        let estimator = GasEstimator::new();
        
        assert_eq!(estimator.estimate_gas_cost(1), 150_000); // base + 1 hop
        assert_eq!(estimator.estimate_gas_cost(3), 250_000); // base + 3 hops
    }

    #[test]
    fn test_arbitrage_opportunity_conversion() {
        let path = ArbitragePath {
            hops: vec![],
            dex_sequence: vec![DexType::Orca, DexType::Raydium],
            estimated_profit_usd: 10.0,
            estimated_profit_pct: 2.5,
            confidence_score: 0.9,
            total_liquidity: 50000.0,
            execution_complexity: 3,
        };
        
        let opportunity: AdvancedMultiHopOpportunity = path.into();
        
        assert_eq!(opportunity.profit_pct, 2.5);
        assert_eq!(opportunity.execution_priority, 8); // 2.5% profit = priority 8
        assert!(opportunity.requires_batch); // complexity > 5 or hops > 2
    }
}
