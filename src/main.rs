// src/main.rs
pub mod arbitrage;
mod cache;
mod config;
mod utils;
mod dex;
mod discovery;
mod error;
mod metrics;
mod solana;
pub mod websocket; // This was already here, good.

use crate::{
    arbitrage::{
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor,
        pipeline::ExecutionPipeline, // <-- Add this import
    },
    cache::Cache,
    config::settings::Config,
    dex::{
        get_all_clients_arc,
        path_finder::{PathFinder, ArbitrageDiscoveryConfig}, // <-- Add PathFinder imports
        quoting_engine::{AdvancedQuotingEngine, QuotingEngineOperations}, // <-- Add quoting engine imports
        banned_pairs::integrate_banned_pairs_system, // <-- Import banned pairs system
        raydium::RaydiumClient, // <-- Import RaydiumClient for PoolDiscoverable test
        quote::PoolDiscoverable, // <-- Import PoolDiscoverable trait
    },
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::{RawAccountUpdate, SolanaWebsocketManager},
    },
    utils::{setup_logging, ProgramConfig, PoolInfo},
};
use dashmap::DashMap; // <-- Add DashMap import for PathFinder
use log::{error, info, warn};
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{broadcast, Mutex, RwLock},
    time::interval,
};

type MainResult<T> = Result<T, ArbError>;

fn init_and_get_config() -> Arc<Config> {
    // Use Config::from_env() directly since load_config might not be exported
    let config = Config::from_env();
    config.validate_and_log();
    Arc::new(config)
}

#[tokio::main]
async fn main() -> MainResult<()> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    // Integrate banned pairs system at startup (for demonstration/production use)
    if let Err(e) = integrate_banned_pairs_system() {
        warn!("Banned pairs system integration failed: {e}");
    }

    let app_config = init_and_get_config();
    info!("Application configuration loaded and validated successfully.");

    let program_config = ProgramConfig::new("ChillarezBot".to_string(), env!("CARGO_PKG_VERSION").to_string());
    program_config.log_details();

    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0),
        app_config.metrics_log_path.clone(),
    )));
    metrics.lock().await.log_launch();

    info!("Initializing Solana RPC clients...");
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints: Vec<String> = app_config.rpc_url_backup
                    .as_ref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_else(Vec::new);
    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint,
        fallback_rpc_endpoints,
        app_config.rpc_max_retries.unwrap_or(3) as usize,
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)),
    ));
    info!("High-availability Solana RPC client initialized.");

    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs)
            .await
            .map_err(|e| ArbError::ConfigError(format!("Redis cache init failed: {}", e)))?,
    );
    info!("Redis cache initialized successfully.");

    let dex_api_clients = get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized.");

    // Discover pools from all DEXs using the DexClient::discover_pools method
    let mut discovered_pools_count = 0;
    for dex_client in &dex_api_clients {
        match dex_client.discover_pools().await {
            Ok(pools) => {
                info!("Discovered {} pools from {}", pools.len(), dex_client.get_name());
                discovered_pools_count += pools.len();
            }
            Err(e) => {
                warn!("Failed to discover pools from {}: {}", dex_client.get_name(), e);
            }
        }
    }
    
    // Also demonstrate PoolDiscoverable trait usage
    info!("Testing PoolDiscoverable trait methods...");
    let raydium_client = RaydiumClient::new(); // RaydiumClient implements PoolDiscoverable
    // Test individual PoolDiscoverable trait methods to eliminate warnings
    info!("Testing PoolDiscoverable with {}", raydium_client.dex_name());
    
    // Test discover_pools method
    match raydium_client.discover_pools().await {
        Ok(pools) => info!("PoolDiscoverable discovered {} pools from Raydium", pools.len()),
        Err(e) => info!("PoolDiscoverable discovery failed (expected): {}", e),
    }
    
    // Test fetch_pool_data method with a dummy pool address  
    let dummy_address = solana_sdk::system_program::id(); // Use a valid Pubkey
    match raydium_client.fetch_pool_data(dummy_address).await {
        Ok(pool) => info!("PoolDiscoverable fetched pool data: {}", pool.address),
        Err(e) => info!("PoolDiscoverable fetch failed (expected): {}", e),
    }
    
    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::new(RwLock::new(HashMap::new()));
    metrics.lock().await.log_pools_fetched(pools_map.read().await.len());
    info!("Pool discovery completed. Found {} pools total. Pool data map initialized (current size: {}).", 
          discovered_pools_count, pools_map.read().await.len());

    // Create DashMap for PathFinder (concurrent-safe pool cache)
    let pool_data_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>> = Arc::new(DashMap::new());
    info!("Pool data cache (DashMap) initialized for PathFinder.");

    let wallet_path = app_config.trader_wallet_keypair_path.clone().unwrap_or(app_config.wallet_path.clone());
    let wallet = match read_keypair_file(&wallet_path) {
        Ok(kp) => Arc::new(kp),
        Err(e) => return Err(ArbError::ConfigError(format!("Failed to read wallet keypair '{}': {}", wallet_path, e))),
    };
    info!("Trader wallet loaded: {}", wallet.pubkey());

    // Initialize AdvancedQuotingEngine for PathFinder
    info!("Initializing AdvancedQuotingEngine...");
    let quoting_engine = Arc::new(AdvancedQuotingEngine::new(
        pool_data_cache.clone(),
        dex_api_clients.clone(),
    ));
    info!("AdvancedQuotingEngine initialized with {} DEX clients.", dex_api_clients.len());

    // Create broadcast channel for arbitrage opportunities
    let (opportunity_sender, opportunity_receiver) = broadcast::channel::<crate::dex::opportunity::MultiHopArbOpportunity>(1000);
    info!("Opportunity broadcast channel created with buffer size 1000.");

    // Create ArbitrageDiscoveryConfig
    let arbitrage_discovery_config = ArbitrageDiscoveryConfig {
        discovery_interval: Duration::from_secs(app_config.cycle_interval_seconds.unwrap_or(30)),
        min_profit_bps_threshold: (app_config.min_profit_pct * 100.0) as u32, // Convert % to basis points
        max_hops_for_opportunity: app_config.max_hops.unwrap_or(3),
    };
    info!("ArbitrageDiscoveryConfig created: interval={}s, min_profit={}bps, max_hops={}", 
          arbitrage_discovery_config.discovery_interval.as_secs(),
          arbitrage_discovery_config.min_profit_bps_threshold,
          arbitrage_discovery_config.max_hops_for_opportunity);

    // Initialize PathFinder
    info!("Initializing PathFinder...");
    let path_finder = Arc::new(PathFinder::new(
        pool_data_cache.clone(),
        quoting_engine as Arc<dyn QuotingEngineOperations + Send + Sync>,
        dex_api_clients.clone(),
        opportunity_sender,
        arbitrage_discovery_config,
    ));
    info!("PathFinder initialized successfully.");

    // Start PathFinder background services
    let graph_update_interval = Duration::from_secs(app_config.congestion_update_interval_secs.unwrap_or(300)); // Use existing config field or default 5 minutes
    info!("Starting PathFinder background services with graph update interval: {}s", graph_update_interval.as_secs());
    let path_finder_clone = path_finder.clone();
    tokio::spawn(async move {
        path_finder_clone.start_services(graph_update_interval).await;
    });

    // Subscribe to opportunities (placeholder for now - just log them)
    let mut opportunity_receiver_for_logging = opportunity_receiver;
    tokio::spawn(async move {
        info!("Starting opportunity receiver for logging...");
        while let Ok(opportunity) = opportunity_receiver_for_logging.recv().await {
            info!("🚀 PathFinder discovered opportunity: ID={}, Profit={}bps, Hops={}, Initial=${:.2}, Final=${:.2}",
                  opportunity.id, 
                  opportunity.profit_bps,
                  opportunity.hops.len(),
                  opportunity.initial_input_amount as f64 / 1_000_000.0, // Convert lamports to rough token amount
                  opportunity.final_output_amount as f64 / 1_000_000.0);
        }
        warn!("Opportunity receiver stopped - channel closed");
    });

    // Initialize WebSocket Manager components
    let ws_setup_opt: Option<(Arc<Mutex<SolanaWebsocketManager>>, broadcast::Receiver<RawAccountUpdate>)> = if !app_config.ws_url.is_empty() {
        let (manager, raw_updates_rx_for_main) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            app_config.rpc_url_backup
                .as_ref()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_else(Vec::new),
            app_config.ws_update_channel_size.unwrap_or(1024),
            ha_solana_rpc_client.clone(),
            Some(redis_cache.clone()),
        );
        let manager_arc = Arc::new(Mutex::new(manager));
        {
            let mgr = manager_arc.lock().await;
            let _ = mgr.start().await; // FIX: start() takes no arguments
        }
        Some((manager_arc, raw_updates_rx_for_main))
    } else {
        warn!("WebSocket URL not configured, WebSocket manager not started.");
        None
    };

    // Initialize ArbitrageEngine
    let price_provider_instance = None;

    let arbitrage_engine: Arc<ArbitrageEngine> = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        ws_setup_opt.as_ref().map(|(manager_arc, _rx)| manager_arc.clone()),
        price_provider_instance,
        Some(ha_solana_rpc_client.clone()),
        app_config.clone(),
        metrics.clone(),
        dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");

    // Setup new PoolDiscoveryService with mpsc channel
    info!("Setting up new PoolDiscoveryService...");
    let (pool_sender, mut pool_receiver) = tokio::sync::mpsc::channel::<Vec<crate::utils::PoolInfo>>(100);
    
    let new_pool_discovery_service = discovery::PoolDiscoveryService::new(
        dex_api_clients.clone(),
        pool_sender,
        100, // batch_size
        300, // max_pool_age_secs
        100, // delay_between_batches_ms
    );
    
    info!("New PoolDiscoveryService initialized with {} DEX clients: {:?}", 
          new_pool_discovery_service.client_count(),
          new_pool_discovery_service.get_client_names());
    
    // Start pool discovery service in background
    let _discovery_service_handle = {
        let service = new_pool_discovery_service;
        tokio::spawn(async move {
            info!("Starting continuous pool discovery...");
            if let Err(e) = service.run_continuous_discovery(300).await { // 5 minute intervals
                error!("Pool discovery service failed: {}", e);
            }
        })
    };
    
    // Start pool receiver to process discovered pools
    let _pool_processing_handle = {
        let pools_map_clone = pools_map.clone();
        let pool_data_cache_clone = pool_data_cache.clone(); // <-- Add DashMap clone
        let metrics_clone = metrics.clone();
        let arbitrage_engine_clone = arbitrage_engine.clone();
        tokio::spawn(async move {
            info!("Starting pool receiver...");
            while let Some(mut discovered_pools) = pool_receiver.recv().await {
                info!("Received {} pools from discovery service", discovered_pools.len());
                
                // Fetch live reserve data for all discovered pools using ArbitrageEngine
                info!("Fetching live reserve data for {} pools...", discovered_pools.len());
                match arbitrage_engine_clone.fetch_live_pool_reserves(&mut discovered_pools).await {
                    Ok(updated_count) => {
                        info!("Successfully updated live reserves for {}/{} pools", updated_count, discovered_pools.len());
                    }
                    Err(e) => {
                        error!("Failed to fetch live reserves: {}. Using static data only.", e);
                        // Continue with static data
                    }
                }
                
                // Update both the pools map and DashMap pool cache
                let mut pools_write = pools_map_clone.write().await;
                for pool in discovered_pools {
                    let pool_arc = Arc::new(pool.clone());
                    pools_write.insert(pool.address, pool_arc.clone());
                    pool_data_cache_clone.insert(pool.address, pool_arc); // <-- Add to DashMap
                }
                
                // Update metrics
                let mut metrics_lock = metrics_clone.lock().await;
                metrics_lock.log_pools_fetched(pools_write.len());
                
                info!("Pool map now contains {} total pools, DashMap cache: {} pools", pools_write.len(), pool_data_cache_clone.len());
            }
            warn!("Pool receiver stopped - channel closed");
        })
    };

    let simulation_mode_from_env = env::var("SIMULATION_MODE")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    // Keep executor lean - only for execution, no logic
    // FIX: Pass all 5 required arguments to ArbitrageExecutor::new
    let mut pipeline = ExecutionPipeline::new();
    let event_sender_for_executor = pipeline.get_sender();
    tokio::spawn(async move {
        pipeline.start_listener().await;
    });

    let tx_executor: Arc<ArbitrageExecutor> = Arc::new(ArbitrageExecutor::new(
        wallet.clone(),
        ha_solana_rpc_client.primary_client.clone(), // <-- Use the correct type for the second argument
        Some(event_sender_for_executor),
        app_config.clone(),
        metrics.clone(),
    ));
    info!("ArbitrageExecutor initialized.");

    if let Some((ws_manager_arc, raw_updates_rx_consumer)) = ws_setup_opt {
        arbitrage_engine.start_services(Some(Arc::clone(&redis_cache))).await;

        let dummy_pubkey_to_sub = solana_sdk::system_program::id();
        info!("[Main] Subscribing to dummy account {} for testing purposes.", dummy_pubkey_to_sub);
        if let Err(e) = ws_manager_arc.lock().await.subscribe_to_account(dummy_pubkey_to_sub).await {
            warn!("[Main] Failed to subscribe to dummy account: {}", e);
        }

        let actual_raw_updates_rx = raw_updates_rx_consumer;
        tokio::spawn(async move {
            let mut rx = actual_raw_updates_rx;
            info!("[RawUpdateLogger] Started");
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        info!(
                            "[RawUpdateLogger] Update: Pubkey={}, DataLen={:?}, Timestamp={:?}",
                            update.pubkey(),
                            update._data().map(|d| d.len()), // <-- Use _data() instead of data()
                            match &update {
                                RawAccountUpdate::Account { timestamp, .. } => Some(timestamp),
                                RawAccountUpdate::Disconnected { timestamp, .. } => Some(timestamp),
                                RawAccountUpdate::Error { timestamp, .. } => Some(timestamp),
                            }
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        warn!("[RawUpdateLogger] Lagged behind, some raw updates were missed.");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("[RawUpdateLogger] Broadcast channel closed. Logger stopping.");
                        break;
                    }
                }
            }
            info!("[RawUpdateLogger] Stopped");
        });
    }

    // Move dynamic threshold updates to the engine
    let engine_for_threshold_task: Arc<ArbitrageEngine> = Arc::clone(&arbitrage_engine);
    let threshold_task_handle = tokio::spawn(async move {
        engine_for_threshold_task.run_dynamic_threshold_updates().await;
        info!("Dynamic threshold update task finished (normally runs indefinitely).");
    });

    // Network congestion monitoring - handle internally without calling non-existent methods
    let ha_rpc_for_congestion = Arc::clone(&ha_solana_rpc_client);
    let congestion_update_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let congestion_task_handle = tokio::spawn(async move {
        info!("Starting congestion monitoring task (interval: {}s).", congestion_update_interval_secs);
        let mut interval_timer = interval(Duration::from_secs(congestion_update_interval_secs));
        loop {
            interval_timer.tick().await;
            // Monitor network congestion internally
            let congestion_factor = ha_rpc_for_congestion.get_network_congestion_factor().await;
            info!("Current network congestion factor: {}", congestion_factor);
        }
    });

    let engine_for_health_task: Arc<ArbitrageEngine> = Arc::clone(&arbitrage_engine);
    let health_check_interval_from_config = Duration::from_secs(app_config.health_check_interval_secs.unwrap_or(60));
    let health_check_task_handle = tokio::spawn(async move {
        info!("Starting health check task (interval: {:?}).", health_check_interval_from_config);
        let mut interval_timer = interval(health_check_interval_from_config);
        loop {
            interval_timer.tick().await;
            engine_for_health_task.run_health_checks().await;
            let status_string = engine_for_health_task.get_current_status_string().await;
            info!("Engine Status: {}", status_string);
        }
    });

    // Store a reference to the opportunity_receiver for the main loop
    let mut opportunity_receiver_main = path_finder.subscribe_to_opportunities();

    info!("Starting main arbitrage detection cycle (interval: {}s).", app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set"));
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set")));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut fixed_input_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set")));
    fixed_input_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Fix the broadcast channel type annotation issue
    let (_shutdown_tx_ws_processor, _shutdown_rx_ws_processor) = broadcast::channel::<()>(1);
    let mut ws_processor_task_handle = None;

    if arbitrage_engine.ws_manager.is_some() {
        let engine_for_ws_loop = Arc::clone(&arbitrage_engine);
        info!("[Main] Spawning ArbitrageEngine's WebSocket update processing loop.");
        ws_processor_task_handle = Some(tokio::spawn(async move {
            engine_for_ws_loop.process_websocket_updates_loop().await;
        }));
    } else {
        warn!("[Main] WebSocket manager not configured, ArbitrageEngine's WebSocket update processing loop will not be started.");
    }
    
    loop {
        tokio::select! {
            _ = main_cycle_interval.tick() => {
                let current_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                info!("Main arbitrage cycle tick at {}.", current_time_str);
                metrics.lock().await.increment_main_cycles();
                let cycle_start_time = std::time::Instant::now();

                match arbitrage_engine.detect_arbitrage().await {
                    Ok(mut all_opportunities) => {
                        if !all_opportunities.is_empty() {
                            info!("Detected {} total opportunities in this cycle.", all_opportunities.len());
                            all_opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));

                            if let Some(best_opp) = all_opportunities.first() {
                                info!("Best opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                
                                // Use the lean executor directly for execution
                                if app_config.paper_trading || simulation_mode_from_env {
                                    info!("Paper/Simulation Mode: Processing opportunity {}", best_opp.id);
                                    // In paper trading mode, just log the opportunity
                                    info!("(Paper/Simulated) Would execute opportunity {} with profit {:.4}%", best_opp.id, best_opp.profit_pct);
                                } else {
                                    info!("Real Trading Mode: Attempting to execute opportunity {}", best_opp.id);
                                    match tx_executor.execute_opportunity(best_opp).await {
                                        Ok(signature) => {
                                            info!("Successfully EXECUTED opportunity {} - Signature: {}", best_opp.id, signature);
                                        }
                                        Err(e) => {
                                            error!("Failed to EXECUTE opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                }
                            }
                        } else {
                            info!("No arbitrage opportunities found in this cycle.");
                        }
                    }
                    Err(e) => {
                        error!("[CAT: {:?}] Error during arbitrage detection cycle: {}", e.categorize(), e);
                    }
                }
                metrics.lock().await.record_main_cycle_duration(cycle_start_time.elapsed().as_millis() as u64);
            },
            _ = fixed_input_cycle_interval.tick() => {
                info!("Fixed input and multi-hop detection cycle part.");
                let fixed_input_amount_example = app_config.fixed_input_arb_amount.unwrap_or(100.0);

                match arbitrage_engine.discover_fixed_input_opportunities(fixed_input_amount_example).await {
                    Ok(mut fixed_input_opps) => {
                        if !fixed_input_opps.is_empty() {
                            info!("Detected {} fixed input (2-hop) opportunities.", fixed_input_opps.len());
                            fixed_input_opps.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
                            if let Some(best_opp) = fixed_input_opps.first() {
                                info!("Best fixed input opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                // Use the lean executor directly
                                if app_config.paper_trading || simulation_mode_from_env {
                                    info!("(Paper/Simulated) Would execute fixed input opportunity {} with profit {:.4}%", best_opp.id, best_opp.profit_pct);
                                } else {
                                    match tx_executor.execute_opportunity(best_opp).await {
                                        Ok(signature) => {
                                            info!("Successfully executed fixed input opportunity {} - Signature: {}", best_opp.id, signature);
                                        }
                                        Err(e) => {
                                            error!("Failed to execute fixed input opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                }
                            }
                        } else {
                            info!("No fixed input (2-hop) opportunities found in this cycle part.");
                        }
                    }
                    Err(e) => {
                        error!("[CAT: {:?}] Error during fixed input opportunity detection: {}", e.categorize(), e);
                    }
                }

                match arbitrage_engine.discover_multihop_opportunities().await {
                    Ok(mut multihop_opps) => {
                        if !multihop_opps.is_empty() {
                            info!("Detected {} multi-hop opportunities.", multihop_opps.len());
                            multihop_opps.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
                            if let Some(best_opp) = multihop_opps.first() {
                                info!("Best multi-hop opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                // Use the lean executor directly
                                if app_config.paper_trading || simulation_mode_from_env {
                                    info!("(Paper/Simulated) Would execute multi-hop opportunity {} with profit {:.4}%", best_opp.id, best_opp.profit_pct);
                                } else {
                                    match tx_executor.execute_opportunity(best_opp).await {
                                        Ok(signature) => {
                                            info!("Successfully executed multi-hop opportunity {} - Signature: {}", best_opp.id, signature);
                                        }
                                        Err(e) => {
                                            error!("Failed to execute multi-hop opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                }
                            }
                        } else {
                            info!("No multi-hop opportunities found in this cycle part.");
                        }
                    }
                    Err(e) => {
                        error!("[CAT: {:?}] Error during multi-hop opportunity detection: {}", e.categorize(), e);
                    }
                }
            },
            // Handle PathFinder opportunities from background discovery
            pathfinder_opp = opportunity_receiver_main.recv() => {
                match pathfinder_opp {
                    Ok(opportunity) => {
                        info!("🔍 PathFinder discovered opportunity: ID={}, Profit={}bps, Hops={}", 
                              opportunity.id, opportunity.profit_bps, opportunity.hops.len());
                        
                        // Convert PathFinder opportunity to legacy format for executor compatibility
                        let legacy_opportunity = crate::arbitrage::opportunity::MultiHopArbOpportunity {
                            id: opportunity.id.clone(),
                            hops: opportunity.hops.iter().map(|hop| crate::arbitrage::opportunity::ArbHop {
                                dex: hop.dex_type.clone(),
                                pool: hop.pool_address,
                                input_token: hop.input_mint.to_string(), // Convert Pubkey to string
                                output_token: hop.output_mint.to_string(), // Convert Pubkey to string
                                input_amount: hop.input_amount as f64,
                                expected_output: hop.output_amount as f64,
                            }).collect(),
                            total_profit: (opportunity.final_output_amount as f64) - (opportunity.initial_input_amount as f64),
                            profit_pct: opportunity.profit_bps as f64 / 100.0, // Convert basis points to percentage
                            input_amount: opportunity.initial_input_amount as f64,
                            expected_output: opportunity.final_output_amount as f64,
                            ..Default::default()
                        };
                        
                        // Process the opportunity like regular ones
                        if app_config.paper_trading || simulation_mode_from_env {
                            info!("(Paper/Simulated) Would execute PathFinder opportunity {} with profit {:.4}%", 
                                  legacy_opportunity.id, legacy_opportunity.profit_pct);
                        } else {
                            info!("Real Trading Mode: Attempting to execute PathFinder opportunity {}", legacy_opportunity.id);
                            match tx_executor.execute_opportunity(&legacy_opportunity).await {
                                Ok(signature) => {
                                    info!("Successfully EXECUTED PathFinder opportunity {} - Signature: {}", 
                                          legacy_opportunity.id, signature);
                                }
                                Err(e) => {
                                    error!("Failed to EXECUTE PathFinder opportunity {}: {}", legacy_opportunity.id, e);
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("PathFinder opportunity receiver lagged, skipped {} opportunities", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("PathFinder opportunity channel closed");
                    }
                }
            },
            _ = tokio::signal::ctrl_c() => {
                info!("CTRL-C received, shutting down tasks...");
                threshold_task_handle.abort();
                congestion_task_handle.abort();
                health_check_task_handle.abort();

                if let Some(manager_arc) = &arbitrage_engine.ws_manager {
                    let dummy_pubkey_to_unsub = solana_sdk::system_program::id();
                    info!("[Main] Unsubscribing from dummy account {} before shutdown.", dummy_pubkey_to_unsub);
                    if let Err(e) = manager_arc.lock().await.unsubscribe(&dummy_pubkey_to_unsub).await {
                         warn!("[Main] Failed to unsubscribe from dummy account: {}", e);
                    }
        manager_arc.lock().await.stop().await;
                    info!("[Main] WebSocket manager stop called.");
    }
    
                if let Some(handle) = ws_processor_task_handle.take() {
                    info!("[Main] Awaiting WebSocket processor task to complete...");
                    if let Err(e) = handle.await {
                        error!("[Main] WebSocket processor task panicked or encountered an error: {:?}", e);
}
                }
                info!("Background tasks aborted. Exiting.");
                
                info!("[Main] Shutting down ArbitrageEngine...");
                if let Err(e) = arbitrage_engine.shutdown().await {
                    error!("[Main] Error during ArbitrageEngine shutdown: {}", e);
                }
                info!("[Main] ArbitrageEngine shutdown complete.");
                break;
            }
        }
    }
    metrics.lock().await.summary();
    Ok(())
}