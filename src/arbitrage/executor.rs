// src/arbitrage/executor.rs
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;

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

#[allow(dead_code)]
fn default_retry_policy() -> RetryPolicy {
    RetryPolicy::new(3, 500, 10_000, 0.3)
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
