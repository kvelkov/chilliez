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
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use spl_token::state::Mint; // Added for unpacking mint data
use solana_sdk::program_pack::Pack; // Added for the Pack trait
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const DEFAULT_COMMITMENT: CommitmentConfig = CommitmentConfig::confirmed();

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

    pub async fn get_recent_prioritization_fees(&self) -> Result<Vec<u64>, ArbError> {
        self.execute_with_retry_and_fallback(
            "get_recent_prioritization_fees",
            |client: Arc<NonBlockingRpcClient>| async move {
                client.get_recent_prioritization_fees(&[]).await
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



    /// Checks the health of the RPC client, primarily by querying the primary client.
    pub async fn is_healthy(&self) -> bool {
        debug!("[RPC HA - is_healthy] Checking RPC health...");
        match self.primary_client.get_health().await {
            Ok(_) => {
                debug!("[RPC HA - is_healthy] Primary RPC client is healthy.");
                true
            }
            Err(e) => {
                warn!("[RPC HA - is_healthy] Primary RPC client health check failed: {}. Consider checking fallback clients if necessary.", e);
                // Optionally, you could try to check fallback clients here as well
                false
            }
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