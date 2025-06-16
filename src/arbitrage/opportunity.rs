//! Opportunity Module
//!
//! This module defines the core structures for representing arbitrage opportunities,
//! including each individual swap ("hop") in a multi-hop route, and the full arbitrage
//! opportunity.
//!
//! In addition to representing the raw data, the opportunity is responsible for:
//!   - Validating its internal consistency.
//!   - Providing profitability checks (by percentage & USD).
//!   - Logging detailed information along its route.
//!   - Simulating execution to update expected outcomes.
//!   - Updating USD estimates given a conversion rate.
//!   - Computing a simple risk score as a placeholder for more advanced models.

use crate::utils::{DexType, PoolInfo};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Represents a single hop in a multi-hop arbitrage route.
#[derive(Debug, Clone)]
pub struct ArbHop {
    /// The DEX used for the swap.
    pub dex: DexType,
    /// The pool where the swap takes place.
    pub pool: Pubkey,
    /// The symbol of the input token.
    pub input_token: String,
    /// The symbol of the output token.
    pub output_token: String,
    /// The amount of input tokens committed to the swap.
    pub input_amount: f64,
    /// The expected amount of output tokens from the swap.
    pub expected_output: f64,
}

/// Enhanced hop structure for advanced arbitrage paths
#[derive(Debug, Clone)]
pub struct EnhancedArbHop {
    /// The pool address where the swap takes place.
    pub pool_address: Pubkey,
    /// The input token mint address.
    pub input_token: Pubkey,
    /// The output token mint address.
    pub output_token: Pubkey,
    /// The amount of input tokens committed to the swap.
    pub input_amount: f64,
    /// The expected amount of output tokens from the swap.
    pub expected_output: f64,
    /// Reference to the pool information.
    pub pool_info: Arc<PoolInfo>,
}

/// Legacy ArbHop structure for backward compatibility
#[derive(Debug, Clone)]
pub struct LegacyArbHop { // not in use - Defined but not instantiated or used elsewhere in the provided codebase.
    /// The DEX used for the swap.
    pub dex: DexType,
    /// The pool where the swap takes place.
    pub pool: Pubkey,
    /// The symbol of the input token.
    pub input_token: String,
    /// The symbol of the output token.
    pub output_token: String,
    /// The amount of input tokens committed to the swap.
    pub input_amount: f64,
    /// The expected amount of output tokens from the swap.
    pub expected_output: f64,
}

/// Represents a full arbitrage opportunity, possibly spanning multiple DEXes and hops.
#[derive(Debug, Clone)]
pub struct MultiHopArbOpportunity {
    /// Unique identifier for the opportunity.
    pub id: String,
    /// The sequence of hops undertaken to complete the arbitrage.
    pub hops: Vec<ArbHop>,
    /// The net profit (in input token units) expected from the trade.
    pub total_profit: f64,
    /// The profit expressed as a percentage of the input.
    pub profit_pct: f64,

    /// The symbol of the input token.
    pub input_token: String,
    /// The symbol of the output token.
    pub output_token: String,
    /// The amount of input tokens.
    pub input_amount: f64,
    /// The expected final output tokens (after all hops).
    pub expected_output: f64,

    /// The sequence of DEX types used.
    pub dex_path: Vec<DexType>,
    /// The sequence of pool addresses used.
    pub pool_path: Vec<Pubkey>,

    /// Optional risk score based on slippage and other factors.
    pub risk_score: Option<f64>,
    /// Any additional notes associated with the opportunity.
    pub notes: Option<String>,

    /// Optional estimated profit in USD.
    pub estimated_profit_usd: Option<f64>,
    /// Optional input amount in USD.
    pub input_amount_usd: Option<f64>,
    /// Optional final output amount in USD.
    pub output_amount_usd: Option<f64>,
    /// The symbols of any intermediate tokens in the swap sequence.
    pub intermediate_tokens: Vec<String>,

    /// For 2-hop, source_pool is the first pool, target_pool is the second.
    /// For multi-hop (>2 hops), source is first and target is last.
    pub source_pool: Arc<PoolInfo>,
    pub target_pool: Arc<PoolInfo>,

    /// Mint addresses for input and output tokens.
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    /// Mint of the token after the first hop for 2-hop (or first intermediate for multi-hop).
    pub intermediate_token_mint: Option<Pubkey>,
    /// Optional estimated gas cost for transaction execution.
    pub estimated_gas_cost: Option<u64>,
    /// Timestamp when the opportunity was detected (for time-sensitive analysis).
    pub detected_at: Option<std::time::Instant>,
}

impl MultiHopArbOpportunity {
    /// Returns true if the opportunity’s profit (as a percentage) meets or exceeds the given threshold.
    /// For example, 0.5 represents 0.5%.
    pub fn is_profitable_by_pct(&self, min_profit_pct_threshold: f64) -> bool {
        self.profit_pct >= min_profit_pct_threshold
    }

    /// Returns true if the opportunity’s estimated profit in USD meets the provided threshold.
    /// If no USD value is available, it logs a warning and falls back to a percentage check.
    pub fn is_profitable_by_usd(&self, min_profit_usd_threshold: f64) -> bool {
        match self.estimated_profit_usd {
            Some(profit_usd) => profit_usd >= min_profit_usd_threshold,
            None => {
                log::warn!(
                    "[Opportunity {}] USD profit not available; falling back on percentage check.",
                    self.id
                );
                self.is_profitable_by_pct(0.0)
            }
        }
    }

    /// Combined profitability check using both percentage and USD thresholds.
    pub fn is_profitable(&self, min_profit_pct_threshold: f64, min_profit_usd_threshold: f64) -> bool {
        self.is_profitable_by_pct(min_profit_pct_threshold) && self.is_profitable_by_usd(min_profit_usd_threshold)
    }

    /// Logs detailed information for each hop in the arbitrage route.
    pub fn log_hop_details(&self) {
        if self.hops.is_empty() {
            log::warn!("[Opportunity {}] No hops to log.", self.id);
            return;
        }
        for (index, hop) in self.hops.iter().enumerate() {
            log::info!(
                "[Opportunity {}][Hop {}] DEX: {:?}, Pool: {}, Input: {} {:.6}, Expected Output: {} {:.6}",
                self.id,
                index + 1,
                hop.dex,
                hop.pool,
                hop.input_token,
                hop.input_amount,
                hop.output_token,
                hop.expected_output
            );
        }
    }

    /// Logs a comprehensive summary of this arbitrage opportunity.
    pub fn log_summary(&self) {
        let path_str = self
            .dex_path
            .iter()
            .map(|dex| format!("{:?}", dex))
            .collect::<Vec<String>>()
            .join(" -> ");
        let intermediate_mints_str = self
            .intermediate_token_mint
            .map_or("N/A".to_string(), |mint| mint.to_string());
        log::info!(
            "[ARB OPPORTUNITY ID: {}] Path: {} | Input: {:.6} {} (Mint: {}) -> Output: {:.6} {} (Mint: {}) | Intermediate Mint(s): {} | Total Profit: {:.6} {} ({:.4}%) | Est. USD Profit: {} | Input USD: {} | Output USD: {} | Source Pool: {} ({:?}) -> Target Pool: {} ({:?}) | Pools Path: {:?} | Notes: {}",
            self.id,
            path_str,
            self.input_amount,
            self.input_token,
            self.input_token_mint,
            self.expected_output,
            self.output_token,
            self.output_token_mint,
            intermediate_mints_str,
            self.total_profit,
            self.output_token,
            self.profit_pct,
            self.estimated_profit_usd.map_or("N/A".to_string(), |p| format!("{:.2}", p)),
            self.input_amount_usd.map_or("N/A".to_string(), |v| format!("{:.2}", v)),
            self.output_amount_usd.map_or("N/A".to_string(), |v| format!("{:.2}", v)),
            self.source_pool.name,
            self.source_pool.address,
            self.target_pool.name,
            self.target_pool.address,
            self.pool_path,
            self.notes.as_deref().unwrap_or("N/A")
        );
        if !self.hops.is_empty() {
            self.log_hop_details();
        } else {
            log::warn!("[Opportunity {}] Summary logged, but no hop details available.", self.id);
        }
    }

    /// VALIDATION: Checks that the opportunity structure is consistent.
    /// Returns true if the essential fields are valid.
    pub fn validate(&self) -> bool {
        if self.input_amount <= 0.0 || self.expected_output <= 0.0 {
            log::error!(
                "[Opportunity {}] Validation failed: input_amount or expected_output is non-positive.",
                self.id
            );
            return false;
        }
        if self.dex_path.len() != self.pool_path.len() || self.pool_path.is_empty() {
            log::error!(
                "[Opportunity {}] Validation failed: Invalid dex_path or pool_path lengths.",
                self.id
            );
            return false;
        }
        true
    }

    /// SIMULATION: Provides a placeholder simulation execution.
    /// In a full implementation, this might recalculate expected outputs given updated reserves.
    pub fn simulate_execution(&self) -> Self { // not in use - Current implementation is a placeholder.
        // Placeholder: simply returns a copy.
        // In a real implementation, this would use market data (via calculator/fee_manager) to recalc outcomes.
        self.clone()
    }

    /// Updates USD-based metrics (estimated profit, input & output USD) using the supplied conversion rate.
    pub fn update_estimated_usd_profit(&mut self, conversion_rate: f64) {
        self.estimated_profit_usd = Some(self.total_profit * conversion_rate);
        self.input_amount_usd = Some(self.input_amount * conversion_rate);
        self.output_amount_usd = Some(self.expected_output * conversion_rate);
    }

    /// Calculates a simple risk score and sets it on the opportunity.
    /// This placeholder risk score is inversely proportional to profit_pct.
    pub fn calculate_risk_score(&mut self) { // not in use - Current implementation is a placeholder.
        if self.profit_pct > 0.0 {
            self.risk_score = Some(1.0 / self.profit_pct);
        } else {
            self.risk_score = Some(1.0);
        }
    }
}

impl Default for MultiHopArbOpportunity {
    fn default() -> Self {
        // Create default PoolInfo instance used as a placeholder.
        let default_pool_info = Arc::new(PoolInfo::default());
        Self {
            id: String::default(),
            hops: Vec::new(),
            total_profit: 0.0,
            profit_pct: 0.0,
            input_token: String::default(),
            output_token: String::default(),
            input_amount: 0.0,
            expected_output: 0.0,
            dex_path: Vec::new(), // This should be Vec<DexType> as per struct definition
            pool_path: Vec::new(),
            risk_score: None,
            notes: None,
            estimated_profit_usd: None,
            input_amount_usd: None,
            output_amount_usd: None,
            intermediate_tokens: Vec::new(),
            source_pool: default_pool_info.clone(),
            target_pool: default_pool_info,
            input_token_mint: Pubkey::default(),
            output_token_mint: Pubkey::default(),
            intermediate_token_mint: None,
            estimated_gas_cost: None,
            detected_at: None,
        }
    }
}

/// Enhanced arbitrage opportunity structure for advanced multi-hop trading
#[derive(Debug, Clone)]
pub struct AdvancedMultiHopOpportunity {
    pub id: String,
    pub path: Vec<EnhancedArbHop>,
    pub dex_sequence: Vec<DexType>,
    pub expected_profit_usd: f64,
    pub profit_pct: f64,
    pub confidence_score: f64,
    pub execution_priority: u8,
    pub estimated_gas_cost: u64,
    pub slippage_tolerance: f64,
    pub max_execution_time_ms: u64,
    pub requires_batch: bool,
    pub total_liquidity: f64,
    pub path_complexity: u8,
}

impl AdvancedMultiHopOpportunity {
    /// Check if this opportunity meets profitability thresholds
    pub fn is_profitable(&self, min_profit_pct: f64, min_profit_usd: f64) -> bool {
        self.profit_pct >= min_profit_pct && self.expected_profit_usd >= min_profit_usd
    }

    /// Calculate execution urgency (1-10, 10 being most urgent)
    pub fn execution_urgency(&self) -> u8 {
        if self.profit_pct > 10.0 {
            10
        } else if self.profit_pct > 5.0 {
            9
        } else if self.profit_pct > 3.0 {
            8
        } else if self.profit_pct > 2.0 {
            7
        } else if self.profit_pct > 1.0 {
            6
        } else {
            5
        }
    }

    /// Log comprehensive summary
    pub fn log_summary(&self) {
        let dex_path: Vec<String> = self.dex_sequence.iter()
            .map(|dex| format!("{:?}", dex))
            .collect();

        log::info!(
            "[ADVANCED ARB {}] Profit: {:.2}% (${:.2}) | Priority: {} | DEX Path: {} | Hops: {} | Liquidity: ${:.0} | Gas: {} | Batch: {}",
            self.id,
            self.profit_pct,
            self.expected_profit_usd,
            self.execution_priority,
            dex_path.join("→"),
            self.path.len(),
            self.total_liquidity,
            self.estimated_gas_cost,
            self.requires_batch
        );
    }

    /// Convert to legacy MultiHopArbOpportunity for compatibility
    pub fn to_legacy(&self) -> MultiHopArbOpportunity {
        let legacy_hops: Vec<ArbHop> = self.path.iter().enumerate().map(|(i, hop)| {
            ArbHop {
                dex: self.dex_sequence.get(i).cloned().unwrap_or(DexType::Unknown("Unknown".to_string())),
                pool: hop.pool_address,
                input_token: hop.input_token.to_string(),
                output_token: hop.output_token.to_string(),
                input_amount: hop.input_amount,
                expected_output: hop.expected_output,
            }
        }).collect();

        let dex_path = self.dex_sequence.clone();
        let pool_path: Vec<Pubkey> = self.path.iter().map(|hop| hop.pool_address).collect();

        let input_amount = self.path.first().map(|h| h.input_amount).unwrap_or(0.0);
        let expected_output = self.path.last().map(|h| h.expected_output).unwrap_or(0.0);
        let total_profit = expected_output - input_amount;

        // Use first and last pool info, or default if empty
        let source_pool = self.path.first()
            .map(|h| Arc::clone(&h.pool_info))
            .unwrap_or_else(|| Arc::new(PoolInfo::default()));
        let target_pool = self.path.last()
            .map(|h| Arc::clone(&h.pool_info))
            .unwrap_or_else(|| Arc::new(PoolInfo::default()));

        MultiHopArbOpportunity {
            id: self.id.clone(),
            hops: legacy_hops,
            total_profit,
            profit_pct: self.profit_pct,
            input_token: self.path.first().map(|h| h.input_token.to_string()).unwrap_or_default(),
            output_token: self.path.last().map(|h| h.output_token.to_string()).unwrap_or_default(),
            input_amount,
            expected_output,
            dex_path,
            pool_path,
            risk_score: Some(1.0 / self.confidence_score.max(0.1)),
            notes: Some(format!("Advanced opportunity with {} hops", self.path.len())),
            estimated_profit_usd: Some(self.expected_profit_usd),
            input_amount_usd: None,
            output_amount_usd: None,
            intermediate_tokens: vec![], // Would need to be computed from path
            source_pool,
            target_pool,
            input_token_mint: self.path.first().map(|h| h.input_token).unwrap_or_default(),
            output_token_mint: self.path.last().map(|h| h.output_token).unwrap_or_default(),
            intermediate_token_mint: None, // Could be computed from intermediate hops
            estimated_gas_cost: Some(self.estimated_gas_cost),
            detected_at: Some(std::time::Instant::now()),
        }
    }
}
