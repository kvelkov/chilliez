use crate::arbitrage::calculator::OpportunityCalculationResult;
use crate::arbitrage::detector::ArbitrageOpportunity;
use crate::arbitrage::fee_manager::FeeEstimationResult;
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, TokenAmount};
use anyhow::{anyhow, Result};
use log::{error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

/// Executes an arbitrage trade between a pair of pools
#[allow(dead_code)]
pub fn execute(
    pair: &(Pubkey, Pubkey),
    calc_result: &OpportunityCalculationResult,
    fee_result: &FeeEstimationResult,
) -> Result<(), String> {
    // In a production implementation, this would:
    // 1. Prepare swap instructions for each pool in the pair
    // 2. Set appropriate compute budget and priority fee
    // 3. Create and sign the transaction
    // 4. Submit and monitor for confirmation

    // Log the execution for now (this would be replaced with real implementation)
    info!(
        "Executing arbitrage between pools {:?} with expected profit ${:.2} ({}%) and fees ${:.2}",
        pair,
        calc_result.profit,
        calc_result.profit_percentage * 100.0,
        fee_result.total_cost
    );

    // Here we would handle real transaction building and submission
    // For now just return success
    Ok(())
}

/// Executor for arbitrage opportunities
#[allow(dead_code)] // Used in integration/test, and as API for future arb orchestration expansion
pub struct ArbitrageExecutor {
    // Wallet used for transactions, needed for any signing in live trading/test
    wallet: Arc<Keypair>,
    // RPC client for transaction submission/monitoring, used in live workflows/tests
    rpc_client: Arc<RpcClient>,
    // Priority fee strategy, for latency-sensitive mainnet ops (future use/experimentation)
    priority_fee: u64,
    // Maximum transaction timeout, for backtest/simulation benchmarks and CI/latency research
    max_timeout: Duration,
    // Simulation mode (no real transactions), toggles pipeline for testnet/paper/live
    simulation_mode: bool,
    // Tracks last observed network congestion factor (multiplied by 100 for fixed-point)
    network_congestion: AtomicU64,
    // Solana RPC client for advanced operations like congestion monitoring
    solana_rpc: Option<Arc<SolanaRpcClient>>,
}

impl ArbitrageExecutor {
    #[allow(dead_code)]
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
            network_congestion: AtomicU64::new(100), // Initialize with default congestion factor (1.0 * 100)
            solana_rpc: None,                        // Optional Solana RPC client, can be set later
        }
    }

    /// Set the Solana RPC client for advanced operations
    pub fn with_solana_rpc(mut self, solana_rpc: Arc<SolanaRpcClient>) -> Self {
        self.solana_rpc = Some(solana_rpc);
        self
    }

    /// Update the network congestion factor
    pub async fn update_network_congestion(&self) -> Result<()> {
        if let Some(solana_rpc) = &self.solana_rpc {
            let congestion = solana_rpc.get_network_congestion_factor().await;
            // Store as fixed-point integer (multiply by 100)
            let congestion_fixed = (congestion * 100.0).round() as u64;
            self.network_congestion
                .store(congestion_fixed, Ordering::Relaxed);
            info!("Updated network congestion factor: {:.2}", congestion);
        }
        Ok(())
    }

    /// Check if an opportunity involves banned token pairs
    pub fn has_banned_tokens(&self, opportunity: &ArbitrageOpportunity) -> bool {
        let token_a = opportunity.source_pool.token_a.symbol.as_str();
        let token_b = opportunity.source_pool.token_b.symbol.as_str();

        crate::arbitrage::detector::ArbitrageDetector::is_permanently_banned(token_a, token_b)
            || crate::arbitrage::detector::ArbitrageDetector::is_temporarily_banned(
                token_a, token_b,
            )
    }

    /// Check if a multi-hop opportunity involves banned token pairs
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

    /// Execute an arbitrage opportunity
    #[allow(dead_code)]
    pub async fn execute(&self, opportunity: ArbitrageOpportunity) -> Result<String> {
        // Check if any tokens in this opportunity are on the ban list
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

        // Build the transaction
        let instructions = self.build_instructions(&opportunity)?;

        if self.simulation_mode {
            // In simulation mode, log and append to XML file
            info!(
                "SIMULATION: Would execute transaction with {} instructions",
                instructions.len()
            );
            // Append simulated trade to XML file
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

        // Create transaction
        let recent_blockhash = self.rpc_client.get_latest_blockhash().await?;

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        // Add priority fee
        let transaction_with_fee = if self.priority_fee > 0 {
            self.add_priority_fee(transaction, self.priority_fee)?
        } else {
            transaction
        };

        // Check if we've exceeded timeout
        if start_time.elapsed() > self.max_timeout {
            return Err(anyhow!("Transaction preparation exceeded timeout"));
        }

        // Send transaction
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
                Ok(signature.to_string())
            }
            Err(e) => {
                error!("Failed to execute arbitrage: {}", e);
                Err(anyhow!("Transaction failed: {}", e))
            }
        }
    }

    /// Calculate optimal priority fee based on opportunity profit and network congestion
    pub fn calculate_optimal_priority_fee(
        &self,
        profit_percentage: f64,
        expected_profit_sol: f64,
    ) -> u64 {
        // Get the network congestion factor (0.5 to 5.0 range, normalized from atomic value)
        let congestion = self.network_congestion.load(Ordering::Relaxed) as f64 / 100.0;

        // Base priority fee based on network congestion
        let mut optimal_fee = self.priority_fee as f64 * congestion;

        // Scale by profit percentage (higher profit = can afford higher fee)
        if profit_percentage > 0.5 {
            let profit_factor = 1.0 + (profit_percentage - 0.5) / 10.0; // Cap at ~1.5x for 5% profit
            optimal_fee *= profit_factor;
        }

        // For large profit opportunities, willing to pay more for priority
        if expected_profit_sol > 0.1 {
            // If profit > 0.1 SOL
            let profit_boost = (expected_profit_sol.min(1.0) * 5000.0) as u64; // Up to 5000 more for 1+ SOL profit
            optimal_fee += profit_boost as f64;
        }

        // Ensure the fee is within reasonable bounds
        let min_fee = 1_000;
        let max_fee = 1_000_000; // 1M micro-lamports max

        (optimal_fee as u64).clamp(min_fee, max_fee)
    }

    /// Get recent priority fees from the network
    #[allow(dead_code)]
    pub async fn get_recent_priority_fees(&self, num_blocks: usize) -> (u64, u64, u64) {
        if let Some(solana_rpc) = &self.solana_rpc {
            return solana_rpc
                .get_recent_prioritization_fees(num_blocks)
                .await
                .unwrap_or((5000, 10000, 25000)); // Default values on error
        }
        (5000, 10000, 25000) // Default low, median, high
    }

    /// Execute a multi-hop arbitrage opportunity asynchronously (atomic if possible)
    pub async fn execute_multihop(&self, opportunity: &MultiHopArbOpportunity) -> Result<String> {
        // Check if any tokens in this multi-hop opportunity are on the ban list
        if self.has_multihop_banned_tokens(opportunity) {
            return Err(anyhow!("Multi-hop opportunity involves banned token pair"));
        }

        let start_time = Instant::now();
        info!(
            "Executing multi-hop arbitrage: {} hops, profit {:.4}%",
            opportunity.hops.len(),
            opportunity.profit_pct
        );

        // Build instructions for all hops
        let mut instructions = Vec::new();
        for (i, hop) in opportunity.hops.iter().enumerate() {
            match hop.dex {
                DexType::Raydium => {
                    instructions.push(self.create_raydium_swap_instruction(
                        &PoolInfo {
                            address: hop.pool,
                            name: format!("hop{}_raydium", i),
                            token_a: Default::default(),
                            token_b: Default::default(),
                            fee_numerator: 0,
                            fee_denominator: 0,
                            last_update_timestamp: 0,
                            dex_type: DexType::Raydium,
                        },
                        &TokenAmount::new(hop.input_amount as u64, 6),
                        true, // TODO: direction
                    )?);
                }
                DexType::Orca => {
                    instructions.push(self.create_orca_swap_instruction(
                        &PoolInfo {
                            address: hop.pool,
                            name: format!("hop{}_orca", i),
                            token_a: Default::default(),
                            token_b: Default::default(),
                            fee_numerator: 0,
                            fee_denominator: 0,
                            last_update_timestamp: 0,
                            dex_type: DexType::Orca,
                        },
                        &TokenAmount::new(hop.input_amount as u64, 6),
                        true, // TODO: direction
                    )?);
                }
                _ => return Err(anyhow!("Unsupported DEX type in multi-hop")),
            }
        }

        if self.simulation_mode {
            info!(
                "SIMULATION: Would execute multi-hop transaction with {} instructions",
                instructions.len()
            );
            let xml_entry = format!(
                "<multihop-trade>\n  <timestamp>{}</timestamp>\n  <profit_pct>{:.4}</profit_pct>\n  <hops>{}</hops>\n  <result>simulated</result>\n</multihop-trade>\n",
                chrono::Utc::now().to_rfc3339(),
                opportunity.profit_pct,
                opportunity.hops.len()
            );
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open("simulated_trades.xml")?;
            file.write_all(xml_entry.as_bytes())?;
            return Ok("simulation-multihop-txid-placeholder".to_string());
        }

        let recent_blockhash = self.rpc_client.get_latest_blockhash().await?;
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        // Calculate optimal priority fee based on profit opportunity and network congestion
        let expected_profit_sol = opportunity.input_amount * opportunity.profit_pct / 100.0;
        let priority_fee =
            self.calculate_optimal_priority_fee(opportunity.profit_pct, expected_profit_sol);

        info!(
            "Using priority fee of {} micro-lamports for {:.2}% profit opportunity",
            priority_fee, opportunity.profit_pct
        );

        let transaction_with_fee = self.add_priority_fee(transaction, priority_fee)?;
        if start_time.elapsed() > self.max_timeout {
            return Err(anyhow!("Transaction preparation exceeded timeout"));
        }
        match self
            .rpc_client
            .send_and_confirm_transaction(&transaction_with_fee)
            .await
        {
            Ok(signature) => {
                let elapsed = start_time.elapsed();
                info!(
                    "Multi-hop arbitrage executed successfully in {:?}: {}",
                    elapsed, signature
                );
                Ok(signature.to_string())
            }
            Err(e) => {
                error!("Failed to execute multi-hop arbitrage: {}", e);
                Err(anyhow!("Multi-hop transaction failed: {}", e))
            }
        }
    }

    /// Build instructions for the arbitrage transaction
    #[allow(dead_code)]
    fn build_instructions(&self, opportunity: &ArbitrageOpportunity) -> Result<Vec<Instruction>> {
        // This is a simplified implementation - real implementation would use actual DEX instructions
        let mut instructions = Vec::new();

        // Add instructions based on the DEX type of source pool
        match opportunity.source_pool.dex_type {
            DexType::Raydium => {
                // Create Raydium swap instruction (simplified for example)
                instructions.push(self.create_raydium_swap_instruction(
                    &opportunity.source_pool,
                    &opportunity.input_amount,
                    true, // is_a_to_b
                )?);
            }
            DexType::Orca => {
                // Create Orca swap instruction
                instructions.push(self.create_orca_swap_instruction(
                    &opportunity.source_pool,
                    &opportunity.input_amount,
                    true, // is_a_to_b
                )?);
            }
            _ => return Err(anyhow!("Unsupported DEX type")),
        }

        // Add instructions based on the DEX type of target pool
        match opportunity.target_pool.dex_type {
            DexType::Raydium => {
                // Create Raydium swap instruction for the second swap
                // In a real implementation, you'd calculate the intermediate amount
                let intermediate_amount = TokenAmount::new(0, 6); // Placeholder

                instructions.push(self.create_raydium_swap_instruction(
                    &opportunity.target_pool,
                    &intermediate_amount,
                    false, // !is_a_to_b
                )?);
            }
            DexType::Orca => {
                // Create Orca swap instruction for the second swap
                let intermediate_amount = TokenAmount::new(0, 6); // Placeholder

                instructions.push(self.create_orca_swap_instruction(
                    &opportunity.target_pool,
                    &intermediate_amount,
                    false, // !is_a_to_b
                )?);
            }
            _ => return Err(anyhow!("Unsupported DEX type")),
        }

        Ok(instructions)
    }

    /// Create a Raydium swap instruction (simplified placeholder)
    #[allow(dead_code)]
    fn create_raydium_swap_instruction(
        &self,
        _pool: &PoolInfo,
        _amount: &TokenAmount,
        _is_a_to_b: bool,
    ) -> Result<Instruction> {
        // This is a placeholder - in a real implementation you would use the actual Raydium instruction
        let program_id = Pubkey::new_unique(); // Placeholder for Raydium program ID

        Ok(Instruction {
            program_id,
            accounts: vec![], // Would include pool accounts, token accounts, etc.
            data: vec![],     // Would include serialized instruction data
        })
    }

    /// Create an Orca swap instruction (simplified placeholder)
    #[allow(dead_code)]
    fn create_orca_swap_instruction(
        &self,
        _pool: &PoolInfo,
        _amount: &TokenAmount,
        _is_a_to_b: bool,
    ) -> Result<Instruction> {
        // This is a placeholder - in a real implementation you would use the actual Orca instruction
        let program_id = Pubkey::new_unique(); // Placeholder for Orca program ID

        Ok(Instruction {
            program_id,
            accounts: vec![], // Would include pool accounts, token accounts, etc.
            data: vec![],     // Would include serialized instruction data
        })
    }

    /// Add priority fee to transaction
    #[allow(dead_code)]
    fn add_priority_fee(&self, transaction: Transaction, fee: u64) -> Result<Transaction> {
        // Create a compute budget instruction with the specified priority fee
        let priority_fee_ix = ComputeBudgetInstruction::set_compute_unit_price(fee);

        // We need to rebuild the transaction with the new instruction
        let mut instructions = vec![priority_fee_ix];

        // Get the original transaction instructions
        let message = transaction.message.clone();
        let account_keys = message.account_keys;

        // Extract original instructions
        for ix in message.instructions {
            let program_idx = ix.program_id_index as usize;
            let program_id = account_keys[program_idx];

            // Convert CompiledInstruction back to Instruction
            let accounts = ix
                .accounts
                .iter()
                .map(|idx| {
                    solana_sdk::instruction::AccountMeta::new_readonly(
                        account_keys[*idx as usize],
                        false,
                    )
                })
                .collect();

            let instruction = Instruction {
                program_id,
                accounts,
                data: ix.data,
            };

            instructions.push(instruction);
        }

        // Create new transaction with priority fee instruction added
        let new_tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            transaction.message.recent_blockhash,
        );

        info!("Added priority fee of {} micro-lamports", fee);
        Ok(new_tx)
    }

    /// Adjust priority fee based on network conditions (adaptive fee calculation)
    #[allow(dead_code)]
    pub async fn adjust_priority_fee(&self) -> Result<u64> {
        // Get congestion factor (stored as fixed-point integer, divided by 100 for the actual value)
        let congestion_factor = self.network_congestion.load(Ordering::Relaxed);

        // Base fee (in micro-lamports)
        let base_fee = 500u64; // Explicit type to avoid ambiguity

        // Calculate adaptive fee (congestion_factor is divided by 100 since it's stored as fixed-point)
        let adaptive_fee = base_fee.saturating_mul(congestion_factor) / 100;

        info!(
            "Adjusted priority fee based on congestion (factor {:.2}): {} micro-lamports",
            congestion_factor as f64 / 100.0,
            adaptive_fee
        );

        Ok(adaptive_fee)
    }

    /// Calculate optimal priority fee based on profit opportunity and network congestion
    #[allow(dead_code)]
    pub async fn calculate_optimal_priority_fee_async(&self, profit_pct: f64) -> Result<u64> {
        // Get basic priority fee adjusted for network congestion
        let base_fee = self.adjust_priority_fee().await?;

        // Get recent fee percentiles to understand market rates if we have a Solana RPC client
        let (p25_fee, p50_fee, p75_fee) = if let Some(solana_rpc) = &self.solana_rpc {
            solana_rpc
                .get_recent_prioritization_fees(20)
                .await
                .unwrap_or((5000, 10000, 25000)) // Use unwrap_or to avoid Clippy warning
        } else {
            (5000, 10000, 25000) // Default values if no Solana RPC client
        };

        // Determine profit tier and adjust fee strategy accordingly
        // Higher profit opportunities warrant more aggressive fee strategies
        let optimal_fee = if profit_pct >= 2.0 {
            // High profit: Be aggressive, use higher percentile
            p75_fee.max(base_fee)
        } else if profit_pct >= 1.0 {
            // Medium profit: Use median fee
            p50_fee.max(base_fee)
        } else if profit_pct >= 0.5 {
            // Low profit: Use lower percentile
            p25_fee.max(base_fee)
        } else {
            // Very low profit: Use base fee only
            base_fee
        };

        info!(
            "Calculated optimal priority fee for {:.2}% profit: {} micro-lamports (p25={}, p50={}, p75={})",
            profit_pct, optimal_fee, p25_fee, p50_fee, p75_fee
        );

        Ok(optimal_fee)
    }
}
