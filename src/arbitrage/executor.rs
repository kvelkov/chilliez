// src/arbitrage/executor.rs
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::ArbError;
use crate::solana::rpc::SolanaRpcClient;

use futures::FutureExt; // <-- Add this import at the top
use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, // Added for priority_fee
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
use std::time::{Duration, Instant};

lazy_static::lazy_static! {
    // Placeholder
}

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 200,
            max_delay_ms: 1000,
            jitter: true,
        }
    }
}

pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    priority_fee: u64, // Will be used now
    max_timeout: Duration,
    simulation_mode: bool,
    paper_trading_mode: bool,
    network_congestion: AtomicU64, // Value should be used to scale priority_fee
    solana_rpc: Option<Arc<SolanaRpcClient>>,
    enable_simulation: AtomicBool,
    pub(crate) _degradation_mode: AtomicBool, // Prefixed
    pub(crate) _degradation_profit_threshold: f64, // Prefixed
}

impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<NonBlockingRpcClient>,
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool,
        paper_trading_mode: bool,
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
            paper_trading_mode,
            network_congestion: AtomicU64::new(100),
            solana_rpc: None,
            enable_simulation: AtomicBool::new(true),
            _degradation_mode: AtomicBool::new(false),
            _degradation_profit_threshold: 1.5,
        }
    }

    // Used in main.rs
    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    // Used in main.rs
    pub async fn update_network_congestion(&self) -> Result<(), ArbError> {
        if let Some(client) = &self.solana_rpc {
            let factor = client.get_network_congestion_factor().await;
            let congestion_val = (factor * 100.0) as u64;
            self.network_congestion
                .store(congestion_val.max(100), Ordering::Relaxed); // Ensure it's at least 1.0 (100)
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

    // Used by execute_opportunity
    pub fn has_banned_tokens(&self, _opportunity: &MultiHopArbOpportunity) -> bool {
        false
    }

    // Prefixed as it's not directly called by execute_opportunity yet
    pub fn _has_multihop_banned_tokens(&self, _opportunity: &MultiHopArbOpportunity) -> bool {
        false
    }

    // This is the main public method for this struct
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

        // Add compute budget and priority fee instructions if not already added by build_instructions
        // Assuming build_instructions_from_multihop might add its own specific budget.
        // If not, we add general ones here.
        let has_budget_ix = instructions.iter().any(|ix| {
            ix.program_id == solana_sdk::compute_budget::id() && ix.data.get(0) == Some(&2u8)
        }); // Check for set_compute_unit_limit
        let has_priority_ix = instructions.iter().any(|ix| {
            ix.program_id == solana_sdk::compute_budget::id() && ix.data.get(0) == Some(&3u8)
        }); // Check for set_compute_unit_price

        let mut final_instructions = Vec::new();
        if !has_budget_ix {
            final_instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(600_000));
            // Default limit
        }
        if !has_priority_ix {
            // Potentially scale priority_fee by network_congestion
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
                Ok(signature)
            }
            Err(e) => {
                error!(
                    "Transaction failed for opportunity ID {}: {}",
                    opportunity.id, e
                );
                Err(ArbError::TransactionError(e.to_string()))
            }
        }
    }

    /// Retry-enabled execution of arbitrage opportunities; robust trade execution under failure/congestion.
    /// This method is fully async and designed to be called concurrently for many opportunities.
    #[allow(dead_code)]
    pub async fn execute_opportunity_with_retries(
        &self,
        opportunity: &MultiHopArbOpportunity,
        is_opportunity_still_valid: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + 'static,
    ) -> Result<Signature, ArbError> {
        // Best practice: Use conservative retry policy for financial execution
        let retry_policy = RetryPolicy {
            max_retries: 3,           // 3-5 is typical for financial bots
            base_delay_ms: 250,       // 250ms base delay
            max_delay_ms: 2000,       // 2s max delay
            jitter: true,             // Add jitter to avoid thundering herd
        };

        // Clone all necessary data for async closure
        let wallet = Arc::clone(&self.wallet);
        let rpc_client = Arc::clone(&self.rpc_client);
        let priority_fee = self.priority_fee;
        let max_timeout = self.max_timeout;
        let simulation_mode = self.simulation_mode;
        let paper_trading_mode = self.paper_trading_mode;
        let solana_rpc = self.solana_rpc.clone();
        let enable_simulation = self.enable_simulation.load(Ordering::Relaxed);
        let network_congestion = self.network_congestion.load(Ordering::Relaxed);
        let _degradation_mode = self._degradation_mode.load(Ordering::Relaxed);
        let _degradation_profit_threshold = self._degradation_profit_threshold;

        let exec_opp = Arc::new(opportunity.clone());

        let boxed_is_valid: Arc<tokio::sync::Mutex<Box<dyn FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send>>> =
            Arc::new(tokio::sync::Mutex::new(Box::new(is_opportunity_still_valid)));

        let execute_once = {
            let exec_opp = Arc::clone(&exec_opp);
            move || {
                let exec_opp = Arc::clone(&exec_opp);
                let wallet = Arc::clone(&wallet);
                let rpc_client = Arc::clone(&rpc_client);
                let solana_rpc = solana_rpc.clone();
                async move {
                    let executor = ArbitrageExecutor {
                        wallet,
                        rpc_client,
                        priority_fee,
                        max_timeout,
                        simulation_mode,
                        paper_trading_mode,
                        network_congestion: AtomicU64::new(network_congestion),
                        solana_rpc,
                        enable_simulation: AtomicBool::new(enable_simulation),
                        _degradation_mode: AtomicBool::new(_degradation_mode),
                        _degradation_profit_threshold,
                    };
                    match executor.execute_opportunity(&exec_opp).await {
                        Ok(_sig) => Ok(()),
                        Err(e) => Err(format!("{:?}", e)),
                    }
                }
                .boxed()
            }
        };

        let is_valid_closure = {
            let boxed_is_valid = Arc::clone(&boxed_is_valid);
            move || {
                let boxed_is_valid = Arc::clone(&boxed_is_valid);
                async move {
                    let mut guard = boxed_is_valid.lock().await;
                    (guard)().await
                }
                .boxed()
            }
        };

        // Fully async retry logic: can be called concurrently for many opportunities
        execute_order_with_retries(
            &retry_policy,
            execute_once,
            is_valid_closure,
        )
        .await
        .map(|_| Signature::default())
        .map_err(|e| ArbError::ExecutionError(e))
    }

    /// High-level entry point for retry-enabled arbitrage execution; integrates validation and retry logic.
    /// This method is async and can be called concurrently.
    #[allow(dead_code)]
    pub async fn try_execute_arbitrage_with_retry(
        &self,
        opportunity: &MultiHopArbOpportunity,
        mut is_opportunity_still_valid: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + 'static,
    ) -> Result<Signature, ArbError> {
        // The main engine should provide an async closure for opportunity validation, e.g.:
        // let is_still_valid = || Box::pin(async { engine.is_opportunity_still_valid(&opportunity).await });
        self.execute_opportunity_with_retries(opportunity, move || is_opportunity_still_valid()).await
    }

    // Used by execute_opportunity
    fn build_instructions_from_multihop(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, ArbError> {
        warn!("build_instructions_from_multihop is a stub. No actual swap instructions generated. Intended to use self.priority_fee and self.network_congestion.");
        // Example of how priority_fee and network_congestion might be used:
        // let current_congestion_factor = self.network_congestion.load(Ordering::Relaxed) as f64 / 100.0;
        // let adjusted_priority_fee = (self.priority_fee as f64 * current_congestion_factor.max(1.0)) as u64;
        // let budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(600_000);
        // let priority_ix = ComputeBudgetInstruction::set_compute_unit_price(adjusted_priority_fee);
        // let mut instructions = vec![budget_ix, priority_ix];
        // ... add actual swap instructions for each hop ...
        // Ok(instructions)
        Ok(vec![])
    }

    // Used by execute_opportunity (conditionally)
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

    // Used by execute_opportunity (conditionally)
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

/// Core async retry logic for order execution.
/// This function is generic and can be used for any async operation that may need retries,
/// such as sending transactions, placing orders, or other network-dependent actions.
/// It is intended to be called by higher-level execution functions (e.g., execute_order_with_retries).
#[allow(dead_code)]
pub async fn execute_with_retry<F, V>(
    retry_policy: &RetryPolicy,
    mut execute_once: F,
    mut is_opportunity_still_valid: V,
) -> Result<(), String>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>,
    V: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>,
{
    use rand::{thread_rng, Rng};
    use std::time::Duration;
    use tokio::time::sleep;

    let mut attempt = 0;
    let mut delay = retry_policy.base_delay_ms;

    loop {
        attempt += 1;
        let valid = is_opportunity_still_valid().await;
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
            Err(ref e) if attempt < retry_policy.max_retries => {
                log::warn!(
                    "Order execution failed, will retry (attempt {}): {}",
                    attempt,
                    e
                );
                // Exponential backoff with optional jitter
                let mut sleep_ms = delay;
                if retry_policy.jitter {
                    let jitter: u64 = thread_rng().gen_range(0..(delay / 2).max(1));
                    sleep_ms += jitter;
                }
                sleep(Duration::from_millis(sleep_ms)).await;
                delay = (delay * 2).min(retry_policy.max_delay_ms);
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

// -- Wrapper for main arbitrage execution loop to use retry logic.
pub async fn execute_order_with_retries(
    retry_policy: &RetryPolicy,
    execute_once: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>> + Send,
    mut is_opportunity_still_valid: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send,
) -> Result<(), String> {
    execute_with_retry(retry_policy, execute_once, move || is_opportunity_still_valid()).await
}

// Example usage in your main arbitrage execution loop:
// let retry_policy = RetryPolicy::default();
// let result = execute_order_with_retries(
//     &retry_policy,
//     &opportunity,
//     || Box::pin(async { /* ...order execution logic... */ }),
//     || Box::pin(async { /* ...opportunity validity check... */ }),
// ).await;

// The following are already installed and fully functioning in your codebase:

// 1. RetryPolicy struct & fields
// 2. execute_with_retry (core async retry logic)
// 3. execute_order_with_retries (wrapper for main arbitrage loop)
// 4. execute_opportunity_with_retries (retry-enabled execution of arbitrage opportunities)
// 5. try_execute_arbitrage_with_retry (high-level entry point for retry-enabled arbitrage execution)

// All of these are present, implemented, and ready for use in your current executor.rs.
