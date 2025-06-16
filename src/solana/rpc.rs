// src/solana/rpc.rs
use crate::error::ArbError;
use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use rand::Rng;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    nonblocking::rpc_client::RpcClient as NonBlockingRpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::RpcFilterType,
    rpc_response::RpcPrioritizationFee as PrioritizationFee,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use spl_token::state::Mint; // Added for unpacking mint data
use solana_sdk::program_pack::Pack; // Added for the Pack trait
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const DEFAULT_COMMITMENT: CommitmentConfig = CommitmentConfig::confirmed();

/// Represents the health status of an individual RPC client
#[derive(Debug, Clone)]
pub struct ClientHealthStatus {
    pub name: String,
    pub is_healthy: bool,
    pub health_endpoint_ok: bool,
    pub functionality_ok: bool,
    pub response_time_ok: bool,
    pub response_time_ms: u64,
    pub last_slot: Option<u64>,
    pub last_successful_request: Option<std::time::Instant>,
    pub error_count: u32,
    pub error_message: Option<String>,
}

/// Represents the overall health status of the RPC system
#[derive(Debug, Clone)]
pub struct RpcHealthStatus {
    pub overall_healthy: bool,
    pub primary_status: ClientHealthStatus,
    pub fallback_statuses: Vec<ClientHealthStatus>,
    pub degraded_mode: bool,
    pub check_duration_ms: u64,
}

/// Provides high-availability RPC with retries/fallbacks.
pub struct SolanaRpcClient {
    pub primary_client: Arc<NonBlockingRpcClient>,
    pub fallback_clients: Vec<Arc<NonBlockingRpcClient>>,
    pub max_retries: usize,
    pub retry_delay: Duration,
}

impl SolanaRpcClient {
    pub fn new(
        primary_endpoint: &str,
        fallback_endpoints: Vec<String>,
        max_retries: usize,
        retry_delay: Duration,
    ) -> Self {
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
        F: FnMut(Arc<NonBlockingRpcClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T, solana_client::client_error::ClientError>> + Send,
        T: Send,
    {
        let mut last_error: Option<solana_client::client_error::ClientError> = None;

        for attempt in 0..self.max_retries {
            match rpc_call_fn(Arc::clone(&self.primary_client)).await {
                Ok(result) => {
                    debug!("[RPC HA - {}] Primary client succeeded on attempt {}", operation_name, attempt + 1);
                    return Ok(result);
                }
                Err(e) => {
                    warn!("[RPC HA - {}] Primary client attempt {}/{} failed: {}", operation_name, attempt + 1, self.max_retries, e);
                    last_error = Some(e);
                    if attempt < self.max_retries - 1 {
                        let mut delay_ms = self.retry_delay.as_millis() as u64;
                        if delay_ms > 0 {
                            let jitter_val = rand::thread_rng().gen_range(0..(delay_ms / 4).max(1));
                            delay_ms += jitter_val;
                        }
                        sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        for (i, fallback_client) in self.fallback_clients.iter().enumerate() {
            debug!("[RPC HA - {}] Attempting with fallback client #{}", operation_name, i + 1);
            match rpc_call_fn(Arc::clone(fallback_client)).await {
                Ok(result) => {
                    info!("[RPC HA - {}] Fallback client #{} succeeded.", operation_name, i + 1);
                    return Ok(result);
                }
                Err(e) => {
                    warn!("[RPC HA - {}] Fallback client #{} failed: {}", operation_name, i + 1, e);
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

    pub async fn get_token_account_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        let op_name = format!("get_token_account_balance({})", pubkey);
        let result = self
            .execute_with_retry_and_fallback(&op_name, |client| async move {
                client.get_token_account_balance(pubkey).await
            })
            .await?;
        
        Ok(result.amount.parse::<u64>()?)
    }

    pub async fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        filters: Vec<RpcFilterType>,
    ) -> Result<Vec<(Pubkey, Vec<u8>)>> {
        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                data_slice: None,
                min_context_slot: None,
            },
            with_context: Some(false),
        };

        let config_clone = config.clone();
        self.execute_with_retry_and_fallback(
            "get_program_accounts",
            move |client: Arc<NonBlockingRpcClient>| {
                let current_config_for_call = config_clone.clone();
                async move {
                    client
                        .get_program_accounts_with_config(program_id, current_config_for_call)
                        .await
                        .map(|accounts| accounts.into_iter().map(|(k, v)| (k, v.data)).collect())
                }
            },
        )
        .await
        .with_context(|| format!("Failed to get program accounts for {}", program_id))
    }

    /// Get recent prioritization fees from Solana network
    pub async fn get_recent_prioritization_fees(&self, accounts: Option<Vec<Pubkey>>) -> Result<Vec<PrioritizationFee>, ArbError> {
        // Try primary client first
        let accounts_ref = accounts.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
        let last_error = match self.primary_client.get_recent_prioritization_fees(accounts_ref).await {
            Ok(fees) => {
                debug!("✅ Got {} recent prioritization fees from primary RPC", fees.len());
                return Ok(fees);
            }
            Err(e) => {
                warn!("❌ Primary RPC failed to get prioritization fees: {}", e);
                ArbError::NetworkError(e.to_string())
            }
        };
        
        // Try fallback clients
        let mut final_error = last_error;
        for (i, fallback) in self.fallback_clients.iter().enumerate() {
            match fallback.get_recent_prioritization_fees(accounts_ref).await {
                Ok(fees) => {
                    debug!("✅ Got {} recent prioritization fees from fallback RPC {}", fees.len(), i);
                    return Ok(fees);
                }
                Err(e) => {
                    warn!("❌ Fallback RPC {} failed to get prioritization fees: {}", i, e);
                    final_error = ArbError::NetworkError(e.to_string());
                }
            }
        }
        
        Err(final_error)
    }

    pub async fn get_account_data(&self, pubkey: &Pubkey) -> Result<Vec<u8>> {
        let op_name = format!("get_account_data({})", pubkey);
        self.execute_with_retry_and_fallback(&op_name, |client| async move {
            // The underlying RpcClient's get_account_data returns Result<Vec<u8>, ClientError>
            // execute_with_retry_and_fallback will handle mapping ClientError to anyhow::Error
            client.get_account_data(pubkey).await
        })
        .await
        .with_context(|| format!("Failed to get account data for {}", pubkey))
    }

    pub async fn get_token_mint_decimals(&self, mint_pubkey: &Pubkey) -> Result<u8> {
        let op_name = format!("get_token_mint_decimals({})", mint_pubkey);
        let account_data = self
            .execute_with_retry_and_fallback(&op_name, |client| async move {
                client.get_account_data(mint_pubkey).await
            })
            .await
            .with_context(|| format!("Failed to get account data for mint {}", mint_pubkey))?;

        Mint::unpack(&account_data)
            .map(|mint_info| mint_info.decimals)
            .map_err(|e| {
                anyhow::anyhow!("Failed to unpack mint account data for {}: {}", mint_pubkey, e)
            })
    }



    /// Comprehensive health check for the RPC client system
    pub async fn is_healthy(&self) -> bool {
        debug!("[RPC HA - is_healthy] Starting comprehensive RPC health check...");
        
        let start_time = std::time::Instant::now();
        
        // Test primary client health
        let primary_healthy = match self.primary_client.get_health().await {
            Ok(_) => {
                debug!("[RPC HA - is_healthy] Primary RPC client health check passed.");
                true
            }
            Err(e) => {
                warn!("[RPC HA - is_healthy] Primary RPC client health check failed: {}", e);
                false
            }
        };
        
        // Test basic functionality with a simple request
        let functionality_healthy = match self.primary_client.get_slot().await {
            Ok(slot) => {
                debug!("[RPC HA - is_healthy] Primary RPC client functionality test passed (slot: {})", slot);
                true
            }
            Err(e) => {
                warn!("[RPC HA - is_healthy] Primary RPC client functionality test failed: {}", e);
                false
            }
        };
        
        // If primary is not healthy, test fallback clients
        let fallback_healthy = if !primary_healthy && !self.fallback_clients.is_empty() {
            let mut any_fallback_healthy = false;
            for (i, fallback_client) in self.fallback_clients.iter().enumerate() {
                match fallback_client.get_health().await {
                    Ok(_) => {
                        debug!("[RPC HA - is_healthy] Fallback client #{} is healthy", i + 1);
                        any_fallback_healthy = true;
                        break;
                    }
                    Err(e) => {
                        warn!("[RPC HA - is_healthy] Fallback client #{} health check failed: {}", i + 1, e);
                    }
                }
            }
            any_fallback_healthy
        } else {
            false
        };
        
        let overall_healthy = primary_healthy && functionality_healthy || fallback_healthy;
        let check_duration = start_time.elapsed();
        
        if overall_healthy {
            info!("[RPC HA - is_healthy] Overall health check passed in {:?}", check_duration);
        } else {
            error!("[RPC HA - is_healthy] Overall health check failed in {:?} - primary_healthy: {}, functionality_healthy: {}, fallback_healthy: {}", 
                   check_duration, primary_healthy, functionality_healthy, fallback_healthy);
        }
        
        overall_healthy
    }

    /// Get detailed health status for monitoring
    pub async fn get_health_status(&self) -> RpcHealthStatus {
        let start_time = std::time::Instant::now();
        
        // Test primary client
        let primary_status = self.test_client_health(&self.primary_client, "Primary").await;
        
        // Test fallback clients
        let mut fallback_statuses = Vec::new();
        for (i, fallback_client) in self.fallback_clients.iter().enumerate() {
            let status = self.test_client_health(fallback_client, &format!("Fallback-{}", i + 1)).await;
            fallback_statuses.push(status);
        }
        
        let total_check_time = start_time.elapsed();
        let overall_healthy = primary_status.is_healthy || fallback_statuses.iter().any(|s| s.is_healthy);
        
        RpcHealthStatus {
            overall_healthy,
            primary_status,
            fallback_statuses,
            degraded_mode: false, // TODO: Determine if degraded mode is active
            check_duration_ms: total_check_time.as_millis() as u64,
        }
    }
    
    /// Test health of a specific RPC client
    async fn test_client_health(&self, client: &Arc<NonBlockingRpcClient>, name: &str) -> ClientHealthStatus {
        let start_time = std::time::Instant::now();
        
        // Test 1: Health endpoint
        let health_ok = client.get_health().await.is_ok();
        
        // Test 2: Basic functionality (get slot)
        let (slot_ok, slot_value) = match client.get_slot().await {
            Ok(slot) => (true, Some(slot)),
            Err(_) => (false, None),
        };
        
        // Test 3: Response time test
        let response_time = start_time.elapsed();
        let response_time_ok = response_time.as_millis() < 5000; // 5 second threshold
        
        let is_healthy = health_ok && slot_ok && response_time_ok;
        
        ClientHealthStatus {
            name: name.to_string(),
            is_healthy,
            health_endpoint_ok: health_ok,
            functionality_ok: slot_ok,
            response_time_ok,
            response_time_ms: response_time.as_millis() as u64,
            last_slot: slot_value,
            last_successful_request: if is_healthy { Some(start_time) } else { None },
            error_count: if is_healthy { 0 } else { 1 },
            error_message: if !is_healthy {
                Some(format!("Health: {}, Functionality: {}, Response: {}ms", 
                           health_ok, slot_ok, response_time.as_millis()))
            } else {
                None
            },
        }
    }

    /// Gets the current network congestion factor based on recent slot progression
    /// Returns a value between 0.0 (no congestion) and 1.0 (high congestion)
    pub async fn get_network_congestion_factor(&self) -> f64 {
        // Simple implementation based on slot progression
        // In production, this could be more sophisticated
        match self.primary_client.get_slot().await {
            Ok(_slot) => {
                // For now, return a mock value
                // TODO: Implement actual congestion calculation based on:
                // - Slot progression rate
                // - Transaction fees
                // - Failed transaction rate
                0.1 // Low congestion for now
            }
            Err(_) => 0.5, // Medium congestion if RPC is having issues
        }
    }
}

// ---
// NOTE: For best performance, use Helius' fast RPC endpoint for transaction sending and bundle submission (Jito).
// Set the environment variable HELIUS_RPC_URL or SOLANA_RPC_URL to your Helius endpoint.
// ---



