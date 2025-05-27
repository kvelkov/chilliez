use anyhow::{anyhow, Result};
use log::{debug, error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::RpcFilterType;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::time::Duration;
use crate::error::ArbError;

/// Provides high-availability RPC with retries/fallbacks.
/// Not yet called by main flow but will be integrated for production HA.
pub struct SolanaRpcClient {
    /// Primary RPC client - will replace direct RpcClient usage in main/test
    pub primary_client: Arc<RpcClient>,
    /// Fallback endpoints for high-availability in production
    pub fallback_clients: Vec<Arc<RpcClient>>,
    /// Maximum number of retry attempts before falling back
    /// Will be configurable for production HA
    pub max_retries: usize,
    /// Delay between retry attempts (with jitter added)
    pub retry_delay: Duration,
}

impl SolanaRpcClient {
    /// Creates a new `SolanaRpcClient` with a primary RPC endpoint and optional fallback endpoints
    ///
    /// # Arguments
    /// * `primary_endpoint` - The main RPC endpoint URL
    /// * `fallback_endpoints` - List of backup RPC endpoints to try if primary fails
    /// * `max_retries` - Maximum retry attempts before trying fallbacks
    /// * `retry_delay` - Base delay between retry attempts
    pub fn new(
        primary_endpoint: &str,
        fallback_endpoints: Vec<String>,
        max_retries: usize,
        retry_delay: Duration,
    ) -> Self {
        let primary_client = Arc::new(RpcClient::new_with_timeout_and_commitment(
            primary_endpoint.to_string(),
            Duration::from_secs(10),
            CommitmentConfig::confirmed(),
        ));

        let fallback_clients = fallback_endpoints
            .iter()
            .map(|endpoint| {
                Arc::new(RpcClient::new_with_timeout_and_commitment(
                    endpoint.clone(),
                    Duration::from_secs(10),
                    CommitmentConfig::confirmed(),
                ))
            })
            .collect();

        Self {
            primary_client,
            fallback_clients,
            max_retries,
            retry_delay,
        }
    }

    /// Get account data with retries and fallbacks
    ///
    /// Attempts to fetch account data from primary RPC endpoint with retries,
    /// then falls back to secondary endpoints if all retries fail.
    ///
    /// # Arguments
    /// * `pubkey` - The account public key to fetch
    pub async fn get_account_data(&self, pubkey: &Pubkey) -> Result<Vec<u8>> {
        let mut retries = 0;
        let mut last_error = None;

        // Try with primary client
        while retries < self.max_retries {
            match self.primary_client.get_account(pubkey).await {
                Ok(account) => {
                    debug!("Fetched account data for {}", pubkey);
                    return Ok(account.data);
                }
                Err(err) => {
                    error!("RPC error fetching account {}: {}", pubkey, err);
                    last_error = Some(err);
                    retries += 1;

                    // Add jitter to avoid thundering herd
                    let jitter = rand::random::<u64>() % 500;
                    tokio::time::sleep(self.retry_delay + Duration::from_millis(jitter)).await;
                }
            }
        }

        // Try fallbacks
        for fallback_client in &self.fallback_clients {
            match fallback_client.get_account(pubkey).await {
                Ok(account) => {
                    info!("Fetched account data via fallback for {}", pubkey);
                    return Ok(account.data);
                }
                Err(err) => {
                    error!("Fallback RPC error: {}", err);
                    last_error = Some(err);
                }
            }
        }

        Err(anyhow!(
            "Failed to get account data after retries. Last error: {:?}",
            last_error
        ))
    }

    /// Get program accounts with a filter and retry logic
    ///
    /// Fetches all accounts owned by a program with specified filters.
    /// Implements retry logic and fallback to secondary endpoints.
    ///
    /// # Arguments
    /// * `program_id` - The program public key to query
    /// * `filters` - RPC filters to apply to the query
    pub async fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        filters: Vec<RpcFilterType>,
    ) -> Result<Vec<(Pubkey, Vec<u8>)>> {
        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut retries = 0;
        while retries < self.max_retries {
            match self
                .primary_client
                .get_program_accounts_with_config(program_id, config.clone())
                .await
            {
                Ok(accounts) => {
                    return Ok(accounts
                        .into_iter()
                        .map(|(pubkey, account)| (pubkey, account.data))
                        .collect());
                }
                Err(err) => {
                    error!("RPC error fetching program accounts: {}", err);
                    retries += 1;

                    let jitter = rand::random::<u64>() % 500;
                    tokio::time::sleep(self.retry_delay + Duration::from_millis(jitter)).await;
                }
            }
        }

        // Try fallbacks
        for fallback_client in &self.fallback_clients {
            match fallback_client
                .get_program_accounts_with_config(program_id, config.clone())
                .await
            {
                Ok(accounts) => {
                    return Ok(accounts
                        .into_iter()
                        .map(|(pubkey, account)| (pubkey, account.data))
                        .collect());
                }
                Err(err) => {
                    error!("Fallback RPC error fetching program accounts: {}", err);
                }
            }
        }

        Err(anyhow!("Failed to get program accounts after retries"))
    }

    /// Check RPC health
    ///
    /// Verifies the primary RPC endpoint is responding to health checks.
    /// Used to validate connectivity before performing operations.
    pub async fn is_healthy(&self) -> bool {
        self.primary_client.get_health().await.is_ok()
    }

    /// Get the current network congestion factor based on recent performance metrics
    /// Returns a value between 1.0 (low congestion) and 5.0 (extreme congestion)
    pub async fn get_network_congestion_factor(&self) -> f64 {
        match self
            .primary_client
            .get_recent_performance_samples(Some(10))
            .await
        {
            Ok(samples) => {
                if samples.is_empty() {
                    return 1.0; // Default to low congestion if no samples
                }

                // Calculate average transaction count and estimate slot processing time
                let avg_tx_count: f64 = samples
                    .iter()
                    .map(|s| s.num_transactions as f64)
                    .sum::<f64>()
                    / samples.len() as f64;

                // Calculate estimated slot time based on sample period and number of slots
                let avg_slot_time: f64 = samples
                    .iter()
                    .map(|s| (s.sample_period_secs as f64 * 1_000_000.0) / s.num_slots as f64) // Convert to microseconds
                    .sum::<f64>()
                    / samples.len() as f64;

                // Higher tx count and longer slot times indicate congestion
                let tx_factor = (avg_tx_count / 2000.0).min(3.0); // Normalize, capped at 3.0
                let time_factor = (avg_slot_time / 600.0).min(2.0); // Normalize, capped at 2.0

                // Combine factors with some weighting
                1.0 + tx_factor + time_factor
            }
            Err(err) => {
                error!("Failed to get performance samples: {}", err);
                1.5 // Default to slightly elevated congestion on error
            }
        }
    }

    /// Fetches recent prioritization fees for dynamic fee adjustment.
    pub async fn get_recent_prioritization_fees(
        &self,
    ) -> Result<Vec<u64>, ArbError> {
        let mut last_err = None;
        for client in std::iter::once(&self.primary_client).chain(self.fallback_clients.iter()) {
            for _ in 0..=self.max_retries {
                // The correct method is get_recent_prioritization_fees(&[Pubkey])
                // Here, we pass an empty slice to get all recent fees.
                match client.get_recent_prioritization_fees(&[]).await {
                    Ok(fees) => {
                        // fees is Vec<RpcPrioritizationFee>, extract the fee values
                        let lamports: Vec<u64> = fees.into_iter().map(|f| f.prioritization_fee).collect();
                        return Ok(lamports);
                    }
                    Err(e) => {
                        last_err = Some(e);
                        tokio::time::sleep(self.retry_delay).await;
                    }
                }
            }
        }
        Err(ArbError::RpcError(format!(
            "Failed to fetch recent prioritization fees: {:?}",
            last_err
        )))
    }
}
