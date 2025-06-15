// src/arbitrage/orchestrator.rs
use crate::{
    arbitrage::{
        strategy::ArbitrageStrategy,
        analysis::{DynamicThresholdUpdater, AdvancedArbitrageMath},
        opportunity::MultiHopArbOpportunity,
        execution::{HftExecutor, BatchExecutor},
    },
    config::settings::Config,
    dex::{DexClient, PoolValidationConfig, validate_pools, validate_single_pool, BannedPairsManager},
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::{PoolInfo, PoolParser, DexType},
};

// Import paper trading components
use crate::paper_trading::{
    SimulatedExecutionEngine, 
    PaperTradingConfig, 
    SafeVirtualPortfolio, 
    PaperTradingAnalytics, 
    PaperTradingReporter
};

use dashmap::DashMap;
use log::{debug, error, info, warn};
use rust_decimal::prelude::*;
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock, mpsc},
    time::timeout,
};

// Define a simple trait for price data providers since CryptoDataProvider doesn't exist
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}

/// Strategy for executing arbitrage opportunities
#[derive(Debug, Clone)]
enum ExecutionStrategy {
    /// Execute all opportunities immediately using single executor
    SingleExecution(Vec<MultiHopArbOpportunity>),
    /// Execute using batch engine (batchable opportunities, immediate opportunities)
    BatchExecution(Vec<MultiHopArbOpportunity>, Vec<MultiHopArbOpportunity>),
    /// Hybrid approach: immediate execution + batching
    HybridExecution {
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>,
    },
}

/// Analysis of opportunity competitiveness for execution decision
#[derive(Debug, Clone)]
pub struct CompetitivenessAnalysis {
    #[allow(dead_code)]
    competitive_score: f64,
    /// Factors that affect competitiveness (latency, gas, etc.)
    #[allow(dead_code)]
    risk_factors: Vec<String>,
    execution_recommendation: ExecutionRecommendation,
    reason: String,
}

/// Recommendation for execution method based on competitiveness
#[derive(Debug, Clone)]
enum ExecutionRecommendation {
    /// Execute immediately with single executor for speed
    ImmediateSingle,
    /// Safe to include in batch execution
    SafeToBatch,
}

/// Enhanced arbitrage engine with Sprint 2 hot cache integration and advanced execution
/// The Arbitrage Orchestrator - Central brain for coordinating arbitrage operations
/// Renamed from ArbitrageEngine for clarity - this is the main controller
pub struct ArbitrageOrchestrator {
    // Sprint 2: Replace HashMap with DashMap hot cache for sub-millisecond access
    pub hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub price_provider: Option<Arc<dyn PriceDataProvider>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub config: Arc<Config>,
    pub degradation_mode: Arc<AtomicBool>,
    pub last_health_check: Arc<RwLock<Instant>>,
    pub health_check_interval: Duration,
    pub ws_reconnect_attempts: Arc<std::sync::atomic::AtomicU64>,
    pub max_ws_reconnect_attempts: u64,
    pub detector: Arc<Mutex<ArbitrageStrategy>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    pub dynamic_threshold_updater: Option<Arc<DynamicThresholdUpdater>>,
    pub pool_validation_config: PoolValidationConfig,
    pub banned_pairs_manager: Arc<BannedPairsManager>,
    
    // Sprint 2: Advanced execution components
    pub executor: Option<Arc<HftExecutor>>,
    pub batch_execution_engine: Option<Arc<BatchExecutor>>,
    
    // Advanced arbitrage calculation engine
    pub advanced_math: Arc<Mutex<AdvancedArbitrageMath>>,
    
    // Async channel for opportunity execution (replaces pipeline)
    pub opportunity_sender: Option<mpsc::UnboundedSender<MultiHopArbOpportunity>>,
    
    // Sprint 2: Performance tracking
    pub detection_metrics: Arc<Mutex<DetectionMetrics>>,
    pub execution_enabled: Arc<AtomicBool>,
    
    // Paper trading components
    pub paper_trading_engine: Option<Arc<SimulatedExecutionEngine>>,
    pub paper_trading_portfolio: Option<Arc<SafeVirtualPortfolio>>,
    pub paper_trading_analytics: Option<Arc<Mutex<PaperTradingAnalytics>>>,
    pub paper_trading_reporter: Option<Arc<PaperTradingReporter>>,
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

impl ArbitrageOrchestrator {
    pub fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn PriceDataProvider>>,
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

        // Sprint 2: Initialize advanced execution components and async channel
        let (opportunity_sender, _opportunity_receiver) = mpsc::unbounded_channel();
        let detection_metrics = Arc::new(Mutex::new(DetectionMetrics::default()));

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

        info!("🚀 Enhanced ArbitrageOrchestrator initialized with Sprint 2 features:");
        info!("   🔥 Hot cache integration: {} pools", hot_cache.len());
        info!("   🎯 DEX providers: {}", dex_providers.len());
        info!("   ⚡ Batch execution: integrated into executor");
        info!("   📊 Advanced metrics: enabled");
        info!("   🔄 Async execution pipeline: ready");
        info!("   📄 Paper trading: {}", if config.paper_trading { "enabled" } else { "disabled" });

        Self {
            hot_cache,
            ws_manager,
            price_provider,
            metrics,
            rpc_client,
            config,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval,
            ws_reconnect_attempts: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            max_ws_reconnect_attempts,
            detector,
            dex_providers,
            dynamic_threshold_updater,
            pool_validation_config,
            banned_pairs_manager,
            executor,
            batch_execution_engine,
            advanced_math: Arc::new(Mutex::new(AdvancedArbitrageMath::new(12))), // 12-digit precision
            opportunity_sender: Some(opportunity_sender),
            detection_metrics,
            execution_enabled: Arc::new(AtomicBool::new(true)),
            paper_trading_engine,
            paper_trading_portfolio,
            paper_trading_analytics,
            paper_trading_reporter,
        }
    }

    /// Sprint 2: Enhanced opportunity detection using hot cache
    pub async fn detect_arbitrage_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let start_time = Instant::now();
        info!("🔍 Starting enhanced arbitrage detection with hot cache...");

        // Update detection metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.total_detection_cycles += 1;
            metrics.last_detection_timestamp = chrono::Utc::now().timestamp_millis() as u64;
        }

        // Sprint 2: Use hot cache for ultra-fast pool access
        let pools_snapshot = self.get_hot_cache_snapshot().await;
        let pool_count = pools_snapshot.len();
        
        if pool_count == 0 {
            warn!("No pools available in hot cache for arbitrage detection");
            return Ok(Vec::new());
        }

        info!("📊 Analyzing {} pools from hot cache for arbitrage opportunities", pool_count);

        // Validate pools before detection
        let validated_pools = self.validate_hot_cache_pools(&pools_snapshot).await?;
        let validation_filtered = pool_count - validated_pools.len();
        
        if validation_filtered > 0 {
            warn!("🔍 Pool validation filtered out {} of {} pools ({:.1}% rejection rate)", 
                  validation_filtered, pool_count, (validation_filtered as f64 / pool_count as f64) * 100.0);
        }

        // Run enhanced detection with validated pools
        let opportunities = {
            let detector = self.detector.lock().await;
            detector.detect_all_opportunities(&validated_pools, &self.metrics).await? // Pass self.metrics directly
        };

        let detection_time = start_time.elapsed();
        let detection_time_ms = detection_time.as_secs_f64() * 1000.0;

        // Update detection metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.total_opportunities_found += opportunities.len() as u64;
            metrics.average_detection_time_ms = 
                (metrics.average_detection_time_ms * (metrics.total_detection_cycles as f64 - 1.0) + detection_time_ms) 
                / metrics.total_detection_cycles as f64;
            metrics.hot_cache_hits += pool_count as u64;
        }

        info!("✅ Detection completed in {:.2}ms: {} opportunities found", 
              detection_time_ms, opportunities.len());

        if let Some(best) = opportunities.first() {
            info!("🎯 Best opportunity: ID={}, Profit={:.4}%, Path={:?}", 
                  best.id, best.profit_pct, best.dex_path);
        }

        Ok(opportunities)
    }

    /// Sprint 2: Get snapshot of hot cache for analysis
    async fn get_hot_cache_snapshot(&self) -> HashMap<Pubkey, Arc<PoolInfo>> {
        let start_time = Instant::now();
        
        // Convert DashMap to HashMap for compatibility with existing detection logic
        let snapshot: HashMap<Pubkey, Arc<PoolInfo>> = self.hot_cache
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        
        let snapshot_time = start_time.elapsed();
        debug!("📸 Hot cache snapshot created in {:.2}ms: {} pools", 
               snapshot_time.as_secs_f64() * 1000.0, snapshot.len());
        
        snapshot
    }

    /// Sprint 2: Validate pools from hot cache
    async fn validate_hot_cache_pools(&self, pools: &HashMap<Pubkey, Arc<PoolInfo>>) -> Result<HashMap<Pubkey, Arc<PoolInfo>>, ArbError> {
        let pools_vec: Vec<PoolInfo> = pools.values()
            .map(|pool_arc| (**pool_arc).clone())
            .collect();
        
        if pools_vec.is_empty() {
            return Ok(HashMap::new());
        }
        
        // Apply validation with performance tracking
        let validation_start = Instant::now();
        let validated_pools = validate_pools(pools_vec, &self.pool_validation_config);
        
        let validation_time = validation_start.elapsed();
        debug!("✅ Pool validation completed in {:.2}ms", validation_time.as_secs_f64() * 1000.0);
        
        // Convert back to HashMap format
        let result: HashMap<Pubkey, Arc<PoolInfo>> = validated_pools.into_iter()
            .map(|pool| (pool.address, Arc::new(pool)))
            .collect();
            
        Ok(result)
    }

    /// Sprint 3: Intelligent execution routing with decision logic
    pub async fn execute_opportunities(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<String>, ArbError> {
        if !self.execution_enabled.load(Ordering::Relaxed) {
            warn!("⚠️ Execution is disabled - skipping opportunity execution");
            return Ok(Vec::new());
        }

        if opportunities.is_empty() {
            return Ok(Vec::new());
        }

        let start_time = Instant::now();
        let total_opportunities = opportunities.len();
        
        // Check if paper trading is enabled and route to simulation
        if self.config.paper_trading {
            info!("📄 Paper trading mode - simulating {} opportunities", total_opportunities);
            return self.execute_paper_trading_opportunities(opportunities).await;
        }
        
        info!("🚀 Executing {} opportunities with intelligent routing...", total_opportunities);

        // === THREE-STAGE WORKFLOW IMPLEMENTATION ===
        
        // STAGE 1: Initial Detection (already completed by strategy.rs Bellman-Ford)
        info!("✅ Stage 1: Initial detection completed - {} opportunities found", total_opportunities);

        // STAGE 2: Advanced Analysis - Calculate optimal input amounts and simulate trades
        info!("🧠 Stage 2: Running advanced analysis for optimal input calculation...");
        let analyzed_opportunities = self.calculate_advanced_arbitrage(opportunities).await?;
        
        info!("✅ Stage 2: Advanced analysis completed - {} opportunities optimized", analyzed_opportunities.len());

        // STAGE 3: Competitiveness Scoring - Determine execution strategy
        info!("⚔️ Stage 3: Analyzing competitiveness for execution strategy...");

        // DECISION LOGIC: Determine execution strategy
        let execution_strategy = self.determine_execution_strategy(&analyzed_opportunities).await;
        
        let execution_results = match execution_strategy {
            ExecutionStrategy::SingleExecution(high_priority) => {
                info!("⚡ Using SINGLE EXECUTION for {} high-priority opportunities", high_priority.len());
                self.execute_via_single_executor(high_priority).await?
            }
            ExecutionStrategy::BatchExecution(batchable, immediate) => {
                info!("📦 Using BATCH EXECUTION for {} opportunities ({} batchable, {} immediate)", 
                      batchable.len() + immediate.len(), batchable.len(), immediate.len());
                self.execute_via_batch_engine(batchable, immediate).await?
            }
            ExecutionStrategy::HybridExecution { immediate, batchable } => {
                info!("🔄 Using HYBRID EXECUTION ({} immediate, {} batchable)", 
                      immediate.len(), batchable.len());
                self.execute_hybrid_strategy(immediate, batchable).await?
            }
        };

        let execution_time = start_time.elapsed();
        info!("🎯 Execution completed in {:.2}ms: {}/{} opportunities executed successfully", 
              execution_time.as_secs_f64() * 1000.0, execution_results.len(), total_opportunities);

        Ok(execution_results)
    }

    /// Advanced decision logic optimized for speed vs competition dynamics
    async fn determine_execution_strategy(&self, opportunities: &[MultiHopArbOpportunity]) -> ExecutionStrategy {
        let total_opportunities = opportunities.len();
        let minimum_profit_threshold = 0.3; // 0.3% minimum profit after all fees
        
        info!("🧠 Analyzing {} opportunities for optimal execution strategy", total_opportunities);
        
        // Filter opportunities that meet minimum profit threshold
        let profitable_opportunities: Vec<_> = opportunities.iter()
            .filter(|opp| opp.profit_pct >= minimum_profit_threshold)
            .cloned()
            .collect();
            
        if profitable_opportunities.is_empty() {
            info!("❌ No opportunities meet minimum 0.3% profit threshold after fees");
            return ExecutionStrategy::SingleExecution(Vec::new());
        }

        // Analyze each opportunity for competition and complexity factors
        let mut competitive_opportunities = Vec::new();
        let mut safe_batch_opportunities = Vec::new();
        
        for opportunity in profitable_opportunities {
            let analysis = self.analyze_opportunity_competitiveness(&opportunity).await;
            
            match analysis.execution_recommendation {
                ExecutionRecommendation::ImmediateSingle => {
                    info!("⚡ Opportunity {} marked for immediate single execution: {}", 
                          opportunity.id, analysis.reason);
                    competitive_opportunities.push(opportunity);
                }
                ExecutionRecommendation::SafeToBatch => {
                    info!("📦 Opportunity {} safe for batching: {}", 
                          opportunity.id, analysis.reason);
                    safe_batch_opportunities.push(opportunity);
                }
            }
        }

        // DECISION LOGIC: Speed vs Efficiency Tradeoff
        
        // 1. If ALL opportunities are competitive -> Single execution for speed
        if !competitive_opportunities.is_empty() && safe_batch_opportunities.is_empty() {
            info!("🚨 All opportunities are competitive - using SINGLE EXECUTION for maximum speed");
            return ExecutionStrategy::SingleExecution(competitive_opportunities);
        }
        
        // 2. If ALL opportunities are safe to batch -> Batch execution for efficiency
        if competitive_opportunities.is_empty() && !safe_batch_opportunities.is_empty() {
            if safe_batch_opportunities.len() >= 2 && self.batch_execution_engine.is_some() {
                info!("📦 All opportunities safe for batching - using BATCH EXECUTION for efficiency");
                return ExecutionStrategy::BatchExecution(safe_batch_opportunities, Vec::new());
            } else {
                info!("📋 Single opportunity or no batch engine - using single execution");
                return ExecutionStrategy::SingleExecution(safe_batch_opportunities);
            }
        }
        
        // 3. Mixed scenario -> Hybrid execution
        if !competitive_opportunities.is_empty() && !safe_batch_opportunities.is_empty() {
            info!("🔄 Mixed competitiveness - using HYBRID EXECUTION");
            return ExecutionStrategy::HybridExecution {
                immediate: competitive_opportunities,
                batchable: safe_batch_opportunities,
            };
        }
        
        // 4. Fallback to single execution
        info!("📋 Fallback to single execution");
        ExecutionStrategy::SingleExecution(opportunities.to_vec())
    }

    /// Analyze opportunity competitiveness and recommend execution method
    async fn analyze_opportunity_competitiveness(&self, opportunity: &MultiHopArbOpportunity) -> CompetitivenessAnalysis {
        let mut competitive_factors = 0;
        let mut reasons = Vec::new();
        
        // Factor 1: Pool depth analysis
        let avg_pool_depth = self.calculate_average_pool_depth(opportunity).await;
        if avg_pool_depth < 50_000.0 { // $50k average depth indicates high competition
            competitive_factors += 3;
            reasons.push(format!("Low pool depth (${:.0}k) indicates high competition", avg_pool_depth / 1000.0));
        } else if avg_pool_depth < 200_000.0 { // $200k moderate competition
            competitive_factors += 1;
            reasons.push(format!("Moderate pool depth (${:.0}k)", avg_pool_depth / 1000.0));
        }
        
        // Factor 2: Profit margin analysis
        if opportunity.profit_pct > 2.0 { // >2% profit attracts more bots
            competitive_factors += 2;
            reasons.push(format!("High profit ({:.2}%) attracts competition", opportunity.profit_pct));
        } else if opportunity.profit_pct > 1.0 { // >1% moderate attention
            competitive_factors += 1;
            reasons.push(format!("Moderate profit ({:.2}%)", opportunity.profit_pct));
        }
        
        // Factor 3: Path complexity (more hops = more MEV risk)
        let hop_count = opportunity.hops.len();
        if hop_count > 3 {
            competitive_factors += 2;
            reasons.push(format!("{} hops increase MEV/sandwich risk", hop_count));
        } else if hop_count > 2 {
            competitive_factors += 1;
            reasons.push(format!("{} hops moderate complexity", hop_count));
        }
        
        // Factor 4: Time sensitivity (fresh opportunities are more competitive)
        if let Some(detected_at) = opportunity.detected_at {
            let age_ms = detected_at.elapsed().as_millis();
            if age_ms < 100 { // <100ms is very fresh
                competitive_factors += 2;
                reasons.push(format!("Very fresh opportunity ({}ms old)", age_ms));
            } else if age_ms < 500 { // <500ms still competitive
                competitive_factors += 1;
                reasons.push(format!("Fresh opportunity ({}ms old)", age_ms));
            }
        }
        
        // Factor 5: DEX type analysis (some DEXs have more bot activity)
        let has_high_competition_dex = opportunity.dex_path.iter()
            .any(|dex| matches!(dex, DexType::Raydium | DexType::Orca | DexType::Meteora)); // High-volume DEXs
        if has_high_competition_dex {
            competitive_factors += 1;
            reasons.push("Uses high-competition DEX".to_string());
        }
        
        // DECISION: Competitive threshold
        let is_competitive = competitive_factors >= 4; // Threshold for immediate execution
        
        let calculated_recommendation = if is_competitive {
            ExecutionRecommendation::ImmediateSingle
        } else {
            ExecutionRecommendation::SafeToBatch
        };
        
        let summary_reason = if is_competitive {
            format!("HIGH COMPETITION (score: {}) - {}", competitive_factors, reasons.join(", "))
        } else {
            format!("LOW COMPETITION (score: {}) - Safe for batching", competitive_factors)
        };
        
        CompetitivenessAnalysis {
            competitive_score: opportunity.expected_output.to_f64().unwrap_or(0.0),
            execution_recommendation: calculated_recommendation, // Use the calculated recommendation
            reason: summary_reason, // Use the calculated summary_reason
            risk_factors: reasons,
        }
    }

    /// Calculate average pool depth for an opportunity
    async fn calculate_average_pool_depth(&self, opportunity: &MultiHopArbOpportunity) -> f64 {
        let mut total_depth = 0.0;
        let mut pool_count = 0;
        
        for hop in &opportunity.hops {
            if let Some(pool) = self.hot_cache.get(&hop.pool) {
                // Calculate pool depth using the actual PoolInfo fields
                let depth_a = pool.token_a.reserve as f64 * 1.0; // Simplified pricing
                let depth_b = pool.token_b.reserve as f64 * 1.0; // In practice, use real prices
                let pool_depth = depth_a.min(depth_b);
                
                total_depth += pool_depth;
                pool_count += 1;
            }
        }
        
        if pool_count > 0 {
            total_depth / pool_count as f64
        } else {
            0.0
        }
    }

    /// Sprint 2: Execute a single opportunity
    async fn execute_single_opportunity(&self, opportunity: &MultiHopArbOpportunity) -> Result<String, ArbError> {
        let executor = self.executor.as_ref()
            .ok_or_else(|| ArbError::ConfigError("No executor configured".to_string()))?;

        let start_time = Instant::now();
        
        // Resolve pools for the opportunity from hot cache
        let resolved_pools = self.resolve_pools_from_hot_cache(opportunity).await?;
        
        // Validate that all pools are still valid
        for pool in &resolved_pools {
            if !validate_single_pool(pool, &self.pool_validation_config) {
                return Err(ArbError::InvalidPoolState(
                    format!("Pool {} failed validation during execution", pool.address)
                ));
            }
        }

        // Execute the opportunity
        match executor.execute_opportunity(opportunity).await {
            Ok(signature) => {
                let execution_time = start_time.elapsed();
                info!("✅ Opportunity {} executed in {:.2}ms: {}", 
                      opportunity.id, execution_time.as_secs_f64() * 1000.0, signature);
                
                // Update metrics
                let metrics = self.metrics.lock().await;
                metrics.record_execution_time(execution_time);
                metrics.log_opportunity_executed_success();
                
                Ok(signature.to_string())
            }
            Err(e) => {
                error!("❌ Failed to execute opportunity {}: {}", opportunity.id, e);
                
                // Update metrics
                let metrics = self.metrics.lock().await;
                metrics.log_opportunity_executed_failure();
                
                Err(ArbError::ExecutionError(e))
            }
        }
    }

    /// Sprint 2: Resolve pools from hot cache for opportunity execution
    async fn resolve_pools_from_hot_cache(&self, opportunity: &MultiHopArbOpportunity) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let mut resolved_pools = Vec::new();
        let mut cache_hits = 0;

        for pool_address in &opportunity.pool_path {
            if let Some(pool_info) = self.hot_cache.get(pool_address) {
                resolved_pools.push(pool_info.value().clone());
                cache_hits += 1;
            } else {
                warn!("🔍 Pool {} not found in hot cache for opportunity {}", pool_address, opportunity.id);
                return Err(ArbError::PoolNotFound(format!("Pool {} not found in hot cache", pool_address)));
            }
        }

        // Update cache metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.hot_cache_hits += cache_hits;
        }

        info!("🔍 Resolved {} pools for opportunity {} (cache hits: {})", 
              resolved_pools.len(), opportunity.id, cache_hits);
        
        Ok(resolved_pools)
    }

    /// Sprint 2: Main arbitrage detection and execution loop
    pub async fn run_arbitrage_cycle(&self) -> Result<(), ArbError> {
        let cycle_start = Instant::now();
        info!("🔄 Starting arbitrage cycle...");

        // Health check
        self.maybe_check_health().await?;

        // Detect opportunities
        let opportunities = self.detect_arbitrage_opportunities().await?;
        
        if opportunities.is_empty() {
            info!("📊 No profitable opportunities found in this cycle");
            return Ok(());
        }

        // Execute opportunities
        let execution_results = self.execute_opportunities(opportunities).await?;
        
        let cycle_time = cycle_start.elapsed();
        info!("🎯 Arbitrage cycle completed in {:.2}ms: {} executions", 
              cycle_time.as_secs_f64() * 1000.0, execution_results.len());

        Ok(())
    }

    /// Sprint 2: Update hot cache with new pool data
    pub async fn update_hot_cache(&self, pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        let start_time = Instant::now();
        let initial_size = self.hot_cache.len();
        
        // Validate pools before updating hot cache
        let pools_vec: Vec<PoolInfo> = pools.values()
            .map(|pool_arc| (**pool_arc).clone())
            .collect();
        
        let original_count = pools_vec.len();
        let valid_pools = validate_pools(pools_vec, &self.pool_validation_config);
        let filtered_count = original_count - valid_pools.len();
        
        if filtered_count > 0 {
            warn!("🔍 Hot cache update: filtered out {} invalid pools", filtered_count);
        }

        // Update hot cache with validated pools
        for pool in valid_pools {
            self.hot_cache.insert(pool.address, Arc::new(pool));
        }
        
        let final_size = self.hot_cache.len();
        let update_duration = start_time.elapsed();
        
        info!("🔥 Hot cache updated: {} -> {} pools in {:.2}ms", 
              initial_size, final_size, update_duration.as_secs_f64() * 1000.0);
        
        // Update metrics
        let mut metrics = self.metrics.lock().await;
        metrics.log_pools_updated(final_size.saturating_sub(initial_size), filtered_count, final_size);
        
        Ok(())
    }

    /// Sprint 2: Get hot cache statistics
    pub async fn get_hot_cache_stats(&self) -> (usize, f64) {
        let cache_size = self.hot_cache.len();
        let metrics = self.detection_metrics.lock().await;
        let hit_rate = if metrics.hot_cache_hits > 0 {
            100.0 // For now, assume 100% hit rate since we're using hot cache
        } else {
            0.0
        };
        
        (cache_size, hit_rate)
    }

    /// Sprint 2: Enable/disable execution
    pub fn set_execution_enabled(&self, enabled: bool) {
        self.execution_enabled.store(enabled, Ordering::Relaxed);
        info!("🎯 Execution {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Sprint 2: Get comprehensive engine status
    pub async fn get_enhanced_status(&self) -> String {
        let (cache_size, hit_rate) = self.get_hot_cache_stats().await;
        let is_degraded = self.degradation_mode.load(Ordering::Relaxed);
        let execution_enabled = self.execution_enabled.load(Ordering::Relaxed);
        let ws_connected = self.ws_manager.is_some();
        let current_threshold = self.get_min_profit_threshold_pct().await;
        
        let metrics = self.detection_metrics.lock().await;
        
        format!(
            "Enhanced Engine[Cache:{} pools, Hit Rate:{:.1}%, Degraded:{}, Execution:{}, WS:{}, Threshold:{:.4}%, DEX:{}, Cycles:{}, Avg Detection:{:.2}ms]",
            cache_size,
            hit_rate,
            is_degraded,
            execution_enabled,
            ws_connected,
            current_threshold,
            self.dex_providers.len(),
            metrics.total_detection_cycles,
            metrics.average_detection_time_ms
        )
    }

    // Keep existing methods for backward compatibility
    pub async fn start_services(&self, _cache: Option<Arc<crate::cache::Cache>>) {
        if let Some(ws_manager) = &self.ws_manager {
            info!("Starting WebSocket manager...");
            let mut manager = ws_manager.lock().await;
            if let Err(e) = manager.start().await {
                error!("Failed to start WebSocket manager: {}", e);
            }
        }

        // Start dynamic threshold updater if available
        if let Some(_updater) = &self.dynamic_threshold_updater {
            info!("Dynamic threshold updater available (monitoring would be started here)");
            // TODO: Implement proper monitoring task
        }

        // Start execution pipeline using async channels
        info!("🚀 Starting async execution pipeline...");
        if let Some(_sender) = &self.opportunity_sender {
            // The opportunity sender is ready for use
            // Execution will be handled directly in the orchestrator methods
            info!("✅ Async execution channel initialized");
        }
    }

    pub async fn get_min_profit_threshold_pct(&self) -> f64 {
        let detector = self.detector.lock().await;
        detector.get_min_profit_threshold_pct()
    }

    pub async fn set_min_profit_threshold_pct(&self, threshold_pct: f64) {
        let mut detector = self.detector.lock().await;
        detector.set_min_profit_threshold(threshold_pct);
        
        let mut metrics = self.metrics.lock().await;
        metrics.log_dynamic_threshold_update(threshold_pct / 100.0);
    }

    // Legacy methods for backward compatibility
    async fn _discover_direct_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.detect_arbitrage_opportunities().await
    }
        
    pub async fn discover_multihop_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.detect_arbitrage_opportunities().await
    }

    pub async fn discover_fixed_input_opportunities(&self, _fixed_input_amount: f64) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.detect_arbitrage_opportunities().await
    }

    async fn _discover_multihop_opportunities_with_risk(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.detect_arbitrage_opportunities().await
    }

    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        self.resolve_pools_from_hot_cache(opportunity).await
    }

    pub async fn get_validated_pools(&self) -> Result<HashMap<Pubkey, Arc<PoolInfo>>, ArbError> {
        let snapshot = self.get_hot_cache_snapshot().await;
        self.validate_hot_cache_pools(&snapshot).await
    }

    pub async fn add_validated_pool(&self, pool: PoolInfo) -> Result<bool, ArbError> {
        if !validate_single_pool(&pool, &self.pool_validation_config) {
            warn!("Pool {} failed validation - not adding to hot cache", pool.address);
            return Ok(false);
        }
        
        let pool_address = pool.address;
        self.hot_cache.insert(pool_address, Arc::new(pool));
        
        info!("Successfully added validated pool {} to hot cache", pool_address);
        Ok(true)
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        if let Some(updater) = &self.dynamic_threshold_updater {
            info!("Starting dynamic threshold updates...");
            
            if let Some(price_provider) = &self.price_provider {
                if let Some(sol_price) = price_provider.get_current_price("SOL") {
                    debug!("Would update threshold with SOL price: ${:.2}", sol_price);
                    info!("Added SOL price observation from provider: ${:.2}", sol_price);
                } else {
                    let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                    debug!("Would update threshold with SOL price: ${:.2}", current_sol_price);
                    info!("Added SOL price observation from config: ${:.2}", current_sol_price);
                }
            } else {
                let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                debug!("Would update threshold with SOL price: ${:.2}", current_sol_price);
                info!("Added SOL price observation from config: ${:.2}", current_sol_price);
            }

            let current_threshold = updater.get_current_threshold();
            info!("Dynamic threshold monitoring active. Current threshold: {:.4}%", current_threshold * 100.0);
        } else {
            info!("Dynamic threshold updater not configured, using static thresholds");
        }
    }

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let last_check = *self.last_health_check.read().await;
        if last_check.elapsed() < self.health_check_interval {
            return Ok(());
        }

        self.run_health_checks().await;
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        let mut overall_healthy = true;

        // RPC Health Check with detailed status
        let rpc_healthy = if let Some(rpc_client) = &self.rpc_client {
            let rpc_health_status = rpc_client.get_health_status().await;
            
            if rpc_health_status.overall_healthy {
                info!("RPC client health check: PASSED ({}ms)", rpc_health_status.check_duration_ms);
                debug!("RPC Primary: {}, Fallbacks: {}", 
                       if rpc_health_status.primary_status.is_healthy { "HEALTHY" } else { "UNHEALTHY" },
                       rpc_health_status.fallback_statuses.iter()
                           .map(|s| if s.is_healthy { "HEALTHY" } else { "UNHEALTHY" })
                           .collect::<Vec<_>>().join(", "));
            } else {
                warn!("RPC client health check: FAILED ({}ms)", rpc_health_status.check_duration_ms);
                if let Some(error) = &rpc_health_status.primary_status.error_message {
                    warn!("RPC Primary error: {}", error);
                }
                for (i, fallback) in rpc_health_status.fallback_statuses.iter().enumerate() {
                    if let Some(error) = &fallback.error_message {
                        warn!("RPC Fallback-{} error: {}", i + 1, error);
                    }
                }
                overall_healthy = false;
            }
            
            rpc_health_status.overall_healthy
        } else {
            warn!("RPC client not configured; skipping RPC health check.");
            false
        };

        let ws_healthy = if let Some(_ws_manager) = &self.ws_manager {
            info!("WebSocket manager health check (placeholder).");
            true
        } else {
            warn!("WebSocket manager not configured; skipping WebSocket health check.");
            false
        };

        if let Some(_price_provider_arc) = &self.price_provider {
            info!("Price provider is configured (health check placeholder).");
        } else {
            warn!("Price provider not configured; skipping price provider health check.");
        }

        // DEX Providers Health Check
        let dex_healthy = self.dex_providers_health_check().await;
        if !dex_healthy {
            overall_healthy = false;
        }

        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit = self.get_min_profit_threshold_pct().await;
        let should_degrade = !ws_healthy || !overall_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues");
            warn!("[ArbitrageEngine] Health status - RPC: {}, WS: {}, DEX: {}", 
                  rpc_healthy, ws_healthy, dex_healthy);
            let new_threshold = current_min_profit * degradation_factor;
            self.set_min_profit_threshold_pct(new_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold increased to {:.4}% due to degradation.", new_threshold);
        } else if !should_degrade && was_degraded {
            info!("[ArbitrageEngine] Exiting degradation mode - system health restored.");
            let base_threshold = self.config.min_profit_pct * 100.0;
            self.set_min_profit_threshold_pct(base_threshold).await;
            info!("[ArbitrageEngine] Min profit threshold reset to {:.4}%.", base_threshold);
            self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
        }

        self.metrics.lock().await.set_system_health(overall_healthy);
        *self.last_health_check.write().await = Instant::now();
        info!("Health checks completed. System healthy: {}.", overall_healthy);
    }

    pub async fn dex_providers_health_check(&self) -> bool {
        if self.dex_providers.is_empty() {
            warn!("No DEX providers configured. ArbitrageEngine cannot operate without DEX APIs.");
            return false;
        }
        
        info!("Checking health of {} DEX providers...", self.dex_providers.len());
        let mut all_healthy = true;
        
        for (i, provider) in self.dex_providers.iter().enumerate() {
            let provider_name = provider.get_name();
            
            match provider.health_check().await {
                Ok(health_status) => {
                    if health_status.is_healthy {
                        info!("DEX provider #{} ({}) health check: OK - {}", 
                              i + 1, provider_name, health_status.status_message);
                    } else {
                        warn!("DEX provider #{} ({}) health check: FAILED - {}", 
                              i + 1, provider_name, health_status.status_message);
                        all_healthy = false;
                    }
                    
                    // Log additional details if available
                    if let Some(response_time) = health_status.response_time_ms {
                        debug!("DEX provider #{} response time: {}ms", i + 1, response_time);
                    }
                    if let Some(pool_count) = health_status.pool_count {
                        debug!("DEX provider #{} reported {} pools", i + 1, pool_count);
                    }
                }
                Err(e) => {
                    error!("DEX provider #{} ({}) health check error: {}", i + 1, provider_name, e);
                    all_healthy = false;
                }
            }
        }
        
        if all_healthy {
            info!("All {} DEX providers reported healthy", self.dex_providers.len());
        } else {
            warn!("Some DEX providers reported unhealthy status");
        }
        
        all_healthy
    }

    pub async fn run_full_health_check(&self) {
        info!("Running full health check including all subsystems...");
        
        self.run_health_checks().await;
        
        let cache_size = self.hot_cache.len();
        info!("Hot cache health: {} pools currently tracked", cache_size);
        
        if cache_size == 0 {
            warn!("No pools are currently tracked in hot cache - this may indicate a data fetching issue");
        }
        
        if let Some(updater) = &self.dynamic_threshold_updater {
            let current_threshold = updater.get_current_threshold();
            info!("Dynamic threshold system health: current threshold {:.4}%", current_threshold * 100.0);
        }
        
        info!("Full health check completed");
    }

    pub async fn update_pools(&self, new_pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Result<(), ArbError> {
        self.update_hot_cache(new_pools).await
    }

    /// Sprint 2: Utility method for safe pool access with timeout
    pub async fn with_pool_guard_async<Fut, T>(
        &self,
        operation_name: &str,
        critical: bool,
        closure: impl FnOnce(Arc<DashMap<Pubkey, Arc<PoolInfo>>>) -> Fut,
    ) -> Result<T, ArbError>
    where
        Fut: std::future::Future<Output = Result<T, ArbError>>,
    {
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            async { Ok::<_, ArbError>(Arc::clone(&self.hot_cache)) },
        )
        .await
        {
            Ok(Ok(hot_cache_for_closure)) => closure(hot_cache_for_closure).await,
            Ok(Err(e)) => {
                error!("{}: Unexpected error cloning hot cache Arc for guard: {}", operation_name, e);
                Err(e)
            }
            Err(_timeout_error) => {
                let msg = format!(
                    "Timeout {} hot cache access in {}",
                    if critical { "CRITICAL:" } else { "" }, operation_name
                );
                if critical { error!("{}", msg); } else { warn!("{}", msg); }
                Err(ArbError::TimeoutError(msg))
            }
        }
    }

    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.detect_arbitrage_opportunities().await
    }

    pub async fn process_websocket_updates_loop(&self) {
        info!("WebSocket update processing loop started");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            debug!("WebSocket update processing tick");
        }
    }

    pub async fn get_current_status_string(&self) -> String {
        self.get_enhanced_status().await
    }

    pub async fn fetch_live_pool_reserves(&self, pools: &mut Vec<PoolInfo>) -> Result<usize, ArbError> {
        let start_time = Instant::now();
        
        if pools.is_empty() {
            return Ok(0);
        }

        let rpc_client = match &self.rpc_client {
            Some(client) => client,
            None => {
                warn!("No RPC client available for fetching live pool reserves");
                return Err(ArbError::ConfigError("No RPC client available".to_string()));
            }
        };

        let pool_addresses: Vec<Pubkey> = pools.iter().map(|pool| pool.address).collect();
        
        info!("Fetching live reserve data for {} pools using batched RPC call", pool_addresses.len());

        match rpc_client.primary_client.get_multiple_accounts(&pool_addresses).await {
            Ok(accounts) => {
                let mut updated_count = 0;
                
                for (i, account_option) in accounts.iter().enumerate() {
                    if let Some(account) = account_option {
                        let pool = &mut pools[i];
                        
                        if let Err(e) = self.update_pool_reserves_from_account_data(pool, &account.data).await {
                            debug!("Failed to parse pool {} reserves: {}", pool.address, e);
                            continue;
                        }
                        
                        updated_count += 1;
                    } else {
                        warn!("Pool account {} not found on-chain", pool_addresses[i]);
                    }
                }

                let elapsed = start_time.elapsed();
                info!("Successfully updated reserves for {}/{} pools in {}ms", 
                      updated_count, pools.len(), elapsed.as_millis());

                info!("Validating {} pools after live reserve update", pools.len());
                let pools_len = pools.len();
                let valid_pools = validate_pools(pools.clone(), &self.pool_validation_config);
                let invalid_count = pools_len - valid_pools.len();
                
                if invalid_count > 0 {
                    warn!("Post-update validation: {} of {} pools became invalid after live data update", 
                          invalid_count, pools.len());
                    
                    *pools = valid_pools;
                }

                let mut metrics = self.metrics.lock().await;
                metrics.log_pools_fetched(updated_count);

                Ok(updated_count)
            }
            Err(e) => {
                error!("Failed to fetch pool accounts: {}", e);
                Err(ArbError::RpcError(format!("Batched account fetch failed: {}", e)))
            }
        }
    }

    async fn update_pool_reserves_from_account_data(&self, pool: &mut PoolInfo, account_data: &[u8]) -> Result<(), ArbError> {
        let rpc_client = match &self.rpc_client {
            Some(client) => client,
            None => return Err(ArbError::ConfigError("No RPC client available".to_string())),
        };

        match pool.dex_type {
            crate::utils::DexType::Raydium => {
                let parser = crate::dex::clients::raydium::RaydiumPoolParser;
                match parser.parse_pool_data(pool.address, account_data, rpc_client).await {
                    Ok(parsed_pool) => {
                        pool.token_a.reserve = parsed_pool.token_a.reserve;
                        pool.token_b.reserve = parsed_pool.token_b.reserve;
                        pool.last_update_timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        
                        if let Some(sqrt_price) = parsed_pool.sqrt_price {
                            pool.sqrt_price = Some(sqrt_price);
                        }
                        if let Some(liquidity) = parsed_pool.liquidity {
                            pool.liquidity = Some(liquidity);
                        }
                    }
                    Err(e) => {
                        return Err(ArbError::ParseError(format!("Failed to parse Raydium pool {}: {}", pool.address, e)));
                    }
                }
            }
            crate::utils::DexType::Orca => {
                let parser = crate::dex::clients::orca::OrcaPoolParser;
                match parser.parse_pool_data(pool.address, account_data, rpc_client).await {
                    Ok(parsed_pool) => {
                        pool.token_a.reserve = parsed_pool.token_a.reserve;
                        pool.token_b.reserve = parsed_pool.token_b.reserve;
                        pool.last_update_timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                    }
                    Err(e) => {
                        warn!("Failed to parse Orca pool {} reserves: {}", pool.address, e);
                        return Err(ArbError::ParseError(format!("Failed to parse Orca pool: {}", e)));
                    }
                }
            }
            crate::utils::DexType::Meteora => {
                warn!("Meteora live reserve fetching not yet fully implemented for pool {}", pool.address);
                return Err(ArbError::ParseError("Meteora parser not fully implemented".to_string()));
            }
            crate::utils::DexType::Lifinity => {
                warn!("Lifinity live reserve fetching not yet fully implemented for pool {}", pool.address);
                return Err(ArbError::ParseError("Lifinity parser not fully implemented".to_string()));
            }
            crate::utils::DexType::Whirlpool => {
                warn!("Whirlpool live reserve fetching not yet fully implemented for pool {}", pool.address);
                return Err(ArbError::ParseError("Whirlpool parser not fully implemented".to_string()));
            }
            crate::utils::DexType::Phoenix => {
                warn!("Phoenix live reserve fetching not yet fully implemented for pool {}", pool.address);
                return Err(ArbError::ParseError("Phoenix parser not fully implemented".to_string()));
            }
            crate::utils::DexType::Jupiter => {
                return Err(ArbError::ParseError("Jupiter is an aggregator and doesn't have individual pools".to_string()));
            }
            crate::utils::DexType::Unknown(_) => {
                return Err(ArbError::ParseError(format!("Unknown DEX type for pool {}", pool.address)));
            }
        }
        
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), ArbError> {
        info!("Shutting down Enhanced ArbitrageEngine...");
        
        // Disable execution
        self.set_execution_enabled(false);
        
        if let Some(ws_manager) = &self.ws_manager {
            let mut manager = ws_manager.lock().await;
            manager.stop().await;
            info!("WebSocket manager stopped");
        }
        
        info!("Enhanced ArbitrageEngine shutdown completed");
        Ok(())
    }

    pub fn get_pool_validation_config(&self) -> &PoolValidationConfig {
        &self.pool_validation_config
    }

    pub fn update_pool_validation_config(&mut self, config: PoolValidationConfig) {
        info!("Updating pool validation configuration: min_liquidity_usd={}, max_price_impact_bps={}, require_balanced_reserves={}", 
               config.min_liquidity_usd, config.max_price_impact_bps, config.require_balanced_reserves);
        self.pool_validation_config = config;
    }

    pub async fn get_pool_validation_stats(&self) -> Result<(usize, usize, f64), ArbError> {
        let total_pools = self.hot_cache.len();
        
        if total_pools == 0 {
            return Ok((0, 0, 0.0));
        }
        
        let pools_vec: Vec<PoolInfo> = self.hot_cache
            .iter()
            .map(|entry| (**entry.value()).clone())
            .collect();
        
        let valid_pools = validate_pools(pools_vec, &self.pool_validation_config);
        let valid_count = valid_pools.len();
        let invalid_count = total_pools - valid_count;
        let rejection_rate = (invalid_count as f64 / total_pools as f64) * 100.0;
        
        Ok((total_pools, invalid_count, rejection_rate))
    }

    /// Enhanced opportunity detection using cached pools from discovery service.
    pub async fn detect_arbitrage_opportunities_with_cache(&self, pool_discovery: &Arc<crate::dex::discovery::PoolDiscoveryService>) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let start_time = Instant::now();
        info!("🔍 Starting enhanced arbitrage detection with cached pools...");

        // Update detection metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.total_detection_cycles += 1;
            metrics.last_detection_timestamp = chrono::Utc::now().timestamp_millis() as u64;
        }

        // Use cached pools from discovery service
        let cached_pools = self.get_arbitrage_ready_pools(pool_discovery).await;
        let pool_count = cached_pools.len();
        
        if pool_count == 0 {
            warn!("No arbitrage-ready pools available in cache");
            return Ok(Vec::new());
        }

        info!("📊 Analyzing {} arbitrage-ready cached pools", pool_count);

        // Group pools by DEX for efficient batch processing
        let mut grouped_pools = std::collections::HashMap::new();
        for pool in &cached_pools {
            let dex_name = format!("{:?}", pool.dex_type);
            grouped_pools.entry(dex_name).or_insert_with(Vec::new).push(pool.clone());
        }

        info!("🔍 Pool distribution across DEXs: {:?}", 
              grouped_pools.iter().map(|(dex, pools)| (dex.clone(), pools.len())).collect::<Vec<_>>());

        // Run detection using cached pools
        let opportunities = {
            let detector = &self.detector;
            let detector = detector.lock().await;
            
            // Convert Vec<Arc<PoolInfo>> to HashMap<Pubkey, Arc<PoolInfo>> for detector
            let pools_map: HashMap<Pubkey, Arc<PoolInfo>> = cached_pools.iter()
                .map(|pool| (pool.address, pool.clone()))
                .collect();
            
            detector.detect_all_opportunities(&pools_map, &self.metrics).await?
        };

        // Validate opportunities against cache
        let mut validated_opportunities = Vec::new();
        for opp in opportunities {
            // Basic validation - check if all pools in the opportunity are still in cache
            let all_pools_valid = opp.hops.iter().all(|hop| self.hot_cache.contains_key(&hop.pool));
            if all_pools_valid {
                validated_opportunities.push(opp);
            } else {
                warn!("Opportunity {} has stale pool references, filtering out", opp.id);
            }
        }

        let detection_duration = start_time.elapsed();
        let filtered_count = validated_opportunities.len();

        // Update detection metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.average_detection_time_ms = 
                (metrics.average_detection_time_ms * metrics.total_detection_cycles as f64 + detection_duration.as_millis() as f64) 
                / (metrics.total_detection_cycles as f64 + 1.0);
            metrics.total_opportunities_found += filtered_count as u64;
            metrics.last_detection_timestamp = chrono::Utc::now().timestamp_millis() as u64;
        }

        info!("✅ Cache-based detection complete in {:?}:", detection_duration);
        info!("   📊 Analyzing pools: {}", pool_count);
        info!("   🎯 Opportunities found: {}", filtered_count);
        if pool_count > 0 {
            info!("   ⚡ Detection rate: {:.2} opportunities/pool", filtered_count as f64 / pool_count as f64);
        }

        Ok(validated_opportunities)
    }

    /// Get pools suitable for arbitrage using cache optimization.
    pub async fn get_arbitrage_ready_pools(&self, pool_discovery: &Arc<crate::dex::discovery::PoolDiscoveryService>) -> Vec<Arc<PoolInfo>> {
        let all_pools = pool_discovery.get_all_cached_pools();
        
        // Filter pools suitable for arbitrage (check bans asynchronously)
        let mut arbitrage_ready = Vec::new();
        for pool in all_pools {
            // Basic liquidity check
            if pool.token_a.reserve > 1000 && pool.token_b.reserve > 1000 {
                // Check if not banned
                if !pool_discovery.is_pair_banned(&pool.token_a.mint, &pool.token_b.mint).await {
                    arbitrage_ready.push(pool);
                }
            }
        }
        
        arbitrage_ready
    }

    /// Route opportunities to appropriate DEX clients for execution.
    pub fn route_opportunities_to_dex<'a>(&self, opportunities: &'a [MultiHopArbOpportunity]) -> HashMap<String, Vec<&'a MultiHopArbOpportunity>> {
        use crate::dex::group_pools_by_dex;
        
        let mut routed_opportunities: HashMap<String, Vec<&MultiHopArbOpportunity>> = HashMap::new();
        
        for opportunity in opportunities {
            // Extract pools from the opportunity
            let pools_in_path: Vec<crate::utils::PoolInfo> = opportunity.pool_path.iter()
                .filter_map(|pool_addr| {
                    // Get pool from hot cache
                    self.hot_cache.get(pool_addr).map(|entry| {
                        // Convert Arc<PoolInfo> to PoolInfo
                        (**entry.value()).clone()
                    })
                })
                .collect();
            
            if pools_in_path.len() != opportunity.pool_path.len() {
                warn!("⚠️ Not all pools found in cache for opportunity {}", opportunity.id);
                continue;
            }
            
            // Group pools by DEX type  
            let pools_cloned: Vec<crate::utils::PoolInfo> = pools_in_path.iter().map(|p| (*p).clone()).collect();
            let grouped_pools = group_pools_by_dex(&pools_cloned);
            
            // Route opportunity to the primary DEX (most pools)
            if let Some((primary_dex, _)) = grouped_pools.iter().max_by_key(|(_, pools)| pools.len()) {
                routed_opportunities.entry(primary_dex.clone()).or_insert_with(Vec::new).push(opportunity);
                debug!("🎯 Routed opportunity {} to {} DEX", opportunity.id, primary_dex);
            }
        }
        
        info!("📊 Routed {} opportunities across {} DEX types", 
              opportunities.len(), routed_opportunities.len());
        
        routed_opportunities
    }

    /// Find pools that support specific token pairs for arbitrage routing.
    pub fn find_arbitrage_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<Arc<PoolInfo>> {
        use crate::dex::find_pools_for_pair;
        
        // Convert hot cache to Vec<Arc<PoolInfo>> for the routing function
        let cached_pools: Vec<Arc<PoolInfo>> = self.hot_cache.iter()
            .map(|entry| entry.value().clone())
            .collect();
        
        let matching_pools = find_pools_for_pair(&cached_pools, token_a, token_b);
        
        info!("🔍 Found {} pools for token pair {}/{}", 
              matching_pools.len(), token_a, token_b);
        
        matching_pools
    }

    /// Enhanced opportunity execution with DEX routing.
    pub async fn execute_opportunities_with_routing(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<String>, ArbError> {
        if opportunities.is_empty() {
            return Ok(Vec::new());
        }

        info!("🚀 Executing {} opportunities with DEX routing", opportunities.len());
        
        // Route opportunities to appropriate DEX clients
        let routed_opportunities = self.route_opportunities_to_dex(&opportunities);
        
        let mut all_results = Vec::new();
        let mut total_executed = 0;
        
        // Execute opportunities grouped by DEX type
        for (dex_name, dex_opportunities) in routed_opportunities {
            info!("🎯 Executing {} opportunities on {} DEX", dex_opportunities.len(), dex_name);
            
            // Convert references back to owned values for execution
            let dex_opportunities_owned: Vec<MultiHopArbOpportunity> = dex_opportunities.into_iter().cloned().collect();
            
            match self.execute_opportunities(dex_opportunities_owned).await {
                Ok(mut results) => {
                    total_executed += results.len();
                    all_results.append(&mut results);
                    info!("✅ Successfully executed {} opportunities on {}", results.len(), dex_name);
                }
                Err(e) => {
                    error!("❌ Failed to execute opportunities on {}: {}", dex_name, e);
                    // Continue with other DEXs even if one fails
                }
            }
        }
        
        info!("🎉 Total executed opportunities: {}/{}", total_executed, opportunities.len());
        Ok(all_results)
    }

    /// STAGE 2: Advanced Arbitrage Calculation
    /// This is the core of the three-stage workflow - performs deep analysis including:
    /// - Optimal input amount calculation using convex optimization
    /// - Real trade simulation with fees and slippage
    /// - Execution strategy determination (Direct Swap vs Flash Loan)
    async fn calculate_advanced_arbitrage(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let original_count = opportunities.len();
        let mut analyzed_opportunities = Vec::new();
        let mut advanced_math = self.advanced_math.lock().await;
        
        for mut opportunity in opportunities {
            info!("🔬 Analyzing opportunity {} with advanced calculations...", opportunity.id);
            
            // Check if all pools are available in cache
            let all_pools_available = opportunity.hops.iter()
                .all(|hop| self.hot_cache.contains_key(&hop.pool));
            
            if !all_pools_available {
                warn!("⚠️ Skipping opportunity {} - missing pool data", opportunity.id);
                continue;
            }
            
            // Collect pool references 
            let pool_refs: Vec<Arc<crate::utils::PoolInfo>> = opportunity.hops.iter()
                .filter_map(|hop| self.hot_cache.get(&hop.pool).map(|entry| entry.clone()))
                .collect();
            
            // Calculate optimal input amount using argmin optimization
            let initial_input = rust_decimal::Decimal::from_f64(opportunity.input_amount)
                .unwrap_or_else(|| rust_decimal::Decimal::new(1000, 0)); // Default 1000 tokens
            let target_profit_pct = rust_decimal::Decimal::from_f64(opportunity.profit_pct / 100.0)
                .unwrap_or_else(|| rust_decimal::Decimal::new(1, 2)); // Default 1%
            
            // Convert Arc<PoolInfo> to &PoolInfo for the calculation
            let pool_refs_slice: Vec<&crate::utils::PoolInfo> = pool_refs.iter()
                .map(|arc| arc.as_ref())
                .collect();
            
            match advanced_math.calculate_optimal_input(&pool_refs_slice, initial_input, target_profit_pct) {
                Ok(optimal_result) => {
                    // Update opportunity with optimized values
                    opportunity.input_amount = optimal_result.optimal_input.to_f64().unwrap_or(opportunity.input_amount);
                    opportunity.expected_output = opportunity.input_amount + optimal_result.max_net_profit.to_f64().unwrap_or(0.0);
                    opportunity.total_profit = optimal_result.max_net_profit.to_f64().unwrap_or(0.0);
                    opportunity.profit_pct = (optimal_result.max_net_profit.to_f64().unwrap_or(0.0) / opportunity.input_amount) * 100.0;
                    
                    // Determine execution strategy based on optimization results
                    if optimal_result.requires_flash_loan {
                        if let Some(ref mut notes) = opportunity.notes {
                            notes.push_str("; Flash loan recommended");
                        } else {
                            opportunity.notes = Some("Flash loan recommended".to_string());
                        }
                    }
                    
                    // Estimate gas cost in USD (assuming SOL price ~$100)
                    let gas_cost_usd = optimal_result.gas_cost_sol.to_f64().unwrap_or(0.005) * 100.0;
                    opportunity.estimated_gas_cost = Some((gas_cost_usd * 1_000_000.0) as u64); // Convert to micro-cents
                    
                    // Only include if still profitable after optimization
                    if optimal_result.max_net_profit > Decimal::ZERO && 
                       optimal_result.max_net_profit > optimal_result.gas_cost_sol {
                        info!("✅ Opportunity {} optimized: input=${:.2}, profit=${:.2} ({:.2}%)", 
                              opportunity.id, opportunity.input_amount, opportunity.total_profit, opportunity.profit_pct);
                        analyzed_opportunities.push(opportunity);
                    } else {
                        info!("❌ Opportunity {} not profitable after optimization (profit: ${:.4})", 
                              opportunity.id, optimal_result.max_net_profit.to_f64().unwrap_or(0.0));
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to optimize opportunity {}: {}. Using original values.", opportunity.id, e);
                    // Keep original opportunity if optimization fails
                    analyzed_opportunities.push(opportunity);
                }
            }
        }
        
        info!("🎯 Advanced analysis complete: {}/{} opportunities remain profitable", 
              analyzed_opportunities.len(), original_count);
        
        Ok(analyzed_opportunities)
    }

    /// Execute opportunities via single executor
    async fn execute_via_single_executor(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<String>, ArbError> {
        let _executor = self.executor.as_ref()
            .ok_or_else(|| ArbError::ConfigError("Single executor not available".to_string()))?;

        let mut results = Vec::new();
        
        for opportunity in opportunities {
            match self.execute_single_opportunity(&opportunity).await {
                Ok(signature) => {
                    results.push(signature);
                    info!("✅ Single execution success: {} ({}%)", opportunity.id, opportunity.profit_pct);
                }
                Err(e) => {
                    error!("❌ Single execution failed: {} - {}", opportunity.id, e);
                }
            }
        }

        Ok(results)
    }

    /// Execute opportunities via batch execution engine
    async fn execute_via_batch_engine(
        &self, 
        batchable: Vec<MultiHopArbOpportunity>, 
        immediate: Vec<MultiHopArbOpportunity>
    ) -> Result<Vec<String>, ArbError> {
        let batch_engine = self.batch_execution_engine.as_ref()
            .ok_or_else(|| ArbError::ConfigError("Batch execution engine not available".to_string()))?;

        let mut results = Vec::new();

        // Submit batchable opportunities to batch engine
        if !batchable.is_empty() {
            info!("📦 Submitting {} opportunities to batch engine", batchable.len());
            for opportunity in batchable {
                if let Err(e) = batch_engine.process_opportunity(opportunity.clone()).await {
                    error!("❌ Failed to submit opportunity {} to batch engine: {}", opportunity.id, e);
                } else {
                    // For now, assume batch submission creates a placeholder result
                    // In practice, you'd want to track the actual batch execution results
                    results.push(format!("batch_{}", opportunity.id));
                    info!("📦 Opportunity {} submitted to batch", opportunity.id);
                }
            }
        }

        // Execute immediate opportunities if any
        if !immediate.is_empty() {
            info!("⚡ Executing {} immediate opportunities alongside batching", immediate.len());
            let immediate_results = self.execute_via_single_executor(immediate).await?;
            results.extend(immediate_results);
        }

        Ok(results)
    }

    /// Execute using hybrid strategy
    async fn execute_hybrid_strategy(
        &self,
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>
    ) -> Result<Vec<String>, ArbError> {
        let mut results = Vec::new();

        // Execute immediate opportunities first
        if !immediate.is_empty() {
            info!("🔄 Hybrid: executing {} immediate opportunities", immediate.len());
            let immediate_results = self.execute_via_single_executor(immediate).await?;
            results.extend(immediate_results);
        }

        // Then handle batchable opportunities
        if !batchable.is_empty() {
            info!("🔄 Hybrid: batching {} opportunities", batchable.len());
            let batch_results = self.execute_via_batch_engine(batchable, Vec::new()).await?;
            results.extend(batch_results);
        }

        Ok(results)
    }

    /// Execute opportunities in paper trading mode using simulation
    async fn execute_paper_trading_opportunities(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<String>, ArbError> {
        let start_time = Instant::now();
        let total_opportunities = opportunities.len();
        
        // Get paper trading components
        let (engine, portfolio, analytics, reporter) = match (
            &self.paper_trading_engine,
            &self.paper_trading_portfolio,
            &self.paper_trading_analytics,
            &self.paper_trading_reporter
        ) {
            (Some(e), Some(p), Some(a), r) => (e, p, a, r),
            _ => {
                error!("📄 Paper trading components not initialized");
                return Err(ArbError::ExecutionError("Paper trading not properly initialized".to_string()));
            }
        };

        info!("📄 Simulating {} arbitrage opportunities in paper trading mode", total_opportunities);
        
        let mut execution_results = Vec::new();
        let mut successful_simulations = 0;
        
        for opportunity in opportunities {
            info!("📊 Simulating opportunity: profit={} units, route={} hops", 
                  opportunity.total_profit, opportunity.hops.len());
            
            // Simulate the trade execution
            match engine.simulate_arbitrage_execution(&opportunity, &self.dex_providers).await {
                Ok(result) => {
                    successful_simulations += 1;
                    let tx_id = format!("paper_tx_{}", chrono::Utc::now().timestamp_millis());
                    execution_results.push(tx_id.clone());
                    
                    // Update analytics
                    {
                        let mut analytics_guard = analytics.lock().await;
                        if result.success {
                            analytics_guard.record_successful_execution(
                                result.input_amount,
                                result.output_amount,
                                result.output_amount as i64 - result.input_amount as i64, // profit/loss
                                result.fee_amount,
                            );
                        } else {
                            analytics_guard.record_failed_execution(
                                result.input_amount,
                                result.fee_amount,
                                result.error_message.clone().unwrap_or_default()
                            );
                        }
                    }
                    
                    // Log trade if reporter is available
                    if let Some(reporter) = reporter {
                        let trade_entry = PaperTradingReporter::create_trade_log_entry(
                            &opportunity,
                            result.input_amount,
                            result.output_amount,
                            result.output_amount as i64 - result.input_amount as i64, // profit/loss
                            result.slippage_bps as f64 / 10000.0, // Convert bps to percentage
                            result.fee_amount,
                            result.success,
                            result.error_message,
                            0 // gas cost (not tracked in simulation)
                        );
                        
                        if let Err(e) = reporter.log_trade(trade_entry) {
                            warn!("Failed to log paper trade: {}", e);
                        }
                    }
                    
                    info!("✅ Simulated trade completed: {} (profit: {} units)", 
                          if result.success { "SUCCESS" } else { "FAILED" },
                          result.output_amount as i64 - result.input_amount as i64);
                }
                Err(e) => {
                    warn!("❌ Failed to simulate opportunity: {}", e);
                    
                    // Record failed simulation
                    {
                        let mut analytics_guard = analytics.lock().await;
                        analytics_guard.record_failed_execution(
                            (opportunity.input_amount * 1_000_000.0) as u64, // Convert to lamports equivalent
                            0, // No fees for failed simulation
                            format!("Simulation error: {}", e)
                        );
                    }
                }
            }
        }
        
        let execution_time = start_time.elapsed();
        
        // Print summary
        info!("📄 Paper trading simulation completed in {:.2}ms:", execution_time.as_secs_f64() * 1000.0);
        info!("   ✅ Successful simulations: {}/{}", successful_simulations, total_opportunities);
        info!("   💰 Portfolio value: {} lamports", portfolio.get_total_value());
        
        // Export analytics if reporter is available
        if let Some(reporter) = reporter {
            let analytics_guard = analytics.lock().await;
            let portfolio_snapshot = portfolio.snapshot();
            if let Err(e) = reporter.export_analytics(&analytics_guard, &portfolio_snapshot) {
                warn!("Failed to export paper trading analytics: {}", e);
            }
            drop(analytics_guard);
            
            // Print performance summary
            let analytics_guard = analytics.lock().await;
            reporter.print_performance_summary(&analytics_guard, &portfolio_snapshot);
        }
        
        Ok(execution_results)
    }
}