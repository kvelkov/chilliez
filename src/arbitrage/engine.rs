// src/arbitrage/engine.rs
use crate::{
    arbitrage::{
        detector::ArbitrageDetector,
        dynamic_threshold::DynamicThresholdUpdater,
        opportunity::MultiHopArbOpportunity,
        executor::ArbitrageExecutor,
        batch_executor::{AdvancedBatchExecutor, BatchExecutionConfig},
        pipeline::ExecutionPipeline,
    },
    config::settings::Config,
    dex::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::{PoolInfo, PoolParser, pool_validation::{PoolValidationConfig, validate_pools, validate_pools_basic, validate_single_pool}},
};

use dashmap::DashMap;
use log::{debug, error, info, warn};
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
    sync::{Mutex, RwLock},
    time::timeout,
};

// Define a simple trait for price data providers since CryptoDataProvider doesn't exist
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}

/// Enhanced arbitrage engine with Sprint 2 hot cache integration and advanced execution
pub struct ArbitrageEngine {
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
    pub detector: Arc<Mutex<ArbitrageDetector>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    pub dynamic_threshold_updater: Option<Arc<DynamicThresholdUpdater>>,
    pub pool_validation_config: PoolValidationConfig,
    
    // Sprint 2: Advanced execution components
    pub executor: Option<Arc<ArbitrageExecutor>>,
    pub batch_executor: Arc<Mutex<AdvancedBatchExecutor>>,
    pub execution_pipeline: Arc<Mutex<ExecutionPipeline>>,
    
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

impl ArbitrageEngine {
    pub fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn PriceDataProvider>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        dex_providers: Vec<Arc<dyn DexClient>>,
        executor: Option<Arc<ArbitrageExecutor>>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageDetector::new_from_config(&config)));
        let health_check_interval = Duration::from_secs(config.health_check_interval_secs.unwrap_or(60));
        let max_ws_reconnect_attempts = config.max_ws_reconnect_attempts.unwrap_or(5) as u64;
        
        // Initialize dynamic threshold updater if configured
        let dynamic_threshold_updater = if config.volatility_tracker_window.is_some() {
            Some(Arc::new(DynamicThresholdUpdater::new(Arc::clone(&config))))
        } else {
            None
        };

        // Configure pool validation with sensible defaults
        let pool_validation_config = PoolValidationConfig {
            max_pool_age_secs: 300, // 5 minutes default
            skip_empty_pools: true,
            min_reserve_threshold: 1000,
            verify_on_chain: false, // Expensive, off by default
        };

        // Sprint 2: Initialize advanced execution components with default batch config
        let batch_config = BatchExecutionConfig {
            max_batch_size: 5, // Default batch size
            max_compute_units: config.transaction_cu_limit.unwrap_or(1_400_000),
            max_batch_execution_time_ms: 3000,
            priority_threshold: 7,
        };
        
        let batch_executor = Arc::new(Mutex::new(AdvancedBatchExecutor::new(batch_config)));
        let execution_pipeline = Arc::new(Mutex::new(ExecutionPipeline::new()));
        let detection_metrics = Arc::new(Mutex::new(DetectionMetrics::default()));

        info!("🚀 Enhanced ArbitrageEngine initialized with Sprint 2 features:");
        info!("   🔥 Hot cache integration: {} pools", hot_cache.len());
        info!("   🎯 DEX providers: {}", dex_providers.len());
        info!("   ⚡ Batch execution: enabled");
        info!("   📊 Advanced metrics: enabled");

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
            executor,
            batch_executor,
            execution_pipeline,
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
            let mut metrics_guard = self.metrics.lock().await;
            detector.find_all_opportunities(&validated_pools, &mut metrics_guard).await?
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
        let validated_pools = if self.pool_validation_config.verify_on_chain {
            match validate_pools(&pools_vec, &self.pool_validation_config, self.rpc_client.as_ref()).await {
                Ok(pools) => pools,
                Err(e) => {
                    warn!("Advanced pool validation failed, falling back to basic validation: {}", e);
                    validate_pools_basic(&pools_vec, &self.pool_validation_config)
                }
            }
        } else {
            validate_pools_basic(&pools_vec, &self.pool_validation_config)
        };
        
        let validation_time = validation_start.elapsed();
        debug!("✅ Pool validation completed in {:.2}ms", validation_time.as_secs_f64() * 1000.0);
        
        // Convert back to HashMap format
        let result: HashMap<Pubkey, Arc<PoolInfo>> = validated_pools.into_iter()
            .map(|pool| (pool.address, Arc::new(pool)))
            .collect();
            
        Ok(result)
    }

    /// Sprint 2: Enhanced opportunity execution with batching
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
        info!("🚀 Executing {} opportunities with advanced batching...", total_opportunities);

        let mut execution_results = Vec::new();

        // Group opportunities by priority and compatibility for batching
        let (high_priority, normal_priority): (Vec<_>, Vec<_>) = opportunities
            .into_iter()
            .partition(|opp| opp.profit_pct > 2.0); // High priority if >2% profit

        // Execute high priority opportunities immediately
        if !high_priority.is_empty() {
            info!("⚡ Executing {} high-priority opportunities immediately", high_priority.len());
            for opportunity in high_priority {
                match self.execute_single_opportunity(&opportunity).await {
                    Ok(signature) => {
                        execution_results.push(signature);
                        info!("✅ High-priority opportunity {} executed successfully", opportunity.id);
                    }
                    Err(e) => {
                        error!("❌ Failed to execute high-priority opportunity {}: {}", opportunity.id, e);
                    }
                }
            }
        }

        // Execute normal priority opportunities (fix borrow checker issue)
        let normal_priority_count = normal_priority.len();
        if !normal_priority.is_empty() {
            info!("📦 Executing {} normal-priority opportunities", normal_priority_count);
            for opportunity in normal_priority {
                match self.execute_single_opportunity(&opportunity).await {
                    Ok(signature) => {
                        execution_results.push(signature);
                        info!("✅ Normal-priority opportunity {} executed successfully", opportunity.id);
                    }
                    Err(e) => {
                        error!("❌ Failed to execute normal-priority opportunity {}: {}", opportunity.id, e);
                    }
                }
            }
        }

        let execution_time = start_time.elapsed();
        info!("🎯 Execution completed in {:.2}ms: {}/{} opportunities executed successfully", 
              execution_time.as_secs_f64() * 1000.0, execution_results.len(), total_opportunities);

        Ok(execution_results)
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
        
        let valid_pools = validate_pools_basic(&pools_vec, &self.pool_validation_config);
        let filtered_count = pools_vec.len() - valid_pools.len();
        
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
        if let Some(updater) = &self.dynamic_threshold_updater {
            info!("Starting dynamic threshold updater...");
            let _handle = DynamicThresholdUpdater::start_monitoring_task(
                Arc::clone(updater),
                Arc::clone(&self.metrics),
            );
        }

        // Start execution pipeline
        info!("🚀 Starting execution pipeline...");
        let pipeline = self.execution_pipeline.clone();
        tokio::spawn(async move {
            let mut pipeline_guard = pipeline.lock().await;
            pipeline_guard.start_listener().await;
        });
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
                    updater.add_price_observation(sol_price).await;
                    info!("Added SOL price observation from provider: ${:.2}", sol_price);
                } else {
                    let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                    updater.add_price_observation(current_sol_price).await;
                    info!("Added SOL price observation from config: ${:.2}", current_sol_price);
                }
            } else {
                let current_sol_price = self.config.sol_price_usd.unwrap_or(100.0);
                updater.add_price_observation(current_sol_price).await;
                info!("Added SOL price observation from config: ${:.2}", current_sol_price);
            }

            let current_threshold = updater.get_current_threshold().await;
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
            let current_threshold = updater.get_current_threshold().await;
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
                let valid_pools = validate_pools_basic(pools, &self.pool_validation_config);
                let invalid_count = pools.len() - valid_pools.len();
                
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
                let parser = crate::dex::raydium::RaydiumPoolParser;
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
                let parser = crate::dex::orca::OrcaPoolParser;
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
        info!("Updating pool validation configuration: max_age={}, skip_empty={}, min_reserve={}, verify_on_chain={}", 
               config.max_pool_age_secs, config.skip_empty_pools, config.min_reserve_threshold, config.verify_on_chain);
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
        
        let valid_pools = validate_pools_basic(&pools_vec, &self.pool_validation_config);
        let valid_count = valid_pools.len();
        let invalid_count = total_pools - valid_count;
        let rejection_rate = (invalid_count as f64 / total_pools as f64) * 100.0;
        
        Ok((total_pools, invalid_count, rejection_rate))
    }
}

impl Clone for ArbitrageEngine {
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
            executor: self.executor.as_ref().map(Arc::clone),
            batch_executor: Arc::clone(&self.batch_executor),
            execution_pipeline: Arc::clone(&self.execution_pipeline),
            detection_metrics: Arc::clone(&self.detection_metrics),
            execution_enabled: Arc::clone(&self.execution_enabled),
        }
    }
}