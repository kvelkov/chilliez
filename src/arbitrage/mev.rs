// src/arbitrage/mev.rs
//! MEV Protection and Jito Integration Module
//!
//!
//! This module consolidates MEV protection strategies and Jito bundle submission
//! functionality into a unified interface for atomic transaction execution.

use crate::{arbitrage::opportunity::MultiHopArbOpportunity, error::ArbError};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash as SolanaHash, // Alias to avoid conflict if Hash is defined elsewhere
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Mutex, time::timeout};

// ========================================================================================
// Configuration Structures
// ========================================================================================

#[derive(Debug, Clone)]
pub struct MevProtectionConfig {
    pub enable_private_mempool: bool,
    pub max_priority_fee_lamports: u64,
    pub dynamic_fee_adjustment: bool,
    pub bundle_transactions: bool,
    pub randomize_execution_timing: bool,
    pub use_flashloan_protection: bool,
    pub max_slippage_protection: f64,
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            enable_private_mempool: true,
            max_priority_fee_lamports: 100_000,
            dynamic_fee_adjustment: true,
            bundle_transactions: true,
            randomize_execution_timing: true,
            use_flashloan_protection: true,
            max_slippage_protection: 0.02, // 2% max slippage
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoConfig {
    /// Jito block engine URL
    pub block_engine_url: String,
    /// Authentication keypair path
    pub auth_keypair_path: Option<String>,
    /// Default tip amount in lamports
    pub default_tip_lamports: u64,
    /// Bundle submission timeout
    pub submission_timeout_ms: u64,
    /// Maximum retries for bundle submission
    pub max_retries: u32,
    /// Tip accounts for different regions
    pub tip_accounts: HashMap<String, Pubkey>,
}

impl Default for JitoConfig {
    fn default() -> Self {
        let mut tip_accounts = HashMap::new();

        // Jito tip accounts (these are real Jito tip accounts)
        tip_accounts.insert(
            "ny".to_string(),
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"
                .parse()
                .unwrap(),
        );
        tip_accounts.insert(
            "ams".to_string(),
            "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"
                .parse()
                .unwrap(),
        );
        tip_accounts.insert(
            "fra".to_string(),
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"
                .parse()
                .unwrap(),
        );
        tip_accounts.insert(
            "tok".to_string(),
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"
                .parse()
                .unwrap(),
        );

        Self {
            block_engine_url: "https://mainnet.block-engine.jito.wtf".to_string(),
            auth_keypair_path: None,
            default_tip_lamports: 10_000,
            submission_timeout_ms: 5_000,
            max_retries: 3,
            tip_accounts,
        }
    }
}

// ========================================================================================
// Metrics and Status Structures
// ========================================================================================

#[derive(Debug, Clone)]
pub struct GasOptimizationMetrics {
    pub average_priority_fee: u64,
    pub successful_transactions: u64,
    pub failed_transactions: u64,
    pub total_gas_saved: u64,
    pub mev_protection_activations: u64,
    pub bundle_success_rate: f64,
}

impl Default for GasOptimizationMetrics {
    fn default() -> Self {
        Self {
            average_priority_fee: 0,
            successful_transactions: 0,
            failed_transactions: 0,
            total_gas_saved: 0,
            mev_protection_activations: 0,
            bundle_success_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkConditions {
    pub current_congestion_level: f64, // 0.0 to 1.0
    pub average_priority_fee: u64,
    pub recent_block_times: Vec<Duration>,
    pub mempool_size_estimate: usize,
    pub last_update: Instant,
}

impl NetworkConditions {
    pub fn new() -> Self {
        Self {
            current_congestion_level: 0.5,
            average_priority_fee: 10_000,
            recent_block_times: Vec::new(), // not in use - Populated in update_network_conditions, but not read by other JitoHandler methods.
            mempool_size_estimate: 1000,
            last_update: Instant::now(),
        }
    }
}

impl Default for NetworkConditions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct MevProtectionStrategy {
    pub use_private_mempool: bool,
    pub priority_fee_multiplier: f64,
    pub bundle_with_other_txs: bool,
    pub add_timing_randomization: bool,
    pub use_flashloan_protection: bool,
    pub max_slippage_tolerance: f64,
    pub recommended_delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct MevProtectionStatus {
    pub is_active: bool,
    pub current_congestion_level: f64,
    pub average_priority_fee: u64,
    pub successful_transactions: u64,
    pub failed_transactions: u64,
    pub total_gas_saved: u64,
    pub bundle_success_rate: f64,
    pub mev_protection_activations: u64,
    pub last_network_update: Instant,
}

// ========================================================================================
// Jito API Structures
// ========================================================================================

#[derive(Debug, Clone, Serialize)]
pub struct BundleSubmissionRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: BundleParams,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleParams {
    pub bundle: Vec<String>, // Base64 encoded transactions
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleSubmissionResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<String>, // Bundle UUID
    pub error: Option<JitoError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JitoError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleStatus {
    pub bundle_id: String,
    pub status: String,
    pub landed_slot: Option<u64>,
    pub transactions: Vec<BundleTransaction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleTransaction {
    pub signature: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JitoBundleResult {
    pub bundle_id: String,
    pub success: bool,
    pub signatures: Vec<Signature>,
    pub landed_slot: Option<u64>,
    pub execution_time_ms: u64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct BundleConfirmationResult {
    success: bool,
    signatures: Vec<Signature>,
    landed_slot: Option<u64>,
    error_message: Option<String>,
}

// ========================================================================================
// Main MEV Handler Implementation
// ========================================================================================

/// Unified MEV Protection and Jito Integration Handler
pub struct JitoHandler {
    mev_config: MevProtectionConfig,
    jito_config: JitoConfig,
    metrics: Arc<Mutex<GasOptimizationMetrics>>,
    network_conditions: Arc<Mutex<NetworkConditions>>,
    _transaction_bundles: Arc<Mutex<HashMap<String, Vec<Transaction>>>>,
    priority_fee_history: Arc<Mutex<Vec<(Instant, u64)>>>,
    http_client: reqwest::Client,
    request_id_counter: std::sync::atomic::AtomicU64,
    rpc_client: Arc<NonBlockingRpcClient>, // For fetching recent blockhash
}

impl JitoHandler {
    /// Create a new JitoHandler with MEV protection
    pub fn new(
        mev_config: MevProtectionConfig,
        jito_config: JitoConfig,
        rpc_url: String, // URL for the RPC client
    ) -> Self {
        info!("🛡️ Initializing MEV Protection & Jito Handler");
        info!(
            "   🔒 Private mempool: {}",
            mev_config.enable_private_mempool
        );
        // ... (other info logs)

        info!(
            "   💰 Dynamic fee adjustment: {}",
            mev_config.dynamic_fee_adjustment
        );
        info!(
            "   📦 Transaction bundling: {}",
            mev_config.bundle_transactions
        );
        info!(
            "   🎲 Randomized timing: {}",
            mev_config.randomize_execution_timing
        );
        info!(
            "   ⚡ Flash loan protection: {}",
            mev_config.use_flashloan_protection
        );
        info!("   🌐 Jito block engine: {}", jito_config.block_engine_url);
        info!(
            "   💰 Default tip: {} lamports",
            jito_config.default_tip_lamports
        );

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(jito_config.submission_timeout_ms))
            .build()
            .expect("Failed to create HTTP client");

        let rpc_client = Arc::new(NonBlockingRpcClient::new_with_commitment(
            rpc_url,
            solana_sdk::commitment_config::CommitmentConfig::confirmed(),
        ));

        Self {
            mev_config,
            jito_config,
            metrics: Arc::new(Mutex::new(GasOptimizationMetrics::default())),
            network_conditions: Arc::new(Mutex::new(NetworkConditions::default())),
            _transaction_bundles: Arc::new(Mutex::new(HashMap::new())),
            priority_fee_history: Arc::new(Mutex::new(Vec::new())),
            http_client,
            request_id_counter: std::sync::atomic::AtomicU64::new(1),
            rpc_client,
        }
    }

    // ========================================================================================
    // MEV Protection Methods
    // ========================================================================================

    /// Calculate optimal priority fee based on network conditions and opportunity value
    pub async fn calculate_optimal_priority_fee(
        &self,
        opportunity: &MultiHopArbOpportunity,
        base_fee: u64,
    ) -> Result<u64, ArbError> {
        let network_conditions = self.network_conditions.lock().await;
        let profit_usd = opportunity.estimated_profit_usd.unwrap_or(0.0);

        // Base calculation: higher profit = willing to pay more for priority
        let profit_based_fee = if profit_usd > 100.0 {
            base_fee * 3 // High profit opportunities get 3x priority
        } else if profit_usd > 50.0 {
            base_fee * 2 // Medium profit opportunities get 2x priority
        } else {
            base_fee // Low profit opportunities use base fee
        };

        // Adjust for network congestion
        let congestion_multiplier = 1.0 + network_conditions.current_congestion_level;
        let congestion_adjusted_fee = (profit_based_fee as f64 * congestion_multiplier) as u64;

        // Apply dynamic adjustment based on recent success rates
        let dynamic_fee = if self.mev_config.dynamic_fee_adjustment {
            self.apply_dynamic_fee_adjustment(congestion_adjusted_fee)
                .await
        } else {
            congestion_adjusted_fee
        };

        // Cap at maximum allowed fee
        let final_fee = dynamic_fee.min(self.mev_config.max_priority_fee_lamports);

        debug!("💰 Priority fee calculation:");
        debug!("   Base fee: {} lamports", base_fee);
        debug!("   Profit-based fee: {} lamports", profit_based_fee);
        debug!("   Congestion multiplier: {:.2}x", congestion_multiplier);
        debug!("   Dynamic adjusted fee: {} lamports", dynamic_fee);
        debug!("   Final fee (capped): {} lamports", final_fee);

        Ok(final_fee)
    }

    /// Apply dynamic fee adjustment based on recent transaction success rates
    async fn apply_dynamic_fee_adjustment(&self, base_fee: u64) -> u64 {
        let metrics = self.metrics.lock().await;
        let total_transactions = metrics.successful_transactions + metrics.failed_transactions;

        if total_transactions == 0 {
            return base_fee;
        }

        let success_rate = metrics.successful_transactions as f64 / total_transactions as f64;

        // If success rate is low, increase fees to improve priority
        let adjustment_factor = if success_rate < 0.7 {
            1.5 // Increase fee by 50% if success rate is below 70%
        } else if success_rate < 0.8 {
            1.2 // Increase fee by 20% if success rate is below 80%
        } else {
            1.0 // Keep fee as is if success rate is good
        };

        (base_fee as f64 * adjustment_factor) as u64
    }

    /// Analyze MEV risk for a given opportunity
    pub async fn analyze_mev_risk(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<f64, ArbError> {
        let _network_conditions = self.network_conditions.lock().await;

        // Base risk factors
        let mut risk_score: f64 = 0.0;

        // Higher profit opportunities are more attractive to MEV bots
        if let Some(profit_usd) = opportunity.estimated_profit_usd {
            if profit_usd > 1000.0 {
                risk_score += 0.8; // Very high MEV risk
            } else if profit_usd > 500.0 {
                risk_score += 0.6; // High MEV risk
            } else if profit_usd > 100.0 {
                risk_score += 0.4; // Medium MEV risk
            } else {
                risk_score += 0.2; // Low MEV risk
            }
        }

        // Network congestion increases MEV risk
        risk_score += _network_conditions.current_congestion_level * 0.3;

        // Multi-hop opportunities are more complex and harder to front-run
        let hop_count = opportunity.hops.len();
        if hop_count > 3 {
            risk_score -= 0.2; // Lower risk for complex paths
        } else if hop_count == 2 {
            risk_score += 0.1; // Higher risk for simple paths
        }

        // Cross-DEX arbitrage is harder to front-run
        let unique_dexs: std::collections::HashSet<_> = opportunity.dex_path.iter().collect();
        if unique_dexs.len() > 1 {
            risk_score -= 0.15; // Lower risk for cross-DEX
        }

        // Clamp risk score between 0.0 and 1.0
        let final_risk_score = risk_score.clamp(0.0, 1.0);

        debug!("🎯 MEV risk analysis for opportunity {}:", opportunity.id);
        debug!(
            "   Profit-based risk: {:.2}",
            opportunity.estimated_profit_usd.unwrap_or(0.0)
        );
        debug!(
            "   Network congestion factor: {:.2}",
            _network_conditions.current_congestion_level
        );
        debug!("   Path complexity factor: {} hops", hop_count);
        debug!("   Cross-DEX factor: {} unique DEXs", unique_dexs.len());
        debug!("   Final MEV risk score: {:.2}", final_risk_score);

        Ok(final_risk_score)
    }

    /// Recommend MEV protection strategy for an opportunity
    pub async fn recommend_protection_strategy(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<MevProtectionStrategy, ArbError> {
        let mev_risk = self.analyze_mev_risk(opportunity).await?;
        let _network_conditions = self.network_conditions.lock().await;

        let strategy = if mev_risk > 0.8 {
            MevProtectionStrategy {
                use_private_mempool: true,
                priority_fee_multiplier: 3.0,
                bundle_with_other_txs: true,
                add_timing_randomization: true,
                use_flashloan_protection: true,
                max_slippage_tolerance: 0.01, // 1% for high-risk
                recommended_delay_ms: 0,      // Execute immediately for high-profit
            }
        } else if mev_risk > 0.5 {
            MevProtectionStrategy {
                use_private_mempool: true,
                priority_fee_multiplier: 2.0,
                bundle_with_other_txs: true,
                add_timing_randomization: false,
                use_flashloan_protection: true,
                max_slippage_tolerance: 0.015, // 1.5% for medium-risk
                recommended_delay_ms: 100,     // Small delay
            }
        } else {
            MevProtectionStrategy {
                use_private_mempool: false,
                priority_fee_multiplier: 1.0,
                bundle_with_other_txs: false,
                add_timing_randomization: false,
                use_flashloan_protection: false,
                max_slippage_tolerance: 0.02, // 2% for low-risk
                recommended_delay_ms: 200,    // Longer delay acceptable
            }
        };

        info!(
            "🛡️ MEV protection strategy for opportunity {} (risk: {:.2}):",
            opportunity.id, mev_risk
        );
        info!("   Private mempool: {}", strategy.use_private_mempool);
        info!(
            "   Priority fee multiplier: {:.1}x",
            strategy.priority_fee_multiplier
        );
        info!("   Bundle transactions: {}", strategy.bundle_with_other_txs);
        info!(
            "   Timing randomization: {}",
            strategy.add_timing_randomization
        );
        info!(
            "   Flash loan protection: {}",
            strategy.use_flashloan_protection
        );
        info!(
            "   Max slippage: {:.1}%",
            strategy.max_slippage_tolerance * 100.0
        );
        info!("   Recommended delay: {}ms", strategy.recommended_delay_ms);

        Ok(strategy)
    }

    // ========================================================================================
    // Jito Bundle Methods
    // ========================================================================================

    /// Submit a bundle to Jito with MEV protection
    pub async fn submit_protected_bundle(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
        instructions_per_opportunity: Vec<Vec<Instruction>>,
        payer_signer: Arc<Keypair>, // The wallet that pays for and signs the arbitrage transactions
        tip_lamports: Option<u64>,
    ) -> Result<JitoBundleResult, ArbError> {
        let tip_amount = tip_lamports.unwrap_or(self.jito_config.default_tip_lamports);

        info!(
            "📦 Submitting MEV-protected bundle with {} opportunities, tip: {} lamports",
            opportunities.len(),
            tip_amount
        );

        // Create MEV-protected bundle
        let bundle_transactions = self
            .create_mev_protected_bundle(opportunities, instructions_per_opportunity, payer_signer)
            .await?;

        // Submit bundle to Jito
        self.submit_bundle(bundle_transactions, Some(tip_amount))
            .await
    }

    /// Create MEV-protected transaction bundle
    pub async fn create_mev_protected_bundle(
        &self, // not in use - This function is public but its current implementation for flashloan_protection and timing_randomization are placeholders.
        opportunities: Vec<MultiHopArbOpportunity>,
        instructions_per_opportunity: Vec<Vec<Instruction>>,
        payer_signer: Arc<Keypair>, // The wallet that pays for and signs the arbitrage transactions
    ) -> Result<Vec<Transaction>, ArbError> {
        if !self.mev_config.bundle_transactions {
            return Err(ArbError::ConfigError(
                "Transaction bundling is disabled".to_string(),
            )); // not in use - Path only taken if bundling is disabled.
        }

        info!(
            "📦 Creating MEV-protected transaction bundle for {} opportunities",
            opportunities.len()
        );

        let mut bundle_transactions = Vec::new();
        let mut total_compute_units = 0u32;
        const MAX_COMPUTE_UNITS_PER_TX: u32 = 1_400_000;
        let recent_blockhash = self.get_recent_blockhash().await?;

        for (i, (opportunity, instructions)) in opportunities
            .iter()
            .zip(instructions_per_opportunity.iter())
            .enumerate()
        {
            // Calculate compute units needed for this opportunity
            let estimated_cu = self.estimate_compute_units(&instructions).await;

            if total_compute_units + estimated_cu > MAX_COMPUTE_UNITS_PER_TX {
                warn!("⚠️ Opportunity {} would exceed compute unit limit, creating separate transaction", i); // not in use - Path only taken if CU limit exceeded for a single opportunity.
                continue;
            }

            // Calculate optimal priority fee for this opportunity
            let base_fee = 10_000; // Base priority fee
            let optimal_fee = self
                .calculate_optimal_priority_fee(opportunity, base_fee)
                .await?;

            // Add compute budget instructions
            let mut tx_instructions = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(estimated_cu),
                ComputeBudgetInstruction::set_compute_unit_price(optimal_fee / estimated_cu as u64),
            ];

            // Add MEV protection instructions
            if self.mev_config.use_flashloan_protection {
                let protection_instructions =
                    self.create_flashloan_protection_instructions().await?;
                tx_instructions.extend(protection_instructions);
            }

            // Add the actual arbitrage instructions
            tx_instructions.extend(instructions.clone());

            // Add anti-MEV randomization
            if self.mev_config.randomize_execution_timing {
                let randomization_instructions =
                    self.create_timing_randomization_instructions().await?;
                tx_instructions.extend(randomization_instructions);
            }

            let mut transaction =
                Transaction::new_with_payer(&tx_instructions, Some(&payer_signer.pubkey()));

            // Sign the transaction
            transaction.sign(&[&payer_signer], recent_blockhash);

            bundle_transactions.push(transaction);
            total_compute_units += estimated_cu;

            info!(
                "📦 Added opportunity {} to bundle (CU: {}, Fee: {} lamports)",
                opportunity.id, estimated_cu, optimal_fee
            );
        }

        info!(
            "✅ MEV-protected bundle created with {} compute units total",
            total_compute_units
        );

        // Update metrics
        {
            let mut metrics = self.metrics.lock().await;
            metrics.mev_protection_activations += 1;
        }

        Ok(bundle_transactions)
    }

    /// Submit a bundle to Jito
    pub async fn submit_bundle(
        &self,
        transactions: Vec<Transaction>,
        tip_lamports: Option<u64>,
    ) -> Result<JitoBundleResult, ArbError> {
        let start_time = Instant::now();
        let tip_amount = tip_lamports.unwrap_or(self.jito_config.default_tip_lamports);

        info!(
            "📦 Submitting bundle with {} transactions, tip: {} lamports",
            transactions.len(),
            tip_amount
        );

        // Validate bundle
        self.validate_bundle(&transactions)?;

        // Create tip transaction
        let tip_payer_keypair = self
            .jito_config
            .auth_keypair_path
            .as_ref()
            .ok_or_else(|| {
                ArbError::ConfigError("Jito auth_keypair_path not configured for tip".to_string())
            })
            .and_then(|path| {
                solana_sdk::signer::keypair::read_keypair_file(path).map_err(|e| {
                    ArbError::ConfigError(format!("Failed to read Jito auth keypair: {}", e))
                })
            })?;
        let tip_transaction = self
            .create_tip_transaction(tip_amount, Arc::new(tip_payer_keypair))
            .await?;

        // Combine transactions with tip
        let mut bundle_transactions = transactions;
        bundle_transactions.push(tip_transaction);

        // Encode transactions to base64
        let encoded_transactions = self.encode_transactions(&bundle_transactions)?;

        // Submit bundle with retries
        let mut last_error = None;

        for attempt in 1..=self.jito_config.max_retries {
            debug!(
                "📤 Bundle submission attempt {}/{}",
                attempt, self.jito_config.max_retries
            );

            match self.submit_bundle_request(&encoded_transactions).await {
                Ok(bundle_id) => {
                    info!("✅ Bundle submitted successfully: {}", bundle_id);

                    // Wait for bundle confirmation
                    let result = self.wait_for_bundle_confirmation(&bundle_id).await?;

                    let execution_time = start_time.elapsed();
                    return Ok(JitoBundleResult {
                        bundle_id,
                        success: result.success,
                        signatures: result.signatures,
                        landed_slot: result.landed_slot,
                        execution_time_ms: execution_time.as_millis() as u64,
                        error_message: result.error_message,
                    });
                }
                Err(e) => {
                    warn!("❌ Bundle submission attempt {} failed: {}", attempt, e);
                    last_error = Some(e);

                    if attempt < self.jito_config.max_retries {
                        // Exponential backoff
                        let delay = Duration::from_millis(100 * (1 << attempt));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| ArbError::ExecutionError("Bundle submission failed".to_string())))
    }

    // ========================================================================================
    // Utility Methods
    // ========================================================================================

    /// Validate bundle before submission
    fn validate_bundle(&self, transactions: &[Transaction]) -> Result<(), ArbError> {
        if transactions.is_empty() {
            return Err(ArbError::ExecutionError(
                "Bundle cannot be empty".to_string(),
            ));
        }

        if transactions.len() > 5 {
            return Err(ArbError::ExecutionError(
                "Bundle too large (max 5 transactions)".to_string(),
            ));
        }

        // Validate each transaction
        for (idx, tx) in transactions.iter().enumerate() {
            if tx.message.instructions.is_empty() {
                return Err(ArbError::ExecutionError(format!(
                    "Transaction {} has no instructions",
                    idx
                )));
            }

            // Check for compute budget instructions
            let has_compute_budget = tx.message.instructions.iter().any(|ix| {
                if let Some(program_id) = tx.message.account_keys.get(ix.program_id_index as usize)
                {
                    *program_id == solana_sdk::compute_budget::id()
                } else {
                    false
                }
            });

            if !has_compute_budget {
                warn!("⚠️ Transaction {} missing compute budget instructions", idx);
            }
        }

        debug!(
            "✅ Bundle validation passed for {} transactions",
            transactions.len()
        );
        Ok(())
    }

    /// Create a tip transaction
    async fn create_tip_transaction(
        &self,
        tip_lamports: u64,
        tip_payer_signer: Arc<Keypair>,
    ) -> Result<Transaction, ArbError> {
        debug!("💰 Creating tip transaction: {} lamports", tip_lamports);

        let tip_account = self
            .jito_config
            .tip_accounts
            .get("ny")
            .or_else(|| self.jito_config.tip_accounts.values().next())
            .ok_or_else(|| ArbError::ConfigError("No tip accounts configured".to_string()))?;

        let payer = tip_payer_signer;

        // Add compute budget for the tip transaction itself, as recommended by Jito
        let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000); // Generous limit for a simple transfer
        let cu_price_ix = ComputeBudgetInstruction::set_compute_unit_price(1); // Minimal price for the tip tx

        let instruction =
            solana_sdk::system_instruction::transfer(&payer.pubkey(), tip_account, tip_lamports);
        let message =
            solana_sdk::message::Message::new(&[instruction.clone()], Some(&payer.pubkey()));
        let recent_blockhash = self.get_recent_blockhash().await?;

        let transaction = Transaction::new(&[&payer], message, recent_blockhash);

        // Jito recommends the tip transaction be the *last* transaction in the bundle.
        // The actual bundle assembly will handle this. Here we just create the signed tip tx.
        let mut tip_tx_with_budget = Transaction::new_with_payer(
            &[cu_limit_ix, cu_price_ix, instruction],
            Some(&payer.pubkey()),
        );
        tip_tx_with_budget.sign(&[payer.as_ref()], recent_blockhash);

        debug!(
            "💰 Created signed tip transaction to: {} from {}",
            tip_account,
            payer.pubkey()
        );
        Ok(transaction)
    }

    /// Encode transactions to base64
    fn encode_transactions(&self, transactions: &[Transaction]) -> Result<Vec<String>, ArbError> {
        let mut encoded = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            match bincode::serialize(tx) {
                Ok(serialized) => {
                    use base64::{engine::general_purpose, Engine as _};
                    let encoded_tx = general_purpose::STANDARD.encode(&serialized);
                    encoded.push(encoded_tx);
                    debug!("📝 Encoded transaction {}: {} bytes", idx, serialized.len());
                }
                Err(e) => {
                    return Err(ArbError::ExecutionError(format!(
                        "Failed to serialize transaction {}: {}",
                        idx, e
                    )));
                }
            }
        }

        Ok(encoded)
    }

    /// Submit bundle request to Jito
    async fn submit_bundle_request(
        &self,
        encoded_transactions: &[String],
    ) -> Result<String, ArbError> {
        let request_id = self
            .request_id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let request = BundleSubmissionRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: "sendBundle".to_string(),
            params: BundleParams {
                bundle: encoded_transactions.to_vec(),
            },
        };

        debug!(
            "📤 Sending bundle request to: {}",
            self.jito_config.block_engine_url
        );

        let response = timeout(
            Duration::from_millis(self.jito_config.submission_timeout_ms),
            self.http_client
                .post(&self.jito_config.block_engine_url)
                .json(&request)
                .send(),
        )
        .await
        .map_err(|_| ArbError::TimeoutError("Bundle submission timeout".to_string()))?
        .map_err(|e| ArbError::NetworkError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ArbError::NetworkError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bundle_response: BundleSubmissionResponse = response
            .json()
            .await
            .map_err(|e| ArbError::ParseError(format!("Failed to parse response: {}", e)))?;

        if let Some(error) = bundle_response.error {
            return Err(ArbError::ExecutionError(format!(
                "Jito error {}: {}",
                error.code, error.message
            )));
        }

        bundle_response
            .result
            .ok_or_else(|| ArbError::ExecutionError("No bundle ID in response".to_string()))
    }

    /// Wait for bundle confirmation
    async fn wait_for_bundle_confirmation(
        &self,
        bundle_id: &str,
    ) -> Result<BundleConfirmationResult, ArbError> {
        debug!("⏳ Waiting for bundle confirmation: {}", bundle_id);

        let start_time = Instant::now();
        let max_wait_time = Duration::from_secs(30); // 30 seconds max wait

        while start_time.elapsed() < max_wait_time {
            match self.get_bundle_status(bundle_id).await {
                Ok(status) => match status.status.as_str() {
                    "landed" => {
                        info!("✅ Bundle landed in slot: {:?}", status.landed_slot);

                        let signatures: Vec<Signature> = status
                            .transactions
                            .iter()
                            .filter_map(|tx| tx.signature.parse().ok())
                            .collect();

                        return Ok(BundleConfirmationResult {
                            success: true,
                            signatures,
                            landed_slot: status.landed_slot,
                            error_message: None,
                        });
                    }
                    "failed" => {
                        let error_msg = status
                            .transactions
                            .iter()
                            .find_map(|tx| tx.error.as_ref())
                            .unwrap_or(&"Unknown error".to_string())
                            .clone();

                        warn!("❌ Bundle failed: {}", error_msg);

                        return Ok(BundleConfirmationResult {
                            success: false,
                            signatures: vec![],
                            landed_slot: None,
                            error_message: Some(error_msg),
                        });
                    }
                    "pending" | "processing" => {
                        debug!("⏳ Bundle status: {}", status.status);
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    _ => {
                        warn!("❓ Unknown bundle status: {}", status.status);
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                },
                Err(e) => {
                    debug!("Failed to get bundle status: {}", e);
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            }
        }

        Err(ArbError::TimeoutError(
            "Bundle confirmation timeout".to_string(),
        ))
    }

    /// Get bundle status from Jito
    async fn get_bundle_status(&self, bundle_id: &str) -> Result<BundleStatus, ArbError> {
        let url = format!(
            "{}/bundle-status/{}",
            self.jito_config.block_engine_url, bundle_id
        );

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| ArbError::NetworkError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ArbError::NetworkError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let status: BundleStatus = response
            .json()
            .await
            .map_err(|e| ArbError::ParseError(format!("Failed to parse bundle status: {}", e)))?;

        Ok(status)
    }

    /// Fetch recent blockhash from Solana RPC
    async fn get_recent_blockhash(&self) -> Result<SolanaHash, ArbError> {
        self.rpc_client.get_latest_blockhash().await.map_err(|e| {
            error!("Failed to get recent blockhash: {}", e);
            ArbError::RpcError(format!("Failed to get recent blockhash: {}", e))
        })
    }

    /// Estimate compute units needed for a set of instructions
    async fn estimate_compute_units(&self, instructions: &[Instruction]) -> u32 {
        // Base compute units per instruction type
        const BASE_CU_PER_INSTRUCTION: u32 = 1_000;
        const SWAP_INSTRUCTION_CU: u32 = 50_000;
        const TRANSFER_INSTRUCTION_CU: u32 = 5_000;

        let mut total_cu = 0;

        for instruction in instructions {
            // Estimate based on instruction data size and accounts
            let instruction_cu = if instruction.data.len() > 100 {
                SWAP_INSTRUCTION_CU // Likely a swap instruction
            } else if instruction.accounts.len() > 5 {
                TRANSFER_INSTRUCTION_CU // Likely a transfer
            } else {
                BASE_CU_PER_INSTRUCTION // Basic instruction
            };

            total_cu += instruction_cu;
        }

        // Add buffer for safety
        let buffered_cu = (total_cu as f64 * 1.2) as u32; // 20% buffer

        debug!(
            "🧮 Estimated compute units: {} (with 20% buffer)",
            buffered_cu
        );
        buffered_cu
    }

    /// Create flash loan protection instructions
    async fn create_flashloan_protection_instructions(&self) -> Result<Vec<Instruction>, ArbError> {
        // Placeholder for flash loan protection logic // not in use
        debug!("⚡ Creating flash loan protection instructions");
        Ok(vec![])
    }

    /// Create timing randomization instructions
    async fn create_timing_randomization_instructions(&self) -> Result<Vec<Instruction>, ArbError> {
        // Placeholder for timing randomization logic // not in use
        debug!("🎲 Creating timing randomization instructions");
        Ok(vec![])
    }

    // ========================================================================================
    // Status and Metrics Methods
    // ========================================================================================

    /// Update network conditions based on recent observations
    pub async fn update_network_conditions(
        &self,
        congestion_level: f64,
        average_priority_fee: u64,
        recent_block_time: Duration,
    ) -> Result<(), ArbError> {
        let mut conditions = self.network_conditions.lock().await;

        conditions.current_congestion_level = congestion_level.clamp(0.0, 1.0);
        conditions.average_priority_fee = average_priority_fee;
        conditions.recent_block_times.push(recent_block_time);

        // Keep only recent block times (last 10)
        if conditions.recent_block_times.len() > 10 {
            conditions.recent_block_times.remove(0);
        }

        conditions.last_update = Instant::now();

        debug!("🌐 Network conditions updated:");
        debug!(
            "   Congestion level: {:.2}",
            conditions.current_congestion_level
        );
        debug!(
            "   Average priority fee: {} lamports",
            conditions.average_priority_fee
        );
        debug!("   Recent block time: {:?}", recent_block_time);

        Ok(())
    }

    /// Record transaction execution result for metrics
    pub async fn record_transaction_result(
        &self,
        success: bool,
        priority_fee_used: u64,
        gas_saved: Option<u64>,
    ) -> Result<(), ArbError> {
        let mut metrics = self.metrics.lock().await;

        if success {
            metrics.successful_transactions += 1;
        } else {
            metrics.failed_transactions += 1;
        }

        // Update average priority fee
        let total_transactions = metrics.successful_transactions + metrics.failed_transactions;
        metrics.average_priority_fee = (metrics.average_priority_fee * (total_transactions - 1)
            + priority_fee_used)
            / total_transactions;

        if let Some(saved) = gas_saved {
            metrics.total_gas_saved += saved;
        }

        // Update bundle success rate
        if metrics.mev_protection_activations > 0 {
            metrics.bundle_success_rate =
                metrics.successful_transactions as f64 / metrics.mev_protection_activations as f64;
        }

        // Record priority fee history
        {
            let mut history = self.priority_fee_history.lock().await;
            history.push((Instant::now(), priority_fee_used));

            // Keep only recent history (last 100 transactions)
            if history.len() > 100 {
                history.remove(0);
            }
        }

        debug!(
            "📊 Transaction result recorded: success={}, fee={} lamports",
            success, priority_fee_used
        );

        Ok(())
    }

    /// Get current MEV protection metrics
    pub async fn get_metrics(&self) -> GasOptimizationMetrics {
        (*self.metrics.lock().await).clone()
    }

    /// Get current network conditions
    pub async fn get_network_conditions(&self) -> NetworkConditions {
        (*self.network_conditions.lock().await).clone()
    }

    /// Get comprehensive MEV protection status
    pub async fn get_protection_status(&self) -> MevProtectionStatus {
        let metrics = self.metrics.lock().await;
        let network_conditions = self.network_conditions.lock().await;

        MevProtectionStatus {
            is_active: self.mev_config.enable_private_mempool,
            current_congestion_level: network_conditions.current_congestion_level,
            average_priority_fee: metrics.average_priority_fee,
            successful_transactions: metrics.successful_transactions,
            failed_transactions: metrics.failed_transactions,
            total_gas_saved: metrics.total_gas_saved,
            bundle_success_rate: metrics.bundle_success_rate,
            mev_protection_activations: metrics.mev_protection_activations,
            last_network_update: network_conditions.last_update,
        }
    }

    /// Get optimal tip amount based on network conditions
    pub async fn get_optimal_tip(&self, base_tip: u64) -> Result<u64, ArbError> {
        // In real implementation, this would: // not in use - Current implementation is a placeholder.
        // 1. Query current network congestion
        // 2. Check recent bundle success rates
        // 3. Calculate optimal tip amount

        // For now, return base tip with small random adjustment
        let adjustment = (base_tip as f64 * 0.1) as u64; // 10% adjustment
        Ok(base_tip + adjustment)
    }

    /// Get tip accounts for different regions
    pub fn get_tip_accounts(&self) -> &HashMap<String, Pubkey> {
        &self.jito_config.tip_accounts
    }

    /// Access transaction bundles for future implementation
    pub async fn get_transaction_bundles(&self) -> HashMap<String, Vec<Transaction>> {
        self._transaction_bundles.lock().await.clone()
    }

    /// Store transaction bundle for future processing
    pub async fn store_transaction_bundle(
        &self,
        bundle_id: String,
        transactions: Vec<Transaction>,
    ) -> Result<(), ArbError> {
        let mut bundles = self._transaction_bundles.lock().await;
        bundles.insert(bundle_id, transactions);
        Ok(())
    }
}

// ========================================================================================
// Tests
// ========================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::opportunity::ArbHop;
    use crate::utils::DexType;
    use solana_sdk::pubkey::Pubkey;

    #[tokio::test]
    async fn test_jito_handler_initialization() {
        let mev_config = MevProtectionConfig::default();
        let jito_config = JitoConfig::default();
        let handler = JitoHandler::new(
            mev_config,
            jito_config,
            "https://api.mainnet-beta.solana.com".to_string(),
        );

        let metrics = handler.get_metrics().await;
        assert_eq!(metrics.successful_transactions, 0);
        assert_eq!(metrics.failed_transactions, 0);
    }

    #[tokio::test]
    async fn test_priority_fee_calculation() {
        let mev_config = MevProtectionConfig::default();
        let max_fee = mev_config.max_priority_fee_lamports;
        let jito_config = JitoConfig::default();
        let handler = JitoHandler::new(
            mev_config,
            jito_config,
            "https://api.mainnet-beta.solana.com".to_string(),
        );

        let opportunity = MultiHopArbOpportunity {
            estimated_profit_usd: Some(100.0),
            ..Default::default()
        };

        let base_fee = 10_000;
        let optimal_fee = handler
            .calculate_optimal_priority_fee(&opportunity, base_fee)
            .await
            .unwrap();

        assert!(optimal_fee >= base_fee);
        assert!(optimal_fee <= max_fee);
    }

    #[tokio::test]
    async fn test_mev_risk_analysis() {
        let mev_config = MevProtectionConfig::default();
        let jito_config = JitoConfig::default();
        let handler = JitoHandler::new(
            mev_config,
            jito_config,
            "https://api.mainnet-beta.solana.com".to_string(),
        );

        let high_profit_opportunity = MultiHopArbOpportunity {
            estimated_profit_usd: Some(1500.0),
            hops: vec![
                ArbHop {
                    dex: DexType::Orca,
                    pool: Pubkey::default(),
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 100.0,
                    expected_output: 2000.0,
                },
                ArbHop {
                    dex: DexType::Raydium,
                    pool: Pubkey::default(),
                    input_token: "USDC".to_string(),
                    output_token: "SOL".to_string(),
                    input_amount: 2000.0,
                    expected_output: 115.0,
                },
            ],
            ..Default::default()
        };

        let risk_score = handler
            .analyze_mev_risk(&high_profit_opportunity)
            .await
            .unwrap();
        assert!(risk_score > 0.5); // High profit should result in higher risk
    }

    #[test]
    fn test_jito_config_default() {
        let config = JitoConfig::default();
        assert!(!config.tip_accounts.is_empty());
        assert!(config.default_tip_lamports > 0);
    }

    #[tokio::test]
    async fn test_bundle_validation() {
        let mev_config = MevProtectionConfig::default();
        let jito_config = JitoConfig::default();
        let handler = JitoHandler::new(
            mev_config,
            jito_config,
            "https://api.mainnet-beta.solana.com".to_string(),
        );

        // Test empty bundle
        let empty_bundle = vec![];
        assert!(handler.validate_bundle(&empty_bundle).is_err());

        // Test valid bundle
        use solana_sdk::message::Message;
        // Add a compute budget instruction to make it valid for Jito
        let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
        let cu_price_ix = ComputeBudgetInstruction::set_compute_unit_price(1);
        let message = Message::new(&[cu_limit_ix, cu_price_ix], None);

        let transaction = Transaction::new_unsigned(message);
        let valid_bundle = vec![transaction];

        assert!(handler.validate_bundle(&valid_bundle).is_ok());
    }

    #[tokio::test]
    async fn test_optimal_tip_calculation() {
        let mev_config = MevProtectionConfig::default();
        let jito_config = JitoConfig::default();
        let handler = JitoHandler::new(
            mev_config,
            jito_config,
            "https://api.mainnet-beta.solana.com".to_string(),
        );

        let base_tip = handler.get_optimal_tip(15_000).await;
        assert!(base_tip.is_ok());
        assert!(base_tip.unwrap() >= 15_000);
    }
}
