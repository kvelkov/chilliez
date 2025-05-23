// src/main.rs

// Module declarations
mod arbitrage;
mod cache;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;

// Crate imports
use crate::{
    arbitrage::{
        // detector::ArbitrageDetector, // Not directly used here
        dynamic_threshold::VolatilityTracker,
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor,
        opportunity::MultiHopArbOpportunity, // Using the unified struct
    },
    cache::Cache,
    config::load_config, // Using load_config which returns Arc<Config>
    dex::get_all_clients_arc,
    error::ArbError,
    metrics::{Metrics, TradeOutcome}, // Added TradeOutcome
    solana::{
        rpc::SolanaRpcClient,
        websocket::{RawAccountUpdate, SolanaWebsocketManager, WebsocketUpdate},
    },
    utils::{load_keypair, setup_logging, PoolInfo},
};

use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Instant};

#[tokio::main]
async fn main() -> Result<(), ArbError> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    let app_config = load_config()?; // Returns Result<Arc<Config>, ArbError>
    info!("Configuration loaded and validated successfully.");

    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0), // Default SOL price if not in config
        app_config.metrics_log_path.clone(),
    )));
    metrics.lock().await.log_launch();

    info!("Initializing Solana RPC clients...");
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints_str = app_config.rpc_url_backup.clone().unwrap_or_default();

    // High-Availability RPC Client
    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint,
        fallback_rpc_endpoints_str, // This is Vec<String>
        app_config.rpc_max_retries.unwrap_or(3),
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)),
    ));
    info!(
        "High-availability Solana RPC client initialized for endpoint: {}",
        primary_rpc_endpoint
    );

    // Direct Non-Blocking RPC Client (for components that specifically need it, like ArbitrageExecutor)
    let direct_rpc_client = Arc::new(NonBlockingRpcClient::new_with_commitment(
        primary_rpc_endpoint.clone(),
        solana_sdk::commitment_config::CommitmentConfig::confirmed(),
    ));
    info!(
        "Direct non-blocking Solana RPC client initialized for endpoint: {}",
        primary_rpc_endpoint
    );

    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs)
            .await
            .map_err(|e| {
                error!(
                    "CRITICAL: Failed to initialize Redis cache at {}: {}",
                    app_config.redis_url, e
                );
                ArbError::ConfigError(format!("Redis cache initialization failed: {}", e))
            })?,
    );
    info!(
        "Redis cache initialized successfully at {}",
        app_config.redis_url
    );

    let dex_api_clients =
        get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized and configured with cache.");

    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    // Log initial pool count
    metrics.lock().await.log_pools_fetched(0); // Initially 0 pools
    info!("Initial pool data map initialized (empty).");

    let wallet_path = app_config.trader_wallet_keypair_path.clone();
    let wallet = match read_keypair_file(&wallet_path) {
        // Use solana_sdk::signature::read_keypair_file
        Ok(kp) => Arc::new(kp),
        Err(e) => {
            error!(
                "Failed to read keypair file from path '{}': {}",
                wallet_path, e
            );
            return Err(ArbError::ConfigError(format!(
                "Failed to read keypair file '{}': {}",
                wallet_path, e
            )));
        }
    };
    info!(
        "Trader wallet loaded successfully from: {}. Pubkey: {}",
        wallet_path,
        wallet.pubkey()
    );

    let tx_executor = Arc::new(
        ArbitrageExecutor::new(
            wallet.clone(),            // Wallet is Arc<Keypair>
            direct_rpc_client.clone(), // Executor uses direct client for sending
            app_config.default_priority_fee_lamports,
            Duration::from_secs(app_config.max_transaction_timeout_seconds),
            app_config.simulation_mode, // General simulation mode
            app_config.paper_trading,   // Specific paper trading mode
        )
        .with_solana_rpc(ha_solana_rpc_client.clone()),
    ); // Optionally give it HA client for its internal features
    info!("ArbitrageExecutor initialized.");

    let ws_manager_instance = if !app_config.ws_url.is_empty() {
        let (manager, _raw_account_update_receiver) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            app_config.rpc_url_backup.clone().unwrap_or_default(), // Pass fallbacks
            app_config.ws_update_channel_size.unwrap_or(1024),     // Default channel size
        );
        // The manager.start() should be called to make it connect and process messages.
        // This is handled by ArbitrageEngine::start_services now.
        Some(Arc::new(Mutex::new(manager)))
    } else {
        warn!("WebSocket URL not configured. Real-time pool updates will be disabled.");
        None
    };

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        Some(ha_solana_rpc_client.clone()), // Engine uses HA client for data fetching
        app_config.clone(),
        metrics.clone(),
        None, // No concrete price_provider implementation yet
        ws_manager_instance.clone(),
        dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");
    arbitrage_engine.start_services().await; // Start WebSocket manager etc.

    // --- Background Tasks ---
    let engine_for_threshold_task = Arc::clone(&arbitrage_engine);
    let mut threshold_task_handle = tokio::spawn(async move {
        engine_for_threshold_task
            .run_dynamic_threshold_updates()
            .await; // This is a loop
        info!("Dynamic threshold update task finished."); // Should ideally not finish unless error
    });

    let executor_for_congestion_task = Arc::clone(&tx_executor);
    let congestion_update_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let mut congestion_task_handle = tokio::spawn(async move {
        info!(
            "Starting network congestion update task (interval: {}s).",
            congestion_update_interval_secs
        );
        let mut interval_timer = interval(Duration::from_secs(congestion_update_interval_secs));
        loop {
            interval_timer.tick().await;
            if let Err(e) = executor_for_congestion_task
                .update_network_congestion()
                .await
            {
                error!("Failed to update network congestion: {}", e);
            }
        }
    });

    let engine_for_health_task = Arc::clone(&arbitrage_engine);
    let mut health_check_task_handle = tokio::spawn(async move {
        engine_for_health_task.run_health_checks().await; // Initial health check
        let health_interval = engine_for_health_task.health_check_interval; // Get from engine
        let mut interval_timer = interval(health_interval);
        loop {
            interval_timer.tick().await;
            engine_for_health_task.run_health_checks().await;
        }
    });

    info!(
        "Starting main arbitrage detection cycle (interval: {}s).",
        app_config.cycle_interval_seconds
    );
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let cycle_start_time = Instant::now();

        tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                info!("Ctrl-C received, initiating shutdown...");
                break;
            },
            ws_update_result = async { // Renamed from ws_update
                if let Some(manager_arc) = &ws_manager_instance {
                    let mut manager_guard = manager_arc.lock().await;
                    manager_guard.try_recv_update().await // Assumes this method exists and is async
                } else {
                    std::future::pending().await // Keep this branch pending if no ws_manager
                }
            }, if ws_manager_instance.is_some() => { // Condition the branch on ws_manager_instance
                match ws_update_result {
                    Ok(Some(WebsocketUpdate::PoolUpdate(updated_pool_info))) => {
                        let mut pools_w = pools_map.write().await;
                        let pool_address = updated_pool_info.address;
                        pools_w.insert(pool_address, Arc::new(updated_pool_info));
                        debug!("Processed pool update from WebSocket for pool {}", pool_address);
                        metrics.lock().await.log_pools_updated(0, 1, pools_w.len());
                    },
                    Ok(Some(WebsocketUpdate::GenericUpdate(msg))) => {
                        info!("Generic WebSocket message: {}", msg);
                    }
                    Ok(None) => { /* No message currently available */ }
                    Err(e) => {
                        error!("WebSocket receive error from manager's processed queue: {:?}", e);
                    }
                }
            },
            _ = main_cycle_interval.tick() => {
                let detection_start_time = Instant::now();
                info!("Main cycle tick: Searching for arbitrage opportunities...");
                metrics.lock().await.increment_main_cycles();

                // Using discover_multihop_opportunities as it's more general.
                // This returns Vec<MultiHopArbOpportunity>
                let opportunities = match arbitrage_engine.discover_multihop_opportunities().await {
                    Ok(ops) => ops,
                    Err(e) => {
                        error!("Error during arbitrage detection: {}", e);
                        Vec::new()
                    }
                };
                let detection_duration = detection_start_time.elapsed();
                info!("Found {} potential opportunities in {:?}", opportunities.len(), detection_duration);

                if !opportunities.is_empty() {
                    let execution_chunk_size = app_config.execution_chunk_size.unwrap_or(1) as usize;
                    for opp_chunk in opportunities.chunks(execution_chunk_size) {
                        let mut execution_futures = Vec::new();
                        for opp_item in opp_chunk { // opp_item is &MultiHopArbOpportunity
                            let opp_clone = opp_item.clone(); // Clone opp_item for async task
                            let tx_executor_clone = Arc::clone(&tx_executor);
                            let metrics_clone = Arc::clone(&metrics);
                            let sol_price = app_config.sol_price_usd.unwrap_or(100.0);
                            let base_tx_fee_sol = app_config.default_priority_fee_lamports as f64 / 1_000_000_000.0; // Approx

                            let dex_path_strings: Vec<String> = opp_clone.dex_path.iter().map(|d| format!("{:?}", d)).collect();

                            // Log attempt (before execution)
                            metrics_clone.lock().await.record_trade_attempt(
                                &opp_clone.id,
                                &dex_path_strings,
                                &opp_clone.input_token, // Symbol
                                &opp_clone.output_token, // Symbol
                                opp_clone.input_amount, // f64
                                opp_clone.expected_output, // f64
                                opp_clone.profit_pct, // Percentage
                                opp_clone.estimated_profit_usd,
                                TradeOutcome::Attempted,
                                None, // actual_profit_usd
                                0,    // execution_time_ms (will be updated on result)
                                0.0,  // tx_cost_sol (will be updated on result)
                                None, // fees_paid_usd
                                Some("Attempting execution".to_string()),
                                None, // transaction_id
                            );

                            execution_futures.push(tokio::spawn(async move {
                                let exec_start_time = Instant::now();
                                let result = tx_executor_clone.execute_opportunity(&opp_clone).await;
                                let exec_duration_ms = exec_start_time.elapsed().as_millis();

                                match result {
                                    Ok(signature) => {
                                        info!("Successfully executed opportunity {}: Signature: {}, Duration: {}ms", opp_clone.id, signature, exec_duration_ms);
                                        // Assuming actual profit is same as estimated for this logging
                                        let actual_profit_usd_val = opp_clone.estimated_profit_usd;
                                        metrics_clone.lock().await.record_trade_attempt(
                                            &opp_clone.id, &dex_path_strings, &opp_clone.input_token, &opp_clone.output_token,
                                            opp_clone.input_amount, opp_clone.expected_output, opp_clone.profit_pct,
                                            opp_clone.estimated_profit_usd,
                                            TradeOutcome::Success,
                                            actual_profit_usd_val,
                                            exec_duration_ms,
                                            base_tx_fee_sol, // Simplified tx_cost_sol
                                            Some(base_tx_fee_sol * sol_price), // Simplified fees_paid_usd
                                            None,
                                            Some(signature.to_string()),
                                        );
                                    }
                                    Err(e) => {
                                        error!("Failed to execute opportunity {}: {}. Duration: {}ms", opp_clone.id, e, exec_duration_ms);
                                        metrics_clone.lock().await.record_trade_attempt(
                                            &opp_clone.id, &dex_path_strings, &opp_clone.input_token, &opp_clone.output_token,
                                            opp_clone.input_amount, opp_clone.expected_output, opp_clone.profit_pct,
                                            opp_clone.estimated_profit_usd,
                                            TradeOutcome::Failure,
                                            None, // No actual profit on failure
                                            exec_duration_ms,
                                            base_tx_fee_sol, // Still log attempted cost
                                            Some(base_tx_fee_sol * sol_price), // Still log attempted cost
                                            Some(e.to_string()),
                                            None,
                                        );
                                    }
                                }
                            }));
                        }
                        futures::future::join_all(execution_futures).await;
                    }
                }
                metrics.lock().await.record_main_cycle_duration(cycle_start_time.elapsed().as_millis() as u64);
            },
            // Task completion/panic handling
            res = &mut threshold_task_handle => {
                error!("Dynamic threshold task exited: {:?}", res);
                // Optionally re-spawn or handle error
                // For now, just break the select if a critical task dies, or log and continue if it's non-critical
                // To re-spawn: threshold_task_handle = tokio::spawn(async move { ... });
            },
            res = &mut congestion_task_handle => {
                error!("Congestion update task exited: {:?}", res);
            },
            res = &mut health_check_task_handle => {
                error!("Health check task exited: {:?}", res);
            }
        }
    }

    // Graceful shutdown of tasks
    info!("Shutting down background tasks...");
    threshold_task_handle.abort();
    congestion_task_handle.abort();
    health_check_task_handle.abort();
    // Optionally await with timeout
    // let _ = tokio::time::timeout(Duration::from_secs(2), threshold_task_handle).await;
    // let _ = tokio::time::timeout(Duration::from_secs(2), congestion_task_handle).await;
    // let _ = tokio::time::timeout(Duration::from_secs(2), health_check_task_handle).await;

    info!("Shutting down. Final metrics summary:");
    metrics.lock().await.summary();

    Ok(())
}