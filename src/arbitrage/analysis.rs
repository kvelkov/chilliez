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
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

// Optimization imports
use argmin::core::{CostFunction, Executor};
use argmin::solver::brent::BrentOpt;

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
        // Use integer arithmetic for precise slippage calculation
        let input_reserve = if is_a_to_b {
            pool.token_a.reserve
        } else {
            pool.token_b.reserve
        };
        
        // Convert input amount to u128 for safe arithmetic
        let input_amount_u128 = input_amount.amount as u128;
        let input_reserve_u128 = input_reserve as u128;
        
        if input_reserve_u128 == 0 {
            return 1.0; // 100% slippage if no liquidity
        }
        
        // Calculate slippage using integer arithmetic: slippage = input / (reserve + input)
        // Use basis points for precision: multiply by 10000, then divide at the end
        let total_liquidity = input_reserve_u128.saturating_add(input_amount_u128);
        if total_liquidity == 0 {
            return 1.0;
        }
        
        let slippage_bps = input_amount_u128
            .saturating_mul(10000)
            .saturating_div(total_liquidity);
            
        // Convert back to decimal (basis points to percentage)
        (slippage_bps as f64) / 10000.0
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
            fee_manager: FeeManager::default(),
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

    /// Internal optimization logic using argmin for convex optimization
    fn optimize_input_amount(
        &self,
        pools: &[&PoolInfo],
        initial_input: Decimal,
        _target_profit_pct: Decimal,
    ) -> Result<OptimalInputResult> {
        let initial_input_f64 = initial_input.to_f64().unwrap_or(1000.0);
        let gas_cost_sol = self.estimate_gas_cost(pools).to_f64().unwrap_or(0.005);
        
        // Create the cost function for optimization
        let cost_function = ArbitrageCostFunction::new(pools, gas_cost_sol);
        
        // Define search bounds - we'll search from 1% to 1000% of initial input
        let min_input = initial_input_f64 * 0.01;
        let max_input = initial_input_f64 * 10.0;
        
        // Use Brent's method for 1D optimization (finding the minimum of our negative profit function)
        let solver = BrentOpt::new(min_input, max_input);
        
        // Execute the optimization
        let result = Executor::new(cost_function, solver)
            .configure(|state| state.max_iters(100).target_cost(1e-8))
            .run();
        
        match result {
            Ok(optimization_result) => {
                let optimal_input_f64 = optimization_result.state().best_param.unwrap_or(initial_input_f64);
                let optimal_input = Decimal::from_f64(optimal_input_f64).unwrap_or(initial_input);
                
                // Calculate final results with the optimal input
                let final_output = self.simulate_multihop_output(pools, optimal_input)?;
                let profit = final_output - optimal_input;
                let gas_cost = Decimal::from_f64(gas_cost_sol).unwrap_or(Decimal::new(5, 3)); // 0.005 SOL default
                let min_outputs = self.calculate_min_outputs(pools, optimal_input)?;
                let log_weights = self.calculate_log_weights(pools)?;
                
                // Verify the optimization found a better result
                let net_profit = profit - gas_cost;
                info!("Optimization completed: optimal_input={}, net_profit={}", optimal_input, net_profit);
                
                Ok(OptimalInputResult {
                    optimal_input,
                    max_net_profit: net_profit,
                    final_output,
                    gas_cost_sol: gas_cost,
                    requires_flash_loan: optimal_input > Decimal::from(1000), // Arbitrary threshold
                    min_outputs,
                    log_weights,
                })
            }
            Err(e) => {
                warn!("Optimization failed: {}. Using initial input as fallback.", e);
                
                // Fallback to initial input if optimization fails
                let final_output = self.simulate_multihop_output(pools, initial_input)?;
                let profit = final_output - initial_input;
                let gas_cost = Decimal::from_f64(gas_cost_sol).unwrap_or(Decimal::new(5, 3));
                let min_outputs = self.calculate_min_outputs(pools, initial_input)?;
                let log_weights = self.calculate_log_weights(pools)?;
                
                Ok(OptimalInputResult {
                    optimal_input: initial_input,
                    max_net_profit: profit - gas_cost,
                    final_output,
                    gas_cost_sol: gas_cost,
                    requires_flash_loan: false,
                    min_outputs,
                    log_weights,
                })
            }
        }
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
pub struct FeeManager {
    pub rpc_client: Option<Arc<crate::solana::rpc::SolanaRpcClient>>,
    pub network_congestion_cache: Arc<Mutex<Option<NetworkCongestionData>>>,
    pub jito_config: JitoConfiguration,
    pub priority_fee_config: PriorityFeeConfiguration,
}

impl Default for FeeManager {
    fn default() -> Self {
        Self {
            rpc_client: None,
            network_congestion_cache: Arc::new(Mutex::new(None)),
            jito_config: JitoConfiguration::default(),
            priority_fee_config: PriorityFeeConfiguration::default(),
        }
    }
}

impl FeeManager {
    /// Create new FeeManager with RPC client for dynamic fee calculation
    pub fn new(rpc_client: Arc<crate::solana::rpc::SolanaRpcClient>) -> Self {
        Self {
            rpc_client: Some(rpc_client),
            network_congestion_cache: Arc::new(Mutex::new(None)),
            jito_config: JitoConfiguration::default(),
            priority_fee_config: PriorityFeeConfiguration::default(),
        }
    }

    /// Calculate dynamic Solana priority fee based on network congestion
    pub async fn calculate_dynamic_priority_fee(&self, compute_units: u32) -> Result<u64> {
        // Update network congestion data
        self.update_congestion_data().await?;
        
        let congestion_data = self.network_congestion_cache.lock().await;
        let congestion_data = congestion_data.as_ref()
            .ok_or_else(|| anyhow!("No congestion data available"))?;

        // Calculate fee per compute unit based on congestion level
        let fee_per_cu_micro_lamports = match congestion_data.congestion_level {
            CongestionLevel::Low => {
                let recent_avg = self.calculate_percentile(&congestion_data.recent_priority_fees, 50.0);
                recent_avg.max(1) // At least 1 micro-lamport
            }
            CongestionLevel::Medium => {
                let recent_75th = self.calculate_percentile(&congestion_data.recent_priority_fees, 75.0);
                recent_75th.max(10)
            }
            CongestionLevel::High => {
                let recent_90th = self.calculate_percentile(&congestion_data.recent_priority_fees, 90.0);
                recent_90th.max(50)
            }
            CongestionLevel::Critical => {
                let recent_95th = self.calculate_percentile(&congestion_data.recent_priority_fees, 95.0);
                recent_95th.max(200)
            }
        };

        // Calculate total priority fee with safety margin
        let base_fee = ((compute_units as f64 * fee_per_cu_micro_lamports as f64) / 1_000_000.0) as u64;
        let adjusted_fee = (base_fee as f64 * self.priority_fee_config.safety_multiplier) as u64;
        
        let final_fee = adjusted_fee
            .max(self.priority_fee_config.min_fee_micro_lamports)
            .min(self.priority_fee_config.max_fee_lamports);

        info!("🔥 Dynamic priority fee calculated: CU={}, fee_per_cu={}, final_fee={} lamports", 
              compute_units, fee_per_cu_micro_lamports, final_fee);

        Ok(final_fee)
    }

    /// Calculate Jito tip for MEV protection
    pub fn calculate_jito_tip(&self, trade_value_sol: f64, complexity_factor: f64) -> u64 {
        if !self.jito_config.enabled {
            return 0;
        }

        // Base tip scaled by trade complexity
        let base_tip = (self.jito_config.base_tip_lamports as f64 * complexity_factor) as u64;
        
        // Value-based tip (percentage of trade value)
        let value_tip = (trade_value_sol * 1_000_000_000.0 * self.jito_config.value_based_scaling) as u64;
        
        let total_tip = base_tip + value_tip;
        
        // Apply min/max constraints
        let final_tip = total_tip
            .max(self.jito_config.min_tip_lamports)
            .min(self.jito_config.max_tip_lamports);

        debug!("💰 Jito tip calculated: base={}, value={}, final={} lamports", 
               base_tip, value_tip, final_tip);

        final_tip
    }

    /// Calculate comprehensive fees for multihop arbitrage
    pub fn calculate_multihop_fees(
        &self,
        pools: &[&PoolInfo],
        input_amount: &TokenAmount,
        sol_price_usd: f64,
    ) -> FeeBreakdown {
        let mut total_protocol_fee = 0.0;
        let mut gas_cost = 0.0;
        let mut slippage_cost = 0.0;
        let mut risk_score = 0.0;

        // Calculate protocol fees for each hop
        for pool in pools {
            let fee_rate = if let Some(bips) = pool.fee_rate_bips {
                bips as f64 / 10000.0
            } else {
                0.003 // Default 0.3%
            };
            total_protocol_fee += fee_rate;

            // Add gas cost per hop
            let hop_gas_cost = match pool.dex_type {
                DexType::Orca => 80_000,
                DexType::Raydium => 100_000,
                DexType::Jupiter => 120_000,
                DexType::Phoenix => 90_000,
                DexType::Meteora => 85_000,
                DexType::Lifinity => 95_000,
                _ => 90_000,
            } as f64 * 0.000000001; // Convert lamports to SOL
            
            gas_cost += hop_gas_cost * sol_price_usd;

            // Estimate slippage based on trade size vs liquidity
            if let Some(liquidity) = pool.liquidity {
                if liquidity > 0 {
                    let trade_impact = (input_amount.amount as f64) / (liquidity as f64);
                    slippage_cost += trade_impact * 0.1; // Simplified slippage model
                }
            }

            // Basic risk scoring
            risk_score += match pool.dex_type {
                DexType::Orca | DexType::Raydium => 0.1,
                DexType::Jupiter => 0.2, // Aggregator has more complexity
                _ => 0.15,
            };
        }

        let total_cost = total_protocol_fee + gas_cost + slippage_cost;
        
        FeeBreakdown {
            protocol_fee: total_protocol_fee,
            gas_fee: gas_cost,
            slippage_cost,
            total_cost,
            explanation: format!(
                "Multihop fees: {} hops, protocol: {:.4}%, gas: ${:.4}, slippage: {:.4}%",
                pools.len(),
                total_protocol_fee * 100.0,
                gas_cost,
                slippage_cost * 100.0
            ),
            risk_score: risk_score / pools.len() as f64, // Average risk
        }
    }

    /// Update network congestion data from RPC
    async fn update_congestion_data(&self) -> Result<()> {
        let rpc_client = self.rpc_client.as_ref()
            .ok_or_else(|| anyhow!("No RPC client configured"))?;

        // Check if cached data is still fresh (< 10 seconds old)
        {
            let cache = self.network_congestion_cache.lock().await;
            if let Some(ref data) = *cache {
                if data.timestamp.elapsed() < Duration::from_secs(10) {
                    return Ok(()); // Use cached data
                }
            }
        }

        // Fetch fresh data from RPC
        let epoch_info = rpc_client.primary_client.get_epoch_info().await?;
        let recent_performance = rpc_client.primary_client.get_recent_performance_samples(Some(10)).await?;
        
        // Calculate average slot time
        let avg_slot_time = if !recent_performance.is_empty() {
            recent_performance.iter()
                .map(|sample| sample.sample_period_secs as f64 / sample.num_slots as f64)
                .sum::<f64>() / recent_performance.len() as f64 * 1000.0 // Convert to ms
        } else {
            400.0 // Default 400ms slot time
        };

        // Simulate priority fee data (in production, use getRecentPrioritizationFees RPC)
        let recent_fees = self.simulate_recent_priority_fees(&avg_slot_time);
        let congestion_level = self.determine_congestion_level(&recent_fees);

        let congestion_data = NetworkCongestionData {
            current_slot: epoch_info.absolute_slot,
            slot_time_ms: avg_slot_time,
            recent_priority_fees: recent_fees,
            congestion_level: congestion_level.clone(),
            timestamp: std::time::Instant::now(),
        };

        // Update cache
        {
            let mut cache = self.network_congestion_cache.lock().await;
            *cache = Some(congestion_data);
        }

        debug!("🔄 Updated network congestion data: slot_time={:.1}ms, level={:?}", 
               avg_slot_time, congestion_level);

        Ok(())
    }

    /// Simulate recent priority fees (replace with actual RPC call in production)
    fn simulate_recent_priority_fees(&self, avg_slot_time_ms: &f64) -> Vec<u64> {
        // Simulate based on slot time - faster slots indicate more congestion
        let base_fee = if *avg_slot_time_ms < 300.0 {
            50 // High congestion
        } else if *avg_slot_time_ms < 400.0 {
            20 // Medium congestion  
        } else {
            5  // Low congestion
        };

        // Generate realistic distribution of recent fees
        (0..50).map(|i| {
            let variance = (i as f64 * 0.1).sin() * base_fee as f64 * 0.3;
            (base_fee as f64 + variance).max(1.0) as u64
        }).collect()
    }

    /// Determine congestion level from recent priority fees
    fn determine_congestion_level(&self, recent_fees: &[u64]) -> CongestionLevel {
        if recent_fees.is_empty() {
            return CongestionLevel::Medium;
        }

        let p75_fee = self.calculate_percentile(recent_fees, 75.0);
        
        match p75_fee {
            0..=10 => CongestionLevel::Low,
            11..=50 => CongestionLevel::Medium,
            51..=200 => CongestionLevel::High,
            _ => CongestionLevel::Critical,
        }
    }

    /// Calculate percentile from fee data
    fn calculate_percentile(&self, fees: &[u64], percentile: f64) -> u64 {
        if fees.is_empty() {
            return 1;
        }

        let mut sorted_fees = fees.to_vec();
        sorted_fees.sort();
        
        let index = ((percentile / 100.0) * (sorted_fees.len() - 1) as f64) as usize;
        sorted_fees[index.min(sorted_fees.len() - 1)]
    }

    /// Estimate compute units for different transaction types
    pub fn estimate_compute_units(&self, pools: &[&PoolInfo]) -> u32 {
        let base_cu = 50_000u32;
        let per_swap_cu = pools.iter().map(|pool| match pool.dex_type {
            DexType::Orca => 80_000,
            DexType::Raydium => 100_000,
            DexType::Jupiter => 120_000,
            DexType::Phoenix => 90_000,
            DexType::Meteora => 85_000,
            DexType::Lifinity => 95_000,
            _ => 90_000,
        }).sum::<u32>();

        base_cu + per_swap_cu
    }
}

// =============================================================================
// Optimization Framework for Arbitrage
// =============================================================================

/// Cost function for arbitrage profit optimization
/// This implements the negative profit function to be minimized (since argmin minimizes)
pub struct ArbitrageCostFunction<'a> {
    pools: &'a [&'a PoolInfo],
    gas_cost_sol: f64,
}

impl<'a> ArbitrageCostFunction<'a> {
    pub fn new(pools: &'a [&'a PoolInfo], gas_cost_sol: f64) -> Self {
        Self { pools, gas_cost_sol }
    }
    
    /// Calculate the negative net profit for a given input amount
    /// Returns negative value because argmin minimizes, but we want to maximize profit
    fn calculate_negative_net_profit(&self, input_amount: f64) -> f64 {
        // Simulate the multi-hop arbitrage execution
        let final_output = self.simulate_arbitrage_execution(input_amount);
        
        // Calculate profit = output - input - gas_cost
        let gross_profit = final_output - input_amount;
        let net_profit = gross_profit - self.gas_cost_sol;
        
        // Return negative profit (since we want to minimize the negative to find maximum)
        -net_profit
    }
    
    /// Simulate arbitrage execution through multiple pools
    fn simulate_arbitrage_execution(&self, input_amount: f64) -> f64 {
        let mut current_amount = input_amount;
        
        for pool in self.pools {
            current_amount = self.simulate_swap_through_pool(pool, current_amount);
            
            // If any swap fails or produces zero output, return 0
            if current_amount <= 0.0 {
                return 0.0;
            }
        }
        
        current_amount
    }
    
    /// Simulate a swap through a single pool using AMM formulas
    fn simulate_swap_through_pool(&self, pool: &PoolInfo, input_amount: f64) -> f64 {
        // This is where we implement proper AMM math based on pool type
        match pool.dex_type {
            DexType::Orca => self.simulate_orca_clmm_swap(pool, input_amount),
            DexType::Raydium => self.simulate_raydium_amm_swap(pool, input_amount),
            DexType::Meteora => self.simulate_meteora_swap(pool, input_amount),
            DexType::Lifinity => self.simulate_lifinity_swap(pool, input_amount),
            DexType::Whirlpool => self.simulate_orca_clmm_swap(pool, input_amount), // Same as Orca
            DexType::Phoenix => self.simulate_raydium_amm_swap(pool, input_amount), // Use AMM fallback for Phoenix
            DexType::Jupiter => {
                warn!("Jupiter is an aggregator, not a single pool DEX");
                self.simulate_raydium_amm_swap(pool, input_amount) // Fallback
            }
            DexType::Unknown(_) => {
                warn!("Unknown DEX type for pool {}, using fallback calculation", pool.address);
                self.simulate_raydium_amm_swap(pool, input_amount) // Fallback to constant product
            }
        }
    }
    
    /// Simulate Orca CLMM swap (Concentrated Liquidity Market Maker)
    fn simulate_orca_clmm_swap(&self, pool: &PoolInfo, input_amount: f64) -> f64 {
        // For CLMM, we need to consider current tick, liquidity density, and price impact
        if let (Some(liquidity), Some(sqrt_price)) = (pool.liquidity, pool.sqrt_price) {
            if liquidity > 0 && sqrt_price > 0 {
                // Use integer arithmetic for price calculation when possible
                let sqrt_price_u128 = sqrt_price as u128;
                let price_scaled = sqrt_price_u128.saturating_mul(sqrt_price_u128);
                let price = (price_scaled as f64) / ((1u128 << 64) as f64); // Scale down from Q64.64 format
                
                let fee_rate_bips = pool.fee_rate_bips.unwrap_or(25);
                
                // Calculate slippage using integer arithmetic
                let input_amount_scaled = (input_amount * 1_000_000.0) as u128; // Scale up for precision
                let liquidity_scaled = liquidity / 1000; // Scale down liquidity to prevent overflow
                
                // Calculate slippage in basis points: slippage_bps = (input / liquidity) * 10000
                let slippage_bps = if liquidity_scaled > 0 {
                    (input_amount_scaled / (liquidity_scaled as u128)).min(1000) // Cap at 10% (1000 bps)
                } else {
                    1000 // 10% default slippage if no liquidity data
                };
                
                // Apply slippage: effective_price = price * (1 + slippage_factor)
                // Convert slippage from basis points to factor
                let price_impact_factor = (10000 + slippage_bps) as f64 / 10000.0;
                let effective_price = price * price_impact_factor;
                
                let output_before_fees = input_amount / effective_price;
                
                // Apply fees using integer arithmetic
                let fee_factor = (10000 - fee_rate_bips) as f64 / 10000.0;
                let output_after_fees = output_before_fees * fee_factor;
                
                output_after_fees.max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
    
    /// Simulate Raydium constant product AMM swap
    fn simulate_raydium_amm_swap(&self, pool: &PoolInfo, input_amount: f64) -> f64 {
        // Constant product formula: x * y = k, using integer arithmetic for precision
        let reserve_a_u128 = pool.token_a.reserve as u128;
        let reserve_b_u128 = pool.token_b.reserve as u128;
        let fee_rate_bips = pool.fee_rate_bips.unwrap_or(25);
        
        if reserve_a_u128 > 0 && reserve_b_u128 > 0 {
            // Scale input amount for integer arithmetic
            let input_amount_scaled = (input_amount * 1_000_000.0) as u128;
            
            // Apply fees using integer arithmetic: input_after_fees = input * (10000 - fee_bips) / 10000
            let input_after_fees = input_amount_scaled
                .saturating_mul(10000u128.saturating_sub(fee_rate_bips as u128))
                .saturating_div(10000);
            
            // Constant product calculation: output = (reserve_b * input_after_fees) / (reserve_a + input_after_fees)
            let denominator = reserve_a_u128.saturating_add(input_after_fees);
            if denominator > 0 {
                let output_scaled = reserve_b_u128
                    .saturating_mul(input_after_fees)
                    .saturating_div(denominator);
                
                // Convert back to f64 (scale down)
                (output_scaled as f64) / 1_000_000.0
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
    
    /// Simulate Meteora DLMM swap
    fn simulate_meteora_swap(&self, pool: &PoolInfo, input_amount: f64) -> f64 {
        // Meteora uses Dynamic Liquidity Market Maker (DLMM) - simplified version
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                let fee_rate = pool.fee_rate_bips.unwrap_or(25) as f64 / 10000.0;
                let base_rate = (liquidity as f64) / 1_000_000.0;
                let output = input_amount * base_rate * (1.0 - fee_rate);
                output.max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
    
    /// Simulate Lifinity proactive market making swap
    fn simulate_lifinity_swap(&self, pool: &PoolInfo, input_amount: f64) -> f64 {
        // Lifinity uses proactive market making - simplified version
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                let fee_rate = pool.fee_rate_bips.unwrap_or(30) as f64 / 10000.0; // Slightly higher fees
                let base_rate = (liquidity as f64) / 1_200_000.0; // Different scaling
                let output = input_amount * base_rate * (1.0 - fee_rate);
                output.max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
}

impl<'a> CostFunction for ArbitrageCostFunction<'a> {
    type Param = f64;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, argmin::core::Error> {
        let cost = self.calculate_negative_net_profit(*param);
        Ok(cost)
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

/// Network congestion data for dynamic fee calculation
#[derive(Debug, Clone)]
pub struct NetworkCongestionData {
    pub current_slot: u64,
    pub slot_time_ms: f64,
    pub recent_priority_fees: Vec<u64>, // micro-lamports per compute unit
    pub congestion_level: CongestionLevel,
    pub timestamp: std::time::Instant,
}

/// Network congestion levels
#[derive(Debug, Clone, PartialEq)]
pub enum CongestionLevel {
    Low,    // < 10 micro-lamports/CU
    Medium, // 10-50 micro-lamports/CU  
    High,   // 50-200 micro-lamports/CU
    Critical, // > 200 micro-lamports/CU
}

/// Jito tip configuration for MEV protection
#[derive(Debug, Clone)]
pub struct JitoConfiguration {
    pub enabled: bool,
    pub min_tip_lamports: u64,
    pub max_tip_lamports: u64,
    pub base_tip_lamports: u64,
    pub value_based_scaling: f64, // Percentage of trade value
}

impl Default for JitoConfiguration {
    fn default() -> Self {
        Self {
            enabled: true,
            min_tip_lamports: 1_000,    // 0.000001 SOL
            max_tip_lamports: 100_000,  // 0.0001 SOL
            base_tip_lamports: 10_000,  // 0.00001 SOL
            value_based_scaling: 0.001, // 0.1% of trade value
        }
    }
}

/// Priority fee configuration
#[derive(Debug, Clone)]
pub struct PriorityFeeConfiguration {
    pub min_fee_micro_lamports: u64,
    pub max_fee_lamports: u64,
    pub target_percentile: f64, // 75th percentile by default
    pub safety_multiplier: f64,
}

impl Default for PriorityFeeConfiguration {
    fn default() -> Self {
        Self {
            min_fee_micro_lamports: 1,
            max_fee_lamports: 50_000, // 0.00005 SOL max
            target_percentile: 75.0,
            safety_multiplier: 1.2,   // 20% safety margin
        }
    }
}

/// Enhanced slippage model with pool depth analysis and volatility adjustments
#[derive(Debug)]
pub struct EnhancedSlippageModel {
    pub depth_analysis_levels: usize,
    pub volatility_window_minutes: u64,
    pub impact_scaling_factor: f64,
    pub price_history: Arc<Mutex<HashMap<String, VecDeque<(f64, std::time::Instant)>>>>,
}

impl Default for EnhancedSlippageModel {
    fn default() -> Self {
        Self {
            depth_analysis_levels: 10,
            volatility_window_minutes: 60,
            impact_scaling_factor: 1.5,
            price_history: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SlippageModel for EnhancedSlippageModel {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64 {
        // Base XYK slippage calculation
        let base_slippage = XYKSlippageModel.estimate_slippage(pool, input_amount, is_a_to_b);
        
        // Calculate pool depth impact
        let depth_impact = self.calculate_depth_impact(pool, input_amount, is_a_to_b);
        
        // Calculate volatility adjustment
        let volatility_multiplier = self.calculate_volatility_adjustment(pool);
        
        // Calculate trade size impact
        let size_impact = self.calculate_trade_size_impact(pool, input_amount);
        
        // Combine all factors
        let enhanced_slippage = base_slippage * 
            (1.0 + depth_impact) * 
            volatility_multiplier * 
            (1.0 + size_impact);

        // Cap slippage at reasonable maximum (50%)
        enhanced_slippage.min(0.5)
    }
}

impl EnhancedSlippageModel {
    /// Calculate slippage impact based on pool depth analysis
    fn calculate_depth_impact(&self, pool: &PoolInfo, input_amount: &TokenAmount, is_a_to_b: bool) -> f64 {
        // Use pool reserves to calculate depth-based slippage
        let input_reserve = if is_a_to_b {
            pool.token_a.reserve
        } else {
            pool.token_b.reserve
        };
        
        if input_reserve == 0 {
            return 0.5; // 50% slippage if no liquidity
        }
        
        // Calculate depth impact: slippage = input / reserve
        let depth_impact = (input_amount.amount as f64) / (input_reserve as f64);
        
        // Cap at reasonable maximum
        depth_impact.min(0.1) // Max 10% depth impact
    }

    /// Calculate volatility adjustment for slippage
    fn calculate_volatility_adjustment(&self, pool: &PoolInfo) -> f64 {
        // Use a simplified volatility calculation based on reserves ratio
        let ratio = if pool.token_b.reserve > 0 {
            (pool.token_a.reserve as f64) / (pool.token_b.reserve as f64)
        } else {
            1.0
        };
        
        // Higher ratio deviation from 1.0 indicates more volatility
        let volatility = (ratio - 1.0).abs() + 1.0;
        volatility.min(3.0) // Cap at 3x multiplier
    }

    /// Calculate trade size impact on slippage
    fn calculate_trade_size_impact(&self, pool: &PoolInfo, input_amount: &TokenAmount) -> f64 {
        let total_liquidity = pool.token_a.reserve + pool.token_b.reserve;
        
        if total_liquidity == 0 {
            return 2.0; // High impact if no liquidity
        }
        
        // Impact as ratio of trade size to total liquidity
        let impact = (input_amount.amount as f64) / (total_liquidity as f64);
        
        // Scale and cap the impact
        (impact * 100.0).min(1.5) // Cap at 1.5x multiplier
    }

}

/// Pool depth analyzer for advanced slippage calculations
pub struct PoolDepthAnalyzer;

impl PoolDepthAnalyzer {
    /// Analyze pool depth to predict slippage at different trade sizes
    pub fn analyze_depth_levels(pool: &PoolInfo, max_levels: usize) -> Vec<(f64, f64)> {
        let mut depth_levels = Vec::new();
        let base_reserve = pool.token_a.reserve.max(pool.token_b.reserve) as f64;
        
        if base_reserve == 0.0 {
            return depth_levels;
        }

        // Generate test trade sizes from 0.1% to 20% of pool
        for i in 1..=max_levels {
            let trade_ratio = (i as f64 * 0.02).min(0.2); // Up to 20%
            let trade_size = base_reserve * trade_ratio;
            
            // Calculate slippage for this trade size
            let test_amount = TokenAmount {
                amount: trade_size as u64,
                decimals: 9,
            };
            
            let slippage = XYKSlippageModel.estimate_slippage(pool, &test_amount, true);
            depth_levels.push((trade_ratio, slippage));
        }

        depth_levels
    }

    /// Recommend optimal trade size to minimize slippage
    pub fn recommend_optimal_trade_size(pool: &PoolInfo, desired_amount: u64) -> u64 {
        let depth_levels = Self::analyze_depth_levels(pool, 20);
        
        // Find the trade size that keeps slippage under 1%
        let max_slippage = 0.01; // 1%
        let base_reserve = pool.token_a.reserve.max(pool.token_b.reserve) as f64;
        
        for (trade_ratio, slippage) in depth_levels {
            if slippage > max_slippage {
                let recommended_size = (base_reserve * trade_ratio * 0.8) as u64; // 80% of threshold
                return recommended_size.min(desired_amount);
            }
        }

        desired_amount // If all levels are acceptable
    }
}
