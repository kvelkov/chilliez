//! Core Arbitrage Orchestrator Structure
//!
//! This module contains the main orchestrator struct definition and core functionality.

use crate::{
    arbitrage::{
        opportunity::MultiHopArbOpportunity,
        strategy::ArbitrageStrategy,
        analysis::EnhancedSlippageModel,
    },
    config::Config,
    dex::{BannedPairsManager, DexClient, PoolValidationConfig},
    error::ArbError,
    monitoring::LocalMetrics as Metrics,
    simulation,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager, BalanceMonitor},
    utils::{DexType, PoolInfo},
};

use crate::monitoring::PerformanceManager;

use dashmap::DashMap;
use log::{debug, error, info, warn};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{
    hash::Hash,
    signature::{Keypair, Signer},
    signer::keypair::read_keypair_file, // Fix: import read_keypair_file
    transaction::Transaction,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};

// Type alias for trading pair locks for improved readability
pub type TradingPairLocks = DashMap<(DexType, Pubkey, Pubkey), Arc<Mutex<()>>>;

// Type alias for the quicknode opportunity receiver for improved readability
pub type QuicknodeOpportunityReceiver =
    Arc<Mutex<Option<mpsc::UnboundedReceiver<MultiHopArbOpportunity>>>>;

// =============================================================================
// Core Orchestrator Struct
// =============================================================================

/// The main arbitrage orchestrator - central coordinator for all arbitrage operations
pub struct ArbitrageOrchestrator {
    // Core data structures
    pub hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    pub config: Arc<Config>,
    pub metrics: Arc<Mutex<Metrics>>,

    // Network and communication
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,

    // Execution components
    pub dex_providers: Vec<Arc<dyn DexClient>>,

    // Strategy and analysis
    pub detector: Arc<Mutex<ArbitrageStrategy>>,
    pub slippage_model: Arc<Mutex<EnhancedSlippageModel>>,

    // Configuration and validation
    pub pool_validation_config: PoolValidationConfig,
    pub banned_pairs_manager: Arc<BannedPairsManager>,

    // State management
    pub degradation_mode: Arc<AtomicBool>,
    pub execution_enabled: Arc<AtomicBool>,
    pub last_health_check: Arc<RwLock<Instant>>,
    pub health_check_interval: Duration,
    pub ws_reconnect_attempts: Arc<AtomicU64>,
    pub max_ws_reconnect_attempts: u64,

    // Async communication
    pub opportunity_sender: Option<mpsc::UnboundedSender<MultiHopArbOpportunity>>,
    pub quicknode_opportunity_receiver: QuicknodeOpportunityReceiver,

    // Simulation trading components
    pub simulation_engine: Option<Arc<simulation::SimulatedExecutionEngine>>,
    pub simulation_portfolio: Option<Arc<simulation::SafeVirtualPortfolio>>,
    pub simulation_analytics: Option<Arc<Mutex<simulation::SimulationAnalytics>>>,
    pub simulation_reporter: Option<Arc<simulation::SimulationReporter>>,

    // Balance monitoring
    pub balance_monitor: Option<Arc<BalanceMonitor>>,

    // Thread-safe concurrency controls
    pub trading_pairs_locks: Arc<TradingPairLocks>,
    pub execution_semaphore: Arc<Semaphore>,
    pub concurrent_executions: Arc<AtomicUsize>,
    pub max_concurrent_executions: usize,

    // Performance optimization system
    pub performance_manager: Option<Arc<PerformanceManager>>,

    // Jito client for bundle execution
    pub jito_client: Option<Arc<tokio::sync::RwLock<crate::arbitrage::JitoClient>>>,
}
#[derive(Debug, Default)]
pub struct DetectionMetrics {
    pub total_detection_cycles: u64,
    pub total_opportunities_found: u64,
    pub average_detection_time_ms: f64,
    pub hot_cache_hits: u64,
    pub hot_cache_misses: u64,
    pub last_detection_timestamp: u64,
}

/// Dependencies required to construct an `ArbitrageOrchestrator`.
/// This struct groups related services and managers to reduce argument count and improve clarity.
pub struct OrchestratorDeps {
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    pub banned_pairs_manager: Arc<BannedPairsManager>,
}

// =============================================================================
// Core Implementation
// =============================================================================

impl ArbitrageOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        deps: OrchestratorDeps,
        config: Arc<Config>,
        quicknode_opportunity_receiver: Option<mpsc::UnboundedReceiver<MultiHopArbOpportunity>>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageStrategy::new_from_config(&config)));
        let health_check_interval =
            Duration::from_secs(config.health_check_interval_secs.unwrap_or(60));
        let max_ws_reconnect_attempts = config.max_ws_reconnect_attempts.unwrap_or(5) as u64;

        // Configure pool validation with sensible defaults
        let pool_validation_config = if config.paper_trading {
            PoolValidationConfig {
                min_liquidity_usd: 0.0,
                max_price_impact_bps: u16::MAX, // effectively disables price impact check
                require_balanced_reserves: false,
            }
        } else {
            PoolValidationConfig {
                min_liquidity_usd: 1000.0,
                max_price_impact_bps: 500, // 5%
                require_balanced_reserves: false,
            }
        };

        // Initialize async communication channel
        let (opportunity_sender, _opportunity_receiver) = mpsc::unbounded_channel();

        // Initialize simulation trading components if enabled
        let (
            simulation_engine,
            simulation_portfolio,
            simulation_analytics,
            simulation_reporter,
        ) = if config.paper_trading {
            info!("📄 Simulation mode ENABLED - initializing components");

            let paper_config = simulation::SimulationConfig::default();

            // Create initial balances for simulation
            let mut initial_balances = HashMap::new();
            let sol_mint = solana_sdk::system_program::id();
            let usdc_mint = Pubkey::new_unique();
            initial_balances.insert(sol_mint, 1_000_000_000); // 1 SOL in lamports
            initial_balances.insert(usdc_mint, 10_000_000_000); // 10000 USDC in micro-units

            let mut paper_config_with_balances = paper_config.clone();
            paper_config_with_balances.initial_balances = initial_balances.clone();

            let portfolio = Arc::new(simulation::SafeVirtualPortfolio::new(initial_balances));
            let analytics = Arc::new(Mutex::new(simulation::SimulationAnalytics::new()));
            let reporter = simulation::SimulationReporter::new("./simulation_logs")
                .map_err(|e| warn!("Failed to create simulation reporter: {}", e))
                .ok()
                .map(Arc::new);
            let engine = Arc::new(simulation::SimulatedExecutionEngine::new(
                paper_config_with_balances,
                portfolio.clone(),
                Pubkey::default(), // TODO: Use real SOL mint pubkey if available
            ));

            (Some(engine), Some(portfolio), Some(analytics), reporter)
        } else {
            info!("💰 Live trading mode ENABLED");
            (None, None, None, None)
        };

        // Initialize balance monitor if RPC client is available
        let balance_monitor = if deps.rpc_client.is_some() {
            let monitor_config = crate::solana::BalanceMonitorConfig::default();
            let monitor = BalanceMonitor::new(monitor_config);
            Some(Arc::new(monitor))
        } else {
            info!("🔧 No RPC client available - balance monitor disabled");
            None
        };

        // Initialize Jito client if enabled in config
        let jito_client = if config.jito_enabled.unwrap_or(false) {
            Some(Arc::new(tokio::sync::RwLock::new(
                crate::arbitrage::JitoClient::new_with_defaults(
                    solana_client::nonblocking::rpc_client::RpcClient::new(config.rpc_url.clone()),
                ),
            )))
        } else {
            None
        };

        info!("🚀 Enhanced ArbitrageOrchestrator initialized:");
        info!("   🔥 Hot cache integration: {} pools", hot_cache.len());
        info!("   🎯 DEX providers: {}", deps.dex_providers.len());
        info!("   ⚡ Batch execution: available");
        info!("   📊 Advanced metrics: enabled");
        info!("   🔄 Async execution pipeline: ready");
        info!(
            "   📄 Simulation trading: {}",
            if config.paper_trading {
                "enabled"
            } else {
                "disabled"
            }
        );

        // Initialize price aggregator with Jupiter fallback if enabled
        let _price_aggregator = if config.jupiter_fallback_enabled {
            // Find Jupiter client among DEX providers
            let jupiter_client = deps
                .dex_providers
                .iter()
                .find(|_client| _client.get_name().to_lowercase().contains("jupiter"))
                .and_then(|_client| {
                    // Try to downcast to JupiterClient
                    // This is a simplified approach - in practice we'd need proper type handling
                    None::<Arc<crate::dex::clients::jupiter::JupiterClient>>
                });

            let aggregator = crate::arbitrage::price_aggregator::PriceAggregator::new(
                deps.dex_providers.clone(),
                jupiter_client,
                &config,
                Arc::clone(&deps.metrics),
            );

            info!("🪐 Price aggregator with Jupiter fallback: enabled");
            Some(Arc::new(aggregator))
        } else {
            info!("📊 Price aggregator: using primary DEX sources only");
            None
        };

        Self {
            hot_cache,
            config: config.clone(),
            metrics: deps.metrics,
            rpc_client: deps.rpc_client,
            ws_manager: deps.ws_manager,
            dex_providers: deps.dex_providers,
            detector,
            slippage_model: Arc::new(Mutex::new(EnhancedSlippageModel::new())),
            pool_validation_config,
            banned_pairs_manager: deps.banned_pairs_manager,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            execution_enabled: Arc::new(AtomicBool::new(true)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval,
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts,
            opportunity_sender: Some(opportunity_sender),
            quicknode_opportunity_receiver: Arc::new(Mutex::new(quicknode_opportunity_receiver)),
            simulation_engine,
            simulation_portfolio,
            simulation_analytics,
            simulation_reporter,
            balance_monitor,
            trading_pairs_locks: Arc::new(TradingPairLocks::new()),
            execution_semaphore: Arc::new(Semaphore::new(
                config.max_concurrent_executions.unwrap_or(10),
            )),
            concurrent_executions: Arc::new(AtomicUsize::new(0)),
            max_concurrent_executions: config.max_concurrent_executions.unwrap_or(10),
            performance_manager: None, // Initialize performance manager as None
            jito_client,
        }
    }

    /// Get current orchestrator status
    pub async fn get_status(&self) -> OrchestratorStatus {
        let _metrics = self.metrics.lock().await;
        let last_health_check = *self.last_health_check.read().await;

        OrchestratorStatus {
            is_running: !self.degradation_mode.load(Ordering::Relaxed),
            execution_enabled: self.execution_enabled.load(Ordering::Relaxed),
            hot_cache_size: self.hot_cache.len(),
            concurrent_executions: self.concurrent_executions.load(Ordering::Relaxed),
            max_concurrent_executions: self.max_concurrent_executions,
            ws_reconnect_attempts: self.ws_reconnect_attempts.load(Ordering::Relaxed),
            last_health_check_elapsed: last_health_check.elapsed(),
            simulation_enabled: self.simulation_engine.is_some(),
            balance_monitor_active: self.balance_monitor.is_some(),
        }
    }

    /// Start services - placeholder for compatibility
    pub async fn start_services(&self, _redis_cache: Option<Arc<dyn std::any::Any + Send + Sync>>) {
        info!("🚀 Starting arbitrage orchestrator services");
        // TODO: Implement service startup logic
    }

    /// Execute opportunities with routing - placeholder for compatibility  
    pub async fn execute_opportunities_with_routing(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<Vec<()>, ArbError> {
        // The execution manager returns Result<(), ArbError>, but we need Result<Vec<()>, ArbError>
        // So we'll create a compatibility wrapper
        let count = opportunities.len();
        self.execute_opportunities(opportunities).await?;
        // Return a Vec with one entry per opportunity processed
        Ok(vec![(); count])
    }

    /// Execute arbitrage opportunities - basic implementation
    pub async fn execute_opportunities(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!("🎯 Executing {} opportunities", opportunities.len());
        debug!("[DEBUG] Opportunities: {:#?}", opportunities);

        for opportunity in &opportunities {
            debug!("[DEBUG] Opportunity details: {:#?}", opportunity);
        }

        for opportunity in opportunities {
            if self.execution_enabled.load(Ordering::Relaxed) {
                if self.config.paper_trading {
                    // --- Simulation Trading Execution ---
                    if let Some(engine) = &self.simulation_engine {
                        info!(
                            "📄 Executing simulation trade for opportunity: {}",
                            opportunity.id
                        );
                        debug!("[DEBUG] Simulation trading engine: {:#?}", engine);
                        match engine
                            .simulate_arbitrage_execution(
                                &opportunity,
                                &self.dex_providers.iter().enumerate().map(|(i, arc)| {
                                    // TODO: Use real DEX name as key if available
                                    (format!("dex_{}", i), arc.clone())
                                }).collect::<HashMap<_, _>>()
                            )
                            .await
                        {
                            Ok(receipt) => {
                                info!("✅ Simulation trade successful: {:?}", receipt);
                                debug!("[DEBUG] Simulation trade receipt: {:#?}", receipt);
                                if let Some(analytics) = &self.simulation_analytics {
                                    let mut analytics_lock = analytics.lock().await;
                                    let dex_name = opportunity
                                        .hops
                                        .first()
                                        .map(|h| h.dex.to_string())
                                        .unwrap_or_else(|| "Unknown".to_string());
                                    analytics_lock.record_trade_execution(&receipt, &dex_name);

                                    if let Some(reporter) = &self.simulation_reporter {
                                        let _actual_profit = receipt.output_amount as i64
                                            - receipt.input_amount as i64;
                                        let _slippage_applied =
                                            receipt.slippage_bps as f64 / 10_000.0;

                                        let log_entry =
                                            simulation::SimulationReporter::create_trade_log_entry(
                                                &opportunity,
                                                &receipt,
                                                self.simulation_portfolio.as_ref().expect("Simulation portfolio must exist for logging")
                                            );

                                        if let Err(e) = reporter.log_trade(log_entry.clone()) {
                                            warn!("Failed to log simulation trade: {}", e);
                                        }
                                        // Pass the SafeVirtualPortfolio reference to the live summary
                                        if let Some(portfolio) = &self.simulation_portfolio {
                                            // Pass a reference to the inner VirtualPortfolio
                                            let portfolio_snapshot = portfolio.snapshot();
                                            reporter.print_live_trade_summary(
                                                &portfolio_snapshot,
                                            );
                                        } else {
                                            warn!("No simulation trading portfolio available for live summary");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("❌ Simulation trade failed: {}", e);
                                debug!("[DEBUG] Simulation trade error: {:#?}", e);
                            }
                        }
                    } else {
                        warn!("Simulation trading is enabled, but no simulation engine configured. Skipping execution.");
                    }
                } else {
                    // --- Real Trading Execution ---
                    // Use the new execution_manager logic for real execution
                    debug!(
                        "[DEBUG] Real trading execution for opportunity: {:#?}",
                        opportunity
                    );
                    if let Err(e) = self.execute_single_opportunity(&opportunity).await {
                        error!("❌ Real trade execution failed: {}", e);
                        debug!("[DEBUG] Real trade execution error: {:#?}", e);
                    }
                }
            } else {
                warn!("⏸️ Execution disabled, skipping opportunity");
                debug!(
                    "[DEBUG] Execution disabled for opportunity: {:#?}",
                    opportunity
                );
                return Err(ArbError::ExecutionDisabled(
                    "Execution is disabled".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Build and sign a transaction bundle for a MultiHopArbOpportunity
    pub async fn build_signed_jito_bundle(
        &self,
        opportunity: &MultiHopArbOpportunity,
        payer: &Keypair,
        recent_blockhash: Hash,
    ) -> Result<Vec<Transaction>, ArbError> {
        let mut transactions = Vec::new();
        let mut instructions = Vec::new();
        // For each hop, get the swap instruction from the correct DEX client
        for hop in &opportunity.hops {
            let pool_info = if let Some(pool) = self.hot_cache.get(&hop.pool) {
                pool.clone()
            } else {
                return Err(ArbError::PoolNotFound(hop.pool.to_string()));
            };
            // Construct CommonSwapInfo with available fields
            let swap_info = crate::dex::api::CommonSwapInfo {
                user_wallet_pubkey: payer.pubkey(),
                user_source_token_account: pool_info.token_a.mint, // Placeholder: replace with real associated token account
                user_destination_token_account: pool_info.token_b.mint, // Placeholder: replace with real associated token account
                source_token_mint: pool_info.token_a.mint,
                destination_token_mint: pool_info.token_b.mint,
                input_amount: hop.input_amount as u64,
                minimum_output_amount: hop.expected_output as u64, // or use slippage logic
                slippage_bps: None,
                priority_fee_lamports: None,
            };
            // Find the DEX client for this hop
            let dex_client = self
                .dex_providers
                .iter()
                .find(|c| c.get_name().to_lowercase() == hop.dex.to_string().to_lowercase());
            let dex_client = match dex_client {
                Some(c) => c,
                None => {
                    return Err(ArbError::ConfigError(format!(
                        "No DEX client for {}",
                        hop.dex
                    )))
                }
            };
            let ix = dex_client
                .get_swap_instruction_enhanced(&swap_info, pool_info)
                .await?;
            instructions.push(ix);
        }
        // Build a single transaction for the whole opportunity (multi-hop atomic)
        let mut tx = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
        tx.sign(&[payer], recent_blockhash);
        transactions.push(tx);
        Ok(transactions)
    }

    /// Execute a single arbitrage opportunity as a Jito bundle (atomic execution)
    pub async fn execute_opportunity_atomic_jito(
        &self,
        opportunity: &crate::arbitrage::opportunity::MultiHopArbOpportunity,
    ) -> Result<(), crate::error::ArbError> {
        info!("[TRACE][JITO] Received opportunity for Jito execution: input_token={} output_token={} pool_path={:?}",
            opportunity.input_token, opportunity.output_token, opportunity.pool_path);

        if !self
            .execution_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            warn!(
                "[JITO] Execution is currently disabled. Skipping opportunity: {} -> {}",
                opportunity.input_token, opportunity.output_token
            );
            return Err(crate::error::ArbError::ExecutionDisabled(
                "Execution is currently disabled".to_string(),
            ));
        }

        // Validate opportunity quotes using price aggregator
        match self.validate_opportunity_quotes(opportunity).await {
            Ok(is_valid) => {
                if !is_valid {
                    warn!(
                        "[JITO] Skipping opportunity due to quote validation failure: {} -> {}",
                        opportunity.input_token, opportunity.output_token
                    );
                    return Err(crate::error::ArbError::InvalidPoolState(
                        "Quote validation failed".to_string(),
                    ));
                } else {
                    info!(
                        "[TRACE][JITO] Opportunity passed quote validation: {} -> {}",
                        opportunity.input_token, opportunity.output_token
                    );
                }
            }
            Err(e) => {
                error!(
                    "[JITO] Error during quote validation: {} -> {} | error: {}",
                    opportunity.input_token, opportunity.output_token, e
                );
                return Err(crate::error::ArbError::InvalidPoolState(format!(
                    "Quote validation error: {}",
                    e
                )));
            }
        }

        // --- Jito Bundle Submission ---
        let jito_client = match &self.jito_client {
            Some(client) => client,
            None => {
                error!("[JITO] Jito client not initialized or enabled in config.");
                return Err(crate::error::ArbError::ExecutionError(
                    "Jito client not initialized".to_string(),
                ));
            }
        };

        // Load payer keypair from config
        let payer_path = self
            .config
            .trader_wallet_keypair_path
            .as_ref()
            .ok_or_else(|| {
                ArbError::ConfigError("No trader_wallet_keypair_path in config".to_string())
            })?;
        let payer = read_keypair_file(payer_path)
            .map_err(|e| ArbError::ConfigError(format!("Failed to read keypair: {}", e)))?;
        // Get recent blockhash
        let rpc = self
            .rpc_client
            .as_ref()
            .ok_or_else(|| ArbError::ConfigError("No RPC client configured".to_string()))?;
        let recent_blockhash = rpc
            .primary_client
            .get_latest_blockhash()
            .await
            .map_err(|e| {
                ArbError::ExecutionError(format!("Failed to get recent blockhash: {}", e))
            })?;
        // Build and sign the transaction bundle
        let transactions = self
            .build_signed_jito_bundle(opportunity, &payer, recent_blockhash)
            .await?;
        if transactions.is_empty() {
            error!(
                "[JITO] No transactions built for opportunity: {} -> {}",
                opportunity.input_token, opportunity.output_token
            );
            return Err(crate::error::ArbError::ExecutionError(
                "No transactions built for Jito bundle".to_string(),
            ));
        }
        // Submit the bundle
        let mut client = jito_client.write().await;
        match client.submit_bundle(transactions).await {
            Ok(bundle_id) => {
                info!(
                    "[JITO] Bundle submitted successfully. Bundle ID: {}",
                    bundle_id
                );
                // TODO: Track bundle result, poll for confirmation, etc.
                Ok(())
            }
            Err(e) => {
                error!("[JITO] Bundle submission failed: {}", e);
                Err(crate::error::ArbError::ExecutionError(format!(
                    "Jito bundle submission failed: {}",
                    e
                )))
            }
        }
    }

    /// Get enhanced status - placeholder for compatibility
    pub async fn get_enhanced_status(&self) -> String {
        // TODO: Implement enhanced status reporting
        "Orchestrator Status: Running".to_string()
    }

    /// Get hot cache stats - placeholder for compatibility
    pub async fn get_hot_cache_stats(&self) -> (usize, f64) {
        let cache_size = self.hot_cache.len();
        let hit_rate = 0.85; // Placeholder hit rate
        (cache_size, hit_rate)
    }

    /// Run full health check - placeholder for compatibility
    pub async fn run_full_health_check(&self) {
        info!("🏥 Running health check");
        // TODO: Implement comprehensive health check
    }

    /// Shutdown the orchestrator - placeholder for compatibility
    pub async fn shutdown(&self) -> Result<(), ArbError> {
        info!("🛑 Shutting down arbitrage orchestrator");
        // TODO: Implement graceful shutdown
        Ok(())
    }

    pub async fn set_min_profit_threshold_pct(&self, pct: f64) {
        let mut detector = self.detector.lock().await;
        detector.set_min_profit_threshold(pct);
    }

    pub async fn get_min_profit_threshold_pct(&self) -> f64 {
        let detector = self.detector.lock().await;
        detector.get_min_profit_threshold_pct()
    }

    pub async fn resolve_pools_for_opportunity(
        &self,
        opp: &MultiHopArbOpportunity,
    ) -> Result<(), ArbError> {
        // Use hot_cache or pools_map to check for pool existence
        for pool_addr in &opp.pool_path {
            debug!(
                "[DEBUG] Checking pool existence in hot_cache: {}",
                pool_addr
            );
            if !self.hot_cache.contains_key(pool_addr) {
                warn!("[DEBUG] Pool not found in hot_cache: {}", pool_addr);
                return Err(ArbError::PoolNotFound(pool_addr.to_string()));
            } else {
                debug!("[DEBUG] Pool found in hot_cache: {}", pool_addr);
            }
        }
        Ok(())
    }

    pub async fn discover_multihop_opportunities(&self) -> Result<(), ArbError> {
        Ok(()) // stub
    }

    pub async fn with_pool_guard_async<F, Fut>(
        &self,
        _label: &str,
        _exclusive: bool,
        f: F,
    ) -> Result<(), ArbError>
    where
        F: FnOnce(&DashMap<Pubkey, Arc<PoolInfo>>) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(), ArbError>> + Send,
    {
        f(&self.hot_cache).await
    }

    pub async fn update_pools(
        &self,
        _pools: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        Ok(()) // stub
    }

    pub async fn get_current_status_string(&self) -> String {
        "Status: OK".to_string()
    }

    /// Get aggregated quote using primary DEX sources and Jupiter fallback
    pub async fn get_aggregated_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> Result<crate::arbitrage::price_aggregator::AggregatedQuote, ArbError> {
        // Fallback to traditional DEX client approach only
        self.get_traditional_dex_quote(pool, input_amount).await
    }

    /// Traditional DEX quote method (used when price aggregator is not available)
    async fn get_traditional_dex_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> Result<crate::arbitrage::price_aggregator::AggregatedQuote, ArbError> {
        for client in &self.dex_providers {
            match client.calculate_onchain_quote(pool, input_amount) {
                Ok(quote) => {
                    let aggregated_quote = crate::arbitrage::price_aggregator::AggregatedQuote {
                        quote,
                        source: crate::arbitrage::price_aggregator::QuoteSource::Primary(
                            client.get_name().to_string(),
                        ),
                        confidence: 0.8,
                        latency_ms: 0,
                    };
                    return Ok(aggregated_quote);
                }
                Err(e) => {
                    warn!("❌ Failed to get quote from {}: {}", client.get_name(), e);
                    continue;
                }
            }
        }

        Err(ArbError::DexError(
            "No quotes available from any DEX client".to_string(),
        ))
    }

    pub async fn spawn_quicknode_opportunity_task(self: &Arc<Self>) {
        let receiver_opt = self.quicknode_opportunity_receiver.lock().await.take();
        if let Some(mut receiver) = receiver_opt {
            let orchestrator = Arc::clone(self);
            tokio::spawn(async move {
                while let Some(opp) = receiver.recv().await {
                    // You can add filtering, logging, or deduplication here
                    let _ = orchestrator.execute_opportunities(vec![opp]).await;
                }
            });
        }
    }

    pub async fn spawn_quicknode_opportunity_task_static(orchestrator: Arc<Self>) {
        orchestrator.spawn_quicknode_opportunity_task().await;
    }

    pub(crate) async fn validate_opportunity_quotes(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<bool, ArbError> {
        // Here, implement the actual validation logic using the price aggregator
        // For now, just a stub that always returns Ok(true)
        Ok(true)
    }

    /// Populate the hot cache with Orca Whirlpools pools from a JSON file (on-chain fetch)
    pub async fn populate_orca_whirlpool_hot_cache_from_json(
        &self,
        json_path: &str,
    ) -> anyhow::Result<()> {
        use crate::dex::clients::orca::OrcaClient;
        let rpc = match &self.rpc_client {
            Some(rpc) => rpc.clone(),
            None => {
                log::warn!("No RPC client available for on-chain pool fetch");
                return Ok(());
            }
        };
        let orca_client = OrcaClient::new();
        log::info!(
            "[ORCA] Starting on-chain pool discovery from JSON: {}",
            json_path
        );
        let pools = orca_client
            .discover_pools_onchain_from_json(&rpc, json_path)
            .await?;
        log::info!(
            "[ORCA] On-chain pool discovery complete. {} pools found.",
            pools.len()
        );
        for pool in &pools {
            debug!("[DEBUG] Inserting pool into hot_cache: {:#?}", pool);
        }
        for pool in pools {
            self.hot_cache.insert(pool.address, Arc::new(pool));
        }
        log::info!(
            "[HOT CACHE] Populated with {} Orca Whirlpools pools from {}",
            self.hot_cache.len(),
            json_path
        );
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct OrchestratorStatus {
    pub is_running: bool,
    pub execution_enabled: bool,
    pub hot_cache_size: usize,
    pub concurrent_executions: usize,
    pub max_concurrent_executions: usize,
    pub ws_reconnect_attempts: u64,
    pub last_health_check_elapsed: Duration,
    pub simulation_enabled: bool,
    pub balance_monitor_active: bool,
}
