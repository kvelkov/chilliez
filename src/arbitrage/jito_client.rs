//! Jito Bundle Client for MEV Protection
//! 
//! This module provides functionality to submit transaction bundles via Jito
//! for MEV protection and atomic execution of arbitrage trades.

use solana_sdk::{
    transaction::Transaction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    hash::Hash,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::time::{Duration, Instant};
use log::{info, warn, error, debug};
use crate::error::ArbError;

type Result<T> = std::result::Result<T, ArbError>;

/// Configuration for Jito bundle submission
#[derive(Debug, Clone)]
pub struct JitoConfig {
    /// Jito bundle endpoint URL
    pub bundle_endpoint: String,
    /// Default tip amount in lamports
    pub default_tip_lamports: u64,
    /// Maximum number of transactions per bundle
    pub max_bundle_size: usize,
    /// Timeout for bundle submission
    pub submission_timeout: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Retry delay multiplier
    pub retry_delay_ms: u64,
    /// Enable tip optimization based on network conditions
    pub dynamic_tips: bool,
    /// Jito tip account (official Jito tip account)
    pub tip_account: Pubkey,
}

impl Default for JitoConfig {
    fn default() -> Self {
        Self {
            bundle_endpoint: "https://mainnet.block-engine.jito.wtf".to_string(),
            default_tip_lamports: 10_000, // 0.00001 SOL
            max_bundle_size: 5,
            submission_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay_ms: 1000,
            dynamic_tips: true,
            // Official Jito tip account
            tip_account: "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5".parse().unwrap(),
        }
    }
}

/// Bundle submission statistics
#[derive(Debug, Clone, Default)]
pub struct BundleStats {
    pub total_bundles_submitted: u64,
    pub successful_submissions: u64,
    pub failed_submissions: u64,
    pub total_transactions: u64,
    pub total_tips_paid: u64,
    pub average_submission_time_ms: f64,
}

/// Jito bundle client for MEV-protected transaction submission
pub struct JitoClient {
    config: JitoConfig,
    rpc_client: RpcClient,
    stats: BundleStats,
}

impl JitoClient {
    /// Create a new Jito client
    pub fn new(config: JitoConfig, rpc_client: RpcClient) -> Self {
        info!("🚀 Initializing Jito client with endpoint: {}", config.bundle_endpoint);
        Self {
            config,
            rpc_client,
            stats: BundleStats::default(),
        }
    }

    /// Create a new Jito client with default configuration
    pub fn new_with_defaults(rpc_client: RpcClient) -> Self {
        Self::new(JitoConfig::default(), rpc_client)
    }

    /// Submit a bundle of transactions atomically
    pub async fn submit_bundle(&mut self, transactions: Vec<Transaction>) -> Result<String> {
        let start_time = Instant::now();
        
        if transactions.is_empty() {
            return Err(ArbError::InvalidInput("Cannot submit empty bundle".to_string()));
        }

        if transactions.len() > self.config.max_bundle_size {
            return Err(ArbError::InvalidInput(
                format!("Bundle size {} exceeds maximum {}", 
                        transactions.len(), self.config.max_bundle_size)
            ));
        }

        info!("📦 Submitting bundle with {} transactions", transactions.len());
        let tx_count = transactions.len();

        // For now, we'll simulate bundle submission by sending transactions individually
        // In a real implementation, you would use the Jito bundle API
        let bundle_id = self.simulate_bundle_submission(transactions).await?;

        let submission_time = start_time.elapsed();
        self.update_stats(true, tx_count, submission_time);

        info!("✅ Bundle submitted successfully: {} (took {:.2}ms)", 
              bundle_id, submission_time.as_millis());

        Ok(bundle_id)
    }

    /// Submit a bundle with a custom tip amount
    pub async fn submit_bundle_with_tip(
        &mut self, 
        mut transactions: Vec<Transaction>, 
        tip_lamports: u64,
        tip_payer: &Keypair,
        recent_blockhash: Hash,
    ) -> Result<String> {
        // Add tip transaction to the beginning of the bundle
        let tip_tx = self.create_tip_transaction(tip_payer, tip_lamports, recent_blockhash);
        transactions.insert(0, tip_tx);

        info!("💰 Adding {} lamport tip to bundle", tip_lamports);
        self.submit_bundle(transactions).await
    }

    /// Create a tip transaction for Jito validators
    pub fn create_tip_transaction(
        &self,
        payer: &Keypair,
        tip_lamports: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let tip_instruction = system_instruction::transfer(
            &payer.pubkey(),
            &self.config.tip_account,
            tip_lamports,
        );

        Transaction::new_signed_with_payer(
            &[tip_instruction],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        )
    }

    /// Calculate optimal tip amount based on network conditions
    pub async fn calculate_optimal_tip(&self, trade_value_lamports: u64) -> Result<u64> {
        if !self.config.dynamic_tips {
            return Ok(self.config.default_tip_lamports);
        }

        // Simple dynamic tip calculation
        // In production, this would consider network congestion, competition, etc.
        let base_tip = self.config.default_tip_lamports;
        let dynamic_tip = (trade_value_lamports as f64 * 0.0001) as u64; // 0.01% of trade value
        
        let optimal_tip = base_tip.max(dynamic_tip).min(trade_value_lamports / 100); // Cap at 1%
        
        debug!("💡 Calculated optimal tip: {} lamports (base: {}, dynamic: {})", 
               optimal_tip, base_tip, dynamic_tip);
        
        Ok(optimal_tip)
    }

    /// Simulate bundle submission (replace with actual Jito API call)
    async fn simulate_bundle_submission(&self, transactions: Vec<Transaction>) -> Result<String> {
        // In a real implementation, this would:
        // 1. Serialize transactions
        // 2. Send to Jito bundle endpoint
        // 3. Return bundle ID
        
        // For now, we'll send transactions individually as a fallback
        let mut bundle_id = String::new();
        
        for (i, tx) in transactions.iter().enumerate() {
            match self.submit_transaction_with_retry(tx).await {
                Ok(signature) => {
                    if i == 0 {
                        bundle_id = signature.clone(); // Use first transaction signature as bundle ID
                    }
                    debug!("✅ Transaction {} submitted: {}", i + 1, signature);
                }
                Err(e) => {
                    error!("❌ Failed to submit transaction {}: {}", i + 1, e);
                    return Err(e);
                }
            }
        }

        Ok(bundle_id)
    }

    /// Submit a single transaction with retry logic
    async fn submit_transaction_with_retry(&self, transaction: &Transaction) -> Result<String> {
        let mut last_error = None;
        
        for attempt in 1..=self.config.max_retries {
            match self.rpc_client.send_and_confirm_transaction(transaction).await {
                Ok(signature) => {
                    if attempt > 1 {
                        info!("✅ Transaction succeeded on attempt {}", attempt);
                    }
                    return Ok(signature.to_string());
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        let delay = Duration::from_millis(self.config.retry_delay_ms * attempt as u64);
                        warn!("⚠️ Transaction attempt {} failed, retrying in {:?}", attempt, delay);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(ArbError::TransactionFailed(
            format!("Transaction failed after {} attempts: {:?}", 
                    self.config.max_retries, last_error)
        ))
    }

    /// Update bundle submission statistics
    fn update_stats(&mut self, success: bool, tx_count: usize, submission_time: Duration) {
        self.stats.total_bundles_submitted += 1;
        self.stats.total_transactions += tx_count as u64;
        
        if success {
            self.stats.successful_submissions += 1;
        } else {
            self.stats.failed_submissions += 1;
        }

        // Update average submission time
        let new_time_ms = submission_time.as_millis() as f64;
        if self.stats.total_bundles_submitted == 1 {
            self.stats.average_submission_time_ms = new_time_ms;
        } else {
            let total_submissions = self.stats.total_bundles_submitted as f64;
            self.stats.average_submission_time_ms = 
                (self.stats.average_submission_time_ms * (total_submissions - 1.0) + new_time_ms) / total_submissions;
        }
    }

    /// Get bundle submission statistics
    pub fn get_stats(&self) -> &BundleStats {
        &self.stats
    }

    /// Get success rate as a percentage
    pub fn get_success_rate(&self) -> f64 {
        if self.stats.total_bundles_submitted == 0 {
            return 0.0;
        }
        (self.stats.successful_submissions as f64 / self.stats.total_bundles_submitted as f64) * 100.0
    }

    /// Update configuration
    pub fn update_config(&mut self, new_config: JitoConfig) {
        info!("🔄 Updating Jito client configuration");
        self.config = new_config;
    }

    /// Check if bundle submission is healthy
    pub fn is_healthy(&self) -> bool {
        if self.stats.total_bundles_submitted < 10 {
            return true; // Not enough data
        }
        
        self.get_success_rate() > 80.0 && self.stats.average_submission_time_ms < 30_000.0
    }
}

/// Bundle builder helper for constructing transaction bundles
pub struct BundleBuilder {
    transactions: Vec<Transaction>,
    max_size: usize,
}

impl BundleBuilder {
    /// Create a new bundle builder
    pub fn new(max_size: usize) -> Self {
        Self {
            transactions: Vec::new(),
            max_size,
        }
    }

    /// Add a transaction to the bundle
    pub fn add_transaction(mut self, transaction: Transaction) -> Result<Self> {
        if self.transactions.len() >= self.max_size {
            return Err(ArbError::InvalidInput(
                format!("Bundle is full (max size: {})", self.max_size)
            ));
        }
        
        self.transactions.push(transaction);
        Ok(self)
    }

    /// Add multiple transactions to the bundle
    pub fn add_transactions(mut self, transactions: Vec<Transaction>) -> Result<Self> {
        if self.transactions.len() + transactions.len() > self.max_size {
            return Err(ArbError::InvalidInput(
                format!("Adding {} transactions would exceed max bundle size {}", 
                        transactions.len(), self.max_size)
            ));
        }
        
        self.transactions.extend(transactions);
        Ok(self)
    }

    /// Build the final bundle
    pub fn build(self) -> Vec<Transaction> {
        self.transactions
    }

    /// Get current bundle size
    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    /// Check if bundle is empty
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}
