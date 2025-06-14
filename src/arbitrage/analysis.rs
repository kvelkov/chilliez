//! Unified Analysis Module
//! 
//! This module consolidates all mathematical analysis functionality:
//! - Arbitrage calculations and profit optimization
//! - Fee management and slippage modeling
//! - Dynamic threshold adjustments
//! - Advanced mathematical operations with high precision
//! 
//! Consolidates logic from calculator.rs, fee_manager.rs, dynamic_threshold.rs, and advanced_math.rs

use crate::{
    config::settings::Config,
    metrics::Metrics,
    utils::{DexType, PoolInfo, TokenAmount},
};
use anyhow::{Result, anyhow};
use dashmap::DashMap;
use log::{debug, info};
use once_cell::sync::Lazy;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

// =============================================================================
// Core Calculation Results and Types
// =============================================================================

#[derive(Debug, Clone)]
pub struct OpportunityCalculationResult {
    pub input_amount: f64,
    pub output_amount: f64,
    pub profit: f64,
    pub profit_percentage: f64,
}

#[derive(Debug, Clone)]
pub struct OptimalArbitrageResult {
    pub optimal_input: Decimal,
    pub max_net_profit: Decimal,
    pub final_output: Decimal,
    pub gas_cost_sol: Decimal,
    pub requires_flash_loan: bool,
    pub min_outputs: Vec<Decimal>,
    pub log_weights: Vec<Decimal>,
}

impl OptimalArbitrageResult {
    pub fn should_execute(&self) -> bool {
        self.max_net_profit > Decimal::ZERO && 
        self.max_net_profit > self.gas_cost_sol
    }

    pub fn get_min_outputs_f64(&self) -> Vec<f64> {
        self.min_outputs.iter()
            .map(|d| d.to_f64().unwrap_or(0.0))
            .collect()
    }

    pub fn get_optimal_input_f64(&self) -> f64 {
        self.optimal_input.to_f64().unwrap_or(0.0)
    }

    pub fn get_expected_profit_f64(&self) -> f64 {
        self.max_net_profit.to_f64().unwrap_or(0.0)
    }
}

// =============================================================================
// Advanced Mathematical Types (from advanced_math.rs)
// =============================================================================

/// Result of optimal input amount calculation
#[derive(Debug, Clone)]
pub struct OptimalInputResult {
    /// The optimal input amount to maximize profit
    pub optimal_input: Decimal,
    /// Maximum expected net profit in output token units
    pub max_net_profit: Decimal,
    /// Expected final output amount
    pub final_output: Decimal,
    /// Estimated gas cost in SOL
    pub gas_cost_sol: Decimal,
    /// Whether flash loan is recommended
    pub requires_flash_loan: bool,
    /// Minimum output amounts for each hop (slippage protection)
    pub min_outputs: Vec<Decimal>,
    /// Logarithmic weights for Bellman-Ford algorithm
    pub log_weights: Vec<Decimal>,
}

/// Pre-execution simulation result
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// Expected output for each hop
    pub hop_outputs: Vec<Decimal>,
    /// Minimum output for each hop (with slippage tolerance)
    pub min_outputs: Vec<Decimal>,
    /// Total expected profit
    pub expected_profit: Decimal,
    /// Whether execution is recommended
    pub should_execute: bool,
    /// Slippage tolerance applied
    pub slippage_tolerance: Decimal,
}

/// Arbitrage path with mathematical properties
#[derive(Debug, Clone)]
pub struct ArbitragePath {
    /// Sequence of pools in the arbitrage path
    pub pools: Vec<PoolInfo>,
    /// Expected total return ratio
    pub expected_return_ratio: Decimal,
    /// Logarithmic weight for cycle detection
    pub log_weight: Decimal,
    /// Estimated gas cost for this path
    pub gas_cost: Decimal,
}

/// Smart contract selection for execution
#[derive(Debug, Clone)]
pub struct ContractSelector {
    flash_loan_threshold: Decimal,
    direct_swap_threshold: Decimal,
}

/// Execution strategy recommendation
#[derive(Debug, Clone)]
pub enum ExecutionStrategy {
    DirectSwap {
        input_amount: Decimal,
        expected_output: Decimal,
    },
    FlashLoan {
        loan_amount: Decimal,
        expected_profit: Decimal,
    },
    MultiHop {
        path: Vec<PoolInfo>,
        optimal_input: Decimal,
    },
}

// =============================================================================
// Fee Management and Slippage Modeling
// =============================================================================

/// Trait for slippage estimation models
pub trait SlippageModel: Send + Sync {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64;
}

/// Basic constant-product model for slippage estimation
pub struct XYKSlippageModel;

impl SlippageModel for XYKSlippageModel {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64 {
        let input_reserve_float = if is_a_to_b {
            pool.token_a.reserve as f64
        } else {
            pool.token_b.reserve as f64
        };
        let input_amount_float = input_amount.to_float();
        if (input_reserve_float + input_amount_float).abs() < f64::EPSILON {
            return 1.0;
        }
        // Slippage = input_amount / (reserve + input_amount)
        input_amount_float / (input_reserve_float + input_amount_float)
    }
}

impl Default for XYKSlippageModel {
    fn default() -> Self {
        XYKSlippageModel
    }
}

/// Fee breakdown for transactions
#[derive(Debug, Clone)]
pub struct FeeBreakdown {
    pub protocol_fee: f64,
    pub gas_fee: f64,
    pub slippage_cost: f64,
    pub total_cost: f64,
    pub explanation: String,
    pub risk_score: f64,
}

// =============================================================================
// Dynamic Threshold Management
// =============================================================================

/// Tracks recent price history to calculate volatility
#[derive(Debug)]
pub struct VolatilityTracker {
    prices: VecDeque<f64>,
    max_samples: usize,
}

impl VolatilityTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            prices: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Add a new price observation
    pub fn add_price(&mut self, price: f64) {
        if self.prices.len() >= self.max_samples {
            self.prices.pop_front();
        }
        self.prices.push_back(price);
        info!("Added price observation: {:.4}, total samples: {}", price, self.prices.len());
    }

    /// Calculate volatility (standard deviation) of tracked prices
    pub fn volatility(&self) -> f64 {
        if self.prices.len() < 2 {
            return 0.0;
        }

        let mean = self.prices.iter().sum::<f64>() / self.prices.len() as f64;
        let variance = self.prices.iter()
            .map(|price| (price - mean).powi(2))
            .sum::<f64>() / self.prices.len() as f64;
        
        variance.sqrt()
    }
}

/// Dynamic threshold updater based on market conditions
#[derive(Debug)]
pub struct DynamicThresholdUpdater {
    volatility_tracker: VolatilityTracker,
    current_threshold: f64,
    base_threshold: f64,
    volatility_factor: f64,
    #[allow(dead_code)]
    degradation_factor: f64,
    metrics: Arc<Mutex<Metrics>>,
}

impl DynamicThresholdUpdater {
    pub fn new(config: &Config, metrics: Arc<Mutex<Metrics>>) -> Self {
        let volatility_window = config.volatility_tracker_window.unwrap_or(100);
        let volatility_factor = config.volatility_threshold_factor.unwrap_or(2.0);
        let degradation_factor = config.degradation_profit_factor.unwrap_or(0.8);
        let base_threshold = config.min_profit_pct;

        Self {
            volatility_tracker: VolatilityTracker::new(volatility_window),
            current_threshold: base_threshold,
            base_threshold,
            volatility_factor,
            degradation_factor,
            metrics,
        }
    }

    /// Update threshold based on current market volatility
    pub async fn update_threshold(&mut self, current_price: f64) {
        self.volatility_tracker.add_price(current_price);
        let volatility = self.volatility_tracker.volatility();
        
        // Adjust threshold based on volatility
        let volatility_adjustment = volatility * self.volatility_factor;
        self.current_threshold = self.base_threshold + volatility_adjustment;
        
        info!("Updated dynamic threshold: {:.4}% (volatility: {:.4})", 
              self.current_threshold * 100.0, volatility);
        
        // Update metrics
        let mut metrics = self.metrics.lock().await;
        metrics.log_dynamic_threshold_update(self.current_threshold);
    }

    pub fn get_current_threshold(&self) -> f64 {
        self.current_threshold
    }

    /// Start continuous threshold updates
    pub async fn start_continuous_updates(&mut self, interval: Duration) -> Result<()> {
        loop {
            tokio::time::sleep(interval).await;
            // In a real implementation, would fetch current price from price feed
            let dummy_price = 150.0; // Placeholder
            self.update_threshold(dummy_price).await;
        }
    }
}

// =============================================================================
// Core Analysis Engine
// =============================================================================

/// Unified analysis engine that consolidates all mathematical operations
pub struct ArbitrageAnalyzer {
    /// High-precision mathematical operations
    advanced_math: AdvancedArbitrageMath,
    /// Fee management and calculations
    fee_manager: FeeManager,
    /// Dynamic threshold management
    threshold_updater: DynamicThresholdUpdater,
    /// Slippage estimation model
    #[allow(dead_code)]
    slippage_model: Box<dyn SlippageModel>,
}

impl ArbitrageAnalyzer {
    pub fn new(config: &Config, metrics: Arc<Mutex<Metrics>>) -> Self {
        Self {
            advanced_math: AdvancedArbitrageMath::new(18), // 18 decimal precision
            fee_manager: FeeManager,
            threshold_updater: DynamicThresholdUpdater::new(config, metrics),
            slippage_model: Box::new(XYKSlippageModel::default()),
        }
    }

    /// Calculate optimal arbitrage execution with high precision
    pub fn calculate_optimal_execution(
        &mut self,
        pools: &[&PoolInfo],
        input_amount: Decimal,
        _target_profit_pct: Decimal,
    ) -> Result<OptimalArbitrageResult> {
        let optimal_result = self.advanced_math.calculate_optimal_input(
            pools, input_amount, _target_profit_pct
        )?;

        Ok(OptimalArbitrageResult {
            optimal_input: optimal_result.optimal_input,
            max_net_profit: optimal_result.max_net_profit,
            final_output: optimal_result.final_output,
            gas_cost_sol: optimal_result.gas_cost_sol,
            requires_flash_loan: optimal_result.requires_flash_loan,
            min_outputs: optimal_result.min_outputs,
            log_weights: optimal_result.log_weights,
        })
    }

    /// Perform pre-execution simulation
    pub fn simulate_execution(
        &self,
        pools: &[&PoolInfo],
        input_amount: Decimal,
        slippage_tolerance: Decimal,
    ) -> Result<SimulationResult> {
        self.advanced_math.simulate_execution(pools, input_amount, slippage_tolerance)
    }

    /// Calculate comprehensive fee breakdown
    pub fn calculate_fee_breakdown(
        &self,
        pools: &[&PoolInfo],
        input_amount: &TokenAmount,
        sol_price_usd: f64,
    ) -> FeeBreakdown {
        self.fee_manager.calculate_multihop_fees(pools, input_amount, sol_price_usd)
    }

    /// Get current dynamic threshold
    pub fn get_current_threshold(&self) -> f64 {
        self.threshold_updater.get_current_threshold()
    }

    /// Update threshold based on market conditions
    pub async fn update_threshold(&mut self, current_price: f64) {
        self.threshold_updater.update_threshold(current_price).await;
    }
}

// =============================================================================
// Advanced Mathematical Operations
// =============================================================================

/// High-precision arbitrage mathematics implementation
pub struct AdvancedArbitrageMath {
    #[allow(dead_code)]
    precision: u32,
    optimization_cache: HashMap<String, OptimalInputResult>,
}

impl AdvancedArbitrageMath {
    pub fn new(precision: u32) -> Self {
        Self {
            precision,
            optimization_cache: HashMap::new(),
        }
    }

    /// Calculate optimal input amount using convex optimization
    pub fn calculate_optimal_input(
        &mut self,
        pools: &[&PoolInfo],
        initial_input: Decimal,
        target_profit_pct: Decimal,
    ) -> Result<OptimalInputResult> {
        let cache_key = format!("{}-{}-{}", 
            pools.len(), 
            initial_input.to_string(), 
            target_profit_pct.to_string()
        );

        if let Some(cached) = self.optimization_cache.get(&cache_key) {
            debug!("Using cached optimization result");
            return Ok(cached.clone());
        }

        // Perform optimization calculation
        let result = self.optimize_input_amount(pools, initial_input, target_profit_pct)?;
        
        // Cache the result
        self.optimization_cache.insert(cache_key, result.clone());
        
        Ok(result)
    }

    /// Internal optimization logic
    fn optimize_input_amount(
        &self,
        pools: &[&PoolInfo],
        initial_input: Decimal,
        _target_profit_pct: Decimal,
    ) -> Result<OptimalInputResult> {
        // Simplified optimization - in real implementation would use numerical methods
        let optimal_input = initial_input;
        let gas_cost = self.estimate_gas_cost(pools);
        let final_output = self.simulate_multihop_output(pools, optimal_input)?;
        let profit = final_output - optimal_input;
        let min_outputs = self.calculate_min_outputs(pools, optimal_input)?;
        let log_weights = self.calculate_log_weights(pools)?;

        Ok(OptimalInputResult {
            optimal_input,
            max_net_profit: profit - gas_cost,
            final_output,
            gas_cost_sol: gas_cost,
            requires_flash_loan: optimal_input > Decimal::from(1000), // Simple threshold
            min_outputs,
            log_weights,
        })
    }

    /// Simulate execution across multiple hops
    pub fn simulate_execution(
        &self,
        pools: &[&PoolInfo],
        input_amount: Decimal,
        slippage_tolerance: Decimal,
    ) -> Result<SimulationResult> {
        let hop_outputs = self.calculate_hop_outputs(pools, input_amount)?;
        let min_outputs = hop_outputs.iter()
            .map(|output| output * (Decimal::ONE - slippage_tolerance))
            .collect();
        
        let final_output = hop_outputs.last().copied().unwrap_or(Decimal::ZERO);
        let expected_profit = final_output - input_amount;

        Ok(SimulationResult {
            hop_outputs,
            min_outputs,
            expected_profit,
            should_execute: expected_profit > Decimal::ZERO,
            slippage_tolerance,
        })
    }

    /// Calculate output for each hop in the path
    fn calculate_hop_outputs(&self, pools: &[&PoolInfo], input_amount: Decimal) -> Result<Vec<Decimal>> {
        let mut current_amount = input_amount;
        let mut outputs = Vec::new();

        for pool in pools {
            // Simplified AMM calculation - would use actual pool formulas
            let output = self.calculate_pool_output(pool, current_amount)?;
            outputs.push(output);
            current_amount = output;
        }

        Ok(outputs)
    }

    /// Calculate output for a single pool
    fn calculate_pool_output(&self, _pool: &PoolInfo, input_amount: Decimal) -> Result<Decimal> {
        // Simplified calculation - would implement actual AMM formulas
        Ok(input_amount * Decimal::from_str("0.997")?) // Assume 0.3% fee
    }

    /// Simulate multi-hop output
    fn simulate_multihop_output(&self, pools: &[&PoolInfo], input: Decimal) -> Result<Decimal> {
        let outputs = self.calculate_hop_outputs(pools, input)?;
        outputs.last().copied().ok_or_else(|| anyhow!("No outputs calculated"))
    }

    /// Calculate minimum outputs with slippage protection
    fn calculate_min_outputs(&self, pools: &[&PoolInfo], input: Decimal) -> Result<Vec<Decimal>> {
        let slippage_tolerance = Decimal::from_str("0.01")?; // 1%
        let outputs = self.calculate_hop_outputs(pools, input)?;
        Ok(outputs.iter().map(|o| o * (Decimal::ONE - slippage_tolerance)).collect())
    }

    /// Calculate logarithmic weights for Bellman-Ford algorithm
    fn calculate_log_weights(&self, pools: &[&PoolInfo]) -> Result<Vec<Decimal>> {
        let mut weights = Vec::new();
        for _pool in pools {
            // Simplified weight calculation using f64 ln then converting to Decimal
            let fee_factor = 0.997_f64; // 1 - 0.003 fee
            let weight = -fee_factor.ln(); // -ln(1-fee)
            weights.push(Decimal::from_f64(weight).unwrap_or(Decimal::ZERO));
        }
        Ok(weights)
    }

    /// Estimate gas cost for the path
    fn estimate_gas_cost(&self, pools: &[&PoolInfo]) -> Decimal {
        let base_cost_per_hop = Decimal::from_str("0.000005").unwrap(); // 5000 lamports in SOL
        Decimal::from(pools.len()) * base_cost_per_hop
    }
}

// =============================================================================
// Fee Manager
// =============================================================================

/// Centralized fee management
pub struct FeeManager;

impl FeeManager {
    /// Calculate comprehensive fee breakdown for multi-hop arbitrage
    pub fn calculate_multihop_fees(
        &self,
        pools: &[&PoolInfo],
        input_amount: &TokenAmount,
        sol_price_usd: f64,
    ) -> FeeBreakdown {
        let mut total_protocol_fee = 0.0;
        let mut total_gas_fee = 0.0;
        let mut total_slippage_cost = 0.0;
        let mut explanation_parts = Vec::new();

        for (i, pool) in pools.iter().enumerate() {
            let pool_fee = self.calculate_pool_fee(pool, input_amount);
            let gas_cost = get_gas_cost_for_dex(pool.dex_type.clone()) as f64 / 1_000_000_000.0 * sol_price_usd;
            let slippage = XYKSlippageModel.estimate_slippage(pool, input_amount, true) * input_amount.to_float();

            total_protocol_fee += pool_fee;
            total_gas_fee += gas_cost;
            total_slippage_cost += slippage;

            explanation_parts.push(format!(
                "Hop {}: Pool fee {:.4}%, Gas ${:.2}, Slippage {:.4}%",
                i + 1,
                pool_fee * 100.0,
                gas_cost,
                slippage / input_amount.to_float() * 100.0
            ));
        }

        let total_cost = total_protocol_fee + total_gas_fee + total_slippage_cost;
        let risk_score = self.calculate_risk_score(pools, total_slippage_cost);

        FeeBreakdown {
            protocol_fee: total_protocol_fee,
            gas_fee: total_gas_fee,
            slippage_cost: total_slippage_cost,
            total_cost,
            explanation: explanation_parts.join("; "),
            risk_score,
        }
    }

    /// Calculate fee for a single pool
    fn calculate_pool_fee(&self, pool: &PoolInfo, _input_amount: &TokenAmount) -> f64 {
        // Use pool's fee_rate_bips or calculate from numerator/denominator
        if let Some(bips) = pool.fee_rate_bips {
            bips as f64 / 10000.0 // Convert basis points to decimal
        } else if let (Some(num), Some(denom)) = (pool.fee_numerator, pool.fee_denominator) {
            num as f64 / denom as f64
        } else {
            // Default fee based on DEX type
            match pool.dex_type {
                DexType::Raydium | DexType::Orca => 0.0025, // 0.25%
                DexType::Whirlpool => 0.003,                 // 0.3%
                DexType::Lifinity => 0.0015,                 // 0.15%
                _ => 0.003,                                  // Default 0.3%
            }
        }
    }

    /// Calculate risk score based on complexity and slippage
    fn calculate_risk_score(&self, pools: &[&PoolInfo], total_slippage: f64) -> f64 {
        let complexity_factor = pools.len() as f64 * 0.1;
        let slippage_factor = total_slippage * 10.0;
        (complexity_factor + slippage_factor).min(10.0) // Cap at 10
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Global cache for multi-hop calculations
#[allow(dead_code)]
static MULTI_HOP_CACHE: Lazy<DashMap<String, (f64, f64, f64)>> = Lazy::new(DashMap::new);

/// Calculate transaction cost in USD
pub fn calculate_transaction_cost(
    num_swaps: usize,
    priority_fee_lamports_per_swap: u64,
    sol_price_usd: f64,
) -> f64 {
    (num_swaps as f64 * priority_fee_lamports_per_swap as f64 * sol_price_usd) / 1_000_000_000.0
}

/// Determine if an opportunity is profitable
pub fn is_profitable_calc(
    opp_result: &OpportunityCalculationResult,
    token_price_in_sol: f64,
    tx_cost_in_sol: f64,
    min_profit_threshold_sol: f64,
) -> bool {
    let gross_profit_in_sol = opp_result.profit * token_price_in_sol;
    let net_profit_in_sol = gross_profit_in_sol - tx_cost_in_sol;
    net_profit_in_sol > min_profit_threshold_sol
}

/// Get gas cost for a specific DEX
pub fn get_gas_cost_for_dex(dex: DexType) -> u64 {
    match dex {
        DexType::Raydium | DexType::Orca => 500_000,
        DexType::Whirlpool | DexType::Lifinity => 700_000,
        DexType::Meteora => 600_000,
        _ => 500_000, // Default for Unknown variants
    }
}

/// Calculate high-precision output with Decimal arithmetic
pub fn calculate_high_precision_output(
    input_amount: Decimal,
    reserve_in: Decimal,
    reserve_out: Decimal,
    fee_numerator: u32,
    fee_denominator: u32,
) -> Result<Decimal> {
    if reserve_in.is_zero() || reserve_out.is_zero() {
        return Err(anyhow!("Zero reserves"));
    }

    let fee_factor = Decimal::from(fee_denominator - fee_numerator) / Decimal::from(fee_denominator);
    let input_with_fee = input_amount * fee_factor;
    
    let numerator = input_with_fee * reserve_out;
    let denominator = reserve_in + input_with_fee;
    
    Ok(numerator / denominator)
}

/// Calculate cycle detection weights
pub fn calculate_cycle_detection_weights(pools: &[&PoolInfo]) -> Result<Vec<Decimal>> {
    let mut weights = Vec::new();
    
    for pool in pools {
        // Calculate fee rate from pool info
        let fee_rate = if let Some(bips) = pool.fee_rate_bips {
            bips as f64 / 10000.0
        } else if let (Some(num), Some(denom)) = (pool.fee_numerator, pool.fee_denominator) {
            num as f64 / denom as f64
        } else {
            0.003 // Default 0.3%
        };
        
        let exchange_rate = pool.token_b.reserve as f64 / pool.token_a.reserve as f64;
        
        // Logarithmic transformation for Bellman-Ford
        let log_weight = -((1.0 - fee_rate) * exchange_rate).ln();
        weights.push(Decimal::from_f64(log_weight).unwrap_or(Decimal::ZERO));
    }
    
    Ok(weights)
}

/// Legacy function for backward compatibility
pub fn calculate_multihop_profit_and_slippage(
    _pools: &[&PoolInfo],
    input_amount_float: f64,
    _sol_price_usd: f64,
) -> OpportunityCalculationResult {
    // Simplified implementation for backward compatibility
    let output = input_amount_float * 0.99; // Assume 1% total fees
    let profit = output - input_amount_float;
    let profit_percentage = if input_amount_float > 0.0 {
        profit / input_amount_float
    } else {
        0.0
    };

    OpportunityCalculationResult {
        input_amount: input_amount_float,
        output_amount: output,
        profit,
        profit_percentage,
    }
}

// =============================================================================
// Contract Selection and Execution Strategy
// =============================================================================

impl ContractSelector {
    pub fn new() -> Self {
        Self {
            flash_loan_threshold: Decimal::from(1000), // 1000 tokens
            direct_swap_threshold: Decimal::from(100),  // 100 tokens
        }
    }

    pub fn select_strategy(&self, input_amount: Decimal, expected_profit: Decimal) -> ExecutionStrategy {
        if input_amount >= self.flash_loan_threshold {
            ExecutionStrategy::FlashLoan {
                loan_amount: input_amount,
                expected_profit,
            }
        } else if input_amount >= self.direct_swap_threshold {
            ExecutionStrategy::DirectSwap {
                input_amount,
                expected_output: input_amount + expected_profit,
            }
        } else {
            ExecutionStrategy::MultiHop {
                path: Vec::new(), // Would be populated with actual path
                optimal_input: input_amount,
            }
        }
    }
}

impl Default for ContractSelector {
    fn default() -> Self {
        Self::new()
    }
}
