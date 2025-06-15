//! Unified Execution Module
//! 
//! This module consolidates all execution-related functionality:
//! - HftExecutor: High-frequency individual trade execution
//! - BatchExecutor: Batch execution with Jito bundle support
//! - ExecutionMetrics and event handling
//! 
//! Combines logic from executor.rs and execution_engine.rs

use crate::{
    arbitrage::{
        opportunity::MultiHopArbOpportunity,
        mev::{JitoHandler, MevProtectionConfig, JitoConfig},
        analysis::{FeeManager, ArbitrageAnalyzer},
        safety::{SafeTransactionHandler, TransactionSafetyConfig},
    },
    config::settings::Config,
    dex::api::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::rpc::SolanaRpcClient,
    utils::{DexType, PoolInfo, TokenAmount}, // Added PoolInfo import
    cache::Cache, // Redis cache
};
use log::{info, warn, error};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use uuid;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Instant,
};
use tokio::sync::{mpsc, Mutex, RwLock}; // Mutex for dex_clients, metrics
use dashmap::DashMap; 
use crate::dex::api::CommonSwapInfo;
use spl_associated_token_account::get_associated_token_address;

// =============================================================================
// Event System and Core Types
// =============================================================================

type EventSender = mpsc::Sender<ExecutorEvent>;

#[derive(Debug)]
pub enum ExecutorEvent {
    OpportunityExecuted {
        opportunity_id: String,
        signature: Option<Signature>,
        timestamp: std::time::SystemTime,
        result: Result<(), String>,
    },
    BatchExecuted {
        batch_id: String,
        bundle_result: BundleExecutionResult,
        timestamp: std::time::SystemTime,
    },
}

// =============================================================================
// High-Frequency Executor (formerly ArbitrageExecutor)
// =============================================================================

#[derive(Clone)]
pub struct HftExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    event_sender: Option<EventSender>,
    config: Arc<Config>,
    metrics: Arc<Mutex<Metrics>>,
    // DEX client management for routing
    dex_clients: Option<Arc<Mutex<HashMap<DexType, Box<dyn DexClient>>>>>,
    hot_cache: Arc<DashMap<solana_sdk::pubkey::Pubkey, Arc<PoolInfo>>>,
    pool_cache: Option<Arc<Cache>>, // This is the Redis cache
    // Enhanced components for dynamic fee/slippage calculation
    fee_manager: Arc<FeeManager>,
    arbitrage_analyzer: Arc<Mutex<ArbitrageAnalyzer>>,
    safety_handler: Arc<SafeTransactionHandler>,
}

impl HftExecutor {
    pub fn new(
        wallet: Arc<Keypair>, 
        rpc_client: Arc<NonBlockingRpcClient>, 
        event_sender: Option<EventSender>,
        config: Arc<Config>, 
        metrics: Arc<Mutex<Metrics>>,
        hot_cache: Arc<DashMap<solana_sdk::pubkey::Pubkey, Arc<PoolInfo>>>,
    ) -> Self {
        // Create enhanced components with proper initialization
        let fee_manager = Arc::new(FeeManager::default());
        let arbitrage_analyzer = Arc::new(Mutex::new(
            ArbitrageAnalyzer::new(&config, metrics.clone())
        ));
        
        // Create simplified safety configuration based on main config
        let safety_config = TransactionSafetyConfig::default();
        
        // Create SolanaRpcClient with proper parameters
        let solana_rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            std::time::Duration::from_millis(1000),
        ));
        
        let safety_handler = Arc::new(
            SafeTransactionHandler::new(solana_rpc_client, safety_config)
        );

        Self { 
            wallet, 
            rpc_client, 
            event_sender, 
            config, 
            metrics,
            dex_clients: None,
            hot_cache,
            pool_cache: None,
            fee_manager,
            arbitrage_analyzer,
            safety_handler,
        }
    }

    /// Executes an opportunity with a rapid re-entry loop to attempt to capture
    /// remaining profit if the opportunity is still viable after the initial execution.
    pub async fn execute_opportunity_with_reentry(
        &self,
        initial_opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Signature>, String> {
        let mut signatures = Vec::new();
        let mut current_opportunity = initial_opportunity.clone();
        let max_reentries = self.config.rpc_max_retries.unwrap_or(0); // Default to 0 if not set (no re-entry)

        info!("Attempting to execute opportunity {} with up to {} re-entries.", initial_opportunity.id, max_reentries);

        for i in 0..=max_reentries { // Loop 0 is the initial execution
            info!("Execution attempt {} for opportunity ID {}", i + 1, current_opportunity.id);

            match self.execute_opportunity(&current_opportunity).await {
                Ok(signature) => {
                    info!("Successfully executed attempt {} for opportunity {}, signature: {}", i + 1, current_opportunity.id, signature);
                    signatures.push(signature);

                    // If this was the last allowed attempt, or no more re-entries configured, break.
                    if i >= max_reentries {
                        break;
                    }

                    // Re-evaluate for re-entry
                    match self._reevaluate_for_reentry(&current_opportunity, &signature).await {
                        Ok(Some(next_opportunity_candidate)) => {
                            // Check if still profitable enough for re-entry
                            let re_entry_threshold_pct = self.config.min_profit_pct * 0.5; // Default re-entry threshold
                            if next_opportunity_candidate.profit_pct >= re_entry_threshold_pct {
                                info!("Opportunity {} still profitable ({}%) after execution, attempting re-entry.", next_opportunity_candidate.id, next_opportunity_candidate.profit_pct);
                                current_opportunity = next_opportunity_candidate;
                                // Optional: small delay if configured
                                if let Some(delay_ms) = self.config.rpc_retry_delay_ms {
                                    if delay_ms > 0 {
                                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                                    }
                                }
                            } else {
                                info!("Opportunity {} no longer profitable enough ({}% < {}%) for re-entry after execution.", next_opportunity_candidate.id, next_opportunity_candidate.profit_pct, re_entry_threshold_pct);
                                break;
                            }
                        }
                        Ok(None) => {
                            info!("Opportunity {} no longer viable for re-entry after execution.", current_opportunity.id);
                            break;
                        }
                        Err(e) => {
                            warn!("Failed to re-evaluate opportunity {} for re-entry: {}. Stopping re-entry.", current_opportunity.id, e);
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Execution attempt {} for opportunity {} failed: {}. Stopping re-entry.", i + 1, current_opportunity.id, e);
                    // If even the first attempt fails, return the error. If a re-entry fails, we've already collected some signatures.
                    if i == 0 { return Err(e); } else { break; }
                }
            }
        }

        if signatures.is_empty() && max_reentries > 0 {
             Err(format!("No successful executions for opportunity {} after attempts.", initial_opportunity.id))
        } else {
             Ok(signatures)
        }
    }

    /// Placeholder for re-evaluating an opportunity after an execution.
    /// In a real implementation, this would:
    /// 1. Fetch updated PoolInfo for all pools in `executed_opportunity.hops` from `self.hot_cache`.
    ///    This might involve a short delay to allow WebSocket updates or direct RPC fetches.
    /// 2. Adjust `input_amount` based on remaining balance or new optimal calculation.
    /// 3. Recalculate profit using an arbitrage analysis module.
    /// 4. Construct a new `MultiHopArbOpportunity` if still viable.
    async fn _reevaluate_for_reentry(
        &self,
        executed_opportunity: &MultiHopArbOpportunity,
        _last_signature: &Signature, // Parameter for context, if needed
    ) -> Result<Option<MultiHopArbOpportunity>, String> {
        warn!("Re-evaluation for rapid re-entry (opportunity ID: {}) is a STUB. Opportunity will not be re-executed further in this cycle.", executed_opportunity.id);
        
        // STUB: For demonstration, let's imagine the profit slightly decreases.
        // In a real scenario, you would fetch fresh pool data and recalculate.
        let mut reevaluated_opp = executed_opportunity.clone();
        reevaluated_opp.profit_pct *= 0.5; // Simulate profit halving
        reevaluated_opp.total_profit *= 0.5;
        reevaluated_opp.expected_output = reevaluated_opp.input_amount + reevaluated_opp.total_profit;
        reevaluated_opp.id = format!("{}_reentry", executed_opportunity.id); // Give it a new ID for clarity

        // Simulate it's still viable but with reduced profit
        // return Ok(Some(reevaluated_opp));

        // For now, always stop after one execution by returning None.
        // To test the loop, you can change this to return Some(reevaluated_opp)
        // and ensure hft_reentry_min_profit_pct is met.
        Ok(None)
    }

    pub async fn execute_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, String> {
        let start_time = Instant::now();
        
        // Step 1: Enhanced fee calculation using FeeManager
        info!("🔄 Calculating dynamic fees for opportunity {}", opportunity.id);
        
        // Collect pools for fee calculation
        let pools_for_fee_calc: Vec<Arc<PoolInfo>> = opportunity.hops.iter()
            .filter_map(|hop| self.hot_cache.get(&hop.pool).map(|arc| arc.value().clone()))
            .collect();
        
        let pools_refs: Vec<&PoolInfo> = pools_for_fee_calc.iter().map(|p| p.as_ref()).collect();
        
        // Estimate compute units using FeeManager
        let estimated_cu = self.fee_manager.estimate_compute_units(&pools_refs);
        
        // Calculate dynamic priority fee
        let dynamic_priority_fee = match self.fee_manager.calculate_dynamic_priority_fee(estimated_cu).await {
            Ok(fee) => fee,
            Err(e) => {
                warn!("Failed to calculate dynamic priority fee: {}. Using config default.", e);
                self.config.transaction_priority_fee_lamports
            }
        };
        
        // Calculate Jito tip for MEV protection
        let trade_value_sol = opportunity.input_amount as f64 / 1_000_000_000.0; // Convert lamports to SOL
        let complexity_factor = opportunity.hops.len() as f64;
        let jito_tip = self.fee_manager.calculate_jito_tip(trade_value_sol, complexity_factor);
        
        info!("💰 Enhanced fee calculation - CU: {}, Priority fee: {} lamports, Jito tip: {} lamports", 
              estimated_cu, dynamic_priority_fee, jito_tip);

        // Step 2: Enhanced slippage calculation using ArbitrageAnalyzer
        let analyzer = self.arbitrage_analyzer.lock().await;
        
        // Create input amount for fee breakdown calculation
        let input_token_amount = TokenAmount {
            amount: opportunity.input_amount as u64, // Convert f64 to u64
            decimals: 9, // Assume SOL decimals for simplicity
        };
        
        // Calculate comprehensive fee breakdown
        let fee_breakdown = analyzer.calculate_fee_breakdown(
            &pools_refs,
            &input_token_amount,
            150.0 // TODO: Get real SOL price
        );
        
        info!("📊 Fee breakdown - Protocol: {:.4}%, Gas: {:.4}%, Slippage: {:.4}%, Total: {:.4}%, Risk: {:.2}", 
              fee_breakdown.protocol_fee * 100.0,
              fee_breakdown.gas_fee * 100.0,
              fee_breakdown.slippage_cost * 100.0,
              fee_breakdown.total_cost * 100.0,
              fee_breakdown.risk_score);

        // Step 3: Build instructions with enhanced calculations
        let instructions = self.build_instructions_from_multihop(opportunity).await?;

        if instructions.is_empty() {
            return Err("No swap instructions generated".to_string());
        }

        // Step 4: Build transaction with dynamic fees
        let recent_blockhash = self.get_latest_blockhash().await?;
        let all_instructions: Vec<Instruction> = [
            ComputeBudgetInstruction::set_compute_unit_limit(estimated_cu),
            ComputeBudgetInstruction::set_compute_unit_price(dynamic_priority_fee),
        ].into_iter().chain(instructions.into_iter()).collect();

        let transaction = Transaction::new_signed_with_payer(
            &all_instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        // Step 5: Safe execution using SafeTransactionHandler
        info!("🚀 Executing transaction with enhanced safety checks...");
        match self.safety_handler.execute_with_safety_checks(
            transaction,
            &pools_refs,
            opportunity.input_amount as u64,
            opportunity.expected_output as u64,
        ).await {
            Ok(transaction_result) => {
                if transaction_result.success {
                    let duration = start_time.elapsed();
                    self.metrics.lock().await.record_execution_time(duration);
                    self.metrics.lock().await.log_opportunity_executed_success();
                    
                    // Log enhanced execution metrics
                    info!("✅ Transaction successful - Signature: {:?}, Fee: {} lamports, Slippage: {:.2}%, Time: {}ms",
                          transaction_result.signature,
                          transaction_result.fee_paid,
                          transaction_result.slippage_experienced * 100.0,
                          transaction_result.execution_time_ms);
                    
                    if let Some(sender) = &self.event_sender {
                        let signature_for_event = transaction_result.signature.as_ref().and_then(|s| s.parse().ok());
                        let event = ExecutorEvent::OpportunityExecuted {
                            opportunity_id: opportunity.id.clone(),
                            signature: signature_for_event,
                            timestamp: std::time::SystemTime::now(),
                            result: Ok(()),
                        };
                        if let Err(e) = sender.send(event).await {
                            log::error!("Failed to send execution success event: {}", e);
                        }
                    }
                    
                    // Parse signature from string result
                    let signature_str = transaction_result.signature
                        .as_ref()
                        .ok_or_else(|| "No signature returned".to_string())?;
                    
                    let signature = signature_str
                        .parse()
                        .map_err(|e| format!("Failed to parse signature: {}", e))?;
                    
                    Ok(signature)
                } else {
                    let error_msg = transaction_result.failure_reason
                        .unwrap_or_else(|| "Unknown transaction failure".to_string());
                    Err(format!("Safe transaction execution failed: {}", error_msg))
                }
            }
            Err(e) => {
                self.metrics.lock().await.log_opportunity_executed_failure();
                error!("❌ Enhanced execution failed: {}", e);
                
                if let Some(sender) = &self.event_sender {
                    let event = ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: None,
                        timestamp: std::time::SystemTime::now(),
                        result: Err(format!("Enhanced execution failed: {}", e)),
                    };
                    if let Err(send_err) = sender.send(event).await {
                        log::error!("Failed to send execution failure event: {}", send_err);
                    }
                }
                Err(format!("Enhanced execution failed: {}", e))
            }
        }
    }

    async fn build_instructions_from_multihop(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, String> {
        let mut all_instructions = Vec::new();

        let dex_clients_map_arc = self.dex_clients.as_ref()
            .ok_or_else(|| "DEX clients not initialized in HftExecutor".to_string())?;
        let dex_clients_map = dex_clients_map_arc.lock().await;
 
        for (hop_index, hop) in opportunity.hops.iter().enumerate() {
            info!("Building instruction for hop {}: {} -> {} on {:?}", hop_index + 1, hop.input_token, hop.output_token, hop.dex);
            
            // --- Retrieve PoolInfo for the current hop from hot_cache ---
            let pool_info_arc = self.hot_cache.get(&hop.pool)
                .ok_or_else(|| format!("Pool {} for hop {} not found in hot_cache", hop.pool, hop_index + 1))?
                .value().clone();

            let dex_client_boxed = dex_clients_map.get(&hop.dex)
                .ok_or_else(|| format!("DexClient not found for {:?}", hop.dex))?;
            // Assuming DexClient is Arc now, if not, this needs adjustment. For Box, direct deref is fine.
            let dex_client = dex_client_boxed.as_ref(); // Get a reference to the trait object

            // --- Determine source and destination mints and decimals ---
            let (source_token_details, dest_token_details) = 
                if pool_info_arc.token_a.symbol.eq_ignore_ascii_case(&hop.input_token) && pool_info_arc.token_b.symbol.eq_ignore_ascii_case(&hop.output_token) {
                    (&pool_info_arc.token_a, &pool_info_arc.token_b)
                } else if pool_info_arc.token_b.symbol.eq_ignore_ascii_case(&hop.input_token) && pool_info_arc.token_a.symbol.eq_ignore_ascii_case(&hop.output_token) {
                    (&pool_info_arc.token_b, &pool_info_arc.token_a)
                } else {
                    return Err(format!("Token symbol mismatch for hop {} in pool {}. Pool tokens: {}/{}, Hop tokens: {}/{}", hop_index + 1, hop.pool, pool_info_arc.token_a.symbol, pool_info_arc.token_b.symbol, hop.input_token, hop.output_token));
                };

            let source_mint = source_token_details.mint;
            let source_decimals = source_token_details.decimals;
            let dest_mint = dest_token_details.mint;
            let dest_decimals = dest_token_details.decimals;

            // --- Calculate amounts in smallest units ---
            let input_amount_u64 = (hop.input_amount * 10f64.powi(source_decimals as i32)) as u64;
            
            // Integer-only slippage calculation for better security and reliability
            let max_slippage_percentage = self.config.max_slippage_pct; // Assuming this is a percentage like 0.5 for 0.5%
            
            // Convert expected_output to integer units first
            let expected_output_base_units = (hop.expected_output * 10f64.powi(dest_decimals as i32)) as u128;
            
            // Calculate slippage using integer arithmetic only
            // slippage_factor_bps = max_slippage_percentage * 100 (convert to basis points)
            // For 0.5%, this becomes 50 basis points
            let slippage_bps = (max_slippage_percentage * 100.0) as u128;
            
            // Calculate minimum output using checked arithmetic
            let minimum_output_amount_u64 = expected_output_base_units
                .checked_mul(10000_u128.checked_sub(slippage_bps).ok_or_else(|| "Slippage calculation underflow".to_string())?)
                .ok_or_else(|| "Slippage calculation overflow in multiplication".to_string())?
                .checked_div(10000)
                .ok_or_else(|| "Slippage calculation division error".to_string())?
                .try_into()
                .map_err(|_| "Minimum output amount too large for u64".to_string())?;

            // --- Get user's ATAs ---
            let user_source_token_account = get_associated_token_address(&self.wallet.pubkey(), &source_mint);
            let user_destination_token_account = get_associated_token_address(&self.wallet.pubkey(), &dest_mint);

            let common_swap_info = CommonSwapInfo {
                user_wallet_pubkey: self.wallet.pubkey(),
                source_token_mint: source_mint,
                destination_token_mint: dest_mint,
                user_source_token_account,
                user_destination_token_account,
                input_amount: input_amount_u64,
                minimum_output_amount: minimum_output_amount_u64,
            };

            let instruction = dex_client.get_swap_instruction_enhanced(&common_swap_info, pool_info_arc.clone()).await
                .map_err(|e| format!("Failed to get swap instruction for hop {}: {}", hop_index + 1, e))?;
            all_instructions.push(instruction);
        }

        Ok(all_instructions)
    }

    async fn get_latest_blockhash(&self) -> Result<Hash, String> {
        self.rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to fetch latest blockhash: {}", e))
    }

    /// Initialize DEX clients for routing
    pub async fn initialize_dex_clients(&mut self, cache: Arc<Cache>) {
        use crate::dex::clients::{OrcaClient, RaydiumClient, MeteoraClient, LifinityClient};
        use std::collections::HashMap;
        
        let mut clients: HashMap<DexType, Box<dyn DexClient>> = HashMap::new();
        
        // Initialize each DEX client
        clients.insert(DexType::Orca, Box::new(OrcaClient::new()));
        clients.insert(DexType::Raydium, Box::new(RaydiumClient::new()));
        clients.insert(DexType::Meteora, Box::new(MeteoraClient::new()));
        clients.insert(DexType::Lifinity, Box::new(LifinityClient::new()));
        
        self.dex_clients = Some(Arc::new(Mutex::new(clients)));
        self.pool_cache = Some(cache);
        
        info!("Initialized DEX clients for Orca, Raydium, Meteora, and Lifinity");
    }
    
    /// Update the pool cache with discovered pools
    pub async fn update_pool_cache(&self, _pools: &[crate::utils::PoolInfo]) {
        // This method would update the internal pool cache
        // For now, it's a stub since the pool cache logic is handled elsewhere
        info!("Pool cache update requested - this is handled by the discovery service");
    }
}

// =============================================================================
// Batch Execution Configuration and Types
// =============================================================================

/// Configuration for the batch execution engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecutionConfig {
    /// Maximum number of opportunities per batch
    pub max_opportunities_per_batch: usize,
    /// Maximum compute units per transaction
    pub max_compute_units_per_tx: u32,
    /// Maximum total compute units per bundle
    pub max_compute_units_per_bundle: u32,
    /// Minimum profit threshold for batching (USD)
    pub min_batch_profit_usd: f64,
    /// Maximum batch assembly time (ms)
    pub max_batch_assembly_time_ms: u64,
    /// Enable parallel simulation
    pub enable_parallel_simulation: bool,
    /// Maximum simulation time per opportunity (ms)
    pub max_simulation_time_ms: u64,
    /// Jito tip amount (lamports)
    pub jito_tip_lamports: u64,
    /// Enable MEV protection
    pub enable_mev_protection: bool,
    /// Bundle submission timeout (ms)
    pub bundle_submission_timeout_ms: u64,
}

impl Default for BatchExecutionConfig {
    fn default() -> Self {
        Self {
            max_opportunities_per_batch: 5,
            max_compute_units_per_tx: 1_400_000,
            max_compute_units_per_bundle: 7_000_000,
            min_batch_profit_usd: 10.0,
            max_batch_assembly_time_ms: 100,
            enable_parallel_simulation: true,
            max_simulation_time_ms: 50,
            jito_tip_lamports: 10_000,
            enable_mev_protection: true,
            bundle_submission_timeout_ms: 1000,
        }
    }
}

/// Represents a batch of arbitrage opportunities to be executed together
#[derive(Debug, Clone)]
pub struct OpportunityBatch {
    pub id: String,
    pub opportunities: Vec<MultiHopArbOpportunity>,
    pub estimated_total_profit_usd: f64,
    pub estimated_total_compute_units: u32,
    pub created_at: Instant,
    pub priority_score: f64,
}

/// Result of simulating an opportunity
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub opportunity_id: String,
    pub success: bool,
    pub estimated_profit_usd: f64,
    pub estimated_compute_units: u32,
    pub simulation_time_ms: u64,
    pub error_message: Option<String>,
}

/// Represents a Jito bundle ready for submission
#[derive(Debug, Clone)]
pub struct JitoBundle {
    pub id: String,
    pub transactions: Vec<Transaction>,
    pub tip_amount_lamports: u64,
    pub estimated_total_profit_usd: f64,
    pub created_at: Instant,
}

/// Result of executing a bundle
#[derive(Debug, Clone)]
pub struct BundleExecutionResult {
    pub bundle_id: String,
    pub success: bool,
    pub signatures: Vec<Option<Signature>>,
    pub execution_time_ms: u64,
    pub actual_profit_usd: Option<f64>,
    pub error_message: Option<String>,
}

/// Metrics for tracking execution performance
#[derive(Debug, Default)]
pub struct ExecutionMetrics {
    pub total_batches_created: u64,
    pub total_batches_executed: u64,
    pub total_successful_batches: u64,
    pub total_failed_batches: u64,
    pub avg_batch_assembly_time_ms: f64,
    pub avg_batch_execution_time_ms: f64,
    pub total_profit_captured_usd: f64,
    pub total_opportunities_processed: u64,
}

// =============================================================================
// Batch Executor
// =============================================================================

pub struct BatchExecutor {
    config: BatchExecutionConfig,
    solana_client: Arc<SolanaRpcClient>,
    wallet: Arc<Keypair>,
    #[allow(dead_code)]
    mev_handler: Option<Arc<JitoHandler>>,
    metrics: Arc<RwLock<ExecutionMetrics>>,
    active_batches: Arc<RwLock<HashMap<String, OpportunityBatch>>>,
    pending_opportunities: Arc<RwLock<Vec<MultiHopArbOpportunity>>>,
    event_sender: Option<EventSender>,
}

impl BatchExecutor {
    pub fn new(
        config: BatchExecutionConfig,
        solana_client: Arc<SolanaRpcClient>,
        wallet: Arc<Keypair>,
        event_sender: Option<EventSender>,
        _rpc_url: String, // Add rpc_url parameter for JitoHandler
    ) -> Self {
        let mev_handler = if config.enable_mev_protection {
            let mev_config = MevProtectionConfig::default();
            let jito_config = JitoConfig::default();
            Some(Arc::new(JitoHandler::new(mev_config, jito_config, "https://api.mainnet-beta.solana.com".to_string())))
        } else {
            None
        };

        Self {
            config,
            solana_client,
            wallet,
            mev_handler,
            metrics: Arc::new(RwLock::new(ExecutionMetrics::default())),
            active_batches: Arc::new(RwLock::new(HashMap::new())),
            pending_opportunities: Arc::new(RwLock::new(Vec::new())),
            event_sender,
        }
    }

    /// Process a new arbitrage opportunity for batch execution
    pub async fn process_opportunity(&self, opportunity: MultiHopArbOpportunity) -> Result<(), ArbError> {
        info!("Processing opportunity {} for batch execution", opportunity.id);
        
        // Add to pending opportunities
        {
            let mut pending = self.pending_opportunities.write().await;
            pending.push(opportunity);
        }

        // Try to create batches from pending opportunities
        self.try_create_batches().await?;
        
        Ok(())
    }

    /// Attempt to create executable batches from pending opportunities
    async fn try_create_batches(&self) -> Result<(), ArbError> {
        let batch_start = Instant::now();
        let pending_opportunities = {
            let mut pending = self.pending_opportunities.write().await;
            std::mem::take(&mut *pending)
        };

        if pending_opportunities.is_empty() {
            return Ok(());
        }

        info!("Attempting to create batches from {} pending opportunities", pending_opportunities.len());

        // Group opportunities into compatible batches
        let batches = self.create_compatible_batches(pending_opportunities).await?;
        
        for batch in batches {
            if batch.opportunities.len() > 1 {
                info!("Created batch {} with {} opportunities (${:.2} profit)", 
                      batch.id, batch.opportunities.len(), batch.estimated_total_profit_usd);
                self.execute_batch(batch).await?;
            } else if let Some(single_opp) = batch.opportunities.into_iter().next() {
                // Single opportunity - add back to pending or execute immediately
                self.pending_opportunities.write().await.push(single_opp);
            }
        }

        let batch_time = batch_start.elapsed().as_millis() as u64;
        self.update_batch_assembly_metrics(batch_time).await;

        Ok(())
    }

    /// Create compatible batches from opportunities
    async fn create_compatible_batches(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<OpportunityBatch>, ArbError> {
        let mut batches = Vec::new();
        let mut remaining_opportunities = opportunities;

        while !remaining_opportunities.is_empty() {
            let mut current_batch_opps = vec![remaining_opportunities.remove(0)];
            let mut current_batch_profit = current_batch_opps[0].estimated_profit_usd.unwrap_or(0.0);
            let mut current_batch_compute_units = self.estimate_compute_units(&current_batch_opps[0]);

            // Try to add compatible opportunities to current batch
            let mut indices_to_remove = Vec::new();
            for (idx, candidate) in remaining_opportunities.iter().enumerate() {
                if current_batch_opps.len() >= self.config.max_opportunities_per_batch {
                    break;
                }
                
                let candidate_compute_units = self.estimate_compute_units(candidate);

                // Check compatibility and constraints
                if self.are_opportunities_compatible(&current_batch_opps, candidate) &&
                   current_batch_compute_units + candidate_compute_units <= self.config.max_compute_units_per_bundle {
                    
                    current_batch_profit += candidate.estimated_profit_usd.unwrap_or(0.0);
                    current_batch_compute_units += candidate_compute_units;
                    indices_to_remove.push(idx);
                }
            }

            // Remove selected opportunities from remaining (in reverse order to maintain indices)
            let mut selected_opps = Vec::new();
            for &idx in indices_to_remove.iter().rev() {
                selected_opps.push(remaining_opportunities.remove(idx));
            }
            current_batch_opps.extend(selected_opps);

            // Create batch if profitable enough
            if current_batch_profit >= self.config.min_batch_profit_usd {
                let batch_size = current_batch_opps.len();
                let batch = OpportunityBatch {
                    id: uuid::Uuid::new_v4().to_string(),
                    opportunities: current_batch_opps,
                    estimated_total_profit_usd: current_batch_profit,
                    estimated_total_compute_units: current_batch_compute_units,
                    created_at: Instant::now(),
                    priority_score: current_batch_profit / (batch_size as f64),
                };
                batches.push(batch);
            } else {
                // Batch not profitable enough, add opportunities back
                remaining_opportunities.extend(current_batch_opps);
            }
        }

        Ok(batches)
    }

    /// Check if two opportunities are compatible for batching
    fn are_opportunities_compatible(&self, batch_opps: &[MultiHopArbOpportunity], candidate: &MultiHopArbOpportunity) -> bool {
        // Check for token conflicts (simplified)
        for existing in batch_opps {
            if self.have_token_conflicts(existing, candidate) {
                return false;
            }
        }
        true
    }

    /// Check if two opportunities have conflicting token usage
    fn have_token_conflicts(&self, opp1: &MultiHopArbOpportunity, opp2: &MultiHopArbOpportunity) -> bool {
        // Simplified conflict detection - check if they use the same input token
        opp1.input_token == opp2.input_token
    }

    /// Estimate compute units for an opportunity
    fn estimate_compute_units(&self, _opportunity: &MultiHopArbOpportunity) -> u32 {
        // Simplified estimation
        300_000 // Base estimate per opportunity
    }

    /// Execute a batch of opportunities
    async fn execute_batch(&self, batch: OpportunityBatch) -> Result<BundleExecutionResult, ArbError> {
        let start_time = Instant::now();
        info!("Executing batch {} with {} opportunities", batch.id, batch.opportunities.len());

        // Simulate all opportunities in parallel if enabled
        if self.config.enable_parallel_simulation {
            if let Err(e) = self.simulate_batch_parallel(&batch).await {
                warn!("Batch simulation failed: {}", e);
                return Err(e);
            }
        }

        // Create Jito bundle
        let jito_bundle = self.create_jito_bundle(&batch).await?;
        
        // Submit bundle
        let result = self.submit_jito_bundle(jito_bundle).await?;
        
        // Update metrics
        let execution_time = start_time.elapsed().as_millis() as u64;
        self.update_execution_metrics(&result, execution_time).await;

        // Send event if configured
        if let Some(sender) = &self.event_sender {
            let event = ExecutorEvent::BatchExecuted {
                batch_id: batch.id.clone(),
                bundle_result: result.clone(),
                timestamp: std::time::SystemTime::now(),
            };
            if let Err(e) = sender.send(event).await {
                error!("Failed to send batch execution event: {}", e);
            }
        }

        Ok(result)
    }

    /// Simulate batch opportunities in parallel
    async fn simulate_batch_parallel(&self, batch: &OpportunityBatch) -> Result<Vec<SimulationResult>, ArbError> {
        info!("Running parallel simulation for batch {}", batch.id);
        
        let mut simulation_tasks = Vec::new();
        
        for opportunity in &batch.opportunities {
            let opp_clone = opportunity.clone();
            
            let task = tokio::spawn(async move {
                let start_time = Instant::now();
                
                // Simulate the opportunity
                // In a real implementation, this would call the appropriate DEX client
                let success = true; // Placeholder
                let simulation_time = start_time.elapsed().as_millis() as u64;
                
                SimulationResult {
                    opportunity_id: opp_clone.id,
                    success,
                    estimated_profit_usd: opp_clone.estimated_profit_usd.unwrap_or(0.0),
                    estimated_compute_units: 300_000, // Estimate
                    simulation_time_ms: simulation_time,
                    error_message: if success { None } else { Some("Simulation failed".to_string()) },
                }
            });
            
            simulation_tasks.push(task);
        }

        // Wait for all simulations to complete
        let simulation_results = futures::future::try_join_all(simulation_tasks)
            .await
            .map_err(|e| ArbError::ExecutionError(format!("Simulation task failed: {}", e)))?;

        // Check if all simulations succeeded
        for result in &simulation_results {
            if !result.success {
                return Err(ArbError::ExecutionError(
                    format!("Simulation failed for opportunity {}: {}", 
                            result.opportunity_id, 
                            result.error_message.as_deref().unwrap_or("Unknown error"))
                ));
            }
        }

        info!("All {} simulations succeeded for batch {}", simulation_results.len(), batch.id);
        Ok(simulation_results)
    }

    /// Create a Jito bundle from a batch
    async fn create_jito_bundle(&self, batch: &OpportunityBatch) -> Result<JitoBundle, ArbError> {
        info!("Creating Jito bundle for batch {}", batch.id);
        
        let mut transactions = Vec::new();
        
        for opportunity in &batch.opportunities {
            // Build transaction for this opportunity
            let transaction = self.build_opportunity_transaction(opportunity).await?;
            transactions.push(transaction);
        }

        Ok(JitoBundle {
            id: batch.id.clone(),
            transactions,
            tip_amount_lamports: self.config.jito_tip_lamports,
            estimated_total_profit_usd: batch.estimated_total_profit_usd,
            created_at: Instant::now(),
        })
    }

    /// Build a transaction for a single opportunity
    async fn build_opportunity_transaction(&self, _opportunity: &MultiHopArbOpportunity) -> Result<Transaction, ArbError> {
        // Placeholder implementation
        // In real implementation, this would:
        // 1. Build swap instructions for the opportunity
        // 2. Add compute budget instructions
        // 3. Create and sign the transaction
        
        let instructions = vec![]; // Placeholder
        let recent_blockhash = self.solana_client.primary_client.get_latest_blockhash().await
            .map_err(|e| ArbError::ExecutionError(format!("Failed to get blockhash: {}", e)))?;
        
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        Ok(transaction)
    }

    /// Submit a Jito bundle
    async fn submit_jito_bundle(&self, bundle: JitoBundle) -> Result<BundleExecutionResult, ArbError> {
        info!("Submitting Jito bundle {} with {} transactions", bundle.id, bundle.transactions.len());
        
        let start_time = Instant::now();
        
        // In real implementation, this would:
        // 1. Submit the bundle to Jito
        // 2. Wait for confirmation
        // 3. Return the result
        
        // Placeholder implementation
        let success = true;
        let signatures = vec![None; bundle.transactions.len()]; // Placeholder
        let execution_time = start_time.elapsed().as_millis() as u64;
        
        Ok(BundleExecutionResult {
            bundle_id: bundle.id,
            success,
            signatures,
            execution_time_ms: execution_time,
            actual_profit_usd: if success { Some(bundle.estimated_total_profit_usd) } else { None },
            error_message: if success { None } else { Some("Bundle execution failed".to_string()) },
        })
    }

    /// Update batch assembly metrics
    async fn update_batch_assembly_metrics(&self, assembly_time_ms: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.total_batches_created += 1;
        metrics.avg_batch_assembly_time_ms = 
            (metrics.avg_batch_assembly_time_ms * (metrics.total_batches_created - 1) as f64 + assembly_time_ms as f64) 
            / metrics.total_batches_created as f64;
    }

    /// Update execution metrics
    async fn update_execution_metrics(&self, result: &BundleExecutionResult, execution_time_ms: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.total_batches_executed += 1;
        
        if result.success {
            metrics.total_successful_batches += 1;
            if let Some(profit) = result.actual_profit_usd {
                metrics.total_profit_captured_usd += profit;
            }
        } else {
            metrics.total_failed_batches += 1;
        }
        
        metrics.avg_batch_execution_time_ms = 
            (metrics.avg_batch_execution_time_ms * (metrics.total_batches_executed - 1) as f64 + execution_time_ms as f64) 
            / metrics.total_batches_executed as f64;
    }

    /// Get current status of the batch execution engine
    pub async fn get_status(&self) -> BatchEngineStatus {
        let metrics = self.metrics.read().await;
        let active_batches_count = self.active_batches.read().await.len();
        let pending_opportunities_count = self.pending_opportunities.read().await.len();

        BatchEngineStatus {
            active_batches_count,
            pending_opportunities_count,
            total_batches_created: metrics.total_batches_created,
            total_batches_executed: metrics.total_batches_executed,
            success_rate: if metrics.total_batches_executed > 0 {
                metrics.total_successful_batches as f64 / metrics.total_batches_executed as f64
            } else {
                0.0
            },
            avg_assembly_time_ms: metrics.avg_batch_assembly_time_ms,
            avg_execution_time_ms: metrics.avg_batch_execution_time_ms,
            total_profit_usd: metrics.total_profit_captured_usd,
        }
    }

    /// Force execution of all pending opportunities
    pub async fn force_execute_pending(&self) -> Result<Vec<BundleExecutionResult>, ArbError> {
        info!("Force executing all pending opportunities");
        
        let pending_opportunities = {
            let mut pending = self.pending_opportunities.write().await;
            std::mem::take(&mut *pending)
        };

        let mut results = Vec::new();
        
        // Create batches without strict profitability requirements
        for chunk in pending_opportunities.chunks(self.config.max_opportunities_per_batch) {
            let batch = OpportunityBatch {
                id: uuid::Uuid::new_v4().to_string(),
                opportunities: chunk.to_vec(),
                estimated_total_profit_usd: chunk.iter().map(|o| o.estimated_profit_usd.unwrap_or(0.0)).sum(),
                estimated_total_compute_units: chunk.len() as u32 * 300_000,
                created_at: Instant::now(),
                priority_score: 1.0,
            };
            
            let result = self.execute_batch(batch).await?;
            results.push(result);
        }

        Ok(results)
    }
}

// =============================================================================
// Status and Monitoring
// =============================================================================

#[derive(Debug, Clone)]
pub struct BatchEngineStatus {
    pub active_batches_count: usize,
    pub pending_opportunities_count: usize,
    pub total_batches_created: u64,
    pub total_batches_executed: u64,
    pub success_rate: f64,
    pub avg_assembly_time_ms: f64,
    pub avg_execution_time_ms: f64,
    pub total_profit_usd: f64,
}

// =============================================================================
// Legacy Type Aliases for Backward Compatibility
// =============================================================================

/// Legacy alias for HftExecutor
pub type ArbitrageExecutor = HftExecutor;

/// Legacy alias for BatchExecutor
pub type BatchExecutionEngine = BatchExecutor;
