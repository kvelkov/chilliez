use anyhow::{anyhow, Result};
use log::{debug, error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::RpcFilterType;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;

/// Provides high-availability RPC with retries/fallbacks.
/// Not yet called by main flow but will be integrated for production HA.
#[allow(dead_code)]
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
    /// Create a new RPC client with primary and fallback endpoints
    ///
    /// # Arguments
    /// * `primary_endpoint` - The main RPC endpoint URL
    /// * `fallback_endpoints` - List of backup RPC endpoints to try if primary fails
    /// * `max_retries` - Maximum retry attempts before trying fallbacks
    /// * `retry_delay` - Base delay between retry attempts
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub async fn is_healthy(&self) -> bool {
        self.primary_client.get_health().await.is_ok()
    }
}
