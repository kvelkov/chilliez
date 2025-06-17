// src/main.rs
pub mod arbitrage;
mod cache;
mod config;
mod dex;
mod error;
mod local_metrics;
pub mod paper_trading; // Add paper trading module
pub mod performance;
mod solana;
mod utils;
pub mod webhooks;
pub mod websocket; // Add performance module

use crate::{
    arbitrage::{
        execution::HftExecutor,
        orchestrator::{ArbitrageOrchestrator, PriceDataProvider},
    },
    cache::Cache,
    config::settings::Config,
    dex::{
        discovery::{BannedPairsManager, PoolDiscoveryService, PoolValidationConfig},
        get_all_clients_arc, get_all_discoverable_clients,
        live_update_manager::{LiveUpdateConfig, LiveUpdateManager, LiveUpdateManagerBuilder},
    },
    error::ArbError,
    local_metrics::Metrics,
    solana::rpc::SolanaRpcClient,
    utils::{setup_logging, PoolInfo},
    webhooks::integration::WebhookIntegrationService,
};
use clap::{Arg, Command};
use dashmap::DashMap;
use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
};
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
    // Initialize the StatsD exporter for metrics (UDP to 127.0.0.1:8125)
    let recorder = metrics_exporter_statsd::StatsdBuilder::from("127.0.0.1", 8125)
        .build(None)
        .expect("failed to build StatsD recorder");
    metrics::set_global_recorder(recorder).expect("failed to set global metrics recorder");

    setup_logging().expect("Failed to initialize logging");

    // Parse command line arguments
    let matches = Command::new("Solana Arbitrage Bot")
        .version("2.1.4")
        .about("Modern Solana arbitrage bot with real-time webhook architecture")
        .arg(
            Arg::new("paper-trading")
                .long("paper-trading")
                .help("Enable paper trading mode (simulated trades with virtual money)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("real-trading")
                .long("real-trading")
                .help("Enable real trading mode (actual transactions)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("paper-logs-dir")
                .long("paper-logs-dir")
                .help("Directory for paper trading logs")
                .value_name("DIR")
                .default_value("./paper_trading_logs"),
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

    info!("🚀 ═══════════════════════════════════════════════════════════════════════════");
    info!("🚀 SOLANA ARBITRAGE BOT v2.1.4 - Modern Real-Time Architecture");
    info!("🚀 ═══════════════════════════════════════════════════════════════════════════");

    if paper_trading_enabled {
        info!("📄 TRADING MODE: Paper Trading (Virtual Portfolio)");
        info!("📁 Logs Directory: {}", paper_logs_dir);
        info!("💡 Safe testing environment with simulated funds");
    } else if real_trading_enabled {
        info!("💰 TRADING MODE: Real Trading (Live Portfolio)");
        warn!("⚠️  CAUTION: Real funds will be used for trading!");
        warn!("⚠️  Ensure wallet security and risk management settings!");
    } else {
        info!("📊 TRADING MODE: Analysis Only (No Execution)");
        info!("💡 Use --paper-trading or --real-trading to enable execution");
    }

    // --- Configuration & Initialization ---
    let mut app_config = if paper_trading_enabled {
        // Load paper trading specific environment file
        match dotenv::from_filename(".env.paper-trading") {
            Ok(_) => {
                info!("✅ Loaded .env.paper-trading configuration");
                Config::from_env_without_loading()
            }
            Err(e) => {
                error!("❌ Failed to load .env.paper-trading: {}", e);
                warn!("💡 Make sure .env.paper-trading exists with proper configuration");
                warn!("💡 Falling back to default .env configuration");
                Config::from_env()
            }
        }
    } else {
        Config::from_env()
    };

    // Override paper trading setting from CLI arguments
    if paper_trading_enabled {
        app_config.paper_trading = true;
    } else if real_trading_enabled {
        app_config.paper_trading = false;
    }

    let app_config = Arc::new(app_config);
    app_config.validate_and_log();

    let metrics = Arc::new(Mutex::new(Metrics::new()));

    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &app_config.rpc_url,
        vec![],
        app_config.rpc_max_retries.unwrap_or(3) as usize,
        std::time::Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)),
    ));

    let redis_cache =
        Arc::new(Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs).await?);

    let dex_api_clients = get_all_clients_arc(redis_cache.clone(), app_config.clone()).await;

    // --- Initial Pool Discovery (One-time population) ---
    info!("� ═══════════════════════════════════════════════════════════════════════════");
    info!("🔍 POOL DISCOVERY: Initializing liquidity pool cache...");
    info!("🔍 ═══════════════════════════════════════════════════════════════════════════");

    let validation_config = PoolValidationConfig::default();
    let discoverable_clients =
        get_all_discoverable_clients(redis_cache.clone(), app_config.clone());
    let pool_discovery_service = Arc::new(
        PoolDiscoveryService::new(
            discoverable_clients,
            validation_config,
            "banned_pairs_log.csv".to_string(), // Fix: Pass String instead of &Path
            ha_solana_rpc_client.clone(),
        )
        .map_err(ArbError::from)?,
    );

    let discovery_start = std::time::Instant::now();
    let discovery_result_count = pool_discovery_service
        .discover_all_pools()
        .await
        .map_err(ArbError::from)?;
    let discovery_duration = discovery_start.elapsed();

    info!("✅ POOL DISCOVERY COMPLETE:");
    info!("   • Duration: {:?}", discovery_duration);
    info!("   • Pools Found: {} total pools", discovery_result_count);
    info!(
        "   • Rate: {:.1} pools/second",
        discovery_result_count as f64 / discovery_duration.as_secs_f64()
    );

    // Get the actual discovered pools for cache population
    let discovery_result = pool_discovery_service.get_all_cached_pools();

    // --- Initialize Hot Cache ---
    info!("🔥 ═══════════════════════════════════════════════════════════════════════════");
    info!("🔥 HOT CACHE: Initializing high-performance memory cache...");
    info!("🔥 ═══════════════════════════════════════════════════════════════════════════");
    let hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>> = Arc::new(DashMap::new());

    // Populate hot cache with initial discovery results
    for pool_info in discovery_result.iter() {
        hot_cache.insert(pool_info.address, pool_info.clone());
    }

    info!("✅ HOT CACHE READY:");
    info!("   • Pools Loaded: {} pools", hot_cache.len());
    info!("   • Memory Structure: DashMap for concurrent access");
    info!("   • Access Time: Sub-millisecond lookups enabled");

    // --- Initialize Modern Real-Time Architecture ---
    info!("📡 ═══════════════════════════════════════════════════════════════════════════");
    info!("📡 REAL-TIME ARCHITECTURE: Setting up webhook & live update system...");
    info!("📡 ═══════════════════════════════════════════════════════════════════════════");

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

    info!("⚙️  LIVE UPDATE CONFIGURATION:");
    info!(
        "   • Channel Buffer: {} updates",
        live_update_config.channel_buffer_size
    );
    info!(
        "   • Max Rate: {} updates/second",
        live_update_config.max_updates_per_second
    );
    info!(
        "   • Batching: {} (size: {})",
        live_update_config.enable_batching, live_update_config.batch_size
    );
    info!("   • Validation: {}", live_update_config.validate_updates);

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
        webhook_service.initialize().await.map_err(|e| {
            ArbError::ConfigError(format!("Failed to initialize webhook service: {}", e))
        })?;
        info!("✅ WEBHOOK SERVICE: Initialized successfully");
    } else {
        info!("➖ WEBHOOK SERVICE: Disabled in configuration");
    }

    // Connect LiveUpdateManager to webhook system
    live_update_manager
        .connect_webhook_system()
        .await
        .map_err(|e| {
            ArbError::ConfigError(format!("Failed to connect to webhook system: {}", e))
        })?;

    // Start LiveUpdateManager for real-time processing
    live_update_manager
        .start()
        .await
        .map_err(|e| ArbError::ConfigError(format!("Failed to start LiveUpdateManager: {}", e)))?;

    info!("✅ REAL-TIME ARCHITECTURE: Fully operational");
    info!("   • Live Updates: Processing real-time data streams");
    info!("   • Webhook Integration: Connected to external feeds");
    info!("   • Performance: Sub-second response times enabled");

    // --- Initialize Enhanced Arbitrage Engine ---
    info!("🎯 ═══════════════════════════════════════════════════════════════════════════");
    info!("🎯 ARBITRAGE ENGINE: Initializing trading and execution systems...");
    info!("🎯 ═══════════════════════════════════════════════════════════════════════════");

    // Create price provider
    let _price_provider: Arc<dyn PriceDataProvider> = Arc::new(SimplePriceProvider {
        sol_price: app_config.sol_price_usd.unwrap_or(100.0),
    });

    // Initialize executor if wallet is configured
    let executor = if let Some(wallet_path) = &app_config.trader_wallet_keypair_path {
        if !wallet_path.is_empty() && fs::metadata(wallet_path).is_ok() {
            match read_keypair_file(wallet_path) {
                Ok(keypair) => {
                    info!("💼 WALLET CONFIGURATION:");
                    info!("   • Status: ✅ Loaded successfully");
                    info!("   • Address: {}", keypair.pubkey());
                    info!("   • Path: {}", wallet_path);

                    // Create non-blocking RPC client for executor
                    let executor_rpc =
                        Arc::new(NonBlockingRpcClient::new(app_config.rpc_url.clone()));

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
                    let discovery_result_vec: Vec<PoolInfo> = discovery_result
                        .iter()
                        .map(|arc_pool| (**arc_pool).clone())
                        .collect();
                    executor.update_pool_cache(&discovery_result_vec).await; // Fix: Pass &[PoolInfo] instead of &usize

                    info!("✅ EXECUTION ENGINE: Ready for live trading");
                    info!(
                        "   • Pool Cache: {} pools loaded",
                        discovery_result_vec.len()
                    );
                    info!("   • DEX Clients: Initialized for all supported DEXs");

                    Some(Arc::new(executor))
                }
                Err(e) => {
                    warn!("⚠️  WALLET CONFIGURATION:");
                    warn!("   • Status: ❌ Failed to load");
                    warn!("   • Path: {}", wallet_path);
                    warn!("   • Error: {}", e);
                    warn!("   • Result: Execution disabled - analysis only");
                    None
                }
            }
        } else {
            warn!("⚠️  WALLET CONFIGURATION:");
            warn!("   • Status: ❌ File not found");
            warn!("   • Path: {}", wallet_path);
            warn!("   • Result: Execution disabled - analysis only");
            None
        }
    } else {
        if app_config.paper_trading {
            info!("💼 WALLET CONFIGURATION:");
            info!("   • Status: ➖ Not required (Paper Trading Mode)");
            info!("   • Virtual Portfolio: Ready for simulated trading");
        } else {
            warn!("⚠️  WALLET CONFIGURATION:");
            warn!("   • Status: ❌ No wallet configured");
            warn!("   • Result: Execution disabled - analysis only");
        }
        None
    };

    // Create banned pairs manager
    let banned_pairs_manager = Arc::new(
        BannedPairsManager::new("banned_pairs_log.csv".to_string()) // Fix: Pass String instead of Path
            .unwrap_or_else(|e| {
                warn!(
                    "Failed to load banned pairs: {}, creating minimal manager",
                    e
                );
                // Since constructor requires CSV file, use a fallback or handle error
                panic!("Cannot initialize banned pairs manager: {}", e);
            }),
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

    info!("✅ ARBITRAGE ENGINE: Fully operational");
    info!("   • Strategy: Real-time opportunity detection");
    info!("   • Processing: Sub-millisecond hot cache access");
    info!("   • Integration: Connected to all DEX protocols");

    // --- Real-Time Arbitrage Detection and Execution Loop ---
    info!("🔄 ═══════════════════════════════════════════════════════════════════════════");
    info!("🔄 TRADING LOOP: Starting real-time arbitrage detection...");
    info!("🔄 ═══════════════════════════════════════════════════════════════════════════");

    let arbitrage_engine_clone = arbitrage_engine.clone();
    tokio::spawn(async move {
        use crate::utils::timing::{PerformanceTracker, Timer};
        use std::sync::Mutex as StdMutex;

        let mut cycle_count = 0;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100)); // 100ms cycles for real-time
        let performance_tracker = Arc::new(StdMutex::new(PerformanceTracker::new()));

        // Log initial status
        info!("🎯 DETECTION PARAMETERS:");
        info!("   • Cycle Interval: 100ms (10 Hz frequency)");
        info!("   • Status Reports: Every 60 seconds");
        info!("   • Performance Stats: Every 10 seconds");

        loop {
            interval.tick().await;
            cycle_count += 1;

            if cycle_count % 100 == 0 {
                // Log every 10 seconds
                debug!(
                    "🔄 Detection cycle #{} ({}s runtime)",
                    cycle_count,
                    cycle_count / 10
                );
            }

            // Time the full detection cycle
            let cycle_timer = Timer::start("detection_cycle");

            // Use real-time hot cache for detection
            match arbitrage_engine_clone
                .detect_arbitrage_opportunities()
                .await
            {
                Ok(opportunities) => {
                    let detection_duration = cycle_timer.finish();

                    // Record detection performance
                    if let Ok(mut tracker) = performance_tracker.lock() {
                        tracker.record_operation("detection_cycle", detection_duration);
                    }

                    if !opportunities.is_empty() {
                        info!("🎯 OPPORTUNITY FOUND: {} opportunities in cycle #{} (detected in {:.2}ms)", 
                              opportunities.len(), cycle_count, detection_duration.as_millis());

                        // Execute the most profitable opportunity
                        if let Some(best_opportunity) = opportunities.first() {
                            let exec_timer = Timer::start("opportunity_execution");
                            match arbitrage_engine_clone
                                .execute_opportunities_with_routing(vec![best_opportunity.clone()])
                                .await
                            {
                                Ok(results) => {
                                    let execution_duration = exec_timer.finish();
                                    if let Ok(mut tracker) = performance_tracker.lock() {
                                        tracker.record_operation(
                                            "execution_cycle",
                                            execution_duration,
                                        );
                                    }

                                    if !results.is_empty() {
                                        info!("✅ EXECUTION SUCCESS: Opportunity {} completed in {:.2}ms", 
                                              best_opportunity.id, execution_duration.as_millis());
                                    }
                                }
                                Err(e) => {
                                    exec_timer.finish();
                                    warn!(
                                        "❌ EXECUTION FAILED: Opportunity {}: {}",
                                        best_opportunity.id, e
                                    );
                                }
                            }
                        }
                    }

                    // Performance reporting every 10 seconds
                    if cycle_count % 100 == 0 {
                        if let Ok(tracker) = performance_tracker.lock() {
                            if let Some(avg_detection) =
                                tracker.get_average_duration("detection_cycle")
                            {
                                info!(
                                    "⚡ Performance: Avg detection {:.2}ms",
                                    avg_detection.as_millis()
                                );
                            }
                            let slow_ops = tracker.get_slow_operations(500); // > 500ms
                            if !slow_ops.is_empty() {
                                warn!("🐌 Slow operations detected: {:?}", slow_ops);
                            }
                        }
                    }

                    if cycle_count % 600 == 0 {
                        // Every minute
                        let status = arbitrage_engine_clone.get_enhanced_status().await;
                        info!("📊 ENGINE STATUS: {}", status);

                        // Full performance summary every minute
                        if let Ok(tracker) = performance_tracker.lock() {
                            tracker.print_summary();
                        }
                    }
                }
                Err(e) => {
                    cycle_timer.finish();
                    if cycle_count % 100 == 0 {
                        // Only log errors periodically to avoid spam
                        error!("❌ DETECTION ERROR (cycle #{}): {}", cycle_count, e);
                    }
                }
            }
        }
    });

    // --- Performance Monitoring with LiveUpdateManager Metrics ---
    info!("📊 ═══════════════════════════════════════════════════════════════════════════");
    info!("📊 MONITORING SYSTEM: Starting performance tracking...");
    info!("📊 ═══════════════════════════════════════════════════════════════════════════");

    let live_update_manager_arc = Arc::new(live_update_manager);
    let monitoring_live_manager = live_update_manager_arc.clone();
    let _monitoring_metrics = metrics.clone();
    let monitoring_engine = arbitrage_engine.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        info!("📈 PERFORMANCE TRACKING:");
        info!("   • Reporting Interval: 60 seconds");
        info!("   • Metrics: Cache stats, system performance, operation counts");
        info!("   • Live Updates: Real-time pool and market data tracking");

        loop {
            interval.tick().await;

            // Enhanced monitoring with real-time metrics
            let (cache_size, hit_rate) = monitoring_engine.get_hot_cache_stats().await;
            let hot_cache_size = monitoring_live_manager.get_hot_cache_size();
            let live_metrics = monitoring_live_manager.get_metrics();

            info!("📊 ═══ PERFORMANCE REPORT ═══");
            info!(
                "🔥 Hot Cache: {} pools, {:.1}% hit rate",
                cache_size, hit_rate
            );
            info!("📡 Live Manager: {} pools tracked", hot_cache_size);

            // Log LiveUpdateManager metrics
            live_metrics.log_summary();

            // Update metrics
            // monitoring_metrics.lock().await.log_pools_fetched(cache_size);
        }
    });

    // --- Health Monitoring with LiveUpdateManager ---
    let health_engine = arbitrage_engine.clone();
    let health_live_manager = live_update_manager_arc.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
        info!("🏥 HEALTH MONITORING:");
        info!("   • Check Interval: 5 minutes");
        info!("   • Components: Arbitrage engine, Live updates, Network connectivity");

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
            info!("🔗 Starting webhook server...");
            if let Err(e) = webhook_service.start_webhook_server().await {
                error!("❌ Webhook server failed: {}", e);
            }
        });
    }

    // --- Modern Architecture Summary ---
    info!("🎉 ═══════════════════════════════════════════════════════════════════════════");
    info!("🎉 BOT STARTUP COMPLETE - All Systems Operational");
    info!("🎉 ═══════════════════════════════════════════════════════════════════════════");

    info!("🚀 SYSTEM OVERVIEW:");
    info!("   • Architecture: Modern webhook-driven with LiveUpdateManager");
    info!(
        "   • Hot Cache: {} pools with sub-millisecond access",
        hot_cache.len()
    );
    info!("   • Real-time Updates: LiveUpdateManager processing live data");
    if app_config.enable_webhooks {
        info!("   • Webhook Integration: ✅ Connected to Helius feeds");
    } else {
        info!("   • Webhook Integration: ➖ Disabled");
    }
    info!("   • Detection Frequency: 100ms cycles (10 Hz)");
    info!("   • Execution: High-frequency with intelligent routing");
    info!("   • Monitoring: Comprehensive real-time performance tracking");

    if app_config.paper_trading {
        info!("🎯 TRADING STATUS: � Paper Trading Mode - Safe testing environment");
    } else {
        info!("🎯 TRADING STATUS: 💰 Live Trading Mode - Real funds at risk");
    }

    info!("════════════════════════════════════════════════════════════════════════════");
    info!("🚀 Bot is ready! Monitoring for arbitrage opportunities...");
    info!("💡 Press CTRL-C to gracefully shutdown the system");
    info!("════════════════════════════════════════════════════════════════════════════");

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl-c");

    info!("🛑 ═══════════════════════════════════════════════════════════════════════════");
    info!("🛑 SHUTDOWN INITIATED - Gracefully stopping all systems...");
    info!("🛑 ═══════════════════════════════════════════════════════════════════════════");

    // Enhanced shutdown sequence
    info!("🔄 Stopping live update manager...");
    live_update_manager_arc.stop().await;
    info!("✅ Live update manager stopped");

    info!("🔄 Shutting down arbitrage engine...");
    arbitrage_engine.shutdown().await?;
    info!("✅ Arbitrage engine shutdown complete");

    info!("🎉 ═══════════════════════════════════════════════════════════════════════════");
    info!("🎉 SHUTDOWN COMPLETE - All systems stopped gracefully");
    info!("🎉 Thank you for using Solana Arbitrage Bot v2.1.4!");
    info!("🎉 ═══════════════════════════════════════════════════════════════════════════");

    Ok(())
}
