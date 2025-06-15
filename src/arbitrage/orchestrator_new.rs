// src/arbitrage/orchestrator_new.rs
// Simplified and refactored ArbitrageOrchestrator
use crate::{
    arbitrage::{
        execution_manager::ExecutionManager,
        market_data::{MarketDataManager, PriceDataProvider},
        strategy_manager::StrategyManager,
        types::DetectionMetrics,
    },
    config::settings::Config,
    dex::{BannedPairsManager, DexClient},
    error::ArbError,
    metrics::Metrics,
    paper_trading::{PaperTradingAnalytics, PaperTradingConfig, PaperTradingReporter, SafeVirtualPortfolio, SimulatedExecutionEngine},
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager, BalanceMonitor, BalanceMonitorConfig},
    utils::PoolInfo,
};
use dashmap::DashMap;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

/// Enhanced arbitrage orchestrator - now simplified and modular
/// The main coordinator for all arbitrage operations
pub struct ArbitrageOrchestrator {
    pub market_data: MarketDataManager,
    pub strategy: StrategyManager,
    pub execution: ExecutionManager,
    pub config: Arc<Config>,
    pub metrics: Arc<Mutex<Metrics>>,
}

impl ArbitrageOrchestrator {
    pub async fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn PriceDataProvider>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_providers: Vec<Arc<dyn DexClient>>,
        executor: Option<Arc<crate::arbitrage::execution::HftExecutor>>,
        batch_execution_engine: Option<Arc<crate::arbitrage::execution::BatchExecutor>>,
        banned_pairs_manager: Arc<BannedPairsManager>,
    ) -> Self {
        info!("🚀 Initializing modular ArbitrageOrchestrator...");

        // Initialize market data manager
        let market_data = MarketDataManager::new(
            hot_cache,
            ws_manager,
            price_provider,
            rpc_client.clone(),
            dex_providers,
            &config,
        );

        // Initialize strategy manager
        let strategy = StrategyManager::new(
            &config,
            Arc::clone(&metrics),
            banned_pairs_manager,
        );

        // Initialize balance monitor for safety
        let balance_monitor = if rpc_client.is_some() {
            info!("💰 Initializing balance monitor for real-time safety checks");
            let monitor_config = BalanceMonitorConfig::default();
            Some(Arc::new(BalanceMonitor::new(monitor_config)))
        } else {
            info!("🔧 No RPC client available - balance monitor disabled");
            None
        };

        // Initialize paper trading components if enabled
        let (paper_trading_engine, paper_trading_portfolio, paper_trading_analytics, paper_trading_reporter) = 
            if config.paper_trading {
                info!("📄 Paper trading mode ENABLED - initializing simulation components");
                
                let paper_config = PaperTradingConfig::default();
                
                // Create initial balances for paper trading (e.g., 1 SOL, 10000 USDC)
                let mut initial_balances = HashMap::new();
                let sol_mint = solana_sdk::system_program::id(); // Simplified - would use actual SOL mint
                let usdc_mint = Pubkey::new_unique(); // Simplified - would use actual USDC mint
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
                info!("💰 Real trading mode - paper trading disabled");
                (None, None, None, None)
            };

        // Initialize execution manager
        let execution = ExecutionManager::new(
            executor,
            batch_execution_engine,
            balance_monitor,
            paper_trading_engine,
            paper_trading_portfolio,
            paper_trading_analytics,
            paper_trading_reporter,
            &config,
        );

        info!("🚀 Enhanced ArbitrageOrchestrator initialized with modular architecture:");
        info!("   🔥 Hot cache integration: {} pools", market_data.hot_cache.len());
        info!("   🎯 DEX providers: {}", market_data.dex_providers.len());
        info!("   ⚡ Execution manager: ready");
        info!("   📊 Strategy manager: ready");
        info!("   📈 Market data manager: ready");
        info!("   📄 Paper trading: {}", if config.paper_trading { "enabled" } else { "disabled" });

        Self {
            market_data,
            strategy,
            execution,
            config,
            metrics,
        }
    }

    /// Main arbitrage detection and execution cycle
    pub async fn run_arbitrage_cycle(&self) -> Result<(), ArbError> {
        info!("🔄 Starting arbitrage cycle...");

        // 1. Health check
        self.market_data.check_health(&self.metrics).await?;

        // 2. Skip execution if in degradation mode
        if self.market_data.is_degraded() {
            warn!("🚨 System in degradation mode - skipping arbitrage cycle");
            return Ok(());
        }

        // 3. Detect opportunities
        let opportunities = self.strategy.detect_arbitrage_opportunities(&self.market_data.hot_cache).await?;

        // 4. Execute opportunities
        if !opportunities.is_empty() {
            info!("🎯 Found {} opportunities, executing...", opportunities.len());
            self.execution.execute_opportunities(opportunities).await?;
        } else {
            info!("📊 No arbitrage opportunities found this cycle");
        }

        // 5. Update dynamic thresholds
        self.strategy.update_dynamic_thresholds().await?;

        info!("✅ Arbitrage cycle completed");
        Ok(())
    }

    /// Get comprehensive system status
    pub async fn get_status(&self) -> SystemStatus {
        let detection_metrics = self.strategy.get_detection_metrics().await;
        let (cache_size, cache_hit_rate) = self.market_data.get_cache_stats();
        let execution_enabled = self.execution.is_execution_enabled();
        let is_degraded = self.market_data.is_degraded();

        SystemStatus {
            is_healthy: !is_degraded,
            execution_enabled,
            degradation_mode: is_degraded,
            cache_size,
            cache_hit_rate,
            detection_metrics,
            paper_trading_mode: self.config.paper_trading,
        }
    }

    /// Enable or disable execution
    pub fn set_execution_enabled(&self, enabled: bool) {
        self.execution.set_execution_enabled(enabled);
    }

    /// Force exit degradation mode
    pub fn exit_degradation_mode(&self) {
        self.market_data.exit_degradation_mode();
    }

    /// Reset detection metrics
    pub async fn reset_metrics(&self) {
        self.strategy.reset_detection_metrics().await;
        info!("📊 System metrics reset");
    }
}

/// Comprehensive system status
#[derive(Debug)]
pub struct SystemStatus {
    pub is_healthy: bool,
    pub execution_enabled: bool,
    pub degradation_mode: bool,
    pub cache_size: usize,
    pub cache_hit_rate: f64,
    pub detection_metrics: DetectionMetrics,
    pub paper_trading_mode: bool,
}

impl SystemStatus {
    pub fn display(&self) {
        info!("📊 System Status:");
        info!("   Health: {}", if self.is_healthy { "✅ Healthy" } else { "❌ Degraded" });
        info!("   Execution: {}", if self.execution_enabled { "✅ Enabled" } else { "⏸️ Disabled" });
        info!("   Cache: {} pools ({:.1}% hit rate)", self.cache_size, self.cache_hit_rate);
        info!("   Detection: {} cycles, {} opportunities", 
              self.detection_metrics.total_detection_cycles, 
              self.detection_metrics.total_opportunities_found);
        info!("   Mode: {}", if self.paper_trading_mode { "📄 Paper Trading" } else { "💰 Real Trading" });
    }
}
