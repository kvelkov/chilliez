use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;
// use crate::utils::{DexType, PoolInfo, TokenAmount}; // Not directly used here after changes
use anyhow::Result as AnyhowResult; // Keep if used for non-ArbError results, else remove
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    // message::Message, // Used via Transaction::new_signed_with_payer
    // pubkey::Pubkey, // Used via Keypair or specific imports
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
    account_decoder::UiAccountEncoding, // Needed for RpcSimulateTransactionConfig
    transaction_status::UiTransactionEncoding, // Correct type for RpcSimulateTransactionConfig.encoding
};
// Removed: use solana_sdk::commitment_config::CommitmentLevel; // Not directly used
// Removed: use solana_sdk::transaction:: TransactionError as SolanaTransactionError; // Not directly used

// Removed: use std::fs::OpenOptions; // Not used
// Removed: use std::io::Write; // Not used
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

lazy_static::lazy_static! { /* ... same ... */ }
fn default_retry_policy() -> RetryPolicy { /* ... same ... */ RetryPolicy::new(3,500,10_000,0.3) }

pub struct ArbitrageExecutor { /* ... same fields ... */
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
    ) -> Self { /* ... same as before ... */ Self { wallet, rpc_client, priority_fee, max_timeout, simulation_mode, paper_trading_mode, network_congestion: AtomicU64::new(100), solana_rpc: None, enable_simulation: AtomicBool::new(true), recent_failures: Arc::new(dashmap::DashMap::new()), degradation_mode: AtomicBool::new(false), degradation_profit_threshold: 1.5 } }
    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self { /* ... same as before ... */ self.solana_rpc = Some(solana_rpc); self}
    pub async fn update_network_congestion(&self) -> Result<(), ArbError> { /* ... same as before ... */ Ok(())}
    pub fn has_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool { /* ... same as before ... */ false}
    pub fn has_multihop_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool { /* ... same as before ... */ false}

    pub async fn execute_opportunity(&self, opportunity: &MultiHopArbOpportunity) -> Result<Signature, ArbError> {
        if self.has_banned_tokens(opportunity) {
            warn!("Opportunity ID: {} involves a banned token pair. Skipping.", opportunity.id);
            return Err(ArbError::ExecutionError("Opportunity involves banned token pair".to_string()));
        }

        let start_time = Instant::now();
        info!( /* ... */ );
        opportunity.log_summary();

        let instructions = match self.build_instructions_from_multihop(opportunity) {
            Ok(instr) => instr,
            Err(e) => {
                error!("Failed to build instructions for opportunity ID {}: {}", opportunity.id, e);
                return Err(e);
            }
        };
        
        if instructions.len() <= 2 && !self.paper_trading_mode && !self.simulation_mode {
             error!("No actual swap instructions were built for opportunity ID: {}. Aborting execution.", opportunity.id);
             return Err(ArbError::ExecutionError("No swap instructions built".to_string()));
        }

        if self.paper_trading_mode { /* ... same ... */ return Ok(Signature::default()); }
        if self.simulation_mode && !self.paper_trading_mode { /* ... same ... */ return Ok(Signature::new_unique()); }

        let recent_blockhash = self.get_latest_blockhash_with_ha().await?;
        let transaction = Transaction::new_signed_with_payer(
            &instructions, Some(&self.wallet.pubkey()), &[&*self.wallet], recent_blockhash,
        );

        if start_time.elapsed() > self.max_timeout { /* ... */ }

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
            Ok(signature) => { /* ... */ Ok(signature) }
            Err(e) => { /* ... */ Err(ArbError::TransactionError(e.to_string())) }
        }
    }

    fn build_instructions_from_multihop(&self, opportunity: &MultiHopArbOpportunity) -> Result<Vec<Instruction>, ArbError> { /* ... same stub ... */ Ok(vec![])}
    fn record_token_pair_failure(&self, opportunity_id: &str, reason: &str) { /* ... same ... */ }
    async fn get_latest_blockhash_with_ha(&self) -> Result<Hash, ArbError> { /* ... same ... */ Err(ArbError::Unknown("todo".to_string())) }
    
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
            encoding: Some(UiTransactionEncoding::Base64), // Corrected
            accounts: None,
            min_context_slot: None,
            inner_instructions: Some(false), // Added and set to a default
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