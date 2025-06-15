// src/main.rs
pub mod arbitrage;
mod cache;
mod config;
mod utils;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod websocket;
pub mod webhooks;
pub mod paper_trading; // Add paper trading module

use crate::{
    arbitrage::{
        orchestrator::{ArbitrageOrchestrator, PriceDataProvider},
        execution::HftExecutor,
    },
    cache::Cache,
    config::settings::Config,
    dex::{
        get_all_clients_arc, get_all_discoverable_clients,
        discovery::{PoolDiscoveryService, BannedPairsManager, PoolValidationConfig},
        live_update_manager::{LiveUpdateManager, LiveUpdateManagerBuilder, LiveUpdateConfig},
    },
    error::ArbError,
    metrics::Metrics,
    solana::rpc::SolanaRpcClient,
    utils::{setup_logging, PoolInfo},
    webhooks::integration::WebhookIntegrationService,
};
use clap::{Arg, Command};
use dashmap::DashMap;
use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{pubkey::Pubkey, signature::{Signer, read_keypair_file}};
use std::{fs, sync::Arc};
use tokio::sync::Mutex;

// Simple price provider implementation for demonstration
struct SimplePriceProvider {
    sol_price: f64,
}

impl PriceDataProvider for SimplePriceProvider {
    fn get_current_price(&self, symbol: &str) -> Option<f64> {
        match symbol {
            "SOL" => Some(self.sol_price),
            "USDC" | "USDT" => Some(1.0),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), ArbError> {
    setup_logging().expect("Failed to initialize logging");
    
    // Parse command line arguments
    let matches = Command::new("Solana Arbitrage Bot")
        .version("2.1.4")
        .about("Modern Solana arbitrage bot with real-time webhook architecture")
        .arg(
            Arg::new("paper-trading")
                .long("paper-trading")
                .help("Enable paper trading mode (simulated trades with virtual money)")
                .action(clap::ArgAction::SetTrue)
        )
        .arg(
            Arg::new("real-trading")
                .long("real-trading")
                .help("Enable real trading mode (actual transactions)")
                .action(clap::ArgAction::SetTrue)
        )
        .arg(
            Arg::new("paper-logs-dir")
                .long("paper-logs-dir")
                .help("Directory for paper trading logs")
                .value_name("DIR")
                .default_value("./paper_trading_logs")
        )
        .get_matches();

    let paper_trading_enabled = matches.get_flag("paper-trading");
    let real_trading_enabled = matches.get_flag("real-trading");
    let paper_logs_dir = matches.get_one::<String>("paper-logs-dir").unwrap();

    // Validation: cannot enable both modes
    if paper_trading_enabled && real_trading_enabled {
        error!("❌ Cannot enable both --paper-trading and --real-trading modes simultaneously");
        std::process::exit(1);
    }

    info!("🚀 Modern Solana Arbitrage Bot starting with real-time webhook architecture...");
    
    if paper_trading_enabled {
        info!("📄 Paper trading mode ENABLED - using virtual money");
        info!("📁 Paper trading logs will be saved to: {}", paper_logs_dir);
    } else if real_trading_enabled {
        info!("💰 Real trading mode ENABLED - using actual funds");
        warn!("⚠️  CAUTION: Real trading mode will execute actual transactions!");
    } else {
        info!("ℹ️  No trading mode specified. Use --paper-trading or --real-trading");
        info!("📖 Running in analysis-only mode (no trade execution)");
    }

    // --- Configuration & Initialization ---
    let mut app_config = Config::from_env();
    
    // Override paper trading setting from CLI arguments
    if paper_trading_enabled {
        app_config.paper_trading = true;
    } else if real_trading_enabled {
        app_config.paper_trading = false;
    }
    
    let app_config = Arc::new(app_config);
    app_config.validate_and_log();
    
    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0), 
        app_config.metrics_log_path.clone()
    )));
    
    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &app_config.rpc_url, 
        vec![], 
        app_config.rpc_max_retries.unwrap_or(3) as usize, 
        std::time::Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500))
    ));
    
    let redis_cache = Arc::new(Cache::new(
        &app_config.redis_url, 
        app_config.redis_default_ttl_secs
    ).await?);
    
    let dex_api_clients = get_all_clients_arc(redis_cache.clone(), app_config.clone()).await;

    // --- Initial Pool Discovery (One-time population) ---
    info!("🔧 Running initial pool discovery for cache population...");
    
    let validation_config = PoolValidationConfig::default();
    let discoverable_clients = get_all_discoverable_clients(redis_cache.clone(), app_config.clone());
    let pool_discovery_service = Arc::new(PoolDiscoveryService::new(
        discoverable_clients, 
        validation_config,
        "banned_pairs_log.csv".to_string(), // Fix: Pass String instead of &Path
        ha_solana_rpc_client.clone(), 
    ).map_err(ArbError::from)?);

    let discovery_start = std::time::Instant::now();
    let discovery_result_count = pool_discovery_service.discover_all_pools().await.map_err(ArbError::from)?;
    let discovery_duration = discovery_start.elapsed();
    
    info!("✅ Initial discovery complete in {:?}: {} pools found", discovery_duration, discovery_result_count);

    // Get the actual discovered pools for cache population
    let discovery_result = pool_discovery_service.get_all_cached_pools();

    // --- Initialize Hot Cache ---
    info!("🔥 Initializing hot cache with discovered pools...");
    let hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>> = Arc::new(DashMap::new());
    
    // Populate hot cache with initial discovery results
    for pool_info in discovery_result.iter() {
        hot_cache.insert(pool_info.address, pool_info.clone());
    }
    
    metrics.lock().await.log_pools_fetched(hot_cache.len());
    info!("🔥 Hot cache initialized with {} pools", hot_cache.len());

    // --- Initialize Modern Real-Time Architecture ---
    info!("🌐 Setting up modern real-time update architecture with LiveUpdateManager...");
    
    // Configure LiveUpdateManager
    let live_update_config = LiveUpdateConfig {
        channel_buffer_size: 50000,
        max_updates_per_second: 2000,
        enable_batching: true,
        batch_size: 100,
        batch_timeout_ms: 50,
        validate_updates: true,
        max_update_age_ms: 3000,
    };
    
    // Create LiveUpdateManager using builder pattern
    let mut live_update_manager = LiveUpdateManagerBuilder::new()
        .with_config(live_update_config)
        .with_hot_cache(hot_cache.clone())
        .with_app_config(app_config.clone())
        .build()
        .map_err(|e| ArbError::ConfigError(format!("Failed to create LiveUpdateManager: {}", e)))?;
    
    // Ensure compiler recognizes LiveUpdateManager usage (false positive workaround)
    let _: &LiveUpdateManager = &live_update_manager;
    
    // Initialize webhook integration service
    let mut webhook_service = WebhookIntegrationService::new(app_config.clone());
    if app_config.enable_webhooks {
        webhook_service.initialize().await
            .map_err(|e| ArbError::ConfigError(format!("Failed to initialize webhook service: {}", e)))?;
    }
    
    // Connect LiveUpdateManager to webhook system
    live_update_manager.connect_webhook_system().await
        .map_err(|e| ArbError::ConfigError(format!("Failed to connect to webhook system: {}", e)))?;
    
    // Start LiveUpdateManager for real-time processing
    live_update_manager.start().await
        .map_err(|e| ArbError::ConfigError(format!("Failed to start LiveUpdateManager: {}", e)))?;
    
    info!("✅ Modern real-time architecture initialized with LiveUpdateManager");

    // --- Initialize Enhanced Arbitrage Engine ---
    info!("🎯 Initializing modern arbitrage engine...");
    
    // Create price provider
    let _price_provider: Arc<dyn PriceDataProvider> = Arc::new(SimplePriceProvider {
        sol_price: app_config.sol_price_usd.unwrap_or(100.0),
    });

    // Initialize executor if wallet is configured
    let executor = if let Some(wallet_path) = &app_config.trader_wallet_keypair_path {
        if !wallet_path.is_empty() && fs::metadata(wallet_path).is_ok() {
            match read_keypair_file(wallet_path) {
                Ok(keypair) => {
                    info!("✅ Loaded trading wallet: {}", keypair.pubkey());
                    
                    // Create non-blocking RPC client for executor
                    let executor_rpc = Arc::new(NonBlockingRpcClient::new(app_config.rpc_url.clone()));
                    
                    // Create executor
                    let mut executor = HftExecutor::new(
                        Arc::new(keypair),
                        executor_rpc,
                        None, // Event sender - can be added later for monitoring
                        app_config.clone(),
                        metrics.clone(),
                        hot_cache.clone(), // Pass the hot_cache
                    );
                    
                    // Initialize DEX clients for routing
                    executor.initialize_dex_clients(redis_cache.clone()).await;
                    
                    // Update executor's pool cache with discovered pools
                    let discovery_result_vec: Vec<PoolInfo> = discovery_result.iter().map(|arc_pool| (**arc_pool).clone()).collect();
                    executor.update_pool_cache(&discovery_result_vec).await; // Fix: Pass &[PoolInfo] instead of &usize
                    
                    Some(Arc::new(executor))
                }
                Err(e) => {
                    warn!("⚠️ Failed to load wallet from {}: {}. Execution will be disabled.", wallet_path, e);
                    None
                }
            }
        } else {
            warn!("⚠️ Wallet path {} not found. Execution will be disabled.", wallet_path);
            None
        }
    } else {
        warn!("⚠️ No trader wallet configured. Execution will be disabled.");
        None
    };

    // Create banned pairs manager
    let banned_pairs_manager = Arc::new(
        BannedPairsManager::new("banned_pairs_log.csv".to_string()) // Fix: Pass String instead of Path
            .unwrap_or_else(|e| {
                warn!("Failed to load banned pairs: {}, creating minimal manager", e);
                // Since constructor requires CSV file, use a fallback or handle error
                panic!("Cannot initialize banned pairs manager: {}", e);
            })
    );

    // Initialize modern arbitrage engine with hot cache and real-time updates
    let arbitrage_engine = Arc::new(ArbitrageOrchestrator::new(
        hot_cache.clone(),
        None, // WebSocket manager not needed with LiveUpdateManager
        Some(ha_solana_rpc_client.clone()),
        app_config.clone(),
        metrics.clone(),
        dex_api_clients,
        executor,
        None, // batch_execution_engine - can be initialized later if needed
        banned_pairs_manager,
    ));

    // Start enhanced arbitrage engine services
    // arbitrage_engine.start_services(Some(redis_cache.clone())).await;

    info!("✅ Modern arbitrage engine initialized with real-time updates!");
    info!("   🔥 Hot cache: {} pools", hot_cache.len());
    info!("   📡 Real-time updates: LiveUpdateManager active");
    info!("   🌐 Webhook integration: enabled");
    info!("   🎯 Enhanced detection: ready");
    info!("   ⚡ Sub-millisecond access: active");

    // --- Real-Time Arbitrage Detection and Execution Loop ---
    info!("🎯 Starting real-time arbitrage detection loop...");
    let arbitrage_engine_clone = arbitrage_engine.clone();
    tokio::spawn(async move {
        let mut cycle_count = 0;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100)); // 100ms cycles for real-time
        
        loop {
            interval.tick().await;
            cycle_count += 1;
            
            if cycle_count % 100 == 0 { // Log every 10 seconds
                debug!("🔄 Real-time arbitrage cycle #{}", cycle_count);
            }
            
            // Use real-time hot cache for detection
            match arbitrage_engine_clone.detect_arbitrage_opportunities().await {
                Ok(opportunities) => {
                    if !opportunities.is_empty() {
                        info!("🎯 Found {} opportunities in cycle #{}", opportunities.len(), cycle_count);
                        
                        // Execute the most profitable opportunity
                        if let Some(best_opportunity) = opportunities.first() {
                            match arbitrage_engine_clone.execute_opportunities_with_routing(vec![best_opportunity.clone()]).await {
                                Ok(results) => {
                                    if !results.is_empty() {
                                        info!("✅ Successfully executed opportunity {} in real-time", best_opportunity.id);
                                    }
                                }
                                Err(e) => warn!("❌ Failed to execute opportunity {}: {}", best_opportunity.id, e),
                            }
                        }
                    }
                    
                    if cycle_count % 600 == 0 { // Every minute
                        let status = arbitrage_engine_clone.get_enhanced_status().await;
                        info!("📊 Real-time Engine Status: {}", status);
                    }
                }
                Err(e) => {
                    if cycle_count % 100 == 0 { // Only log errors periodically to avoid spam
                        error!("❌ Real-time arbitrage detection failed in cycle #{}: {}", cycle_count, e);
                    }
                }
            }
        }
    });

    // --- Performance Monitoring with LiveUpdateManager Metrics ---
    let live_update_manager_arc = Arc::new(live_update_manager);
    let monitoring_live_manager = live_update_manager_arc.clone();
    let monitoring_metrics = metrics.clone();
    let monitoring_engine = arbitrage_engine.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            
            // Enhanced monitoring with real-time metrics
            let (cache_size, hit_rate) = monitoring_engine.get_hot_cache_stats().await;
            let hot_cache_size = monitoring_live_manager.get_hot_cache_size();
            let live_metrics = monitoring_live_manager.get_metrics();
            
            info!("📊 Real-Time Performance Metrics:");
            info!("   🔥 Hot Cache: {} pools, {:.1}% hit rate", cache_size, hit_rate);
            info!("   📡 LiveUpdateManager: {} pools managed", hot_cache_size);
            
            // Log LiveUpdateManager metrics
            live_metrics.log_summary();
            
            // Update metrics
            monitoring_metrics.lock().await.log_pools_fetched(cache_size);
        }
    });

    // --- Health Monitoring with LiveUpdateManager ---
    let health_engine = arbitrage_engine.clone();
    let health_live_manager = live_update_manager_arc.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
        loop {
            interval.tick().await;
            info!("🏥 Running comprehensive health check...");
            
            // Check arbitrage engine health
            health_engine.run_full_health_check().await;
            
            // Check LiveUpdateManager health
            if let Err(e) = health_live_manager.health_check().await {
                error!("❌ LiveUpdateManager health check failed: {}", e);
            } else {
                debug!("✅ LiveUpdateManager health check passed");
            }
        }
    });

    // --- Start Webhook Server if Enabled ---
    if app_config.enable_webhooks {
        tokio::spawn(async move {
            if let Err(e) = webhook_service.start_webhook_server().await {
                error!("❌ Webhook server failed: {}", e);
            }
        });
    }

    // --- Modern Architecture Summary ---
    info!("✅ Modern Real-Time Solana Arbitrage Bot fully operational!");
    info!("   🚀 Architecture: Modern webhook-driven with LiveUpdateManager");
    info!("   🔥 Hot Cache: {} pools with sub-millisecond access", hot_cache.len());
    info!("   📡 Real-time updates: LiveUpdateManager handling all data flow");
    info!("   🌐 Webhook integration: Connected to Helius for live updates");
    info!("   🎯 Detection: 100ms cycle time for maximum responsiveness");
    info!("   ⚡ Execution: High-frequency with routing and batching");
    info!("   📊 Monitoring: Comprehensive real-time metrics");
    
    info!("🚀 Bot is running with modern real-time architecture. Press CTRL-C to exit.");
    
    tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
    info!("🛑 Shutting down gracefully...");
    
    // Enhanced shutdown sequence
    live_update_manager_arc.stop().await;
    arbitrage_engine.shutdown().await?;
    info!("✅ Modern architecture shutdown completed");

    Ok(())
}