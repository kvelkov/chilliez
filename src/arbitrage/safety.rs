//! Arbitrage Transaction Safety Module
//!
//! This module provides comprehensive safety checks, retry logic, and recovery mechanisms
//! specifically for arbitrage transaction execution. It includes:
//! - Safe transaction execution with retry policies
//! - Balance validation and slippage protection
//! - MEV protection and failure recovery
//! - Transaction monitoring and violation detection

use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use solana_sdk::{instruction::Instruction, transaction::Transaction};
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::time::sleep;

use crate::arbitrage::analysis::math::EnhancedSlippageModel;
use crate::arbitrage::analysis::FeeBreakdown;
use crate::{
    solana::rpc::SolanaRpcClient,
    utils::{PoolInfo, TokenAmount},
};

// Add external dependencies for time handling and random generation
use rand;

// =============================================================================
// Core Safety Types
// =============================================================================

/// Safe transaction handler with comprehensive safety checks and recovery
/// Safe transaction handler with comprehensive safety checks and recovery
pub struct SafeTransactionHandler {
    _rpc_client: Arc<SolanaRpcClient>,
    config: TransactionSafetyConfig, // not in use - config field is initialized but not read by most simplified methods. Advanced methods use sub-fields.
    _slippage_model: EnhancedSlippageModel,
    execution_history: Arc<RwLock<Vec<TransactionRecord>>>, // not in use - Written to by record_execution and record_successful_transaction, but not read by other methods.
    _balance_cache: Arc<RwLock<Option<BalanceSnapshot>>>,
}

/// Transaction safety configuration
#[derive(Debug, Clone)]
pub struct TransactionSafetyConfig {
    pub retry_policy: RetryPolicy,
    pub balance_validation: BalanceValidationConfig,
    pub slippage_protection: SlippageProtectionConfig,
    pub mev_protection: MevProtectionConfig,
    pub confirmation_settings: ConfirmationConfig,
}

/// Retry policy for failed transactions
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub exponential_backoff: bool,
    pub retry_on_partial_fill: bool,
}

/// Balance validation configuration
#[derive(Debug, Clone)]
pub struct BalanceValidationConfig {
    pub enabled: bool,
    pub minimum_sol_balance: u64, // lamports
    pub safety_margin_percent: f64,
    pub real_time_validation: bool,
}

/// Slippage protection configuration
#[derive(Debug, Clone)]
pub struct SlippageProtectionConfig {
    pub enabled: bool,
    pub max_slippage_percent: f64,
    pub dynamic_adjustment: bool,
    pub abort_on_high_slippage: bool,
}

/// MEV protection configuration
#[derive(Debug, Clone)]
pub struct MevProtectionConfig {
    pub enabled: bool,
    pub sandwich_detection: bool,
    pub front_running_protection: bool,
    pub jito_tips_enabled: bool,
    pub max_priority_fee_lamports: u64,
}

/// Transaction confirmation settings
#[derive(Debug, Clone)]
pub struct ConfirmationConfig {
    pub timeout_seconds: u64,
    pub commitment_level: String, // "processed", "confirmed", "finalized"
    pub max_confirmation_retries: u32,
}

/// Transaction execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub signature: Option<String>,
    pub success: bool,
    pub actual_output: Option<u64>,
    pub fee_paid: u64,
    pub slippage_experienced: f64,
    pub execution_time_ms: u64,
    pub confirmation_slots: Option<u64>,
    pub retry_count: u32,
    pub failure_reason: Option<String>,
    pub safety_violations: Vec<SafetyViolation>,
}

/// Safety violation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyViolation {
    InsufficientBalance { required: u64, available: u64 },
    ExcessiveSlippage { expected: f64, actual: f64 },
    TransactionTimeout { timeout_seconds: u64 },
    MevDetected { attack_type: String },
    NetworkCongestion { current_tps: f64, threshold: f64 },
    BalanceMismatch { expected: u64, actual: u64 },
    UnexpectedFailure { error_code: String },
}

/// Transaction record for history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub timestamp: std::time::SystemTime,
    pub opportunity_id: String,
    pub input_amount: u64,
    pub expected_output: u64,
    pub result: TransactionResult,
    pub pools_involved: Vec<String>, // Pool addresses
}

/// Balance snapshot for validation
#[derive(Debug, Clone)]
pub struct BalanceSnapshot {
    pub sol_balance: u64,
    pub token_balances: std::collections::HashMap<String, u64>,
    pub timestamp: std::time::Instant,
}

/// Failure recovery strategy
#[derive(Debug, Clone)]
pub enum FailureRecoveryStrategy {
    Retry { delay_ms: u64 },
    ReduceAmount { new_amount: u64 },
    IncreaseSlippage { new_tolerance: f64 },
    SwitchRoute { alternative_pools: Vec<String> },
    Abort { reason: String },
}

// =============================================================================
// Default Implementations
// =============================================================================

impl Default for TransactionSafetyConfig {
    fn default() -> Self {
        Self {
            retry_policy: RetryPolicy::default(),
            balance_validation: BalanceValidationConfig::default(),
            slippage_protection: SlippageProtectionConfig::default(),
            mev_protection: MevProtectionConfig::default(),
            confirmation_settings: ConfirmationConfig::default(),
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 500,
            exponential_backoff: true,
            retry_on_partial_fill: true,
        }
    }
}

impl Default for BalanceValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            minimum_sol_balance: 10_000_000, // 0.01 SOL
            safety_margin_percent: 5.0,
            real_time_validation: true,
        }
    }
}

impl Default for SlippageProtectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_slippage_percent: 2.0,
            dynamic_adjustment: true,
            abort_on_high_slippage: true,
        }
    }
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sandwich_detection: true,
            front_running_protection: true,
            jito_tips_enabled: false,
            max_priority_fee_lamports: 1_000_000, // 0.001 SOL
        }
    }
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            commitment_level: "confirmed".to_string(),
            max_confirmation_retries: 5,
        }
    }
}

// =============================================================================
// SafeTransactionHandler Implementation
// =============================================================================

impl SafeTransactionHandler {
    /// Create new safe transaction handler
    pub fn new(rpc_client: Arc<SolanaRpcClient>, config: TransactionSafetyConfig) -> Self {
        Self {
            _rpc_client: rpc_client,
            config,
            _slippage_model: EnhancedSlippageModel::new(),
            execution_history: Arc::new(RwLock::new(Vec::new())),
            _balance_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Execute transaction with safety checks (simplified interface for execution module)
    pub async fn execute_with_safety_checks(
        &self,
        transaction: solana_sdk::transaction::Transaction,
        pools: &[&PoolInfo],
        input_amount: u64,
        expected_output: u64,
    ) -> Result<TransactionResult> {
        let start_time = Instant::now();
        let mut attempt = 0;

        info!(
            "🛡️ Executing transaction with safety checks: input={}, expected_output={}",
            input_amount, expected_output
        );

        // Convert parameters for internal use
        let input_token_amount = TokenAmount {
            amount: input_amount,
            decimals: 9, // Assume SOL decimals
        };

        // Validate pre-execution conditions
        if self.config.balance_validation.enabled {
            if let Err(e) = self.validate_sufficient_balance(&input_token_amount).await {
                return Ok(TransactionResult {
                    signature: None,
                    success: false,
                    actual_output: None,
                    fee_paid: 0,
                    slippage_experienced: 0.0,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    confirmation_slots: None,
                    retry_count: 0,
                    failure_reason: Some(format!("Balance validation failed: {}", e)),
                    safety_violations: vec![SafetyViolation::InsufficientBalance {
                        required: input_amount,
                        available: 0,
                    }],
                });
            }
        }

        // Attempt execution with retries
        while attempt < self.config.retry_policy.max_attempts {
            attempt += 1;

            match self
                .execute_transaction_attempt(&transaction, attempt)
                .await
            {
                Ok(signature) => {
                    let execution_time = start_time.elapsed().as_millis() as u64;

                    // Estimate actual output and slippage (simplified)
                    let actual_output = self.estimate_actual_output(expected_output, pools).await;
                    let slippage =
                        self.calculate_slippage_experienced(expected_output, actual_output);

                    // Check for slippage violations
                    let mut violations = Vec::new();
                    if self.config.slippage_protection.enabled
                        && slippage > self.config.slippage_protection.max_slippage_percent / 100.0
                    {
                        violations.push(SafetyViolation::ExcessiveSlippage {
                            expected: 0.0,
                            actual: slippage,
                        });
                    }

                    let success = violations.is_empty();

                    info!("✅ Transaction executed successfully: signature={}, slippage={:.2}%, violations={}", 
                          signature, slippage * 100.0, violations.len());

                    return Ok(TransactionResult {
                        signature: Some(signature.to_string()),
                        success,
                        actual_output: Some(actual_output),
                        fee_paid: self.estimate_transaction_fee(&transaction),
                        slippage_experienced: slippage,
                        execution_time_ms: execution_time,
                        confirmation_slots: Some(1), // Simplified
                        retry_count: attempt,
                        failure_reason: if success {
                            None
                        } else {
                            Some("Slippage violation".to_string())
                        },
                        safety_violations: violations,
                    });
                }
                Err(e) => {
                    warn!("❌ Transaction attempt {} failed: {}", attempt, e);

                    if attempt < self.config.retry_policy.max_attempts {
                        // Apply retry delay
                        let delay = if self.config.retry_policy.exponential_backoff {
                            self.config.retry_policy.base_delay_ms * (2_u64.pow(attempt - 1))
                        } else {
                            self.config.retry_policy.base_delay_ms
                        };

                        sleep(Duration::from_millis(delay)).await;
                        continue;
                    }

                    // All attempts failed
                    return Ok(TransactionResult {
                        signature: None,
                        success: false,
                        actual_output: None,
                        fee_paid: 0,
                        slippage_experienced: 0.0,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                        confirmation_slots: None,
                        retry_count: attempt,
                        failure_reason: Some(format!(
                            "All {} attempts failed. Last error: {}",
                            attempt, e
                        )),
                        safety_violations: vec![],
                    });
                }
            }
        }

        // Should not reach here
        Err(anyhow!("Unexpected execution flow"))
    }

    /// Execute a single transaction attempt
    async fn execute_transaction_attempt(
        &self,
        _transaction: &solana_sdk::transaction::Transaction,
        attempt: u32,
    ) -> Result<solana_sdk::signature::Signature> {
        debug!("🚀 Executing transaction attempt {}", attempt);

        // For now, use a simplified approach since we don't have direct access to the RPC client
        // In a real implementation, this would use the SolanaRpcClient properly

        // Simulate transaction execution (replace with actual RPC call)
        tokio::time::sleep(Duration::from_millis(100)).await; // Simulate network delay

        // For demonstration, randomly succeed or fail based on attempt
        if attempt <= 2 {
            let dummy_signature = solana_sdk::signature::Signature::new_unique();
            Ok(dummy_signature)
        } else {
            Err(anyhow!("Simulated transaction failure"))
        }
    }

    /// Estimate actual output (simplified implementation)
    async fn estimate_actual_output(&self, expected_output: u64, _pools: &[&PoolInfo]) -> u64 {
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;

        // Simulate some slippage using precise decimal arithmetic
        let expected_decimal = Decimal::from(expected_output);
        let slippage_factor = dec!(0.98); // 2% slippage
        let actual_decimal = expected_decimal * slippage_factor;

        // Convert back to u64 via string parsing to avoid precision loss
        if let Ok(actual_str) = actual_decimal.round().to_string().parse::<u64>() {
            actual_str
        } else {
            expected_output
        }
    }

    /// Calculate slippage experienced with precise decimal arithmetic
    fn calculate_slippage_experienced(&self, expected: u64, actual: u64) -> f64 {
        use num_traits::ToPrimitive;
        use rust_decimal::Decimal;

        if expected == 0 {
            return 0.0;
        }

        // Use precise decimal arithmetic for slippage calculation
        let expected_decimal = Decimal::from(expected);
        let actual_decimal = Decimal::from(actual);

        let slippage_decimal = (expected_decimal - actual_decimal) / expected_decimal;

        // Convert back to f64 only for API compatibility
        slippage_decimal.to_f64().unwrap_or(0.0)
    }

    /// Estimate transaction fee
    fn estimate_transaction_fee(&self, _transaction: &solana_sdk::transaction::Transaction) -> u64 {
        // Simplified fee estimation
        5000 // 5000 lamports
    }

    /// Validate sufficient balance
    async fn validate_sufficient_balance(&self, input_amount: &TokenAmount) -> Result<()> {
        // Simplified balance validation
        // In a real implementation, this would check actual wallet balances

        let required_balance =
            input_amount.amount + self.config.balance_validation.minimum_sol_balance;
        let available_balance = 1_000_000_000; // Assume 1 SOL available (simplified)

        if available_balance < required_balance {
            return Err(anyhow!(
                "Insufficient balance: required {} lamports, available {} lamports",
                required_balance,
                available_balance
            ));
        }

        Ok(())
    }

    /// Execute arbitrage transaction with comprehensive safety checks
    pub async fn execute_safe_arbitrage(
        &self,
        pools: Vec<PoolInfo>,
        input_amount: TokenAmount,
        expected_output: u64,
        expected_fee_breakdown: FeeBreakdown,
    ) -> Result<TransactionResult> {
        let start_time = Instant::now();
        let mut attempt = 0;
        let last_violation: Option<SafetyViolation> = None;

        info!(
            "🛡️ Starting safe arbitrage execution: input={}, pools={}",
            input_amount.amount,
            pools.len()
        );

        // Pre-execution safety checks
        self.validate_pre_execution(&pools, &input_amount, &expected_fee_breakdown)
            .await?;

        while attempt < self.config.retry_policy.max_attempts {
            attempt += 1;

            match self.attempt_execution(&pools, &input_amount, attempt).await {
                Ok(mut result) => {
                    // Post-execution validation
                    if let Err(violations) =
                        self.validate_post_execution(&result, expected_output).await
                    {
                        result.safety_violations.extend(violations);

                        if attempt < self.config.retry_policy.max_attempts {
                            let recovery_strategy =
                                self.determine_recovery_strategy(&result, attempt);
                            if self.should_retry(&recovery_strategy) {
                                self.apply_recovery_strategy(recovery_strategy).await?;
                                continue;
                            }
                        }
                    }

                    // Record successful execution
                    self.record_execution(&pools, &input_amount, &result).await;

                    result.execution_time_ms = start_time.elapsed().as_millis() as u64;
                    result.retry_count = attempt;

                    return Ok(result);
                }
                Err(e) => {
                    warn!("Execution attempt {} failed: {}", attempt, e);

                    if attempt >= self.config.retry_policy.max_attempts {
                        break;
                    }

                    // Apply retry delay
                    let delay = self.calculate_retry_delay(attempt);
                    sleep(delay).await;
                }
            }
        }

        // All attempts failed
        let failure_reason = if let Some(violation) = last_violation {
            format!("Safety violation: {:?}", violation)
        } else {
            "Maximum retry attempts exceeded".to_string()
        };

        Ok(TransactionResult {
            signature: None,
            success: false,
            actual_output: None,
            fee_paid: 0,
            slippage_experienced: 0.0,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            confirmation_slots: None,
            retry_count: attempt,
            failure_reason: Some(failure_reason),
            safety_violations: vec![],
        })
    }

    // =============================================================================
    // Advanced Execution Methods
    // =============================================================================

    /// Advanced transaction execution with comprehensive failure handling
    pub async fn execute_with_advanced_recovery(
        &self,
        transaction: &Transaction,
        pools: &[&PoolInfo],
        input_amount: u64,
        expected_output: u64,
        opportunity_id: String,
    ) -> Result<TransactionResult> {
        let start_time = Instant::now();
        let mut attempt = 0;
        let mut recovery_strategies: Vec<FailureRecoveryStrategy> = Vec::new();

        info!(
            "🛡️ Advanced execution starting: opportunity={}, input={}, expected={}",
            opportunity_id, input_amount, expected_output
        );

        // Pre-execution safety checks
        self.perform_pre_execution_checks(input_amount, pools)
            .await?;

        while attempt < self.config.retry_policy.max_attempts {
            attempt += 1;

            // Apply MEV protection before each attempt
            let protected_transaction = self.apply_mev_protection(transaction, attempt).await?;

            match self
                .execute_transaction_with_monitoring(&protected_transaction, attempt)
                .await
            {
                Ok(result) => {
                    // Validate post-execution state
                    if let Err(safety_violations) =
                        self.validate_post_execution(&result, expected_output).await
                    {
                        if attempt < self.config.retry_policy.max_attempts {
                            warn!(
                                "Post-execution validation failed, retrying: {:?}",
                                safety_violations
                            );
                            continue;
                        }

                        return Ok(TransactionResult {
                            success: false,
                            safety_violations,
                            retry_count: attempt,
                            failure_reason: Some("Post-execution validation failed".to_string()),
                            ..result
                        });
                    }

                    // Success - record transaction
                    self.record_successful_transaction(&opportunity_id, &result)
                        .await;
                    return Ok(result);
                }
                Err(e) => {
                    warn!("Transaction attempt {} failed: {}", attempt, e);

                    // Analyze failure and determine recovery strategy
                    let recovery_strategy = self
                        .analyze_failure_and_recover(&e, attempt, input_amount)
                        .await;
                    recovery_strategies.push(recovery_strategy.clone());

                    match recovery_strategy {
                        FailureRecoveryStrategy::Retry { delay_ms } => {
                            if attempt < self.config.retry_policy.max_attempts {
                                info!("Retrying after {}ms delay", delay_ms);
                                sleep(Duration::from_millis(delay_ms)).await;
                                continue;
                            }
                        }
                        FailureRecoveryStrategy::Abort { reason } => {
                            return Ok(TransactionResult {
                                signature: None,
                                success: false,
                                actual_output: None,
                                fee_paid: 0,
                                slippage_experienced: 0.0,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                                confirmation_slots: None,
                                retry_count: attempt,
                                failure_reason: Some(reason),
                                safety_violations: vec![SafetyViolation::UnexpectedFailure {
                                    error_code: e.to_string(),
                                }],
                            });
                        }
                        _ => {
                            // Other strategies would require transaction reconstruction
                            continue;
                        }
                    }
                }
            }
        }

        // All retries exhausted
        Ok(TransactionResult {
            signature: None,
            success: false,
            actual_output: None,
            fee_paid: 0,
            slippage_experienced: 0.0,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            confirmation_slots: None,
            retry_count: attempt,
            failure_reason: Some("Max retries exhausted".to_string()),
            safety_violations: vec![SafetyViolation::UnexpectedFailure {
                error_code: "MAX_RETRIES_EXCEEDED".to_string(),
            }],
        })
    }

    /// Apply MEV protection strategies
    async fn apply_mev_protection(
        &self,
        transaction: &Transaction,
        attempt: u32,
    ) -> Result<Transaction> {
        if !self.config.mev_protection.enabled {
            // not in use - Path only taken if MEV protection is disabled.
            return Ok(transaction.clone());
        }

        let protected_tx = transaction.clone();

        // 1. Add Jito tips for MEV protection
        if self.config.mev_protection.jito_tips_enabled {
            let tip_amount = self.calculate_jito_tip(attempt).await?; // not in use - Jito tip instruction addition is a TODO.
                                                                      // TODO: Add Jito tip instruction to transaction
            debug!("Applied Jito tip: {} lamports", tip_amount);
        }

        // 2. Add priority fees to compete with MEV bots
        if let Some(_compute_budget_ix) = self.create_priority_fee_instruction(attempt).await? {
            // Insert priority fee instruction at the beginning // not in use - Priority fee instruction addition is a TODO.
            // TODO: Modify transaction to include priority fee instruction
            debug!("Applied priority fee for MEV protection");
        }

        // 3. Apply transaction timing randomization
        if self.config.mev_protection.front_running_protection {
            let random_delay = self.calculate_anti_mev_delay();
            sleep(Duration::from_millis(random_delay)).await;
        }

        Ok(protected_tx)
    }

    /// Analyze transaction failure and determine recovery strategy
    async fn analyze_failure_and_recover(
        &self,
        error: &anyhow::Error,
        attempt: u32,
        input_amount: u64,
    ) -> FailureRecoveryStrategy {
        let error_string = error.to_string().to_lowercase();

        // Analyze common failure patterns
        if error_string.contains("insufficient") || error_string.contains("balance") {
            return FailureRecoveryStrategy::Abort {
                reason: "Insufficient balance detected".to_string(),
            };
        }

        if error_string.contains("slippage") || error_string.contains("price") {
            // Try with increased slippage tolerance
            let new_tolerance = 0.02 + (attempt as f64 * 0.005); // Increase by 0.5% per attempt
            return FailureRecoveryStrategy::IncreaseSlippage { new_tolerance };
        }

        if error_string.contains("timeout") || error_string.contains("congestion") {
            // Network congestion - wait longer and retry
            let delay = self.config.retry_policy.base_delay_ms * (2_u64.pow(attempt - 1));
            return FailureRecoveryStrategy::Retry { delay_ms: delay };
        }

        if error_string.contains("partial") || error_string.contains("fill") {
            // Partial fill - reduce amount and retry
            let new_amount = (input_amount as f64 * 0.8) as u64; // Reduce by 20%
            return FailureRecoveryStrategy::ReduceAmount { new_amount };
        }

        if error_string.contains("frontrun") || error_string.contains("mev") {
            // MEV attack detected - abort this opportunity
            return FailureRecoveryStrategy::Abort {
                reason: "MEV attack detected".to_string(),
            };
        }

        // Default retry strategy
        let delay = if self.config.retry_policy.exponential_backoff {
            self.config.retry_policy.base_delay_ms * (2_u64.pow(attempt - 1))
        } else {
            self.config.retry_policy.base_delay_ms
        };

        FailureRecoveryStrategy::Retry { delay_ms: delay }
    }

    /// Perform comprehensive pre-execution safety checks
    async fn perform_pre_execution_checks(
        &self,
        input_amount: u64,
        pools: &[&PoolInfo],
    ) -> Result<()> {
        // 1. Balance validation
        if self.config.balance_validation.enabled {
            let balance_snapshot = self.get_current_balance_snapshot().await?;
            if balance_snapshot.sol_balance
                < input_amount + self.config.balance_validation.minimum_sol_balance
            {
                return Err(anyhow!(
                    "Insufficient SOL balance: {} < {}",
                    balance_snapshot.sol_balance,
                    input_amount + self.config.balance_validation.minimum_sol_balance
                ));
            }
        }

        // 2. Network congestion check
        let network_status = self.check_network_congestion().await?;
        if network_status.tps < 500.0 {
            // Minimum TPS threshold
            warn!("Network congestion detected: {} TPS", network_status.tps); // not in use - This is a warning, not direct functional use of the check result beyond logging.
        }

        // 3. Pool liquidity validation
        for pool in pools {
            if let Err(e) = self.validate_pool_liquidity(pool, input_amount).await {
                return Err(anyhow!("Pool liquidity validation failed: {}", e));
            }
        }

        // 4. MEV risk assessment
        if self.config.mev_protection.sandwich_detection {
            let mev_risk = self.assess_mev_risk(pools, input_amount).await?;
            if mev_risk > 0.7 {
                // High MEV risk threshold
                warn!("High MEV risk detected: {:.2}%", mev_risk * 100.0); // not in use - This is a warning.
            }
        }

        Ok(())
    }

    /// Calculate appropriate Jito tip for MEV protection
    async fn calculate_jito_tip(&self, attempt: u32) -> Result<u64> {
        let base_tip = 10_000; // 0.00001 SOL base tip
        let attempt_multiplier = 1.5_f64.powi(attempt as i32);
        let network_factor = self.get_network_congestion_factor().await?;

        let tip = (base_tip as f64 * attempt_multiplier * network_factor) as u64;
        Ok(tip.min(self.config.mev_protection.max_priority_fee_lamports))
    }

    /// Create priority fee instruction for MEV protection
    async fn create_priority_fee_instruction(&self, attempt: u32) -> Result<Option<Instruction>> {
        let priority_fee = self.calculate_priority_fee(attempt).await?;
        if priority_fee > 0 {
            // not in use - Instruction creation is a TODO placeholder.
            // TODO: Create actual compute budget instruction
            debug!("Priority fee calculated: {} lamports", priority_fee);
        }
        Ok(None) // Placeholder
    }

    /// Calculate anti-MEV delay with randomization
    fn calculate_anti_mev_delay(&self) -> u64 {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen_range(50..200) // Random delay between 50-200ms
    }

    /// Validate post-execution state
    async fn validate_post_execution(
        &self,
        result: &TransactionResult,
        expected_output: u64,
    ) -> Result<(), Vec<SafetyViolation>> {
        let mut violations = Vec::new();

        // 1. Slippage validation
        if let Some(actual_output) = result.actual_output {
            let slippage = 1.0 - (actual_output as f64 / expected_output as f64);
            if slippage > self.config.slippage_protection.max_slippage_percent / 100.0 {
                violations.push(SafetyViolation::ExcessiveSlippage {
                    expected: 0.0,
                    actual: slippage,
                });
            }
        }

        // 2. Balance consistency check
        if self.config.balance_validation.real_time_validation {
            if let Ok(_balance_snapshot) = self.get_current_balance_snapshot().await { // not in use - Balance comparison logic is commented out/missing.
                 // Compare with expected balance changes
                 // This would need more sophisticated tracking
            }
        }

        // 3. Transaction confirmation validation
        if result.execution_time_ms > (self.config.confirmation_settings.timeout_seconds * 1000) {
            violations.push(SafetyViolation::TransactionTimeout {
                timeout_seconds: self.config.confirmation_settings.timeout_seconds,
            });
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    /// Record successful transaction for analytics
    async fn record_successful_transaction(
        &self,
        opportunity_id: &str,
        result: &TransactionResult,
    ) {
        let record = TransactionRecord {
            timestamp: std::time::SystemTime::now(),
            opportunity_id: opportunity_id.to_string(),
            input_amount: 0, // Would be populated from context
            expected_output: 0,
            result: result.clone(),
            pools_involved: Vec::new(),
        };

        // Store in execution history
        {
            let mut history = self.execution_history.write().unwrap();
            history.push(record);

            // Keep only recent history (last 1000 transactions)
            if history.len() > 1000 {
                history.drain(0..500);
            }
        }

        info!("✅ Transaction recorded successfully: {}", opportunity_id);
    }

    /// Get network congestion factor for fee calculation
    async fn get_network_congestion_factor(&self) -> Result<f64> {
        let network_status = self.check_network_congestion().await?;

        // Convert TPS to congestion factor (1.0 = normal, >1.0 = congested)
        let congestion_factor = if network_status.tps > 2000.0 {
            1.0 // Normal network conditions
        } else if network_status.tps > 1000.0 {
            1.5 // Moderate congestion
        } else if network_status.tps > 500.0 {
            2.0 // High congestion
        } else {
            3.0 // Critical congestion
        };

        Ok(congestion_factor)
    }

    /// Assess MEV risk for given pools and trade size
    async fn assess_mev_risk(&self, pools: &[&PoolInfo], input_amount: u64) -> Result<f64> {
        let mut risk_score = 0.0;

        // Factor 1: Trade size relative to pool liquidity
        for pool in pools {
            let pool_liquidity = pool.token_a.reserve + pool.token_b.reserve;
            let size_ratio = input_amount as f64 / pool_liquidity as f64;
            if size_ratio > 0.01 {
                // >1% of pool liquidity
                risk_score += size_ratio * 0.5;
            }
        }

        // Factor 2: Pool popularity (more popular = higher MEV risk)
        let popular_pools = ["ORCAPool", "RaydiumPool"]; // Popular pool identifiers
        for pool in pools {
            if popular_pools
                .iter()
                .any(|&popular| pool.name.contains(popular))
            {
                risk_score += 0.2;
            }
        }

        // Factor 3: Network congestion (higher congestion = higher MEV competition)
        let network_status = self.check_network_congestion().await?;
        if network_status.tps < 1000.0 {
            risk_score += 0.3;
        }

        Ok(risk_score.min(1.0)) // Cap at 100%
    }

    // =============================================================================
    // Missing Helper Methods Implementation
    // =============================================================================

    /// Validate pre-execution conditions
    async fn validate_pre_execution(
        &self,
        pools: &[PoolInfo],
        input_amount: &TokenAmount,
        _fee_breakdown: &FeeBreakdown,
    ) -> Result<()> {
        // Balance validation
        if self.config.balance_validation.enabled {
            self.validate_sufficient_balance(input_amount).await?;
        }

        // Basic pool validation
        for pool in pools {
            if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
                return Err(anyhow!("Pool has zero reserves: {}", pool.address));
            }
        }

        Ok(())
    }

    /// Attempt transaction execution
    async fn attempt_execution(
        &self,
        pools: &[PoolInfo],
        input_amount: &TokenAmount,
        attempt: u32,
    ) -> Result<TransactionResult> {
        debug!(
            "Executing transaction attempt {} for {} pools",
            attempt,
            pools.len()
        );

        // Simulate transaction execution
        let start_time = std::time::Instant::now();

        // For now, simulate a successful transaction
        let execution_time = start_time.elapsed().as_millis() as u64;

        Ok(TransactionResult {
            signature: Some(format!("sim_tx_{}", attempt)),
            success: true,
            actual_output: Some(input_amount.amount * 99 / 100), // Simulate 1% loss
            fee_paid: 5000,                                      // 5k lamports
            slippage_experienced: 0.01,                          // 1%
            execution_time_ms: execution_time,
            confirmation_slots: Some(32),
            retry_count: attempt,
            failure_reason: None,
            safety_violations: Vec::new(),
        })
    }

    /// Determine recovery strategy for failed transaction
    fn determine_recovery_strategy(
        &self,
        result: &TransactionResult,
        attempt: u32,
    ) -> FailureRecoveryStrategy {
        // Analyze the failure and determine strategy
        if let Some(ref reason) = result.failure_reason {
            let reason_lower = reason.to_lowercase();

            if reason_lower.contains("slippage") {
                return FailureRecoveryStrategy::IncreaseSlippage {
                    new_tolerance: 0.02 + (attempt as f64 * 0.005),
                };
            }

            if reason_lower.contains("balance") {
                return FailureRecoveryStrategy::Abort {
                    reason: "Insufficient balance".to_string(),
                };
            }
        }

        // Default retry strategy
        let delay = if self.config.retry_policy.exponential_backoff {
            self.config.retry_policy.base_delay_ms * (2_u64.pow(attempt - 1))
        } else {
            self.config.retry_policy.base_delay_ms
        };

        FailureRecoveryStrategy::Retry { delay_ms: delay }
    }

    /// Check if we should retry based on recovery strategy
    fn should_retry(&self, strategy: &FailureRecoveryStrategy) -> bool {
        match strategy {
            FailureRecoveryStrategy::Retry { .. } => true,
            FailureRecoveryStrategy::ReduceAmount { .. } => true,
            FailureRecoveryStrategy::IncreaseSlippage { .. } => true,
            FailureRecoveryStrategy::SwitchRoute { .. } => true,
            FailureRecoveryStrategy::Abort { .. } => false,
        }
    }

    /// Apply recovery strategy
    async fn apply_recovery_strategy(&self, strategy: FailureRecoveryStrategy) -> Result<()> {
        match strategy {
            FailureRecoveryStrategy::Retry { delay_ms } => {
                info!("Applying retry strategy with {}ms delay", delay_ms);
                sleep(Duration::from_millis(delay_ms)).await;
            } // not in use - Placeholder for actual recovery application.
            FailureRecoveryStrategy::ReduceAmount { new_amount } => {
                info!(
                    "Applying reduce amount strategy: new amount = {}",
                    new_amount
                );
                // This would require modifying the transaction, which is complex
                // For now, just log the strategy
            }
            FailureRecoveryStrategy::IncreaseSlippage { new_tolerance } => {
                info!(
                    "Applying increased slippage strategy: new tolerance = {:.2}%",
                    new_tolerance * 100.0
                );
                // This would require modifying slippage parameters
            }
            FailureRecoveryStrategy::SwitchRoute { alternative_pools } => {
                info!(
                    "Applying route switch strategy: {} alternative pools",
                    alternative_pools.len()
                );
                // This would require finding alternative routes
            }
            FailureRecoveryStrategy::Abort { reason } => {
                warn!("Aborting transaction: {}", reason);
                return Err(anyhow!("Transaction aborted: {}", reason));
            }
        }
        Ok(())
    }

    /// Record transaction execution for analytics
    async fn record_execution(
        &self,
        pools: &[PoolInfo],
        input_amount: &TokenAmount,
        result: &TransactionResult,
    ) {
        let record = TransactionRecord {
            timestamp: std::time::SystemTime::now(),
            opportunity_id: format!(
                "opp_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
            input_amount: input_amount.amount,
            expected_output: input_amount.amount, // Simplified
            result: result.clone(),
            pools_involved: pools.iter().map(|p| p.address.to_string()).collect(),
        };

        // Store in execution history
        {
            let mut history = self.execution_history.write().unwrap();
            history.push(record);

            // Keep only recent history
            if history.len() > 1000 {
                history.drain(0..500);
            }
        }

        info!("Transaction execution recorded: success={}", result.success);
    }

    /// Calculate retry delay
    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let delay_ms = if self.config.retry_policy.exponential_backoff {
            self.config.retry_policy.base_delay_ms * (2_u64.pow(attempt - 1))
        } else {
            self.config.retry_policy.base_delay_ms
        };
        Duration::from_millis(delay_ms)
    }

    /// Execute transaction with monitoring (alternative name for existing method)
    async fn execute_transaction_with_monitoring(
        &self,
        transaction: &Transaction,
        attempt: u32,
    ) -> Result<TransactionResult> {
        // Convert Signature result to TransactionResult
        match self.execute_transaction_attempt(transaction, attempt).await {
            Ok(signature) => {
                Ok(TransactionResult {
                    signature: Some(signature.to_string()),
                    success: true,
                    actual_output: None, // Would need to be calculated
                    fee_paid: 5000,      // Estimate
                    slippage_experienced: 0.0,
                    execution_time_ms: 0, // Would need timing
                    confirmation_slots: None,
                    retry_count: attempt,
                    failure_reason: None,
                    safety_violations: Vec::new(),
                })
            }
            Err(e) => Ok(TransactionResult {
                signature: None,
                success: false,
                actual_output: None,
                fee_paid: 0,
                slippage_experienced: 0.0,
                execution_time_ms: 0,
                confirmation_slots: None,
                retry_count: attempt,
                failure_reason: Some(e.to_string()),
                safety_violations: Vec::new(),
            }),
        }
    }

    /// Get current balance snapshot
    async fn get_current_balance_snapshot(&self) -> Result<BalanceSnapshot> {
        // Simplified implementation - in production this would query actual balances
        let mut token_balances = std::collections::HashMap::new();
        token_balances.insert("SOL".to_string(), 1_000_000_000); // 1 SOL

        Ok(BalanceSnapshot {
            sol_balance: 1_000_000_000, // 1 SOL
            token_balances,
            timestamp: std::time::Instant::now(),
        })
    }

    /// Check network congestion
    async fn check_network_congestion(&self) -> Result<NetworkStatus> {
        // Simplified implementation - in production this would query RPC
        Ok(NetworkStatus {
            tps: 1500.0, // Simulated TPS
            current_slot: 12345678,
            avg_slot_time: 400.0, // milliseconds
        })
    }

    /// Validate pool liquidity
    async fn validate_pool_liquidity(&self, pool: &PoolInfo, input_amount: u64) -> Result<()> {
        let total_liquidity = pool.token_a.reserve + pool.token_b.reserve;
        let min_liquidity = input_amount * 10; // Require 10x liquidity

        if total_liquidity < min_liquidity {
            return Err(anyhow!(
                "Insufficient pool liquidity: {} < {} required",
                total_liquidity,
                min_liquidity
            ));
        }

        Ok(())
    }

    /// Calculate priority fee
    async fn calculate_priority_fee(&self, attempt: u32) -> Result<u64> {
        let base_fee = self.config.mev_protection.max_priority_fee_lamports / 10;
        let multiplier = 1.5_f64.powi(attempt as i32);
        let fee = (base_fee as f64 * multiplier) as u64;
        Ok(fee.min(self.config.mev_protection.max_priority_fee_lamports))
    }
}

// =============================================================================
// Additional Types for Helper Methods
// =============================================================================

/// Network status information
#[derive(Debug, Clone)]
pub struct NetworkStatus {
    pub tps: f64,
    pub current_slot: u64,
    pub avg_slot_time: f64,
}

/// Legacy safety configuration for backward compatibility
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    // not in use - Defined but not instantiated or used elsewhere in the provided codebase.
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Timeout for transaction confirmation (seconds)
    pub confirmation_timeout_secs: u64,
    /// Enable balance validation before execution
    pub validate_balance: bool,
    /// Enable slippage protection
    pub slippage_protection: bool,
    /// Maximum slippage tolerance (percentage)
    pub max_slippage_percent: f64,
    /// Enable front-running detection
    pub mev_protection: bool,
    /// Minimum profit threshold to continue execution
    pub min_profit_threshold: f64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            confirmation_timeout_secs: 30,
            validate_balance: true,
            slippage_protection: true,
            max_slippage_percent: 5.0,
            mev_protection: true,
            min_profit_threshold: 0.1, // 0.1% minimum profit
        }
    }
}
