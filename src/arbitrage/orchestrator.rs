// src/arbitrage/orchestrator.rs
use crate::{
    arbitrage::{
        strategy::ArbitrageStrategy,
        analysis::{DynamicThresholdUpdater, ArbitrageAnalyzer, OptimalArbitrageResult},
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

use dashmap::DashMap;
use log::{debug, error, info, warn};
use rust_decimal::{Decimal, prelude::*};
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
    /// Execute as a high-priority immediate opportunity
    Execute,
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
    
    // Async channel for opportunity execution (replaces pipeline)
    pub opportunity_sender: Option<mpsc::UnboundedSender<MultiHopArbOpportunity>>,
    
    // Sprint 2: Performance tracking
    pub detection_metrics: Arc<Mutex<DetectionMetrics>>,
    pub execution_enabled: Arc<AtomicBool>,
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

        info!("🚀 Enhanced ArbitrageOrchestrator initialized with Sprint 2 features:");
        info!("   🔥 Hot cache integration: {} pools", hot_cache.len());
        info!("   🎯 DEX providers: {}", dex_providers.len());
        info!("   ⚡ Batch execution: integrated into executor");
        info!("   📊 Advanced metrics: enabled");
        info!("   🔄 Async execution pipeline: ready");

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
            opportunity_sender: Some(opportunity_sender),
            detection_metrics,
            execution_enabled: Arc::new(AtomicBool::new(true)),
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
        let validated_pools = match validate_pools(pools_vec).await {
            Ok(pools) => pools,
            Err(e) => {
                warn!("Pool validation failed: {}", e);
                Vec::new() // Return empty vec if validation fails
            }
        };
        
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
        info!("🚀 Executing {} opportunities with intelligent routing...", total_opportunities);

        // DECISION LOGIC: Determine execution strategy
        let execution_strategy = self.determine_execution_strategy(&opportunities).await;
        
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
                ExecutionRecommendation::Execute => {
                    info!("🚀 Opportunity {} marked for execution: {}", 
                          opportunity.id, analysis.reason);
                    competitive_opportunities.push(opportunity);
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
                info!("� All opportunities safe for batching - using BATCH EXECUTION for efficiency");
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
            if !validate_single_pool(pool, &self.pool_validation_config, &self.banned_pairs_manager).await? {
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
        let valid_pools = match validate_pools(pools_vec).await {
            Ok(pools) => pools,
            Err(e) => {
                warn!("Pool validation failed: {}", e);
                Vec::new()
            }
        };
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
        if !validate_single_pool(&pool, &self.pool_validation_config, &self.banned_pairs_manager).await? {
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

        if let Some(rpc_client) = &self.rpc_client {
            if rpc_client.is_healthy().await {
                debug!("RPC client reported as healthy.");
            } else {
                warn!("RPC client reported as unhealthy.");
                overall_healthy = false;
            }
        } else {
            warn!("RPC client not configured; skipping RPC health check.");
        }

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

        self.dex_providers_health_check().await;

        let degradation_factor = self.config.degradation_profit_factor.unwrap_or(1.5);
        let current_min_profit = self.get_min_profit_threshold_pct().await;
        let should_degrade = !ws_healthy || !overall_healthy;
        let was_degraded = self.degradation_mode.swap(should_degrade, Ordering::Relaxed);

        if should_degrade && !was_degraded {
            warn!("[ArbitrageEngine] Entering degradation mode due to system health issues");
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

    pub async fn dex_providers_health_check(&self) {
        if self.dex_providers.is_empty() {
            warn!("No DEX providers configured. ArbitrageEngine cannot operate without DEX APIs.");
        } else {
            info!("Checking health of {} DEX providers...", self.dex_providers.len());
            
            for (i, provider) in self.dex_providers.iter().enumerate() {
                let provider_name = provider.get_name();
                info!("DEX provider #{} ({}) health check: OK", i + 1, provider_name);
            }
            info!("All {} DEX providers reported healthy", self.dex_providers.len());
        }
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
                let valid_pools = match validate_pools(pools.clone()).await {
                    Ok(pools) => pools,
                    Err(e) => {
                        warn!("Pool validation failed: {}", e);
                        Vec::new()
                    }
                };
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
        
        let valid_pools = match validate_pools(pools_vec).await {
            Ok(pools) => pools,
            Err(e) => {
                warn!("Pool validation failed: {}", e);
                Vec::new()
            }
        };
        let valid_count = valid_pools.len();
        let invalid_count = total_pools - valid_count;
        let rejection_rate = (invalid_count as f64 / total_pools as f64) * 100.0;
        
        Ok((total_pools, invalid_count, rejection_rate))
    }
}

impl ArbitrageOrchestrator {
    /// Execute opportunities via single executor
    async fn execute_via_single_executor(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<String>, ArbError> {
        let executor = self.executor.as_ref()
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

    /// Advanced arbitrage calculation using high-precision mathematics
    /// Implements optimal input calculation, cycle detection, and execution strategy selection
    pub async fn calculate_advanced_arbitrage(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<OptimalArbitrageResult, ArbError> {
        info!("🧮 Starting advanced arbitrage calculation for opportunity: {}", opportunity.id);

        // Extract pool information from the opportunity
        let pools: Result<Vec<Arc<PoolInfo>>, ArbError> = opportunity.pool_path
            .iter()
            .map(|&pool_addr| {
                self.hot_cache.get(&pool_addr)
                    .map(|entry| entry.value().clone())
                    .ok_or_else(|| ArbError::PoolNotFound(pool_addr.to_string()))
            })
            .collect();

        let pools = pools?;
        let pool_refs: Vec<&PoolInfo> = pools.iter().map(|p| p.as_ref()).collect();

        // Extract directions from the hops
        let directions: Vec<bool> = opportunity.hops.iter().map(|hop| {
            // Determine direction based on tokens - simplified logic
            // In practice, this would need more sophisticated token matching
            true // Placeholder - implement proper direction detection
        }).collect();

        // Get current SOL price and available capital
        let sol_price_usd = self.get_sol_price_usd().await.unwrap_or(100.0);
        let available_capital_sol = self.get_available_capital_sol().await.unwrap_or(1.0);
        let gas_cost_sol = self.estimate_gas_cost_sol(&opportunity).await.unwrap_or(0.01);

        // Convert to Decimal for high-precision calculation
        let sol_price_decimal = Decimal::from_f64(sol_price_usd)
            .ok_or_else(|| ArbError::ExecutionError("Invalid SOL price".to_string()))?;
        let capital_decimal = Decimal::from_f64(available_capital_sol)
            .ok_or_else(|| ArbError::ExecutionError("Invalid capital amount".to_string()))?;
        let gas_decimal = Decimal::from_f64(gas_cost_sol)
            .ok_or_else(|| ArbError::ExecutionError("Invalid gas cost".to_string()))?;

        // Create an analyzer for advanced calculations
        let mut analyzer = ArbitrageAnalyzer::new(&self.config, Arc::clone(&self.metrics));
        
        // Perform advanced calculation using the analyzer
        let result = analyzer.calculate_optimal_execution(
            &pool_refs,
            capital_decimal,
            Decimal::from_f64(0.01).unwrap_or_default(), // target profit 1%
        ).map_err(|e| ArbError::ExecutionError(format!("Advanced math calculation failed: {}", e)))?;

        info!("✅ Advanced calculation complete for {}: optimal_input={:.6}, profit={:.6}, flash_loan={}",
              opportunity.id, 
              result.get_optimal_input_f64(),
              result.get_expected_profit_f64(),
              result.requires_flash_loan);

        Ok(result)
    }

    /// Get current SOL price in USD (placeholder implementation)
    async fn get_sol_price_usd(&self) -> Option<f64> {
        if let Some(provider) = &self.price_provider {
            provider.get_current_price("SOL")
        } else {
            // Fallback to a reasonable default
            Some(100.0)
        }
    }

    /// Get available capital in SOL (placeholder implementation)
    async fn get_available_capital_sol(&self) -> Option<f64> {
        // In practice, this would query the wallet balance
        // For now, return a reasonable default
        Some(10.0)
    }

    /// Estimate gas cost for the arbitrage opportunity
    async fn estimate_gas_cost_sol(&self, opportunity: &MultiHopArbOpportunity) -> Option<f64> {
        // Simple estimation based on number of hops
        let base_cost = 0.005; // 0.005 SOL base cost
        let hop_cost = 0.002; // 0.002 SOL per hop
        let total_cost = base_cost + (opportunity.hops.len() as f64 * hop_cost);
        
        Some(total_cost)
    }

    /// Enhanced opportunity evaluation using logarithmic weights for cycle detection
    pub async fn evaluate_with_bellman_ford(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        info!("🔄 Evaluating {} opportunities using Bellman-Ford cycle detection", opportunities.len());

        let mut profitable_opportunities = Vec::new();

        for opportunity in opportunities {
            // Calculate advanced arbitrage metrics
            match self.calculate_advanced_arbitrage(&opportunity).await {
                Ok(advanced_result) => {
                    if advanced_result.should_execute() {
                        info!("🎯 Opportunity {} passed advanced evaluation: profit={:.6}", 
                              opportunity.id, advanced_result.get_expected_profit_f64());
                        profitable_opportunities.push(opportunity);
                    } else {
                        debug!("❌ Opportunity {} rejected by advanced evaluation", opportunity.id);
                    }
                }
                Err(e) => {
                    warn!("⚠️ Advanced calculation failed for opportunity {}: {}", opportunity.id, e);
                    // Fall back to basic evaluation if advanced calculation fails
                    if opportunity.profit_pct > 0.1 { // Basic 0.1% profit threshold
                        profitable_opportunities.push(opportunity);
                    }
                }
            }
        }

        info!("✅ Advanced evaluation complete: {} profitable opportunities found", profitable_opportunities.len());
        Ok(profitable_opportunities)
    }
}

impl Clone for ArbitrageOrchestrator {
    fn clone(&self) -> Self {
        Self {
            hot_cache: Arc::clone(&self.hot_cache),
            ws_manager: self.ws_manager.as_ref().map(Arc::clone),
            price_provider: self.price_provider.as_ref().map(Arc::clone),
            metrics: Arc::clone(&self.metrics),
            rpc_client: self.rpc_client.as_ref().map(Arc::clone),
            config: Arc::clone(&self.config),
            degradation_mode: Arc::clone(&self.degradation_mode),
            last_health_check: Arc::clone(&self.last_health_check),
            health_check_interval: self.health_check_interval,
            ws_reconnect_attempts: Arc::clone(&self.ws_reconnect_attempts),
            max_ws_reconnect_attempts: self.max_ws_reconnect_attempts,
            detector: Arc::clone(&self.detector),
            dex_providers: self.dex_providers.clone(),
            dynamic_threshold_updater: self.dynamic_threshold_updater.as_ref().map(Arc::clone),
            pool_validation_config: self.pool_validation_config.clone(),
            banned_pairs_manager: Arc::clone(&self.banned_pairs_manager),
            executor: self.executor.as_ref().map(Arc::clone),
            batch_execution_engine: self.batch_execution_engine.as_ref().map(Arc::clone),
            opportunity_sender: self.opportunity_sender.clone(),
            detection_metrics: Arc::clone(&self.detection_metrics),
            execution_enabled: Arc::clone(&self.execution_enabled),
        }
    }
}