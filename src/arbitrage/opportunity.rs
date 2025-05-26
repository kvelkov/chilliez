// src/arbitrage/opportunity.rs
//! Defines the core struct for representing multi-hop, cross-DEX arbitrage opportunities.

// FeeManager might not be directly used here but good for context
// Removed: use crate::arbitrage::fee_manager::{FeeBreakdown, FeeManager}; 
use crate::utils::{DexType, PoolInfo}; // TokenAmount for context if needed -- Removed TokenAmount as it's unused
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use log; // For logging

/// Represents a single hop in a multi-hop arbitrage route.
#[derive(Debug, Clone)]
pub struct ArbHop {
    pub dex: DexType,
    pub pool: Pubkey, 
    pub input_token: String,  
    pub output_token: String, 
    pub input_amount: f64,    
    pub expected_output: f64, 
}

/// Represents a full arbitrage opportunity, possibly spanning multiple DEXes and hops.
#[derive(Debug, Clone)]
pub struct MultiHopArbOpportunity {
    pub id: String, 
    pub hops: Vec<ArbHop>,
    pub total_profit: f64, 
    pub profit_pct: f64, 

    pub input_token: String,
    pub output_token: String, 
    pub input_amount: f64,
    pub expected_output: f64,

    pub dex_path: Vec<DexType>,
    pub pool_path: Vec<Pubkey>,

    pub risk_score: Option<f64>,
    pub notes: Option<String>,

    pub estimated_profit_usd: Option<f64>, 
    pub input_amount_usd: Option<f64>,     
    pub output_amount_usd: Option<f64>,    // Made sure this is logged
    pub intermediate_tokens: Vec<String>, 

    // For 2-hop, source_pool is the first pool, target_pool is the second.
    // For >2 hops, source is first, target is last.
    pub source_pool: Arc<PoolInfo>, 
    pub target_pool: Arc<PoolInfo>, 

    pub input_token_mint: Pubkey,    
    pub output_token_mint: Pubkey,   
    // Mint of the token after the first hop for 2-hop, or first intermediate for multi-hop.
    pub intermediate_token_mint: Option<Pubkey>, 
}

// Default implementation for easier testing or placeholder creation
impl Default for MultiHopArbOpportunity {
    fn default() -> Self {
        // Create default PoolInfo instances for source and target pools
        let default_pool_info = Arc::new(PoolInfo::default());
        MultiHopArbOpportunity {
            id: "default_opportunity_id".to_string(),
            hops: vec![],
            total_profit: 0.0,
            profit_pct: 0.0,
            input_token: "UNKNOWN_IN".to_string(),
            output_token: "UNKNOWN_OUT".to_string(),
            input_amount: 0.0,
            expected_output: 0.0,
            dex_path: vec![],
            pool_path: vec![],
            risk_score: None,
            notes: Some("Default Opportunity".to_string()),
            estimated_profit_usd: None,
            input_amount_usd: None,
            output_amount_usd: None,
            intermediate_tokens: vec![],
            source_pool: Arc::clone(&default_pool_info),
            target_pool: Arc::clone(&default_pool_info),
            input_token_mint: Pubkey::default(),
            output_token_mint: Pubkey::default(),
            intermediate_token_mint: None,
        }
    }
}


impl MultiHopArbOpportunity {
    /// Returns true if the opportunity's profit_pct meets or exceeds the minimum threshold.
    /// min_profit_pct_threshold is expected as a percentage, e.g., 0.5 for 0.5%.
    pub fn is_profitable_by_pct(&self, min_profit_pct_threshold: f64) -> bool {
        self.profit_pct >= min_profit_pct_threshold
    }

    // Added a new method to check profitability in USD, if available
    pub fn is_profitable_by_usd(&self, min_profit_usd_threshold: f64) -> bool {
        match self.estimated_profit_usd {
            Some(profit_usd) => profit_usd >= min_profit_usd_threshold,
            None => {
                // Fallback: if USD profit is not available, check against percentage threshold
                // This makes the function more robust if USD values are missing.
                // Consider if this fallback is desired or if it should strictly be false.
                // For now, let's assume if USD is not available, we rely on percentage.
                // This behavior might need adjustment based on overall strategy.
                log::warn!(
                    "[OPP ID: {}] USD profit not available for is_profitable_by_usd check. Falling back to percentage check.",
                    self.id
                );
                self.is_profitable_by_pct(0.0) // Or some other default/configurable percentage if USD is primary
            }
        }
    }

    /// Combined profitability check using both percentage and USD thresholds.
    pub fn is_profitable(&self, min_profit_pct_threshold: f64, min_profit_usd_threshold: f64) -> bool {
        self.is_profitable_by_pct(min_profit_pct_threshold) && self.is_profitable_by_usd(min_profit_usd_threshold)
    }


    /// Logs details of each hop in the arbitrage opportunity.
    pub fn log_hop_details(&self) {
        if self.hops.is_empty() {
            log::warn!("[OPP ID: {}] No hops to log.", self.id);
            return;
        }
        for (i, hop) in self.hops.iter().enumerate() {
            log::info!(
                "[OPP ID: {}][HOP {}] DEX: {:?}, Pool: {}, Input: {} {:.6}, Output: {} {:.6}",
                self.id,
                i + 1,
                hop.dex,
                hop.pool,
                hop.input_token,
                hop.input_amount,
                hop.output_token,
                hop.expected_output
            );
        }
    }

    /// Logs a summary of the multi-hop arbitrage opportunity.
    pub fn log_summary(&self) {
        let path_str = self.dex_path.iter().map(|d| format!("{:?}", d)).collect::<Vec<String>>().join(" -> ");
        let intermediate_mints_str = self.intermediate_token_mint.map_or_else(
            || "N/A".to_string(), 
            |mint| mint.to_string()
        );

        log::info!(
            "[ARB OPPORTUNITY ID: {}] Path: {} | Input: {:.6} {} (Mint: {}) -> Output: {:.6} {} (Mint: {}) | Intermediate Mint(s): {} | Profit: {:.6} {} ({:.4}%) | Est. USD Profit: {:?}, Input USD: {:?}, Output USD: {:?} | Source Pool: {} ({}), Target Pool: {} ({}) | Pools Path: {:?} | Notes: {}",
            self.id,
            path_str,
            self.input_amount,
            self.input_token,
            self.input_token_mint,
            self.expected_output,
            self.output_token,
            self.output_token_mint,
            intermediate_mints_str, // Using the formatted string for Option<Pubkey>
            self.total_profit,
            self.output_token, 
            self.profit_pct,
            self.estimated_profit_usd.map_or_else(|| "N/A".to_string(), |p| format!("{:.2}", p)),
            self.input_amount_usd.map_or_else(|| "N/A".to_string(), |p| format!("{:.2}", p)),
            self.output_amount_usd.map_or_else(|| "N/A".to_string(), |p| format!("{:.2}", p)),
            self.source_pool.name, self.source_pool.address, // Log source_pool name and address
            self.target_pool.name, self.target_pool.address, // Log target_pool name and address
            self.pool_path,
            self.notes.as_deref().unwrap_or("N/A")
        );
        if !self.hops.is_empty() {
            self.log_hop_details();
        } else {
            log::warn!("[OPP ID: {}] Summary logged, but no hops were present in the opportunity.", self.id);
        }
    }
}

// analyze_arbitrage_opportunity was previously here.
// It's specific to FeeManager's capabilities and less about the Opportunity struct itself.
// If needed, it should live in `fee_manager.rs` or be a free function in `arbitrage/mod.rs`
// that uses FeeManager. For now, removing it from here to keep this file focused on the struct.