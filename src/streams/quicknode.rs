// src/streams/quicknode.rs
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize; // Added for JavaScript bridge
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[derive(Debug, Deserialize)]
pub struct QuickNodeEvent {
    pub data: Vec<BlockData>,
}

#[derive(Debug, Deserialize)]
pub struct BlockData {
    #[serde(rename = "blockHeight")]
    pub block_height: u64,
    #[serde(rename = "blockTime")]
    pub block_time: i64,
    pub blockhash: String,
    #[serde(rename = "parentSlot")]
    pub parent_slot: u64,
    #[serde(rename = "previousBlockhash")]
    pub previous_blockhash: String,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub meta: Option<TransactionMeta>,
    pub transaction: Option<TransactionData>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionMeta {
    #[serde(rename = "computeUnitsConsumed")]
    pub compute_units_consumed: Option<u64>,
    pub err: Option<serde_json::Value>,
    pub fee: u64,
    #[serde(rename = "innerInstructions")]
    pub inner_instructions: Option<Vec<InnerInstruction>>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
    #[serde(rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(rename = "postBalances")]
    pub post_balances: Vec<u64>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionData {
    pub message: Message,
    pub signatures: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[serde(rename = "accountKeys")]
    pub account_keys: Vec<String>,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Deserialize)]
pub struct Instruction {
    pub accounts: Vec<u8>,
    pub data: String,
    #[serde(rename = "programIdIndex")]
    pub program_id_index: u8,
    #[serde(rename = "stackHeight")]
    pub stack_height: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct InnerInstruction {
    pub index: u8,
    pub instructions: Vec<ParsedInstruction>,
}

#[derive(Debug, Deserialize)]
pub struct ParsedInstruction {
    pub parsed: Option<serde_json::Value>,
    pub program: Option<String>,
    #[serde(rename = "programId")]
    pub program_id: String,
    #[serde(rename = "stackHeight")]
    pub stack_height: Option<u8>,
    pub accounts: Option<Vec<String>>,
    pub data: Option<String>,
}

// DEX-specific data structures for your arbitrage bot
#[derive(Debug, Clone)]
pub struct DexSwapEvent {
    pub signature: String,
    pub block_slot: u64,
    pub block_time: i64,
    pub dex_program: String,
    pub swap_accounts: Vec<String>,
    pub token_amounts: TokenAmounts,
    pub fee_lamports: u64,
}

#[derive(Debug, Clone)]
pub struct TokenAmounts {
    pub token_a_before: u64,
    pub token_a_after: u64,
    pub token_b_before: u64,
    pub token_b_after: u64,
}

impl QuickNodeEvent {
    /// Extract DEX swap events from QuickNode webhook data
    pub fn extract_dex_swaps(&self) -> Vec<DexSwapEvent> {
        let mut swaps = Vec::new();

        // DEX program IDs to monitor
        let dex_programs = [
            "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin", // Orca
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",  // Whirlpool
            "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB", // Meteora
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium CLMM
        ];

        for block in &self.data {
            for tx in &block.transactions {
                // Skip failed transactions
                if let Some(meta) = &tx.meta {
                    if meta.err.is_some() {
                        continue;
                    }
                }

                if let Some(tx_data) = &tx.transaction {
                    // Check if transaction involves DEX programs
                    for instruction in &tx_data.message.instructions {
                        let program_id_index = instruction.program_id_index as usize;
                        if program_id_index < tx_data.message.account_keys.len() {
                            let program_id = &tx_data.message.account_keys[program_id_index];

                            if dex_programs.contains(&program_id.as_str()) {
                                // Extract swap data
                                if let Some(swap) =
                                    self.parse_dex_swap(block, tx, tx_data, program_id, instruction)
                                {
                                    swaps.push(swap);
                                }
                            }
                        }
                    }
                }
            }
        }

        swaps
    }

    fn parse_dex_swap(
        &self,
        block: &BlockData,
        tx: &Transaction,
        tx_data: &TransactionData,
        program_id: &str,
        instruction: &Instruction,
    ) -> Option<DexSwapEvent> {
        let meta = tx.meta.as_ref()?;
        let signature = tx_data.signatures.first()?.clone();

        // Extract account addresses involved in the swap
        let mut swap_accounts = Vec::new();
        for &account_index in &instruction.accounts {
            if (account_index as usize) < tx_data.message.account_keys.len() {
                swap_accounts.push(tx_data.message.account_keys[account_index as usize].clone());
            }
        }

        // Calculate token amounts from balance changes
        let token_amounts = self.calculate_token_amounts(&meta.pre_balances, &meta.post_balances);

        Some(DexSwapEvent {
            signature,
            block_slot: block.parent_slot, // Use parent slot as the actual slot
            block_time: block.block_time,
            dex_program: program_id.to_string(),
            swap_accounts,
            token_amounts,
            fee_lamports: meta.fee,
        })
    }

    fn calculate_token_amounts(&self, pre_balances: &[u64], post_balances: &[u64]) -> TokenAmounts {
        // Simplified token amount calculation
        // In a real implementation, you'd parse the instruction data and log messages
        // to get exact token amounts

        let mut token_a_before = 0;
        let mut token_a_after = 0;
        let mut token_b_before = 0;
        let mut token_b_after = 0;

        // Find the accounts with balance changes (simplified)
        for i in 0..pre_balances.len().min(post_balances.len()) {
            if pre_balances[i] != post_balances[i] {
                if token_a_before == 0 {
                    token_a_before = pre_balances[i];
                    token_a_after = post_balances[i];
                } else if token_b_before == 0 {
                    token_b_before = pre_balances[i];
                    token_b_after = post_balances[i];
                    break;
                }
            }
        }

        TokenAmounts {
            token_a_before,
            token_a_after,
            token_b_before,
            token_b_after,
        }
    }
}

// Integration with your existing arbitrage system
impl DexSwapEvent {
    /// Convert to your existing PoolInfo format for arbitrage detection
    pub fn to_pool_update(&self) -> Option<crate::utils::PoolInfo> {
        // This would integrate with your existing pool info structure
        // Implementation depends on your current PoolInfo format
        None // Placeholder
    }

    /// Check if this swap represents a significant price movement
    pub fn is_significant_price_change(&self, threshold_bps: u64) -> bool {
        let token_a_change = if self.token_amounts.token_a_after > self.token_amounts.token_a_before
        {
            self.token_amounts.token_a_after - self.token_amounts.token_a_before
        } else {
            self.token_amounts.token_a_before - self.token_amounts.token_a_after
        };

        let token_a_change_bps = if self.token_amounts.token_a_before > 0 {
            (token_a_change * 10_000) / self.token_amounts.token_a_before
        } else {
            0
        };

        token_a_change_bps >= threshold_bps
    }
}

// 🧪 PAPER TRADING: QuickNode DEX Analysis Integration
// These structures match the JSON output from our JavaScript QuickNode analyzer

/// DEX Analysis data from JavaScript QuickNode analyzer
/// This matches the exact format returned by solana_dex_analyzer.js
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickNodeDexAnalysis {
    pub message: String,
    #[serde(rename = "blockTime")]
    pub block_time: u64,
    pub slot: u64,
    pub programs: HashMap<String, DexProgramMetrics>,
    #[serde(rename = "totalDexTransactions")]
    pub total_dex_transactions: u32,
    #[serde(rename = "totalValueChange")]
    pub total_value_change: String, // e.g., "2.3673 SOL"
}

/// Individual DEX program metrics from JavaScript analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexProgramMetrics {
    pub name: String,
    pub invocations: u32,
    pub transactions: u32,
    #[serde(rename = "valueChange")]
    pub value_change: String, // e.g., "-0.0002 SOL"
    #[serde(rename = "successfulTxs")]
    pub successful_txs: u32,
    #[serde(rename = "failedTxs")]
    pub failed_txs: u32,
    #[serde(rename = "uniqueUserCount")]
    pub unique_user_count: usize,
    #[serde(rename = "transactionShare")]
    pub transaction_share: String, // e.g., "11.96%"
    #[serde(rename = "successRate")]
    pub success_rate: String, // e.g., "100.00%"
}

/// 🧪 PAPER TRADING: Arbitrage opportunity detected from DEX analysis
#[derive(Debug, Clone)]
pub struct DexArbitrageOpportunity {
    pub dex_name: String,
    pub program_id: String,
    pub success_rate: f64, // 0.0 to 100.0
    pub transaction_volume: u32,
    pub value_change_sol: f64,
    pub transaction_share: f64, // 0.0 to 100.0
    pub estimated_profit_sol: f64,
    pub confidence_score: f64, // 0.0 to 1.0
    pub block_slot: u64,
    pub block_time: u64,
}

// 🧪 PAPER TRADING: QuickNode DEX Analysis Client
/// Client for receiving and processing DEX analysis from JavaScript
pub struct QuickNodeDexAnalysisClient {
    /// Minimum success rate to consider an opportunity (e.g., 90.0)
    pub min_success_rate: f64,
    /// Minimum transaction volume per block
    pub min_transaction_volume: u32,
    /// Minimum value change to consider significant
    pub min_value_change_sol: f64,
}

impl QuickNodeDexAnalysisClient {
    /// Create a new DEX analysis client with paper trading parameters
    pub fn new_for_paper_trading() -> Self {
        Self {
            min_success_rate: 90.0,
            min_transaction_volume: 5,
            min_value_change_sol: 0.01,
        }
    }

    /// 🧪 PAPER TRADING: Process DEX analysis data from JavaScript
    /// This method receives the JSON data from our JavaScript analyzer
    pub fn process_dex_analysis(&self, analysis: QuickNodeDexAnalysis) -> Vec<DexArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        for (program_id, metrics) in &analysis.programs {
            if let Some(opportunity) = self.evaluate_opportunity(program_id, metrics, &analysis) {
                opportunities.push(opportunity);
            }
        }

        // Sort by confidence score (highest first)
        opportunities.sort_by(|a, b| b.confidence_score.partial_cmp(&a.confidence_score).unwrap_or(std::cmp::Ordering::Equal));

        opportunities
    }

    /// 🧪 PAPER TRADING: Evaluate if a DEX program presents a trading opportunity
    fn evaluate_opportunity(
        &self,
        program_id: &str,
        metrics: &DexProgramMetrics,
        analysis: &QuickNodeDexAnalysis,
    ) -> Option<DexArbitrageOpportunity> {
        // Parse numeric values from string formats
        let success_rate = self.parse_percentage(&metrics.success_rate)?;
        let transaction_share = self.parse_percentage(&metrics.transaction_share)?;
        let value_change_sol = self.parse_sol_amount(&metrics.value_change)?;

        // Apply filters for paper trading opportunities
        if success_rate < self.min_success_rate {
            return None;
        }

        if metrics.transactions < self.min_transaction_volume {
            return None;
        }

        if value_change_sol.abs() < self.min_value_change_sol {
            return None;
        }

        // Calculate confidence score based on multiple factors
        let confidence_score = self.calculate_confidence_score(
            success_rate,
            metrics.transactions,
            transaction_share,
            value_change_sol,
        );

        // Estimate potential profit (simplified model for paper trading)
        let estimated_profit_sol = self.estimate_profit(
            value_change_sol,
            transaction_share,
            success_rate,
        );

        Some(DexArbitrageOpportunity {
            dex_name: metrics.name.clone(),
            program_id: program_id.to_string(),
            success_rate,
            transaction_volume: metrics.transactions,
            value_change_sol,
            transaction_share,
            estimated_profit_sol,
            confidence_score,
            block_slot: analysis.slot,
            block_time: analysis.block_time,
        })
    }

    /// Parse percentage string like "100.00%" to 100.0
    fn parse_percentage(&self, percentage_str: &str) -> Option<f64> {
        percentage_str
            .trim_end_matches('%')
            .parse::<f64>()
            .ok()
    }

    /// Parse SOL amount string like "-0.0002 SOL" to -0.0002
    fn parse_sol_amount(&self, sol_str: &str) -> Option<f64> {
        sol_str
            .trim_end_matches(" SOL")
            .parse::<f64>()
            .ok()
    }

    /// 🧪 PAPER TRADING: Calculate confidence score for opportunity
    /// Returns value between 0.0 and 1.0
    fn calculate_confidence_score(
        &self,
        success_rate: f64,
        transaction_count: u32,
        transaction_share: f64,
        value_change_sol: f64,
    ) -> f64 {
        // Weighted scoring model for paper trading
        let success_weight = 0.4;
        let volume_weight = 0.3;
        let share_weight = 0.2;
        let value_weight = 0.1;

        // Normalize each factor to 0.0-1.0 range
        let success_score = (success_rate / 100.0).min(1.0);
        let volume_score = (transaction_count as f64 / 50.0).min(1.0); // Cap at 50 transactions
        let share_score = (transaction_share / 100.0).min(1.0);
        let value_score = (value_change_sol.abs() / 1.0).min(1.0); // Cap at 1 SOL

        success_weight * success_score
            + volume_weight * volume_score
            + share_weight * share_score
            + value_weight * value_score
    }

    /// 🧪 PAPER TRADING: Estimate profit potential
    /// Simplified model for paper trading simulation
    fn estimate_profit(
        &self,
        value_change_sol: f64,
        transaction_share: f64,
        success_rate: f64,
    ) -> f64 {
        // Basic arbitrage profit estimation for paper trading
        let base_profit = value_change_sol.abs() * 0.01; // 1% of value change
        let volume_multiplier = (transaction_share / 100.0) * 2.0; // More volume = more opportunity
        let success_multiplier = success_rate / 100.0; // Risk adjustment

        base_profit * volume_multiplier * success_multiplier
    }

    /// 🧪 PAPER TRADING: Generate trade recommendation
    pub fn generate_trade_recommendation(&self, opportunity: &DexArbitrageOpportunity) -> TradeRecommendation {
        let direction = if opportunity.value_change_sol > 0.0 {
            TradeDirection::Long
        } else {
            TradeDirection::Short
        };

        let position_size = self.calculate_position_size(opportunity);

        TradeRecommendation {
            dex_name: opportunity.dex_name.clone(),
            direction,
            position_size_sol: position_size,
            confidence: opportunity.confidence_score,
            expected_profit_sol: opportunity.estimated_profit_sol,
            risk_level: self.assess_risk_level(opportunity),
            entry_reason: format!(
                "{}% success rate, {} transactions, {:.4} SOL value change",
                opportunity.success_rate,
                opportunity.transaction_volume,
                opportunity.value_change_sol
            ),
        }
    }

    /// Calculate appropriate position size for paper trading
    fn calculate_position_size(&self, opportunity: &DexArbitrageOpportunity) -> f64 {
        // Base position size calculation for paper trading
        let base_size = 10.0; // 10 SOL base
        let confidence_multiplier = opportunity.confidence_score;
        let max_position = 100.0; // 100 SOL max

        (base_size * confidence_multiplier).min(max_position)
    }

    /// Assess risk level of the opportunity
    fn assess_risk_level(&self, opportunity: &DexArbitrageOpportunity) -> RiskLevel {
        if opportunity.success_rate >= 95.0 && opportunity.confidence_score >= 0.8 {
            RiskLevel::Low
        } else if opportunity.success_rate >= 85.0 && opportunity.confidence_score >= 0.6 {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        }
    }
}

/// 🧪 PAPER TRADING: Trade recommendation generated from DEX analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecommendation {
    pub dex_name: String,
    pub direction: TradeDirection,
    pub position_size_sol: f64,
    pub confidence: f64,
    pub expected_profit_sol: f64,
    pub risk_level: RiskLevel,
    pub entry_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for TradeDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeDirection::Long => write!(f, "LONG"),
            TradeDirection::Short => write!(f, "SHORT"),
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "LOW"),
            RiskLevel::Medium => write!(f, "MEDIUM"),
            RiskLevel::High => write!(f, "HIGH"),
        }
    }
}

// 🧪 PAPER TRADING: Integration point for JavaScript bridge
/// This function will be called from JavaScript with DEX analysis data
pub fn process_quicknode_dex_analysis_for_paper_trading(
    analysis_json: &str,
) -> Result<Vec<TradeRecommendation>> {
    let analysis: QuickNodeDexAnalysis = serde_json::from_str(analysis_json)?;
    let client = QuickNodeDexAnalysisClient::new_for_paper_trading();
    
    let opportunities = client.process_dex_analysis(analysis);
    let recommendations = opportunities
        .iter()
        .map(|opp| client.generate_trade_recommendation(opp))
        .collect();
    
    Ok(recommendations)
}

// 🔗 JAVASCRIPT-RUST BRIDGE: FFI functions for Node.js integration
// These functions provide a C-compatible interface for JavaScript to call

/// 🔗 FFI Bridge: Process QuickNode DEX analysis from JavaScript
/// 
/// # Safety
/// This function is unsafe because it deals with raw pointers from JavaScript.
/// The caller must ensure the input_json pointer is valid and null-terminated.
#[no_mangle]
pub unsafe extern "C" fn process_quicknode_dex_analysis_ffi(
    input_json: *const c_char,
) -> *mut c_char {
    if input_json.is_null() {
        return std::ptr::null_mut();
    }

    // Convert C string to Rust string
    let c_str = match CStr::from_ptr(input_json).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Process the analysis
    let result = match process_quicknode_dex_analysis_for_paper_trading(c_str) {
        Ok(recommendations) => {
            // Convert recommendations to JSON for JavaScript
            match serde_json::to_string(&recommendations) {
                Ok(json) => json,
                Err(_) => r#"{"error": "Failed to serialize recommendations"}"#.to_string(),
            }
        }
        Err(e) => {
            format!(r#"{{"error": "Analysis failed: {}"}}"#, e)
        }
    };

    // Convert result back to C string for JavaScript
    match CString::new(result) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
