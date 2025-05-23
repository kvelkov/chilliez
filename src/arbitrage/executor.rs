use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;
// Removed unused utils imports: DexType, PoolInfo, TokenAmount
// use crate::utils::{DexType, PoolInfo, TokenAmount};
// Removed unused anyhow import
// use anyhow::{anyhow, Result}; 
use anyhow::Result; // Keep anyhow::Result if it's actually used for non-ArbError results
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    // Removed unused Message and Pubkey from solana_sdk direct imports, they are used via Transaction or Keypair
    // message::Message,
    // pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
    account_decoder::UiAccountEncoding, // For RpcSimulateTransactionConfig
    commitment_config::CommitmentLevel, // For RpcSimulateTransactionConfig
    transaction:: giấy TransactionError as SolanaTransactionError, // For RpcSimulateTransactionResult
};
use solana_transaction_status::UiTransactionEncoding; // For RpcSimulateTransactionConfig encoding

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

lazy_static::lazy_static! {
    static ref RPC_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "rpc", 5, 3, Duration::from_secs(60), Duration::from_secs(300)
    );
    static ref DEX_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "dex", 3, 2, Duration::from_secs(30), Duration::from_secs(120)
    );
    static ref EXECUTION_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "execution", 3, 5, Duration::from_secs(60), Duration::from_secs(300)
    );
}

fn default_retry_policy() -> RetryPolicy {
    RetryPolicy::new(3, 500, 10_000, 0.3)
}

pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    priority_fee: u64,
    max_timeout: Duration,
    simulation_mode: bool,
    paper_trading_mode: bool,
    network_congestion: AtomicU64,
    solana_rpc: Option<Arc<SolanaRpcClient>>,
    enable_simulation: AtomicBool,
    recent_failures: Arc<dashmap::DashMap<String, (Instant, u32)>>,
    degradation_mode: AtomicBool,
    degradation_profit_threshold: f64,
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
            recent_failures: Arc::new(dashmap::DashMap::new()),
            degradation_mode: AtomicBool::new(false),
            degradation_profit_threshold: 1.5,
        }
    }

    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    pub async fn update_network_congestion(&self) -> Result<(), ArbError> { // Changed to ArbError
        if let Some(solana_rpc_client) = &self.solana_rpc {
            let congestion_factor = solana_rpc_client.get_network_congestion_factor().await;
            let congestion_stored = (congestion_factor * 100.0).round() as u64;
            self.network_congestion.store(congestion_stored, Ordering::Relaxed);
            info!("Updated network congestion factor: {:.2} (Stored: {})", congestion_factor, congestion_stored);
        } else {
            warn!("SolanaRpcClient (HA) not available for network congestion update.");
        }
        Ok(())
    }

    pub fn has_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        for hop in &opportunity.hops {
            if crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(&hop.input_token, &hop.output_token)
                || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(&hop.input_token, &hop.output_token)
            {
                return true;
            }
        }
        false
    }
    
    pub fn has_multihop_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        self.has_banned_tokens(opportunity)
    }

    pub async fn execute_opportunity(&self, opportunity: &MultiHopArbOpportunity) -> Result<Signature, ArbError> {
        if self.has_banned_tokens(opportunity) {
            warn!("Opportunity ID: {} involves a banned token pair. Skipping.", opportunity.id);
            return Err(ArbError::ExecutionError("Opportunity involves banned token pair".to_string()));
        }

        let start_time = Instant::now();
        info!(
            "Executing arbitrage opportunity ID: {} | Path: {:?} -> {:?} | Expected Profit: {:.4}%",
            opportunity.id, opportunity.input_token, opportunity.output_token, opportunity.profit_pct
        );
        opportunity.log_summary();

        let instructions = match self.build_instructions_from_multihop(opportunity) {
            Ok(instr) => instr,
            Err(e) => {
                error!("Failed to build instructions for opportunity ID {}: {}", opportunity.id, e);
                return Err(e); // build_instructions_from_multihop now returns ArbError
            }
        };
        
        if instructions.len() <= 2 && !self.paper_trading_mode && !self.simulation_mode  {
             error!("No actual swap instructions were built for opportunity ID: {}. Aborting execution.", opportunity.id);
             return Err(ArbError::ExecutionError("No swap instructions built".to_string()));
        }

        if self.paper_trading_mode {
            info!("[PAPER TRADING] Simulating execution for opportunity ID: {}. {} instructions.", opportunity.id, instructions.len());
            // XML Logging remains
            return Ok(Signature::default());
        }
        
        if self.simulation_mode && !self.paper_trading_mode {
             info!("[SIMULATION MODE] Simulating transaction for opportunity ID: {}. {} instructions.", opportunity.id, instructions.len());
            return Ok(Signature::new_unique());
        }

        let recent_blockhash = self.get_latest_blockhash_with_ha().await?;
        
        let transaction = Transaction::new_signed_with_payer(
            &instructions, Some(&self.wallet.pubkey()), &[&*self.wallet], recent_blockhash,
        );

        if start_time.elapsed() > self.max_timeout {
            return Err(ArbError::TimeoutError(format!("Transaction preparation exceeded timeout for opportunity ID: {}", opportunity.id)));
        }

        if self.enable_simulation.load(Ordering::Relaxed) {
            match self.simulate_transaction_for_execution(&transaction).await {
                Ok(_) => info!("Pre-flight simulation successful for opportunity ID: {}.", opportunity.id),
                Err(e) => {
                    error!("Pre-flight simulation failed for opportunity ID: {}: {:?}", opportunity.id, e);
                    self.record_token_pair_failure(&opportunity.id, &format!("SimulationFailure: {:?}", e));
                    return Err(e);
                }
            }
        }
        
        let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(), |ha_client| ha_client.primary_client.clone(),
        );

        match rpc_to_use.send_and_confirm_transaction_with_spinner(&transaction).await {
            Ok(signature) => {
                let elapsed = start_time.elapsed();
                info!("Arbitrage executed successfully for opportunity ID {} in {:?}: {}", opportunity.id, elapsed, signature);
                Ok(signature)
            }
            Err(e) => {
                error!("Failed to execute arbitrage for opportunity ID {}: {}", opportunity.id, e);
                self.record_token_pair_failure(&opportunity.id, &e.to_string());
                Err(ArbError::TransactionError(e.to_string()))
            }
        }
    }

    fn build_instructions_from_multihop(&self, opportunity: &MultiHopArbOpportunity) -> Result<Vec<Instruction>, ArbError> {
        let mut instructions = Vec::new();
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(1_400_000));
        instructions.push(ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee));

        warn!("Executing build_instructions_from_multihop STUB for opportunity ID: {}. Only compute budget instructions will be added.", opportunity.id);
        if opportunity.hops.is_empty() {
            error!("Opportunity ID: {} has no hops defined. Cannot build instructions.", opportunity.id);
            return Err(ArbError::ExecutionError("No hops in opportunity".to_string()));
        }
        // *** Placeholder for actual instruction building logic based on opportunity.hops ***
        for hop in &opportunity.hops {
             info!("STUB: Would build instruction for Hop: DEX {:?}, Pool {}, {} -> {}",
                hop.dex, hop.pool, hop.input_token, hop.output_token
            );
        }
        
        if instructions.len() <= 2 && !opportunity.hops.is_empty() { // Only compute budget instructions but hops exist
             warn!("No swap instructions were generated for opportunity ID: {}. Hops found: {}", opportunity.id, opportunity.hops.len());
             return Err(ArbError::ExecutionError(format!("No swap instructions generated for opportunity {}", opportunity.id )));
        }
        Ok(instructions)
    }

    fn record_token_pair_failure(&self, opportunity_id: &str, reason: &str) {
        let mut entry = self.recent_failures.entry(opportunity_id.to_string()).or_insert((Instant::now(), 0));
        entry.0 = Instant::now();
        entry.1 += 1;
        warn!("Recorded failure for opportunity ID: {}. Reason: {}. Total failures for this ID: {}", opportunity_id, reason, entry.1);
        if entry.1 > 5 {
            warn!("Opportunity ID {} has failed {} times. Consider temporary ban.", opportunity_id, entry.1);
        }
    }

    async fn get_latest_blockhash_with_ha(&self) -> Result<Hash, ArbError> {
        let client_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(),
            |ha_client| ha_client.primary_client.clone(),
        );
        client_to_use.get_latest_blockhash().await.map_err(|e| ArbError::RpcError(e.to_string()))
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
            encoding: Some(UiTransactionEncoding::Base64), // Corrected: UiTransactionEncoding
            accounts: None,
            min_context_slot: None,
            inner_instructions: None, // Added missing field
        };

        match client_to_use.simulate_transaction_with_config(transaction, sim_config).await {
            Ok(sim_response) => {
                if let Some(err) = &sim_response.value.err {
                    error!("Transaction simulation failed: {:?}", err);
                    Err(ArbError::SimulationFailed(format!("{:?}", err)))
                } else {
                    info!("Transaction simulation successful. Logs: {:?}", sim_response.value.logs.as_deref().unwrap_or_default());
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