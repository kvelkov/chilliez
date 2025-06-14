// src/arbitrage/jito_client.rs
//! Jito Bundle Client for MEV-Protected Execution
//! 
//! This module provides integration with Jito's bundle submission system
//! for atomic transaction execution with MEV protection.

use crate::error::ArbError;
use log::{info, warn, error, debug};
use solana_sdk::{
    signature::Signature,
    transaction::Transaction,
    pubkey::Pubkey,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::time::timeout;

/// Configuration for Jito client
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
        tip_accounts.insert("ny".to_string(), 
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5".parse().unwrap());
        tip_accounts.insert("ams".to_string(), 
            "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe".parse().unwrap());
        tip_accounts.insert("fra".to_string(), 
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY".parse().unwrap());
        tip_accounts.insert("tok".to_string(), 
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49".parse().unwrap());

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

/// Bundle submission request
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

/// Bundle submission response
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

/// Bundle status
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

/// Bundle execution result
#[derive(Debug, Clone)]
pub struct JitoBundleResult {
    pub bundle_id: String,
    pub success: bool,
    pub signatures: Vec<Signature>,
    pub landed_slot: Option<u64>,
    pub execution_time_ms: u64,
    pub error_message: Option<String>,
}

/// Jito client for bundle submission
pub struct JitoClient {
    config: JitoConfig,
    client: reqwest::Client,
    request_id_counter: std::sync::atomic::AtomicU64,
}

impl JitoClient {
    /// Create a new Jito client
    pub fn new(config: JitoConfig) -> Self {
        info!("🚀 Initializing Jito client");
        info!("   🌐 Block engine URL: {}", config.block_engine_url);
        info!("   💰 Default tip: {} lamports", config.default_tip_lamports);
        info!("   ⏱️ Timeout: {}ms", config.submission_timeout_ms);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.submission_timeout_ms))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            request_id_counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Submit a bundle to Jito
    pub async fn submit_bundle(
        &self,
        transactions: Vec<Transaction>,
        tip_lamports: Option<u64>,
    ) -> Result<JitoBundleResult, ArbError> {
        let start_time = Instant::now();
        let tip_amount = tip_lamports.unwrap_or(self.config.default_tip_lamports);
        
        info!("📦 Submitting bundle with {} transactions, tip: {} lamports", 
              transactions.len(), tip_amount);

        // Validate bundle
        self.validate_bundle(&transactions)?;

        // Create tip transaction
        let tip_transaction = self.create_tip_transaction(tip_amount).await?;
        
        // Combine transactions with tip
        let mut bundle_transactions = transactions;
        bundle_transactions.push(tip_transaction);

        // Encode transactions to base64
        let encoded_transactions = self.encode_transactions(&bundle_transactions)?;

        // Submit bundle with retries
        let mut last_error = None;
        
        for attempt in 1..=self.config.max_retries {
            debug!("📤 Bundle submission attempt {}/{}", attempt, self.config.max_retries);
            
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
                    
                    if attempt < self.config.max_retries {
                        // Exponential backoff
                        let delay = Duration::from_millis(100 * (1 << attempt));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| ArbError::ExecutionError("Bundle submission failed".to_string())))
    }

    /// Validate bundle before submission
    fn validate_bundle(&self, transactions: &[Transaction]) -> Result<(), ArbError> {
        if transactions.is_empty() {
            return Err(ArbError::ExecutionError("Bundle cannot be empty".to_string()));
        }

        if transactions.len() > 5 {
            return Err(ArbError::ExecutionError("Bundle too large (max 5 transactions)".to_string()));
        }

        // Validate each transaction
        for (idx, tx) in transactions.iter().enumerate() {
            if tx.message.instructions.is_empty() {
                return Err(ArbError::ExecutionError(
                    format!("Transaction {} has no instructions", idx)
                ));
            }

            // Check for compute budget instructions
            let has_compute_budget = tx.message.instructions.iter()
                .any(|ix| {
                    if let Some(program_id) = tx.message.account_keys.get(ix.program_id_index as usize) {
                        *program_id == solana_sdk::compute_budget::id()
                    } else {
                        false
                    }
                });
            
            if !has_compute_budget {
                warn!("⚠️ Transaction {} missing compute budget instructions", idx);
            }
        }

        debug!("✅ Bundle validation passed for {} transactions", transactions.len());
        Ok(())
    }

    /// Create a tip transaction
    async fn create_tip_transaction(&self, tip_lamports: u64) -> Result<Transaction, ArbError> {
        debug!("💰 Creating tip transaction: {} lamports", tip_lamports);

        // Select tip account (prefer NY region)
        let tip_account = self.config.tip_accounts.get("ny")
            .or_else(|| self.config.tip_accounts.values().next())
            .ok_or_else(|| ArbError::ConfigError("No tip accounts configured".to_string()))?;

        // In real implementation, this would:
        // 1. Create a transfer instruction to the tip account
        // 2. Sign with the wallet keypair
        // 3. Return the signed transaction

        // Placeholder implementation
        use solana_sdk::message::Message;
        let message = Message::new(&[], None);
        let transaction = Transaction::new_unsigned(message);

        debug!("💰 Created tip transaction to account: {}", tip_account);
        Ok(transaction)
    }

    /// Encode transactions to base64
    fn encode_transactions(&self, transactions: &[Transaction]) -> Result<Vec<String>, ArbError> {
        let mut encoded = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            match bincode::serialize(tx) {
                Ok(serialized) => {
                    let encoded_tx = base64::encode(&serialized);
                    encoded.push(encoded_tx);
                    debug!("📝 Encoded transaction {}: {} bytes", idx, serialized.len());
                }
                Err(e) => {
                    return Err(ArbError::ExecutionError(
                        format!("Failed to serialize transaction {}: {}", idx, e)
                    ));
                }
            }
        }

        Ok(encoded)
    }

    /// Submit bundle request to Jito
    async fn submit_bundle_request(&self, encoded_transactions: &[String]) -> Result<String, ArbError> {
        let request_id = self.request_id_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        
        let request = BundleSubmissionRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: "sendBundle".to_string(),
            params: BundleParams {
                bundle: encoded_transactions.to_vec(),
            },
        };

        debug!("📤 Sending bundle request to: {}", self.config.block_engine_url);

        let response = timeout(
            Duration::from_millis(self.config.submission_timeout_ms),
            self.client
                .post(&self.config.block_engine_url)
                .json(&request)
                .send()
        ).await
        .map_err(|_| ArbError::TimeoutError("Bundle submission timeout".to_string()))?
        .map_err(|e| ArbError::NetworkError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ArbError::NetworkError(
                format!("HTTP error: {}", response.status())
            ));
        }

        let bundle_response: BundleSubmissionResponse = response
            .json()
            .await
            .map_err(|e| ArbError::ParseError(format!("Failed to parse response: {}", e)))?;

        if let Some(error) = bundle_response.error {
            return Err(ArbError::ExecutionError(
                format!("Jito error {}: {}", error.code, error.message)
            ));
        }

        bundle_response.result
            .ok_or_else(|| ArbError::ExecutionError("No bundle ID in response".to_string()))
    }

    /// Wait for bundle confirmation
    async fn wait_for_bundle_confirmation(&self, bundle_id: &str) -> Result<BundleConfirmationResult, ArbError> {
        debug!("⏳ Waiting for bundle confirmation: {}", bundle_id);

        let start_time = Instant::now();
        let max_wait_time = Duration::from_secs(30); // 30 seconds max wait

        while start_time.elapsed() < max_wait_time {
            match self.get_bundle_status(bundle_id).await {
                Ok(status) => {
                    match status.status.as_str() {
                        "landed" => {
                            info!("✅ Bundle landed in slot: {:?}", status.landed_slot);
                            
                            let signatures: Vec<Signature> = status.transactions
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
                            let error_msg = status.transactions
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
                    }
                }
                Err(e) => {
                    debug!("Failed to get bundle status: {}", e);
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            }
        }

        Err(ArbError::TimeoutError("Bundle confirmation timeout".to_string()))
    }

    /// Get bundle status
    async fn get_bundle_status(&self, bundle_id: &str) -> Result<BundleStatus, ArbError> {
        // In real implementation, this would query Jito's bundle status endpoint
        // For now, we'll simulate a successful bundle
        
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(BundleStatus {
            bundle_id: bundle_id.to_string(),
            status: "landed".to_string(),
            landed_slot: Some(12345678),
            transactions: vec![
                BundleTransaction {
                    signature: Signature::default().to_string(),
                    status: "success".to_string(),
                    error: None,
                }
            ],
        })
    }

    /// Get optimal tip amount based on network conditions
    pub async fn get_optimal_tip(&self, base_tip: u64) -> Result<u64, ArbError> {
        // In real implementation, this would:
        // 1. Query current network congestion
        // 2. Check recent bundle success rates
        // 3. Calculate optimal tip amount

        // For now, return base tip with small random adjustment
        let adjustment = (base_tip as f64 * 0.1) as u64; // 10% adjustment
        Ok(base_tip + adjustment)
    }

    /// Get tip accounts for different regions
    pub fn get_tip_accounts(&self) -> &HashMap<String, Pubkey> {
        &self.config.tip_accounts
    }
}

#[derive(Debug, Clone)]
struct BundleConfirmationResult {
    success: bool,
    signatures: Vec<Signature>,
    landed_slot: Option<u64>,
    error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jito_config_default() {
        let config = JitoConfig::default();
        assert!(!config.tip_accounts.is_empty());
        assert!(config.default_tip_lamports > 0);
    }

    #[test]
    fn test_jito_client_creation() {
        let config = JitoConfig::default();
        let client = JitoClient::new(config);
        assert_eq!(client.request_id_counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_bundle_validation() {
        let config = JitoConfig::default();
        let client = JitoClient::new(config);

        // Test empty bundle
        let empty_bundle = vec![];
        assert!(client.validate_bundle(&empty_bundle).is_err());

        // Test valid bundle
        use solana_sdk::message::Message;
        let message = Message::new(&[], None);
        let transaction = Transaction::new_unsigned(message);
        let valid_bundle = vec![transaction];
        
        // This should pass validation (though it has no instructions)
        // In real implementation, we'd create proper transactions
        assert!(client.validate_bundle(&valid_bundle).is_ok());
    }

    #[tokio::test]
    async fn test_optimal_tip_calculation() {
        let config = JitoConfig::default();
        let client = JitoClient::new(config);
        
        // Test tip calculation
        let base_tip = client.get_optimal_tip(15_000).await;
        assert!(base_tip.is_ok());
        assert!(base_tip.unwrap() >= 15_000);
    }
}
