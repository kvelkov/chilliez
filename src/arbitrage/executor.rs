// /Users/kiril/Desktop/chilliez/src/arbitrage/executor.rs
// src/arbitrage/executor.rs
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, CircuitBreaker, RetryPolicy}; // Import CircuitBreaker and RetryPolicy from error module
use crate::solana::rpc::SolanaRpcClient;

use futures::FutureExt;
use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_transaction_status::UiTransactionEncoding;

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    priority_fee: u64,
    max_timeout: Duration,
    simulation_mode: bool,
    paper_trading_mode: bool,
    // Wrap Atomics in Arc for Clone
    network_congestion: Arc<AtomicU64>,
    solana_rpc: Option<Arc<SolanaRpcClient>>,
    enable_simulation: Arc<AtomicBool>,
    _degradation_mode: Arc<AtomicBool>, // Prefixed
    _degradation_profit_threshold: f64, // Prefixed (f64 is Clone)
    // Added fields for error handling
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    retry_policy: RetryPolicy,
}

impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<NonBlockingRpcClient>,
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool,
        paper_trading_mode: bool,
        circuit_breaker: Arc<RwLock<CircuitBreaker>>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
            paper_trading_mode,
            network_congestion: Arc::new(AtomicU64::new(100)), // Wrap in Arc
            solana_rpc: None,
            enable_simulation: Arc::new(AtomicBool::new(true)), // Wrap in Arc
            _degradation_mode: Arc::new(AtomicBool::new(false)), // Wrap in Arc
            _degradation_profit_threshold: 1.5,
            circuit_breaker,
            retry_policy,
        }
    }

    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    pub async fn update_network_congestion(&self) -> Result<(), ArbError> {
        if let Some(client) = &self.solana_rpc {
            let factor = client.get_network_congestion_factor().await;
            let congestion_val = (factor * 100.0) as u64;
            self.network_congestion
                .store(congestion_val.max(100), Ordering::Relaxed);
            info!(
                "Network congestion factor updated to: {:.2} (stored as {})",
                factor,
                congestion_val.max(100)
            );
        } else {
            debug!("SolanaRpcClient (for HA) not available for congestion update, using default.");
            self.network_congestion.store(100, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn has_banned_tokens(&self, _opportunity: &MultiHopArbOpportunity) -> bool {
        false
    }

    pub fn _has_multihop_banned_tokens(&self, _opportunity: &MultiHopArbOpportunity) -> bool {
        false
    }

    pub async fn execute_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, ArbError> {
        if self.has_banned_tokens(opportunity) {
            warn!(
                "Opportunity ID: {} involves a banned token pair. Skipping.",
                opportunity.id
            );
            return Err(ArbError::ExecutionError(
                "Opportunity involves banned token pair".to_string(),
            ));
        }

        let cb_guard = self.circuit_breaker.read().await;
        if cb_guard.is_open() {
            warn!(
                "Circuit breaker is open. Skipping execution for opportunity ID: {}",
                opportunity.id
            );
            return Err(ArbError::CircuitBreakerOpen);
        }
        drop(cb_guard); // Release read lock

        let start_time = Instant::now();
        info!(
            "Attempting to execute opportunity ID: {}. Details:",
            opportunity.id
        );
        opportunity.log_summary();

        let mut instructions = self.build_instructions_from_multihop(opportunity)?;

        if instructions.is_empty()
            || instructions
                .iter()
                .all(|ix| ix.program_id == solana_sdk::compute_budget::id())
        {
            if !self.paper_trading_mode && !self.simulation_mode {
                error!("No actual swap instructions were built for opportunity ID: {}. Aborting execution.", opportunity.id);
                return Err(ArbError::ExecutionError(
                    "No swap instructions built".to_string(),
                ));
            }
        }

        if self.paper_trading_mode {
            info!(
                "[PAPER TRADING] Simulated execution for opportunity ID: {}",
                opportunity.id
            );
            return Ok(Signature::default());
        }

        if self.simulation_mode && !self.paper_trading_mode {
            info!(
                "[SIMULATION MODE] Simulating execution for opportunity ID: {}",
                opportunity.id
            );
            return Ok(Signature::new_unique());
        }

        let recent_blockhash = self.get_latest_blockhash_with_ha().await?;

        let has_budget_ix = instructions.iter().any(|ix| {
            ix.program_id == solana_sdk::compute_budget::id() && ix.data.get(0) == Some(&2u8)
        });
        let has_priority_ix = instructions.iter().any(|ix| {
            ix.program_id == solana_sdk::compute_budget::id() && ix.data.get(0) == Some(&3u8)
        });

        let mut final_instructions = Vec::new();
        if !has_budget_ix {
            final_instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(600_000));
        }
        if !has_priority_ix {
            let current_congestion_factor =
                self.network_congestion.load(Ordering::Relaxed) as f64 / 100.0;
            let adjusted_priority_fee =
                (self.priority_fee as f64 * current_congestion_factor.max(1.0)) as u64;
            final_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
                adjusted_priority_fee,
            ));
            info!(
                "Using adjusted priority fee: {} (base: {}, congestion: {:.2}x)",
                adjusted_priority_fee, self.priority_fee, current_congestion_factor
            );
        }
        final_instructions.append(&mut instructions);

        let transaction = Transaction::new_signed_with_payer(
            &final_instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        if start_time.elapsed() > self.max_timeout {
            warn!(
                "Max timeout reached before sending transaction for opportunity ID: {}",
                opportunity.id
            );
            return Err(ArbError::TimeoutError(
                "Transaction construction timeout".to_string(),
            ));
        }

        if self.enable_simulation.load(Ordering::Relaxed) {
            match self.simulate_transaction_for_execution(&transaction).await {
                Ok(_) => info!(
                    "Pre-flight simulation successful for opportunity ID: {}.",
                    opportunity.id
                ),
                Err(e) => {
                    error!(
                        "Pre-flight simulation failed for opportunity ID: {}: {:?}",
                        opportunity.id, e
                    );
                    // Potentially update circuit breaker on simulation failure
                    self.circuit_breaker.write().await.record_failure();
                    return Err(e);
                }
            }
        }

        let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(),
            |ha_client| ha_client.primary_client.clone(),
        );

        match rpc_to_use
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await
        {
            Ok(signature) => {
                info!(
                    "Successfully executed opportunity ID: {}! Signature: {}",
                    opportunity.id, signature
                );
                self.circuit_breaker.write().await.record_success();
                Ok(signature)
            }
            Err(e) => {
                error!(
                    "Transaction failed for opportunity ID {}: {}",
                    opportunity.id, e
                );
                self.circuit_breaker.write().await.record_failure();
                Err(ArbError::TransactionError(e.to_string()))
            }
        }
    }

    #[allow(dead_code)]
    pub async fn execute_opportunity_with_retries<'env, FVS>(
        &'env self,
        opportunity: &MultiHopArbOpportunity,
        is_opportunity_still_valid: FVS,
    ) -> Result<Signature, ArbError>
    where
        FVS: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env>> + Send + 'env,
    {
        let retry_policy_ref = &self.retry_policy;
        let executor_clone = self.clone(); // This now works due to Arc<Atomic...>
        let exec_opp_arc = Arc::new(opportunity.clone());

        // Box the is_opportunity_still_valid closure to store it in Arc<Mutex<...>>
        // The lifetime 'env ensures that the boxed future can borrow from the environment
        // captured by is_opportunity_still_valid.
        let boxed_is_valid: Arc<tokio::sync::Mutex<Box<dyn FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env>> + Send + 'env>>> =
            Arc::new(tokio::sync::Mutex::new(Box::new(is_opportunity_still_valid)));

        let execute_once = {
            let exec_opp = Arc::clone(&exec_opp_arc);
            // executor_clone is moved into this outer closure
            // The future returned by the inner async block will borrow from executor_clone
            move || {
                let exec_opp_inner = Arc::clone(&exec_opp);
                let executor_for_async = executor_clone.clone(); // Clone Arc for the async block
                async move {
                    match executor_for_async.execute_opportunity(&exec_opp_inner).await {
                        Ok(_sig) => Ok(()),
                        Err(e) => Err(format!("{:?}", e)),
                    }
                }
                .boxed() // Box the future
            }
        };

        let is_valid_closure_for_retry = {
            let boxed_is_valid_clone = Arc::clone(&boxed_is_valid);
            move || {
                let inner_boxed_is_valid = Arc::clone(&boxed_is_valid_clone);
                async move {
                    let mut guard = inner_boxed_is_valid.lock().await;
                    (guard)().await // Invoke the boxed FnMut
                }
                .boxed() // Box the future
            }
        };

        execute_order_with_retries(
            retry_policy_ref,
            execute_once,
            is_valid_closure_for_retry,
        )
        .await
        .map(|_| Signature::default()) // If Ok, return a default/dummy signature
        .map_err(|e| ArbError::ExecutionError(e))
    }

    #[allow(dead_code)]
    pub async fn try_execute_arbitrage_with_retry<'env, FVS>(
        &'env self,
        opportunity: &MultiHopArbOpportunity,
        is_opportunity_still_valid: FVS,
    ) -> Result<Signature, ArbError>
    where FVS: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env>> + Send + 'env {
        self.execute_opportunity_with_retries(opportunity, is_opportunity_still_valid).await
    }

    fn build_instructions_from_multihop(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, ArbError> {
        warn!("build_instructions_from_multihop is a stub. No actual swap instructions generated.");
        Ok(vec![])
    }

    async fn get_latest_blockhash_with_ha(&self) -> Result<Hash, ArbError> {
        if let Some(client) = &self.solana_rpc {
            client
                .primary_client
                .get_latest_blockhash()
                .await
                .map_err(|e| ArbError::RpcError(e.to_string()))
        } else {
            self.rpc_client
                .get_latest_blockhash()
                .await
                .map_err(|e| ArbError::RpcError(e.to_string()))
        }
    }

    async fn simulate_transaction_for_execution(
        &self,
        transaction: &Transaction,
    ) -> Result<solana_client::rpc_response::RpcSimulateTransactionResult, ArbError> {
        let client_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(),
            |ha_client| ha_client.primary_client.clone(),
        );

        let sim_config = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            encoding: Some(UiTransactionEncoding::Base64),
            accounts: None,
            min_context_slot: None,
            inner_instructions: false,
        };

        match client_to_use
            .simulate_transaction_with_config(transaction, sim_config)
            .await
        {
            Ok(sim_response) => {
                if let Some(err) = &sim_response.value.err {
                    error!("Transaction simulation failed: {:?}", err);
                    Err(ArbError::SimulationFailed(format!("{:?}", err)))
                } else {
                    info!(
                        "Transaction simulation successful. Logs: {:?}",
                        sim_response.value.logs.as_deref().unwrap_or_default()
                    );
                    Ok(sim_response.value)
                }
            }
            Err(e) => {
                error!("Error during transaction simulation RPC call: {}", e);
                Err(ArbError::RpcError(e.to_string()))
            }
        }
    }
}

#[allow(dead_code)]
pub async fn execute_with_retry<'env_lifetime, F, V>(
    retry_policy: &RetryPolicy,
    mut execute_once: F,
    mut is_opportunity_still_valid: V,
) -> Result<(), String>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'env_lifetime>>,
    V: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env_lifetime>>,
{
    use rand::{thread_rng, Rng};
    use tokio::time::sleep; // Ensure sleep is imported from tokio::time

    let mut attempt = 0;
    let mut current_delay_ms = retry_policy.base_delay.as_millis();
    loop {
        attempt += 1;
        let valid = (is_opportunity_still_valid)().await;
        if !valid {
            log::warn!("Retry aborted: opportunity no longer valid (attempt {})", attempt);
            return Err("Opportunity no longer valid".to_string());
        }

        let result = execute_once().await;
        match result {
            Ok(_) => {
                log::info!("Order executed successfully (attempt {})", attempt);
                return Ok(());
            }
            Err(ref e) if attempt < retry_policy.max_attempts => {
                log::warn!(
                    "Order execution failed, will retry (attempt {}): {}",
                    attempt,
                    e
                );
                let jitter_ms: u128 = thread_rng().gen_range(0..(current_delay_ms / 2).max(1));
                let sleep_duration_ms = current_delay_ms + jitter_ms;

                sleep(Duration::from_millis(sleep_duration_ms as u64)).await;
                current_delay_ms = (current_delay_ms * 2).min(retry_policy.max_delay.as_millis());
            }
            Err(e) => {
                log::error!(
                    "Order execution failed, no more retries (attempt {}): {}",
                    attempt,
                    e
                );
                return Err(e);
            }
        }
    }
}

pub async fn execute_order_with_retries<'env_lifetime, F, V>(
    retry_policy: &RetryPolicy,
    execute_once: F, // Removed mut
    is_opportunity_still_valid: V, // Removed mut
) -> Result<(), String>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'env_lifetime>>,
    V: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env_lifetime>>,
{
    execute_with_retry(retry_policy, execute_once, is_opportunity_still_valid).await
}
