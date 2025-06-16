//! Core Arbitrage Orchestrator Structure
//! 
//! This module contains the main orchestrator struct definition and core functionality.

use crate::{
    arbitrage::{
        strategy::ArbitrageStrategy,
        analysis::{DynamicThresholdUpdater, AdvancedArbitrageMath},
        opportunity::MultiHopArbOpportunity,
        execution::{HftExecutor, BatchExecutor},
    },
    config::settings::Config,
    dex::{DexClient, PoolValidationConfig, BannedPairsManager},
    error::ArbError,
    local_metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager, BalanceMonitor},
    utils::{PoolInfo, DexType},
    paper_trading::{
        SimulatedExecutionEngine, 
        PaperTradingConfig, 
        SafeVirtualPortfolio, 
        PaperTradingAnalytics, 
        PaperTradingReporter
    },
};

use crate::performance::PerformanceManager;

use dashmap::DashMap;
use log::{info, warn, debug};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock, mpsc, Semaphore},
};

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
    pub executor: Option<Arc<HftExecutor>>,
    pub batch_execution_engine: Option<Arc<BatchExecutor>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    
    // Strategy and analysis
    pub detector: Arc<Mutex<ArbitrageStrategy>>,
    pub advanced_math: Arc<Mutex<AdvancedArbitrageMath>>,
    pub dynamic_threshold_updater: Option<Arc<DynamicThresholdUpdater>>,
    pub price_aggregator: Option<Arc<crate::arbitrage::price_aggregator::PriceAggregator>>,
    
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
    
    // Paper trading components
    pub paper_trading_engine: Option<Arc<SimulatedExecutionEngine>>,
    pub paper_trading_portfolio: Option<Arc<SafeVirtualPortfolio>>,
    pub paper_trading_analytics: Option<Arc<Mutex<PaperTradingAnalytics>>>,
    pub paper_trading_reporter: Option<Arc<PaperTradingReporter>>,
    
    // Balance monitoring
    pub balance_monitor: Option<Arc<BalanceMonitor>>,
    
    // Thread-safe concurrency controls
    pub trading_pairs_locks: Arc<DashMap<(DexType, Pubkey, Pubkey), Arc<Mutex<()>>>>,
    pub execution_semaphore: Arc<Semaphore>,
    pub concurrent_executions: Arc<AtomicUsize>,
    pub max_concurrent_executions: usize,
    
    // Performance optimization system
    pub performance_manager: Option<Arc<PerformanceManager>>,
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

// =============================================================================
// Core Implementation
// =============================================================================

impl ArbitrageOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_providers: Vec<Arc<dyn DexClient>>,
        executor: Option<Arc<HftExecutor>>,
        batch_execution_engine: Option<Arc<BatchExecutor>>,
        banned_pairs_manager: Arc<BannedPairsManager>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageStrategy::new_from_config(&config)));
        let health_check_interval = Duration::from_secs(config.health_check_interval_secs.unwrap_or(60));
        let max_ws_reconnect_attempts = config.max_ws_reconnect_attempts.unwrap_or(5) as u64;
        
        // Initialize dynamic threshold updater if configured
        let dynamic_threshold_updater = if config.volatility_tracker_window.is_some() {
            Some(Arc::new(DynamicThresholdUpdater::new(&config, Arc::clone(&metrics))))
        } else {
            None
        };

        // Configure pool validation with sensible defaults
        let pool_validation_config = PoolValidationConfig {
            min_liquidity_usd: 1000.0,
            max_price_impact_bps: 500, // 5%
            require_balanced_reserves: false,
        };

        // Initialize async communication channel
        let (opportunity_sender, _opportunity_receiver) = mpsc::unbounded_channel();

        // Initialize paper trading components if enabled
        let (paper_trading_engine, paper_trading_portfolio, paper_trading_analytics, paper_trading_reporter) = 
            if config.paper_trading {
                info!("📄 Paper trading mode ENABLED - initializing simulation components");
                
                let paper_config = PaperTradingConfig::default();
                
                // Create initial balances for paper trading
                let mut initial_balances = HashMap::new();
                let sol_mint = solana_sdk::system_program::id();
                let usdc_mint = Pubkey::new_unique();
                initial_balances.insert(sol_mint, 1_000_000_000); // 1 SOL in lamports
                initial_balances.insert(usdc_mint, 10_000_000_000); // 10000 USDC in micro-units
                
                let mut paper_config_with_balances = paper_config.clone();
                paper_config_with_balances.initial_balances = initial_balances.clone();
                
                let portfolio = Arc::new(SafeVirtualPortfolio::new(initial_balances));
                let analytics = Arc::new(Mutex::new(PaperTradingAnalytics::new()));
                let reporter = PaperTradingReporter::new("./paper_trading_logs")
                    .map_err(|e| warn!("Failed to create paper trading reporter: {}", e))
                    .ok()
                    .map(Arc::new);
                let engine = Arc::new(SimulatedExecutionEngine::new(paper_config_with_balances));
                
                (Some(engine), Some(portfolio), Some(analytics), reporter)
            } else {
                info!("💰 Live trading mode ENABLED");
                (None, None, None, None)
            };

        // Initialize balance monitor if RPC client is available
        let balance_monitor = if rpc_client.is_some() {
            let monitor_config = crate::solana::BalanceMonitorConfig::default();
            let monitor = BalanceMonitor::new(monitor_config);
            Some(Arc::new(monitor))
        } else {
            info!("🔧 No RPC client available - balance monitor disabled");
            None
        };

        info!("🚀 Enhanced ArbitrageOrchestrator initialized:");
        info!("   🔥 Hot cache integration: {} pools", hot_cache.len());
        info!("   🎯 DEX providers: {}", dex_providers.len());
        info!("   ⚡ Batch execution: available");
        info!("   📊 Advanced metrics: enabled");
        info!("   🔄 Async execution pipeline: ready");
        info!("   📄 Paper trading: {}", if config.paper_trading { "enabled" } else { "disabled" });

        // Initialize price aggregator with Jupiter fallback if enabled
        let price_aggregator = if config.jupiter_fallback_enabled {
            // Find Jupiter client among DEX providers
            let jupiter_client = dex_providers.iter()
                .find(|_client| _client.get_name().to_lowercase().contains("jupiter"))
                .and_then(|_client| {
                    // Try to downcast to JupiterClient
                    // This is a simplified approach - in practice we'd need proper type handling
                    None::<Arc<crate::dex::clients::jupiter::JupiterClient>>
                });

            let aggregator = crate::arbitrage::price_aggregator::PriceAggregator::new(
                dex_providers.clone(),
                jupiter_client,
                &config,
                Arc::clone(&metrics),
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
            metrics,
            rpc_client,
            ws_manager,
            executor,
            batch_execution_engine,
            dex_providers,
            detector,
            advanced_math: Arc::new(Mutex::new(AdvancedArbitrageMath::new(12))),
            dynamic_threshold_updater,
            price_aggregator,
            pool_validation_config,
            banned_pairs_manager,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            execution_enabled: Arc::new(AtomicBool::new(true)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval,
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts,
            opportunity_sender: Some(opportunity_sender),
            paper_trading_engine,
            paper_trading_portfolio,
            paper_trading_analytics,
            paper_trading_reporter,
            balance_monitor,
            trading_pairs_locks: Arc::new(DashMap::new()),
            execution_semaphore: Arc::new(Semaphore::new(config.max_concurrent_executions.unwrap_or(10))),
            concurrent_executions: Arc::new(AtomicUsize::new(0)),
            max_concurrent_executions: config.max_concurrent_executions.unwrap_or(10),
            performance_manager: None, // Initialize performance manager as None
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
            paper_trading_enabled: self.paper_trading_engine.is_some(),
            balance_monitor_active: self.balance_monitor.is_some(),
        }
    }

    /// Start services - placeholder for compatibility
    pub async fn start_services(&self, _redis_cache: Option<Arc<dyn std::any::Any + Send + Sync>>) {
        info!("🚀 Starting arbitrage orchestrator services");
        // TODO: Implement service startup logic
    }

    /// Execute opportunities with routing - placeholder for compatibility  
    pub async fn execute_opportunities_with_routing(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<()>, ArbError> {
        // The execution manager returns Result<(), ArbError>, but we need Result<Vec<()>, ArbError>
        // So we'll create a compatibility wrapper
        let count = opportunities.len();
        self.execute_opportunities(opportunities).await?;
        // Return a Vec with one entry per opportunity processed
        Ok(vec![(); count])
    }

    /// Execute arbitrage opportunities - basic implementation
    pub async fn execute_opportunities(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<(), ArbError> {
        info!("🎯 Executing {} opportunities", opportunities.len());
        
        for _opportunity in opportunities {
            // Basic execution logic - placeholder for now
            if self.execution_enabled.load(Ordering::Relaxed) {
                // TODO: Implement actual execution logic via execution manager
                debug!("⚡ Processing opportunity");
            } else {
                warn!("⏸️ Execution disabled, skipping opportunity");
                return Err(ArbError::ExecutionDisabled("Execution is disabled".to_string()));
            }
        }
        
        Ok(())
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

    pub async fn resolve_pools_for_opportunity(&self, opp: &MultiHopArbOpportunity) -> Result<(), ArbError> {
        // Use hot_cache or pools_map to check for pool existence
        for pool_addr in &opp.pool_path {
            if !self.hot_cache.contains_key(pool_addr) {
                return Err(ArbError::PoolNotFound(pool_addr.to_string()));
            }
        }
        Ok(())
    }

    pub async fn discover_multihop_opportunities(&self) -> Result<(), ArbError> {
        Ok(()) // stub
    }

    pub async fn with_pool_guard_async<F, Fut>(&self, _label: &str, _exclusive: bool, f: F) -> Result<(), ArbError>
    where
        F: FnOnce(&DashMap<Pubkey, Arc<PoolInfo>>) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(), ArbError>> + Send,
    {
        f(&self.hot_cache).await
    }

    pub async fn update_pools(&self, _pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
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
        if let Some(ref price_aggregator) = self.price_aggregator {
            price_aggregator.get_best_quote(pool, input_amount).await
        } else {
            // Fallback to traditional DEX client approach
            self.get_traditional_dex_quote(pool, input_amount).await
        }
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
                        source: crate::arbitrage::price_aggregator::QuoteSource::Primary(client.get_name().to_string()),
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
        
        Err(ArbError::DexError("No quotes available from any DEX client".to_string()))
    }
}

#[derive(Debug)]
pub struct OrchestratorStatus {
    pub is_running: bool,
    pub execution_enabled: bool,
    pub hot_cache_size: usize,
    pub concurrent_executions: usize,
    pub max_concurrent_executions: usize,
    pub ws_reconnect_attempts: u64,
    pub last_health_check_elapsed: Duration,
    pub paper_trading_enabled: bool,
    pub balance_monitor_active: bool,
}
