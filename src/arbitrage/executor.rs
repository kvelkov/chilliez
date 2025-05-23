use crate::arbitrage::calculator::OpportunityCalculationResult;
// use crate::arbitrage::detector::ArbitrageOpportunity; // Removed, using MultiHopArbOpportunity
use crate::arbitrage::fee_manager::FeeEstimationResult;
use crate::arbitrage::opportunity::MultiHopArbOpportunity; // Using the unified struct
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, TokenAmount}; // Assuming PoolInfo and TokenAmount might be needed for instruction building
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient; // Aliased to avoid conflict
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

// Circuit breakers (remains unchanged)
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

// Legacy execute function - if kept, should also use MultiHopArbOpportunity or be removed
#[allow(dead_code)]
pub fn execute_legacy( // Renamed to avoid conflict with async execute
    _pair: &(Pubkey, Pubkey), // Typically derived from opportunity
    _calc_result: &OpportunityCalculationResult,
    _fee_result: &FeeEstimationResult,
) -> Result<(), String> {
    info!(
        "Executing legacy arbitrage with expected profit ${:.2} ({}%) and fees ${:.2}",
        _calc_result.profit,
        _calc_result.profit_percentage * 100.0,
        _fee_result.total_cost
    );
    Ok(())
}

pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>, // Changed to non-blocking
    priority_fee: u64,
    max_timeout: Duration,
    simulation_mode: bool,
    network_congestion: AtomicU64,
    solana_rpc: Option<Arc<SolanaRpcClient>>, // High-availability client
    enable_simulation: AtomicBool,
    recent_failures: Arc<dashmap::DashMap<String, (Instant, u32)>>,
    degradation_mode: AtomicBool,
    degradation_profit_threshold: f64,
    paper_trading_mode: bool, // Added field
}

impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<NonBlockingRpcClient>, // Changed to non-blocking
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool, // This is general simulation, distinct from paper_trading_mode
        paper_trading_mode: bool, // Added parameter
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
            network_congestion: AtomicU64::new(100),
            solana_rpc: None,
            enable_simulation: AtomicBool::new(true), // Default behavior for pre-flight checks
            recent_failures: Arc::new(dashmap::DashMap::new()),
            degradation_mode: AtomicBool::new(false),
            degradation_profit_threshold: 1.5, // Example: 1.5x normal profit threshold
            paper_trading_mode, // Initialize field
        }
    }

    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    pub async fn update_network_congestion(&self) -> Result<()> {
        if let Some(solana_rpc_client) = &self.solana_rpc { // Use the HA client if available
            let congestion = solana_rpc_client.get_network_congestion_factor().await;
            let congestion_fixed = (congestion * 100.0).round() as u64;
            self.network_congestion.store(congestion_fixed, Ordering::Relaxed);
            info!("Updated network congestion factor: {:.2}", congestion);
        } else { // Fallback to direct client if HA client is not configured
            // This part would need a similar method on the NonBlockingRpcClient or a helper
            warn!("HA SolanaRpcClient not configured for network congestion update. Using default RPC client (if implemented).");
            // Example placeholder for direct client, assuming a similar method or default value
            // let congestion = self.rpc_client.get_congestion_factor_somehow().await;
            // self.network_congestion.store(congestion, Ordering::Relaxed);
        }
        Ok(())
    }
    
    // Updated to use MultiHopArbOpportunity
    pub fn has_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        // If it's a simple 2-hop represented in MultiHopArbOpportunity
        if opportunity.hops.len() <= 2 { // Simple check, might need refinement
            let token_a_symbol = &opportunity.source_pool.token_a.symbol;
            let token_b_symbol = &opportunity.source_pool.token_b.symbol;
            if crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(token_a_symbol, token_b_symbol)
                || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(token_a_symbol, token_b_symbol)
            {
                return true;
            }
            if let Some(intermediate_mint) = opportunity.intermediate_token_mint { // Check intermediate if present
                 let intermediate_symbol = opportunity.intermediate_tokens.get(0).map(|s| s.as_str()).unwrap_or("");
                 if crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(token_b_symbol, intermediate_symbol)
                    || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(token_b_symbol, intermediate_symbol)
                 {
                     return true;
                 }
            }
        } else { // For longer multi-hop paths
            return self.has_multihop_banned_tokens(opportunity);
        }
        false
    }


    pub fn has_multihop_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        for hop in &opportunity.hops {
            if crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(
                &hop.input_token, // Assuming input_token is symbol
                &hop.output_token, // Assuming output_token is symbol
            ) || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(
                &hop.input_token,
                &hop.output_token,
            ) {
                return true;
            }
        }
        false
    }
    
    // This `execute` method processes the unified MultiHopArbOpportunity
    // For actual execution, it would primarily build instructions based on the `hops` vector.
    pub async fn execute_opportunity(&self, opportunity: &MultiHopArbOpportunity) -> Result<String> {
        if self.has_banned_tokens(opportunity) {
            warn!("Opportunity ID: {} involves a banned token pair. Skipping.", opportunity.id);
            return Err(anyhow!("Opportunity involves banned token pair"));
        }

        let start_time = Instant::now();
        info!(
            "Executing arbitrage opportunity ID: {} | Path: {:?} -> {:?} | Expected Profit: {:.4}%",
            opportunity.id,
            opportunity.input_token,
            opportunity.output_token,
            opportunity.profit_pct
        );
        opportunity.log_summary(); // Log detailed summary

        // Build instructions from opportunity.hops
        // This is a placeholder. Actual instruction building is complex and DEX-specific.
        let instructions = self.build_instructions_from_multihop(opportunity)?;

        if self.simulation_mode || self.paper_trading_mode {
            info!(
                "{} MODE: Would execute transaction with {} instructions for opportunity ID: {}",
                if self.paper_trading_mode { "PAPER TRADING" } else { "SIMULATION" },
                instructions.len(),
                opportunity.id
            );
             let xml_entry = format!(
                "<trade>\n  <timestamp>{}</timestamp>\n  <id>{}</id>\n  <input_token>{}</input_token>\n  <output_token>{}</output_token>\n  <profit_percentage>{:.4}</profit_percentage>\n  <input_amount>{:.6}</input_amount>\n  <expected_output>{:.6}</expected_output>\n  <dex_path>{:?}</dex_path>\n  <result>{}</result>\n</trade>\n",
                chrono::Utc::now().to_rfc3339(),
                opportunity.id,
                opportunity.input_token,
                opportunity.output_token,
                opportunity.profit_pct,
                opportunity.input_amount,
                opportunity.expected_output,
                opportunity.dex_path,
                if self.paper_trading_mode { "paper_traded" } else { "simulated" },
            );
            // Consider a more robust logging mechanism than direct file writes here
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(
                if self.paper_trading_mode { "paper_trades.xml" } else { "simulated_trades.xml" }
            ) {
                if let Err(e) = file.write_all(xml_entry.as_bytes()) {
                    warn!("Failed to write to trade log file: {}", e);
                }
            } else {
                 warn!("Failed to open trade log file.");
            }
            return Ok(format!("{}-txid-placeholder", if self.paper_trading_mode { "paper" } else { "simulated" }));
        }


        let recent_blockhash = self
            .get_latest_blockhash_with_ha().await // Use HA client for blockhash
            .map_err(|e| ArbError::RpcError(format!("Failed to get blockhash: {}", e)))?;

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        let transaction_with_fee = if self.priority_fee > 0 {
            self.add_priority_fee(transaction, self.priority_fee)?
        } else {
            transaction
        };

        if start_time.elapsed() > self.max_timeout {
            return Err(anyhow!("Transaction preparation exceeded timeout for opportunity ID: {}", opportunity.id));
        }

        // Use HA client for simulation and sending if available
        let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(), // Fallback to direct non-blocking
            |ha_client| ha_client.primary_client.clone(), // Assuming primary_client is Arc<NonBlockingRpcClient>
                                                          // Or, if SolanaRpcClient has its own send/simulate methods, use those directly.
        );


        if self.enable_simulation.load(Ordering::Relaxed) {
             // Simulate transaction
            let sim_config = RpcSimulateTransactionConfig {
                sig_verify: false, // Verification is done before sending actual tx
                replace_recent_blockhash: true, // Good for simulation
                commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
                encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                accounts: None,
                min_context_slot: None,
            };
            match rpc_to_use.simulate_transaction_with_config(&transaction_with_fee, sim_config).await {
                Ok(sim_response) => {
                    if let Some(err) = sim_response.value.err {
                        error!("Transaction simulation failed for opportunity ID {}: {:?}", opportunity.id, err);
                        return Err(ArbError::SimulationFailed(format!("{:?}", err)).into());
                    }
                    info!("Transaction simulation successful for opportunity ID {}: {:?}", opportunity.id, sim_response.value.logs);
                }
                Err(e) => {
                    error!("Error during transaction simulation for opportunity ID {}: {}", opportunity.id, e);
                    return Err(ArbError::SimulationFailed(e.to_string()).into());
                }
            }
        }

        match rpc_to_use.send_and_confirm_transaction_with_spinner(&transaction_with_fee).await {
            Ok(signature) => {
                let elapsed = start_time.elapsed();
                info!(
                    "Arbitrage executed successfully for opportunity ID {} in {:?}: {}",
                    opportunity.id, elapsed, signature
                );
                // Record success for specific token pairs if needed (complex for multi-hop)
                // self.record_token_pair_success(...);
                Ok(signature.to_string())
            }
            Err(e) => {
                error!("Failed to execute arbitrage for opportunity ID {}: {}", opportunity.id, e);
                // Record failure for specific token pairs if needed
                Err(ArbError::TransactionError(e.to_string()).into())
            }
        }
    }

    // Placeholder for building instructions from MultiHopArbOpportunity
    // This needs to be implemented based on specific DEX interactions for each hop.
    fn build_instructions_from_multihop(&self, opportunity: &MultiHopArbOpportunity) -> Result<Vec<Instruction>> {
        let mut instructions = Vec::new();
        // Add Compute Budget Instructions first
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)); // Example limit
        instructions.push(ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee));


        for hop in &opportunity.hops {
            // Here, you would translate each `ArbHop` into one or more `Instruction`
            // This requires knowing:
            // 1. The specific DEX program ID for `hop.dex`.
            // 2. The instruction format for that DEX's swap operation.
            // 3. Accounts required for the swap (e.g., user token accounts, pool accounts, authority).
            // This is highly DEX-specific and complex.
            warn!("Placeholder: Building instruction for hop: {:?} on DEX {:?}", hop, hop.dex);
            // Example (conceptual, not real):
            // let swap_instruction = match hop.dex {
            //     DexType::Orca => build_orca_swap_instruction(self.wallet.pubkey(), hop, ...),
            //     DexType::Raydium => build_raydium_swap_instruction(self.wallet.pubkey(), hop, ...),
            //     _ => return Err(anyhow!("Unsupported DEX type for instruction building: {:?}", hop.dex)),
            // };
            // instructions.push(swap_instruction);
        }
        if instructions.len() <= 2 { // Only compute budget instructions
             return Err(anyhow!("No swap instructions were built for opportunity ID: {}. Check DEX implementation for hops.", opportunity.id));
        }
        Ok(instructions)
    }


    fn add_priority_fee(&self, mut transaction: Transaction, fee: u64) -> Result<Transaction> {
        let mut instructions = vec![ComputeBudgetInstruction::set_compute_unit_price(fee)];
        instructions.extend(transaction.message.instructions.iter().cloned());
        let message = Message::new_with_compiled_instructions(
            transaction.message.header.num_required_signatures,
            transaction.message.header.num_readonly_signed_accounts,
            transaction.message.header.num_readonly_unsigned_accounts,
            transaction.message.account_keys.clone(),
            transaction.message.recent_blockhash,
            instructions,
        );
        transaction.message = message;
        Ok(transaction)
    }
    
    #[allow(dead_code)]
    fn record_token_pair_success(&self, _token_a_symbol: &str, _token_b_symbol: &str) {
        // Logic to track successful pairs, potentially reducing scrutiny or increasing confidence
    }

    #[allow(dead_code)]
    fn record_token_pair_failure(&self, opportunity_id: &str) {
        let mut entry = self.recent_failures.entry(opportunity_id.to_string()).or_insert((Instant::now(), 0));
        entry.0 = Instant::now(); // Update timestamp
        entry.1 += 1; // Increment failure count
        warn!("Recorded failure for opportunity ID: {}. Total failures for this ID: {}", opportunity_id, entry.1);
        // Potentially trigger temporary ban or increased scrutiny after N failures
    }


    async fn get_latest_blockhash_with_ha(&self) -> Result<Hash, solana_client::client_error::ClientError> {
        if let Some(ha_client) = &self.solana_rpc {
            // Prefer HA client if available
            // Assuming SolanaRpcClient has a method like get_latest_blockhash
            // For now, let's assume it uses its primary_client or similar logic internally
            ha_client.primary_client.get_latest_blockhash().await
        } else {
            self.rpc_client.get_latest_blockhash().await
        }
    }

    // Simulates a transaction without sending it to the network.
    // This method was defined in the original file with different parameters.
    // Consolidating and ensuring it uses the non-blocking client.
    pub async fn simulate_transaction(
        &self,
        transaction: &Transaction, // Changed from instructions and payer
    ) -> Result<solana_client::rpc_response::RpcSimulateTransactionResult, ArbError> {
        // Use the appropriate RPC client (HA or direct)
        let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
            || self.rpc_client.clone(),
            |ha_client| ha_client.primary_client.clone(), // Assuming primary_client is Arc<NonBlockingRpcClient>
        );

        let sim_config = RpcSimulateTransactionConfig {
            sig_verify: false, // Usually false for simulation
            replace_recent_blockhash: true, // Useful for simulation
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            accounts: None,
            min_context_slot: None,
        };

        match rpc_to_use.simulate_transaction_with_config(transaction, sim_config).await {
            Ok(sim_response) => {
                if let Some(err) = &sim_response.value.err {
                     error!("Transaction simulation failed: {:?}", err);
                     Err(ArbError::SimulationFailed(format!("{:?}", err)))
                } else {
                    info!("Transaction simulation successful. Logs: {:?}", sim_response.value.logs);
                    Ok(sim_response.value)
                }
            }
            Err(e) => {
                error!("Error during transaction simulation: {}", e);
                Err(ArbError::RpcError(e.to_string()))
            }
        }
    }
    
    pub async fn sign_and_send_transaction_with_retries(
        &self,
        instructions: &[Instruction],
        signers: &[&Keypair],
        max_retries: u32,
    ) -> Result<Signature, ArbError> {
        if self.paper_trading_mode {
            info!(
                "[Paper Trading] Simulating transaction send for instructions: {:?}",
                instructions.iter().map(|ix| ix.program_id.to_string()).collect::<Vec<_>>()
            );
            return Ok(Signature::default()); // Dummy signature for paper trading
        }

        let mut attempts = 0;
        loop {
            attempts += 1;
            if attempts > max_retries {
                return Err(ArbError::TransactionError(format!(
                    "Failed to send transaction after {} retries due to blockhash issues.",
                    max_retries
                )));
            }

            let latest_blockhash = self.get_latest_blockhash_with_ha().await.map_err(|e| {
                ArbError::SolanaRpcError(format!("Retry {}: Failed to get latest blockhash: {}", attempts, e))
            })?;
            
            let mut final_instructions = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
                ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee),
            ];
            final_instructions.extend_from_slice(instructions);

            let payer_pubkey = signers.get(0).ok_or_else(|| ArbError::TransactionError("No payer provided".to_string()))?.pubkey();
            
            let mut transaction = Transaction::new_with_payer(&final_instructions, Some(&payer_pubkey));
            transaction.sign(signers, latest_blockhash);

            // Use the appropriate RPC client (HA or direct)
            let rpc_to_use = self.solana_rpc.as_ref().map_or_else(
                || self.rpc_client.clone(),
                |ha_client| ha_client.primary_client.clone(), 
            );

            match rpc_to_use.send_and_confirm_transaction_with_spinner(&transaction).await {
                Ok(signature) => return Ok(signature),
                Err(e) => {
                    if e.to_string().contains("Blockhash not found") || e.to_string().contains("Transaction version too old") {
                        warn!("Transaction failed due to blockhash issue (Attempt {}/{}): {}. Retrying...", attempts, max_retries, e);
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    return Err(ArbError::TransactionError(format!("Failed to send transaction: {}", e)));
                }
            }
        }
    }

    // Aliased to avoid conflict with the more general `execute_opportunity`
    pub async fn execute_arbitrage_opportunity_specific( // Renamed
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, ArbError> {
        info!(
            "Attempting to execute specific arbitrage opportunity: Input: {} {}, Output: {} {}, Profit: {:.4}%, Dex Path: {:?}",
            opportunity.input_amount,
            opportunity.input_token_mint, // Changed from input_token
            opportunity.expected_output, // Changed from expected_output_amount
            opportunity.output_token_mint, // Changed from output_token
            opportunity.profit_pct,
            opportunity.dex_path // .iter().map(|d| d.get_name()).collect::<Vec<_>>() // Needs DexType to have get_name()
        );

        if self.paper_trading_mode {
            info!("[Paper Trading] Simulating execution of opportunity ID: {}", opportunity.id);
            // TODO: Add actual paper trading logic if needed
            Ok(Signature::default()) 
        } else {
            // This would call the actual transaction sending logic.
            // For now, it's a placeholder as instruction building is complex.
            warn!("Actual transaction execution for MultiHopArbOpportunity not fully implemented in this stub.");
            Err(ArbError::ExecutionError("Execution logic not fully implemented".to_string()))
        }
    }
}