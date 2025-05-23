use crate::arbitrage::calculator::OpportunityCalculationResult;
use crate::arbitrage::detector::ArbitrageOpportunity;
use crate::arbitrage::fee_manager::FeeEstimationResult;
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::error::{ArbError, CircuitBreaker, RetryPolicy};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, TokenAmount};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer}, // Added Signature
    transaction::Transaction,
};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

// Circuit breakers for different components
lazy_static::lazy_static! {
    static ref RPC_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "rpc",
        5,  // 5 errors
        3,  // 3 consecutive successes to reset
        Duration::from_secs(60),  // 60 second window
        Duration::from_secs(300), // 5 minute reset timeout
    );

    static ref DEX_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "dex",
        3,  // 3 errors
        2,  // 2 consecutive successes to reset
        Duration::from_secs(30),  // 30 second window
        Duration::from_secs(120), // 2 minute reset timeout
    );

    static ref EXECUTION_CIRCUIT_BREAKER: CircuitBreaker = CircuitBreaker::new(
        "execution",
        3,  // 3 errors
        5,  // 5 consecutive successes to reset
        Duration::from_secs(60),  // 60 second window
        Duration::from_secs(300), // 5 minute reset timeout
    );
}

// Default retry policy
fn default_retry_policy() -> RetryPolicy {
    RetryPolicy::new(
        3,      // max 3 attempts
        500,    // 500ms base delay
        10_000, // 10 second max delay
        0.3,    // 30% jitter
    )
}

/// Executes an arbitrage trade between a pair of pools (legacy/simple, not async, not used in main pipeline)
pub fn execute(
    pair: &(Pubkey, Pubkey),
    calc_result: &OpportunityCalculationResult,
    fee_result: &FeeEstimationResult,
) -> Result<(), String> {
    info!(
        "Executing arbitrage between pools {:?} with expected profit ${:.2} ({}%) and fees ${:.2}",
        pair,
        calc_result.profit,
        calc_result.profit_percentage * 100.0,
        fee_result.total_cost
    );
    Ok(())
}

/// Executor for arbitrage opportunities
#[allow(dead_code)]
pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
    priority_fee: u64,
    max_timeout: Duration,
    simulation_mode: bool,
    network_congestion: AtomicU64,
    solana_rpc: Option<Arc<SolanaRpcClient>>,
    enable_simulation: AtomicBool,
    recent_failures: Arc<dashmap::DashMap<String, (Instant, u32)>>,
    degradation_mode: AtomicBool,
    degradation_profit_threshold: f64,
}

#[allow(dead_code)]
impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>,
        rpc_client: Arc<RpcClient>,
        priority_fee: u64,
        max_timeout: Duration,
        simulation_mode: bool,
    ) -> Self {
        Self {
            wallet,
            rpc_client,
            priority_fee,
            max_timeout,
            simulation_mode,
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

    pub async fn update_network_congestion(&self) -> Result<()> {
        if let Some(solana_rpc) = &self.solana_rpc {
            let congestion = solana_rpc.get_network_congestion_factor().await;
            let congestion_fixed = (congestion * 100.0).round() as u64;
            self.network_congestion
                .store(congestion_fixed, Ordering::Relaxed);
            info!("Updated network congestion factor: {:.2}", congestion);
        }
        Ok(())
    }

    pub fn has_banned_tokens(&self, opportunity: &ArbitrageOpportunity) -> bool {
        let token_a = opportunity.source_pool.token_a.symbol.as_str();
        let token_b = opportunity.source_pool.token_b.symbol.as_str();

        crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(token_a, token_b)
            || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(
                token_a, token_b,
            )
    }

    pub fn has_multihop_banned_tokens(&self, opportunity: &MultiHopArbOpportunity) -> bool {
        for hop in &opportunity.hops {
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

    pub async fn execute(&self, opportunity: ArbitrageOpportunity) -> Result<String> {
        if self.has_banned_tokens(&opportunity) {
            return Err(anyhow!("Opportunity involves banned token pair"));
        }

        let start_time = Instant::now();
        info!(
            "Executing arbitrage: {} -> {} ({}% profit)",
            opportunity.source_pool.name,
            opportunity.target_pool.name,
            opportunity.profit_percentage
        );

        let instructions = self.build_instructions(&opportunity)?;

        if self.simulation_mode {
            info!(
                "SIMULATION: Would execute transaction with {} instructions",
                instructions.len()
            );
            let xml_entry = format!(
                "<trade>\n  <timestamp>{}</timestamp>\n  <source_pool>{}</source_pool>\n  <target_pool>{}</target_pool>\n  <profit_percentage>{:.4}</profit_percentage>\n  <input_amount>{:?}</input_amount>\n  <expected_output>{:?}</expected_output>\n  <result>simulated</result>\n</trade>\n",
                chrono::Utc::now().to_rfc3339(),
                opportunity.source_pool.name,
                opportunity.target_pool.name,
                opportunity.profit_percentage,
                opportunity.input_amount,
                opportunity.expected_output,
            );
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open("simulated_trades.xml")?;
            file.write_all(xml_entry.as_bytes())?;
            return Ok("simulation-txid-placeholder".to_string());
        }

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
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
            return Err(anyhow!("Transaction preparation exceeded timeout"));
        }

        if self.enable_simulation.load(Ordering::Relaxed) {
            self.simulate_transaction(&transaction_with_fee).await?;
        }

        match self
            .rpc_client
            .send_and_confirm_transaction(&transaction_with_fee)
            .await
        {
            Ok(signature) => {
                let elapsed = start_time.elapsed();
                info!(
                    "Arbitrage executed successfully in {:?}: {}",
                    elapsed, signature
                );
                self.record_token_pair_success(
                    opportunity.source_pool.token_a.symbol.as_str(),
                    opportunity.source_pool.token_b.symbol.as_str(),
                );
                Ok(signature.to_string())
            }
            Err(e) => {
                error!("Failed to execute arbitrage: {}", e);
                Err(anyhow!("Transaction failed: {}", e))
            }
        }
    }

    /// Simulates a transaction without sending it to the network.
    pub async fn simulate_transaction(&self, instructions: &[Instruction], payer: &Keypair) -> Result<solana_client::rpc_response::RpcSimulateTransactionResult, ArbError> {
        let latest_blockhash = self.get_latest_blockhash().await?;

        // It's good practice to include a compute budget request for simulation
        // as it can affect execution and resource usage.
        let mut final_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), // Default limit
            ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee),
        ];
        final_instructions.extend_from_slice(instructions);

        let message = Message::new(&final_instructions, Some(&payer.pubkey()));
        let transaction = Transaction::new_unsigned(message);
        
        self.rpc_client.simulate_transaction(&transaction).await
    }


    async fn get_latest_blockhash(&self) -> Result<Hash, ArbError> {
        self.rpc_client.get_latest_blockhash().await
    }
    
    // Helper to sign and send a transaction, with retries for blockhash expiration.
    // This is a simplified version. Production systems might need more sophisticated retry logic.
    pub async fn sign_and_send_transaction_with_retries(
        &self,
        instructions: &[Instruction],
        signers: &[&Keypair], // Expecting a slice of Keypair references
        max_retries: u32,     // Max retries for blockhash issues
    ) -> Result<Signature, ArbError> {
        if self.paper_trading_mode {
            info!("[Paper Trading] Simulating transaction send for instructions: {:?}", instructions.iter().map(|ix| ix.program_id.to_string()).collect::<Vec<_>>());
            // In paper trading, we might still want to simulate to catch obvious errors.
            // For now, just returning a dummy signature.
            return Ok(Signature::default());
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

            let latest_blockhash = self.get_latest_blockhash().await.map_err(|e| {
                ArbError::SolanaRpcError(format!("Retry {}: Failed to get latest blockhash: {}", attempts, e))
            })?;
            
            let mut final_instructions = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), // Example limit
                ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee),
            ];
            final_instructions.extend_from_slice(instructions);


            // The payer is typically the first signer
            let payer_pubkey = signers.get(0).ok_or_else(|| ArbError::TransactionError("No payer provided for transaction".to_string()))?.pubkey();
            let message = Message::new(&final_instructions, Some(&payer_pubkey));
            let mut transaction = Transaction::new_with_payer(&final_instructions, Some(&payer_pubkey));


            // Sign the transaction
            // The `sign` method on Transaction expects a slice of `&dyn Signer`
            // and the blockhash.
            transaction.sign(signers, latest_blockhash);


            match self.rpc_client.send_and_confirm_transaction(&transaction).await {
                Ok(signature) => return Ok(signature),
                Err(e) => {
                    // Check if the error is related to blockhash expiration.
                    // This is a simplified check; actual error messages/codes might vary.
                    if e.to_string().contains("Blockhash not found") || e.to_string().contains("Transaction version too old") {
                        warn!(
                            "Transaction failed due to blockhash issue (Attempt {}/{}): {}. Retrying...",
                            attempts, max_retries, e
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await; // Wait before retrying
                        continue; // Retry the loop to get a new blockhash
                    }
                    // For other errors, return immediately.
                    return Err(ArbError::TransactionError(format!(
                        "Failed to send transaction: {}",
                        e
                    )));
                }
            }
        }
    }


    /// Executes a prepared arbitrage opportunity.
    pub async fn execute_arbitrage_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, ArbError> {
        info!(
            "Attempting to execute arbitrage opportunity: Input: {} {}, Output: {} {}, Profit: {:.4}%, Dex Path: {:?}",
            opportunity.input_amount,
            opportunity.input_token_mint,
            opportunity.expected_output_amount,
            opportunity.output_token_mint,
            opportunity.profit_pct,
            opportunity.dex_path.iter().map(|d| d.get_name()).collect::<Vec<_>>()
        );

        if self.paper_trading_mode {
            info!("[Paper Trading] Simulating execution of opportunity ID: {}", opportunity.id);
            // Simulate profit/loss logging if
        }
        // TODO: Add actual paper trading logic if needed, e.g., logging to a file or database.
        Ok(Signature::default()) // Return a dummy signature for paper trading
    }
}
