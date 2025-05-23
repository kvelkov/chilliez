use crate::arbitrage::opportunity::MultiHopArbOpportunity; // Using the unified struct
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient; // For High-Availability RPC
use crate::utils::{DexType, PoolInfo, TokenAmount}; // TokenAmount might be needed for amounts in instructions
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

// Circuit breakers remain unchanged
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

// default_retry_policy remains unchanged
fn default_retry_policy() -> RetryPolicy {
    RetryPolicy::new(3, 500, 10_000, 0.3)
}

pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    // This rpc_client is the direct NonBlockingRpcClient, used as a fallback or for specific calls.
    rpc_client: Arc<NonBlockingRpcClient>,
    priority_fee: u64,
    max_timeout: Duration,
    // simulation_mode is for higher-level logic (e.g., engine deciding not to send to executor)
    // paper_trading_mode is specifically for this executor to log instead of sending.
    simulation_mode: bool, // General flag, if true, executor might still simulate before "paper trading"
    paper_trading_mode: bool, // If true, logs as paper trade, doesn't send to network
    network_congestion: AtomicU64,
    solana_rpc: Option<Arc<SolanaRpcClient>>, // Preferred HA RPC client
    enable_simulation: AtomicBool,            // Pre-flight simulation before sending live
    recent_failures: Arc<dashmap::DashMap<String, (Instant, u32)>>, // Tracks failures per opportunity ID
    degradation_mode: AtomicBool, // Not directly used by executor but part of system state
    degradation_profit_threshold: f64, // Not directly used by executor
}

impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<NonBlockingRpcClient>, // Fallback/direct client
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool,    // Overall simulation flag from config
        paper_trading_mode: bool, // Specific paper trading flag from config
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
            paper_trading_mode,
            network_congestion: AtomicU64::new(100), // Default: 1.0 congestion factor
            solana_rpc: None,                        // To be set with with_solana_rpc
            enable_simulation: AtomicBool::new(true), // Enable pre-flight simulation by default
            recent_failures: Arc::new(dashmap::DashMap::new()),
            degradation_mode: AtomicBool::new(false),
            degradation_profit_threshold: 1.5, // Example threshold multiplier
        }
    }

    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    pub async fn update_network_congestion(&self) -> Result<()> {
        if let Some(solana_rpc_client) = &self.solana_rpc {
            let congestion_factor = solana_rpc_client.get_network_congestion_factor().await;
            // Store congestion factor, e.g., as u64 by multiplying (1.23 -> 123)
            let congestion_stored = (congestion_factor * 100.0).round() as u64;
            self.network_congestion
                .store(congestion_stored, Ordering::Relaxed);
            info!(
                "Updated network congestion factor: {:.2} (Stored: {})",
                congestion_factor, congestion_stored
            );
        } else {
            warn!("SolanaRpcClient (HA) not available for network congestion update. Congestion remains at last known value or default.");
        }
        Ok(())
    }

    // Updated to use MultiHopArbOpportunity, checks hops
    pub fn has_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        for hop in &opportunity.hops {
            // Assuming hop.input_token and hop.output_token are symbols
            if crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(
                &hop.input_token,
                &hop.output_token,
            ) || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(
                &hop.input_token,
                &hop.output_token,
            ) {
                return true;
            }
        }
        false
    }

    // Kept for distinct multi-hop specific logic if any, though `has_banned_tokens` now iterates hops.
    pub fn has_multihop_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        self.has_banned_tokens(opportunity) // Delegates to the more generic hop-iterating version
    }

    // Main execution entry point, renamed to avoid conflict with legacy.
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
            "Attempting to execute arbitrage opportunity ID: {} | Path: {:?} -> {:?} | Expected Profit: {:.4}%",
            opportunity.id,
            opportunity.input_token, // Symbol
            opportunity.output_token, // Symbol
            opportunity.profit_pct
        );
        opportunity.log_summary();

        // *** CRITICAL STUB START ***
        let instructions = match self.build_instructions_from_multihop(opportunity) {
            Ok(instr) => instr,
            Err(e) => {
                error!(
                    "Failed to build instructions for opportunity ID {}: {}",
                    opportunity.id, e
                );
                return Err(ArbError::ExecutionError(format!(
                    "Instruction building failed: {}",
                    e
                )));
            }
        };
        // *** CRITICAL STUB END ***
        // If instructions only contains compute budget, it means no swap instructions were added.
        if instructions.len() <= 2 && !self.paper_trading_mode && !self.simulation_mode {
            // Compute budget adds 2
            error!("No actual swap instructions were built for opportunity ID: {}. Aborting execution.", opportunity.id);
            return Err(ArbError::ExecutionError(
                "No swap instructions built".to_string(),
            ));
        }

        if self.paper_trading_mode {
            info!(
                "[PAPER TRADING] Simulating execution for opportunity ID: {}. {} instructions.",
                opportunity.id,
                instructions.len()
            );
            let xml_entry = format!(
                "<trade>\n  <timestamp>{}</timestamp>\n  <id>{}</id>\n  <input_token>{}</input_token>\n  <output_token>{}</output_token>\n  <profit_percentage>{:.4}</profit_percentage>\n  <input_amount>{:.6}</input_amount>\n  <expected_output>{:.6}</expected_output>\n  <dex_path>{:?}</dex_path>\n  <result>paper_traded</result>\n</trade>\n",
                chrono::Utc::now().to_rfc3339(), opportunity.id, opportunity.input_token, opportunity.output_token,
                opportunity.profit_pct, opportunity.input_amount, opportunity.expected_output, opportunity.dex_path
            );
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open("paper_trades.xml")
            {
                if let Err(e) = file.write_all(xml_entry.as_bytes()) {
                    warn!("Failed to write to paper_trades.xml: {}", e);
                }
            } else {
                warn!("Failed to open paper_trades.xml.");
            }
            return Ok(Signature::default()); // Return dummy signature for paper trading
        }

        if self.simulation_mode && !self.paper_trading_mode {
            // General simulation if not paper trading
            info!(
                "[SIMULATION MODE] Simulating transaction for opportunity ID: {}. {} instructions.",
                opportunity.id,
                instructions.len()
            );
            // Here you might call self.simulate_transaction with a fully formed dummy transaction
            // For now, just log and return a placeholder
            return Ok(Signature::new_unique()); // Placeholder signature
        }

        // Proceed with live execution if not paper trading or general simulation mode
        let recent_blockhash = self.get_latest_blockhash_with_ha().await?;

        // Instructions should already include compute budget from build_instructions_from_multihop
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet], // Wallet needs to be Deref<Target = Keypair> or similar
            recent_blockhash,
        );

        if start_time.elapsed() > self.max_timeout {
            return Err(ArbError::TimeoutError(format!(
                "Transaction preparation exceeded timeout for opportunity ID: {}",
                opportunity.id
            )));
        }

        // Pre-flight simulation if enabled
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
                    self.record_token_pair_failure(&opportunity.id, "SimulationFailure");
                    return Err(e);
                }
            }
        }

        // Use HA client for sending if available, else fallback
        let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(),
            |ha_client| ha_client.primary_client.clone(),
        );

        match rpc_to_use
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await
        {
            Ok(signature) => {
                let elapsed = start_time.elapsed();
                info!(
                    "Arbitrage executed successfully for opportunity ID {} in {:?}: {}",
                    opportunity.id, elapsed, signature
                );
                // self.record_token_pair_success(&opportunity.input_token, &opportunity.output_token); // Needs symbol properties
                Ok(signature)
            }
            Err(e) => {
                error!(
                    "Failed to execute arbitrage for opportunity ID {}: {}",
                    opportunity.id, e
                );
                self.record_token_pair_failure(&opportunity.id, &e.to_string());
                Err(ArbError::TransactionError(e.to_string()))
            }
        }
    }

    // Placeholder for building instructions. THIS IS CRITICAL AND NEEDS FULL IMPLEMENTATION.
    fn build_instructions_from_multihop(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, ArbError> {
        let mut instructions = Vec::new();
        // Add Compute Budget Instructions first for all transactions
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)); // Example limit
        instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
            self.priority_fee,
        ));

        warn!("Executing build_instructions_from_multihop STUB for opportunity ID: {}. Only compute budget instructions will be added.", opportunity.id);
        if opportunity.hops.is_empty() && instructions.len() <= 2 {
            //This case should ideally be caught before calling execute, or build_instructions should ensure hops are present.
            error!(
                "Opportunity ID: {} has no hops defined. Cannot build instructions.",
                opportunity.id
            );
            return Err(ArbError::ExecutionError(
                "No hops in opportunity".to_string(),
            ));
        }

        for hop_idx in 0..opportunity.hops.len() {
            let hop = &opportunity.hops[hop_idx];
            // ### ACTUAL INSTRUCTION BUILDING LOGIC NEEDED HERE ###
            // This would involve:
            // 1. Identifying the DEX program ID from `hop.dex`.
            // 2. Getting the user's token accounts for `hop.input_token` (mint) and `hop.output_token` (mint).
            //    - This requires knowing the mint Pubkeys for the symbols in `hop.input_token` / `hop.output_token`.
            //    - The `MultiHopArbOpportunity` needs to carry mint info or `hop.input_token`/`output_token` should be mint addresses.
            //    - Create ATAs if they don't exist (though for arbitrage, they usually should).
            // 3. Getting pool-specific accounts (e.g., pool's token vaults, LP mint, authority).
            // 4. Constructing the swap instruction data according to that DEX's protocol.
            // Example (purely conceptual):
            // let (input_token_mint_pubkey, output_token_mint_pubkey) = self.get_mints_for_hop(hop)?;
            // let user_input_ata = spl_associated_token_account::get_associated_token_address(&self.wallet.pubkey(), &input_token_mint_pubkey);
            // let user_output_ata = spl_associated_token_account::get_associated_token_address(&self.wallet.pubkey(), &output_token_mint_pubkey);
            // ... fetch other accounts for pool `hop.pool` ...
            // let swap_ix = dex_specific_instruction_builder(
            //     hop.dex_program_id, // Need this
            //     user_input_ata,
            //     user_output_ata,
            //     hop.pool_accounts, // Need these
            //     hop.input_amount_atomic, // Need atomic amount
            //     hop.min_expected_output_atomic, // Need atomic amount
            // );
            // instructions.push(swap_ix);
            info!(
                "STUB: Would build instruction for Hop {}: DEX {:?}, Pool {}, {} {:.6} -> {} {:.6}",
                hop_idx + 1,
                hop.dex,
                hop.pool,
                hop.input_token,
                hop.input_amount,
                hop.output_token,
                hop.expected_output
            );
        }

        // If only compute budget instructions are present, it means no useful work will be done.
        if instructions.len() <= 2 && !opportunity.hops.is_empty() {
            warn!("No swap instructions were generated for opportunity ID: {}. Check DEX implementation for hops. Hops found: {}", opportunity.id, opportunity.hops.len());
            // Depending on strictness, you might return an error here.
            // For now, allowing it to proceed if in sim/paper mode, but real execution would fail or do nothing.
            return Err(ArbError::ExecutionError(format!(
                "No swap instructions generated for opportunity {}",
                opportunity.id
            )));
        }

        Ok(instructions)
    }

    // Removed add_priority_fee as it's integrated into build_instructions_from_multihop

    fn record_token_pair_failure(&self, opportunity_id: &str, reason: &str) {
        let mut entry = self
            .recent_failures
            .entry(opportunity_id.to_string())
            .or_insert((Instant::now(), 0));
        entry.0 = Instant::now();
        entry.1 += 1;
        warn!(
            "Recorded failure for opportunity ID: {}. Reason: {}. Total failures for this ID: {}",
            opportunity_id, reason, entry.1
        );
        // Implement banning logic if entry.1 exceeds a threshold
        if entry.1 > 5 {
            // Example threshold
            warn!(
                "Opportunity ID {} has failed {} times. Consider temporary ban.",
                opportunity_id, entry.1
            );
            // Here you might call ArbitrageDetector::log_banned_pair for the tokens involved in `opportunity_id`
            // This requires `opportunity_id` to be parsable back to token pairs or the opportunity object itself.
        }
    }

    async fn get_latest_blockhash_with_ha(&self) -> Result<Hash, ArbError> {
        let client_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(), // Fallback to direct non-blocking
            |ha_client| ha_client.primary_client.clone(), // Assuming primary_client is Arc<NonBlockingRpcClient>
        );
        client_to_use
            .get_latest_blockhash()
            .await
            .map_err(|e| ArbError::RpcError(e.to_string()))
    }

    // Renamed to avoid conflict with the one that takes instructions, payer
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
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            accounts: None,
            min_context_slot: None,
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

    // sign_and_send_transaction_with_retries is a good utility but might be too low-level
    // if execute_opportunity handles the transaction lifecycle.
    // If kept, ensure it's used appropriately or integrated into execute_opportunity's retry logic.
    // For now, commenting out its direct usage from main, as execute_opportunity is the primary path.
    #[allow(dead_code)]
    pub async fn sign_and_send_transaction_with_retries(
        &self,
        instructions: &[Instruction],
        signers: &[&Keypair],
        max_retries: u32,
    ) -> Result<Signature, ArbError> {
        if self.paper_trading_mode {
            info!(
                "[Paper Trading] Simulating sign_and_send for instructions count: {}",
                instructions.len()
            );
            return Ok(Signature::default());
        }

        let mut attempts = 0;
        loop {
            attempts += 1;
            if attempts > max_retries {
                return Err(ArbError::TransactionError(format!(
                    "Max retries ({}) exceeded for sending transaction.",
                    max_retries
                )));
            }

            let latest_blockhash = self.get_latest_blockhash_with_ha().await?;

            // Instructions should already include compute budget if prepared by build_instructions_from_multihop
            // If not, add them here. For consistency, better to have build_instructions_from_multihop do it.
            // let mut final_instructions = vec![
            //     ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            //     ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee),
            // ];
            // final_instructions.extend_from_slice(instructions);

            let payer_pubkey = signers
                .get(0)
                .ok_or_else(|| {
                    ArbError::TransactionError("No payer keypair provided.".to_string())
                })?
                .pubkey();
            let mut transaction = Transaction::new_with_payer(instructions, Some(&payer_pubkey));

            // Signers must include the payer.
            // The `sign` method on Transaction expects a slice of `&dyn Signer` (Keypair implements Signer)
            transaction.sign(signers, latest_blockhash);

            let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
                || self.rpc_client.clone(),
                |ha_client| ha_client.primary_client.clone(),
            );

            match rpc_to_use
                .send_and_confirm_transaction_with_spinner(&transaction)
                .await
            {
                Ok(signature) => return Ok(signature),
                Err(e) => {
                    if e.to_string().contains("Blockhash not found")
                        || e.to_string().contains("Transaction version too old")
                    {
                        warn!(
                            "Retrying transaction due to blockhash issue (Attempt {}/{}): {}",
                            attempts, max_retries, e
                        );
                        tokio::time::sleep(Duration::from_millis(500 + (attempts as u64 * 200)))
                            .await; // Incremental backoff
                        continue;
                    }
                    return Err(ArbError::TransactionError(format!(
                        "Failed to send transaction: {}",
                        e
                    )));
                }
            }
        }
    }
}