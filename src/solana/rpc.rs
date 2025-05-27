use crate::error::ArbError;
use anyhow::{Context, Result}; // Removed unused `anyhow` direct import, Context is still used.
use log::{debug, error, info, warn};
use rand::Rng; // For jitter
use solana_account_decoder::UiAccountEncoding; // Corrected import path
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_filter::RpcFilterType;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep; // For async sleep

#[allow(dead_code)] // Silencing warning as NonBlockingRpcClient::new_with_commitment handles timeout differently
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30); // Define if not already present
const DEFAULT_COMMITMENT: CommitmentConfig = CommitmentConfig::confirmed(); // Define if not already present

/// Provides high-availability RPC with retries/fallbacks.
pub struct SolanaRpcClient {
    /// Primary RPC client - will replace direct RpcClient usage in main/test
    pub primary_client: Arc<NonBlockingRpcClient>,
    /// Fallback endpoints for high-availability in production
    pub fallback_clients: Vec<Arc<NonBlockingRpcClient>>,
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
        fallback_endpoints: Vec<String>, // Corrected type
        max_retries: usize,
        retry_delay: Duration,
    ) -> Self {
        // NonBlockingRpcClient does not have new_with_commitment_and_timeout.
        // Using new_with_commitment. Timeout is typically handled by the underlying HTTP client.
        let primary_client = Arc::new(NonBlockingRpcClient::new_with_commitment(
            primary_endpoint.to_string(),
            DEFAULT_COMMITMENT,
        ));

        let fallback_clients = fallback_endpoints
            .iter()
            .map(|url| {
                Arc::new(NonBlockingRpcClient::new_with_commitment(
                    url.clone(),
                    DEFAULT_COMMITMENT,
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

    async fn execute_with_retry_and_fallback<F, Fut, T>(
        &self,
        operation_name: &str,
        mut rpc_call_fn: F,
    ) -> anyhow::Result<T>
    where
        F: FnMut(Arc<NonBlockingRpcClient>) -> Fut, // Changed to take owned Arc for closure
        Fut: std::future::Future<Output = Result<T, solana_client::client_error::ClientError>>
            + Send,
        T: Send,
    {
        let mut last_error: Option<solana_client::client_error::ClientError> = None;

        // Try primary client with retries
        debug!(
            "[RPC HA - {}] Attempting with primary client",
            operation_name
        );
        for attempt in 0..self.max_retries {
            match rpc_call_fn(Arc::clone(&self.primary_client)).await {
                // Clone Arc for capture
                Ok(result) => {
                    debug!(
                        "[RPC HA - {}] Primary client succeeded on attempt {}",
                        operation_name,
                        attempt + 1
                    );
                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        "[RPC HA - {}] Primary client attempt {}/{} failed: {}",
                        operation_name,
                        attempt + 1,
                        self.max_retries,
                        e
                    );
                    last_error = Some(e);
                    if attempt < self.max_retries - 1 {
                        let mut delay_ms = self.retry_delay.as_millis() as u64;
                        if self.retry_delay.as_millis() > 0 {
                            let jitter_val = rand::thread_rng().gen_range(0..(delay_ms / 4).max(1));
                            delay_ms += jitter_val;
                        }
                        sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        // Try fallback clients
        debug!(
            "[RPC HA - {}] Primary client failed after all retries. Attempting fallback clients.",
            operation_name
        );
        for (i, fallback_client) in self.fallback_clients.iter().enumerate() {
            debug!(
                "[RPC HA - {}] Attempting with fallback client #{}",
                operation_name,
                i + 1
            );
            match rpc_call_fn(Arc::clone(fallback_client)).await {
                // Clone Arc for capture
                Ok(result) => {
                    info!(
                        "[RPC HA - {}] Fallback client #{} succeeded.",
                        operation_name,
                        i + 1
                    );
                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        "[RPC HA - {}] Fallback client #{} failed: {}",
                        operation_name,
                        i + 1,
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        let final_error_message = format!("[RPC HA - {}] All RPC attempts failed.", operation_name);
        error!("{}", final_error_message);
        Err(match last_error {
            Some(e) => anyhow::Error::from(e).context(final_error_message),
            None => anyhow::anyhow!(final_error_message),
        })
    }

    /// Get account data with retries and fallbacks
    ///
    /// Attempts to fetch account data from primary RPC endpoint with retries,
    /// then falls back to secondary endpoints if all retries fail.
    ///
    /// # Arguments
    /// * `pubkey` - The account public key to fetch
    pub async fn get_account_data(&self, pubkey: &Pubkey) -> Result<Vec<u8>> {
        // TODO: Integrate calls to this method where raw account data is needed (e.g., TokenMetadataCache, pool data fetching)
        self.execute_with_retry_and_fallback(
            "get_account_data",
            |client: Arc<NonBlockingRpcClient>| async move {
                client.get_account(pubkey).await.map(|acc| acc.data)
            },
        )
        .await
        .with_context(|| format!("Failed to get account data for {}", pubkey))
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
            account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                // Removed Some() wrapper
                encoding: Some(UiAccountEncoding::Base64), // Corrected path
                commitment: Some(CommitmentConfig::confirmed()),
                data_slice: None,
                min_context_slot: None,
            },
            with_context: Some(false),
        };

        // Clone config here because it's captured by the async move block and FnMut closure.
        let config_clone = config.clone();
        // The closure needs to capture a value that can be cloned for each call.
        // The `async move` block will capture `config_clone` (the outer one).
        // Inside the `async move` block, we clone it again for the actual RPC call.
        self.execute_with_retry_and_fallback(
  "get_program_accounts",
            move |client: Arc<NonBlockingRpcClient>| {
                // Clone config_clone here, so each invocation of the async block gets its own copy.
                let current_config_for_call = config_clone.clone();
                async move {
                    client.get_program_accounts_with_config(program_id, current_config_for_call).await
                        .map(|accounts| accounts.into_iter().map(|(k, v)| (k, v.data)).collect())
                }
            },
        )
        .await
        .with_context(|| format!("Failed to get program accounts for {}", program_id))
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
        match self.get_recent_prioritization_fees().await {
            Ok(fees) => {
                if fees.is_empty() {
                    return 1.0; // No fee data, assume normal
                }
                let mut sorted_fees = fees;
                sorted_fees.sort_unstable();

                let p75_index = (sorted_fees.len() as f64 * 0.75).floor() as usize;
                let p75_fee = sorted_fees
                    .get(p75_index.min(sorted_fees.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or(0);

                const BASELINE_PRIORITY_FEE: u64 = 5000;

                if p75_fee <= BASELINE_PRIORITY_FEE {
                    1.0
                } else if p75_fee <= BASELINE_PRIORITY_FEE * 5 {
                    1.0 + (p75_fee as f64 / BASELINE_PRIORITY_FEE as f64 - 1.0) * 0.25
                } else if p75_fee <= BASELINE_PRIORITY_FEE * 10 {
                    2.0
                } else {
                    3.0
                }
            }
            Err(err) => {
                error!(
                    "Failed to get recent prioritization fees for congestion factor: {}",
                    err
                );
                1.5 // Default to slightly elevated congestion on error if fees can't be fetched
            }
        }
    }

    /// Fetches recent prioritization fees for dynamic fee adjustment.
    pub async fn get_recent_prioritization_fees(&self) -> Result<Vec<u64>, ArbError> {
        self.execute_with_retry_and_fallback(
            "get_recent_prioritization_fees",
            |client: Arc<NonBlockingRpcClient>| async move {
                client.get_recent_prioritization_fees(&[]).await // Pass empty slice for global fees
            },
        )
        .await
        .map(|fees_response| {
            fees_response
                .into_iter()
                .map(|fee_info| fee_info.prioritization_fee)
                .collect()
        })
        .map_err(|e| ArbError::RpcError(e.to_string()))
    }
}
