//! ArbitrageExecutor is responsible for executing on-chain orders for validated arbitrage opportunities.
//!
//! It converts the opportunity into a transaction, submits it (or simulates it) on-chain,
//! and reports the result via an optional event channel. It uses a circuit breaker and retry
//! policy for robust, resilient behavior, and communicates with external dependencies such as
//! the Solana RPC client and a high-availability (HA) RPC client when available.

use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;
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
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    priority_fee: u64,
    max_timeout: Duration,
    simulation_mode: bool,
    paper_trading_mode: bool,
    network_congestion: Arc<AtomicU64>,
    solana_rpc: Option<Arc<SolanaRpcClient>>,
    enable_simulation: Arc<AtomicBool>,
    _degradation_mode: Arc<AtomicBool>,
    _degradation_profit_threshold: f64,
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    retry_policy: RetryPolicy,
    event_sender: Option<tokio::sync::mpsc::Sender<ExecutorEvent>>,
}

/// ExecutorEvent is sent to notify observers (through the pipeline or separate logging)
// about the result of executing an opportunity.
pub enum ExecutorEvent {
    OpportunityExecuted {
        opportunity_id: String,
        signature: Option<Signature>,
        timestamp: Instant,
        result: Result<(), ArbError>,
    },
}

impl ArbitrageExecutor {
    /// Constructs a new executor with all required parameters.
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<NonBlockingRpcClient>,
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool,
        paper_trading_mode: bool,
        circuit_breaker: Arc<RwLock<CircuitBreaker>>,
        retry_policy: RetryPolicy,
        event_sender: Option<tokio::sync::mpsc::Sender<ExecutorEvent>>,
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
            paper_trading_mode,
            network_congestion: Arc::new(AtomicU64::new(100)),
            solana_rpc: None,
            enable_simulation: Arc::new(AtomicBool::new(true)),
            _degradation_mode: Arc::new(AtomicBool::new(false)),
            _degradation_profit_threshold: 1.5,
            circuit_breaker,
            retry_policy,
            event_sender,
        }
    }

    /// Attaches a high-availability Solana RPC client for redundancy.
    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    /// Updates the network congestion factor based on data from the Solana RPC client.
    pub async fn update_network_congestion(&self) -> Result<(), ArbError> {
        if let Some(client) = &self.solana_rpc {
            let factor = client.get_network_congestion_factor().await;
            let congestion_value = (factor * 100.0) as u64;
            self.network_congestion
                .store(congestion_value.max(100), Ordering::Relaxed);
            info!(
                "Network congestion updated: factor {:.2} (stored as {})",
                factor,
                congestion_value.max(100)
            );
        } else {
            debug!("SolanaRpcClient not available; using default congestion value (100).");
            self.network_congestion.store(100, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Checks whether any token pairs in the opportunity are banned.
    pub fn has_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        use crate::arbitrage::detector::ArbitrageDetector;
        for hop in &opportunity.hops {
            if ArbitrageDetector::is_permanently_banned(&hop.input_token, &hop.output_token)
                || ArbitrageDetector::is_temporarily_banned(&hop.input_token, &hop.output_token)
            {
                return true;
            }
        }
        false
    }

    /// Executes a single attempt at processing an arbitrage opportunity.
    /// Builds the transaction, adds compute budget instructions if necessary, and sends or simulates the transaction.
    async fn execute_single_opportunity_attempt(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, ArbError> {
        let start_time = Instant::now();
        info!("Executing opportunity: {}.", opportunity.id);
        opportunity.log_summary();

        let mut instructions = self.build_instructions_from_multihop(opportunity)?;
        if instructions.is_empty()
            || instructions.iter().all(|ix| ix.program_id == solana_sdk::compute_budget::id())
        {
            if !self.paper_trading_mode && !self.simulation_mode {
                let err = ArbError::ExecutionError("No swap instructions built".to_string());
                error!("[Execution Error] Opportunity {} – no swap instructions.", opportunity.id);
                if let Some(sender) = &self.event_sender {
                    let _ = sender
                        .send(ExecutorEvent::OpportunityExecuted {
                            opportunity_id: opportunity.id.clone(),
                            signature: None,
                            timestamp: Instant::now(),
                            result: Err(err.clone()),
                        })
                        .await;
                }
                return Err(err);
            }
        }

        if self.paper_trading_mode {
            info!(
                "[PAPER TRADING] Simulated execution for opportunity: {}",
                opportunity.id
            );
            if let Some(sender) = &self.event_sender {
                let _ = sender
                    .send(ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: Some(Signature::default()),
                        timestamp: Instant::now(),
                        result: Ok(()),
                    })
                    .await;
            }
            return Ok(Signature::default());
        }

        if self.simulation_mode && !self.paper_trading_mode {
            info!(
                "[SIMULATION MODE] Simulating execution for opportunity: {}",
                opportunity.id
            );
            let sig = Signature::new_unique();
            if let Some(sender) = &self.event_sender {
                let _ = sender
                    .send(ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: Some(sig.clone()),
                        timestamp: Instant::now(),
                        result: Ok(()),
                    })
                    .await;
            }
            return Ok(sig);
        }

        let recent_blockhash = self.get_latest_blockhash_with_ha().await?;

        // Insert compute budget instructions if not already added.
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
            let congestion_factor =
                self.network_congestion.load(Ordering::Relaxed) as f64 / 100.0;
            let adjusted_priority = (self.priority_fee as f64 * congestion_factor.max(1.0)) as u64;
            final_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
                adjusted_priority,
            ));
            info!(
                "Adjusted priority fee: {} (base: {}, congestion: {:.2}x)",
                adjusted_priority, self.priority_fee, congestion_factor
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
            let err = ArbError::TimeoutError("Transaction construction timeout".to_string());
            warn!(
                "Timeout reached before sending transaction for opportunity: {}",
                opportunity.id
            );
            if let Some(sender) = &self.event_sender {
                let _ = sender
                    .send(ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: None,
                        timestamp: Instant::now(),
                        result: Err(err.clone()),
                    })
                    .await;
            }
            error!("Timeout during transaction construction for opportunity: {}", opportunity.id);
            return Err(err);
        }

        // Pre-flight simulation if enabled.
        if self.enable_simulation.load(Ordering::Relaxed) {
            match self.simulate_transaction_for_execution(&transaction).await {
                Ok(_) => info!(
                    "Simulation successful for opportunity: {}.",
                    opportunity.id
                ),
                Err(e) => {
                    error!(
                        "Simulation failed for opportunity: {}: {:?}",
                        opportunity.id, e
                    );
                    if let Some(sender) = &self.event_sender {
                        let _ = sender
                            .send(ExecutorEvent::OpportunityExecuted {
                                opportunity_id: opportunity.id.clone(),
                                signature: None,
                                timestamp: Instant::now(),
                                result: Err(e.clone()),
                            })
                            .await;
                    }
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
                    "Executed opportunity {} successfully. Signature: {}",
                    opportunity.id, signature
                );
                if let Some(sender) = &self.event_sender {
                    let _ = sender
                        .send(ExecutorEvent::OpportunityExecuted {
                            opportunity_id: opportunity.id.clone(),
                            signature: Some(signature.clone()),
                            timestamp: Instant::now(),
                            result: Ok(()),
                        })
                        .await;
                }
                Ok(signature)
            }
            Err(e) => {
                let err = ArbError::TransactionError(e.to_string());
                error!(
                    "Transaction failed for opportunity {}: {}",
                    opportunity.id, e
                );
                if let Some(sender) = &self.event_sender {
                    let _ = sender
                        .send(ExecutorEvent::OpportunityExecuted {
                            opportunity_id: opportunity.id.clone(),
                            signature: None,
                            timestamp: Instant::now(),
                            result: Err(err.clone()),
                        })
                        .await;
                }
                error!("Execution failed for opportunity {}: {}", opportunity.id, err);
                Err(err)
            }
        }
    }

    /// Public method to execute an opportunity.
    /// It first checks for banned tokens, then wraps the execution in a circuit breaker.
    pub async fn execute_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, ArbError> {
        if self.has_banned_tokens(opportunity) {
            let err = ArbError::ExecutionError("Opportunity involves banned token pair".to_string());
            warn!("Skipping opportunity {} due to banned tokens.", opportunity.id);
            if let Some(sender) = &self.event_sender {
                let _ = sender
                    .send(ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: None,
                        timestamp: Instant::now(),
                        result: Err(err.clone()),
                    })
                    .await;
            }
            return Err(err);
        }

        // Use the circuit breaker to guard the execution attempt.
        let self_clone = self.clone();
        let opp_clone = opportunity.clone();
        self.circuit_breaker.write().await.execute(async move {
            self_clone
                .execute_single_opportunity_attempt(&opp_clone)
                .await
        }).await
    }

    /// Executes an opportunity with retries. It checks whether the opportunity remains valid before each attempt.
    pub async fn execute_opportunity_with_retries<'env, FVS>(
        &'env self,
        opportunity: &MultiHopArbOpportunity,
        mut is_opportunity_still_valid: FVS,
    ) -> Result<Signature, ArbError>
    where
        FVS: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'env>>
            + Send
            + 'env,
    {
        let self_clone = self.clone();
        let opp_arc = Arc::new(opportunity.clone());
        let fvs_mutex = Arc::new(tokio::sync::Mutex::new(is_opportunity_still_valid));

        let operation = || {
            let executor_clone = self_clone.clone();
            let opp_clone = opp_arc.clone();
            let fvs_mutex_clone = Arc::clone(&fvs_mutex);
            async move {
                let mut validity_checker = fvs_mutex_clone.lock().await;
                if !(*validity_checker)().await {
                    warn!("Opportunity {} no longer valid before retry.", opp_clone.id);
                    return Err(ArbError::ExecutionError("Opportunity no longer valid".to_string()));
                }
                executor_clone.execute_opportunity(&opp_clone).await
            }
        };

        self.retry_policy.execute(operation).await
    }

    /// Stub: Builds transaction instructions from a multi-hop arbitrage opportunity.
    /// This function currently returns an empty vector, awaiting an advanced implementation.
    fn build_instructions_from_multihop(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, ArbError> {
        warn!("build_instructions_from_multihop stub invoked; no swap instructions generated.");
        Ok(vec![])
    }

    /// Retrieves the latest blockhash, using the high-availability client if available.
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

    /// Simulates a transaction execution pre-flight. Returns an error if simulation fails.
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
                    error!("Simulation error: {:?}", err);
                    Err(ArbError::SimulationFailed(format!("{:?}", err)))
                } else {
                    info!(
                        "Simulation passed for transaction. Logs: {:?}",
                        sim_response.value.logs.as_deref().unwrap_or_else(|| &[])
                    );
                    Ok(sim_response.value)
                }
            }
            Err(e) => {
                error!("RPC simulation error: {}", e);
                Err(ArbError::RpcError(e.to_string()))
            }
        }
    }
}
