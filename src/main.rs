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
pub mod websocket; // Consider if this should be solana::websocket

// Crate imports
use crate::{
    arbitrage::{
        dynamic_threshold::VolatilityTracker,
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor,
        opportunity::MultiHopArbOpportunity, // Using the unified struct
    },
    cache::Cache,
    config::load_config,
    dex::get_all_clients_arc,
    error::ArbError,
    metrics::{Metrics, TradeOutcome},
    solana::{
        rpc::SolanaRpcClient,
        websocket::{SolanaWebsocketManager, WebsocketUpdate, RawAccountUpdate}, // Added RawAccountUpdate
    },
    utils::{setup_logging, PoolInfo, load_keypair}, // Removed ProgramConfig as it's unused here
};

use log::{debug, error, info, warn};
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient; // Aliased
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

    let app_config = Arc::new(load_config()?);
    info!("Configuration loaded and validated successfully.");

    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0),
        app_config.metrics_log_path.clone(),
    )));
    metrics.lock().await.log_launch();

    info!("Initializing Solana RPC clients...");
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints_str = app_config.rpc_url_backup.clone().unwrap_or_default();

    // High-Availability RPC Client
    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint,
        fallback_rpc_endpoints_str,
        app_config.rpc_max_retries.unwrap_or(3), // Default retries
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)), // Default delay
    ));
    info!("High-availability Solana RPC client initialized.");

    // Direct Non-Blocking RPC Client (for components that specifically need it)
    let direct_rpc_client = Arc::new(
        NonBlockingRpcClient::new_with_commitment(
            primary_rpc_endpoint.clone(),
            solana_sdk::commitment_config::CommitmentConfig::confirmed(),
        ),
    );
    info!("Direct non-blocking Solana RPC client initialized.");


    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs)
            .await
            .map_err(|e| {
                error!("CRITICAL: Failed to initialize Redis cache at {}: {}", app_config.redis_url, e);
                ArbError::ConfigError(format!("Redis cache initialization failed: {}", e))
            })?,
    );
    info!("Redis cache initialized successfully at {}", app_config.redis_url);

    let dex_api_clients = get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized and configured with cache.");

    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    metrics.lock().await.log_pools_fetched(pools_map.read().await.len());
    info!("Initial pool data map initialized.");

    let wallet_path = app_config.trader_wallet_keypair_path.clone();
    let wallet = match read_keypair_file(&wallet_path) {
        Ok(kp) => Arc::new(kp),
        Err(e) => {
            error!("Failed to read keypair file from path '{}': {}", wallet_path, e);
            return Err(ArbError::ConfigError(format!("Failed to read keypair file '{}': {}", wallet_path, e)));
        }
    };
    info!("Trader wallet loaded successfully from: {}. Pubkey: {}", wallet_path, wallet.pubkey());

    // ArbitrageExecutor uses the direct NonBlockingRpcClient for sending transactions
    let tx_executor = Arc::new(ArbitrageExecutor::new(
        wallet.clone(),
        direct_rpc_client.clone(), // Using the direct non-blocking client
        app_config.default_priority_fee_lamports,
        Duration::from_secs(app_config.max_transaction_timeout_seconds),
        app_config.paper_trading, // General simulation for testing before live
        app_config.paper_trading, // Specific paper trading mode
    ).with_solana_rpc(ha_solana_rpc_client.clone())); // Provide HA client for internal HA features if any
    info!("ArbitrageExecutor initialized.");


    let ws_manager_instance = if !app_config.ws_url.is_empty() {
        let (manager, _receiver) = SolanaWebsocketManager::new( // receiver is for RawAccountUpdate
            app_config.ws_url.clone(),
            app_config.rpc_url_backup.clone().unwrap_or_default(), // Using RPC fallbacks as WS fallbacks concept
            app_config.ws_update_channel_size.unwrap_or(1024),
        );
        Some(Arc::new(Mutex::new(manager))) // Wrap manager in Arc<Mutex<>>
    } else {
        warn!("WebSocket URL not configured. Real-time pool updates will be disabled.");
        None
    };


    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        Some(ha_solana_rpc_client.clone()), // Engine uses HA client for its operations
        app_config.clone(),
        metrics.clone(),
        None, // No specific price provider implementation yet
        ws_manager_instance.clone(),
        dex_api_clients.clone(), // Pass initialized DEX clients
    ));
    info!("ArbitrageEngine initialized.");
    arbitrage_engine.start_services().await;


    // --- Background Tasks ---
    let engine_clone_for_threshold = Arc::clone(&arbitrage_engine);
    let config_clone_for_threshold = Arc::clone(&app_config); // Clone Arc<Config>
    let mut threshold_task_handle = tokio::spawn(async move {
        info!("Starting dynamic profit threshold update task (interval: {}s).", config_clone_for_threshold.dynamic_threshold_update_interval_secs.unwrap_or(60));
        let mut interval_timer = interval(Duration::from_secs(config_clone_for_threshold.dynamic_threshold_update_interval_secs.unwrap_or(60)));
        // VolatilityTracker should be part of the engine or managed state if its history is important across ticks
        // For simplicity here, it's local to the task.
        let mut vol_tracker = VolatilityTracker::new(config_clone_for_threshold.volatility_tracker_window.unwrap_or(20));

        loop {
            interval_timer.tick().await;
            // In a real scenario, fetch a relevant price for volatility calculation
            let price_for_volatility = 1.0; // Placeholder
            vol_tracker.add_price(price_for_volatility);
            let current_volatility = vol_tracker.volatility();
            let base_threshold = config_clone_for_threshold.min_profit_pct;
            let factor = config_clone_for_threshold.volatility_threshold_factor.unwrap_or(0.5);
            let new_threshold = crate::arbitrage::dynamic_threshold::recommend_min_profit_threshold(current_volatility, base_threshold, factor);
            engine_clone_for_threshold.set_min_profit_threshold(new_threshold).await;
            debug!("Dynamic profit threshold updated: {:.4}% (vol: {:.6})", new_threshold * 100.0, current_volatility);
        }
    });

    let executor_clone_for_congestion = Arc::clone(&tx_executor);
    let congestion_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let mut congestion_task_handle = tokio::spawn(async move {
        info!("Starting network congestion update task (interval: {}s).", congestion_interval_secs);
        let mut interval_timer = interval(Duration::from_secs(congestion_interval_secs));
        loop {
            interval_timer.tick().await;
            if let Err(e) = executor_clone_for_congestion.update_network_congestion().await {
                error!("Failed to update network congestion: {}", e);
            }
        }
    });
    
    let engine_clone_for_health = Arc::clone(&arbitrage_engine);
    let health_check_interval_secs = app_config.health_check_interval_secs.unwrap_or(60);
    let mut health_check_task_handle = tokio::spawn(async move {
         info!("Starting health check task (interval: {}s).", health_check_interval_secs);
        let mut interval_timer = interval(Duration::from_secs(health_check_interval_secs));
        loop {
            interval_timer.tick().await;
            engine_clone_for_health.run_health_checks().await; // Call the method
        }
    });


    info!("Starting main arbitrage detection cycle (interval: {}s).", app_config.cycle_interval_seconds);
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
            ws_update_result = async {
                if let Some(manager_arc) = &ws_manager_instance {
                    let mut manager_guard = manager_arc.lock().await;
                    // Assuming try_recv_update is the method to get parsed WebsocketUpdate
                    manager_guard.try_recv_update().await // This needs to be implemented in SolanaWebsocketManager
                } else {
                    std::future::pending().await // Keep this branch pending if no ws_manager
                }
            }, if ws_manager_instance.is_some() => {
                match ws_update_result {
                    Ok(Some(WebsocketUpdate::PoolUpdate(updated_pool_info))) => {
                        let mut pools_w = pools_map.write().await;
                        let pool_address = updated_pool_info.address;
                        pools_w.insert(pool_address, Arc::new(updated_pool_info));
                        debug!("Processed pool update from WebSocket for pool {}", pool_address);
                        metrics.lock().await.log_pools_updated(0, 1, pools_w.len()); // 0 new, 1 updated
                    },
                    Ok(Some(WebsocketUpdate::GenericUpdate(msg))) => {
                        info!("Generic WebSocket message: {}", msg);
                    }
                    Ok(None) => { /* No message currently available from ws_manager's processed queue */ }
                    Err(e) => { // This error comes from try_recv_update if channel closed etc.
                        error!("WebSocket receive error from processed queue: {:?}. Check SolanaWebsocketManager.", e);
                        // Reconnect logic might be handled within SolanaWebsocketManager's own tasks
                    }
                }
            },
            _ = main_cycle_interval.tick() => {
                let detection_start_time = Instant::now();
                info!("Main cycle tick: Searching for arbitrage opportunities...");
                metrics.lock().await.increment_main_cycles();

                // Using discover_multihop_opportunities as it's more general
                let opportunities = match arbitrage_engine.discover_multihop_opportunities().await {
                    Ok(ops) => ops,
                    Err(e) => {
                        error!("Error during arbitrage detection: {}", e);
                        Vec::new()
                    }
                };
                let detection_duration = detection_start_time.elapsed();
                info!("Found {} opportunities in {:?}", opportunities.len(), detection_duration);

                if !opportunities.is_empty() {
                    let execution_chunk_size = app_config.execution_chunk_size.unwrap_or(1) as usize;
                    for opp_chunk in opportunities.chunks(execution_chunk_size) {
                        let mut execution_futures = Vec::new();
                        for opp in opp_chunk {
                            let opp_clone = opp.clone(); // Clone opp for async task
                            let tx_executor_clone = Arc::clone(&tx_executor);
                            let metrics_clone = Arc::clone(&metrics);
                            let base_tx_fee_sol = app_config.default_priority_fee_lamports as f64 / 1_000_000_000.0;


                            // Log attempt before spawning task
                            let dex_path_strings: Vec<String> = opp_clone.dex_path.iter().map(|d| format!("{:?}", d)).collect();
                            metrics_clone.lock().await.record_trade_attempt(
                                &opp_clone.id,
                                &dex_path_strings,
                                &opp_clone.input_token,
                                &opp_clone.output_token,
                                opp_clone.input_amount,
                                opp_clone.expected_output,
                                opp_clone.profit_pct,
                                opp_clone.estimated_profit_usd,
                                TradeOutcome::Attempted, // Outcome
                                None, // actual_profit_usd
                                0, // execution_time_ms
                                0.0, // tx_cost_sol
                                None, // fees_paid_usd
                                None, // error_message
                                None, // transaction_id
                            );

                            execution_futures.push(tokio::spawn(async move {
                                let exec_start_time = Instant::now();
                                let result = tx_executor_clone.execute_opportunity(&opp_clone).await; // Use execute_opportunity
                                let exec_duration_ms = exec_start_time.elapsed().as_millis();

                                match result {
                                    Ok(signature) => {
                                        info!("Successfully executed opportunity {}: Signature: {}, Duration: {}ms", opp_clone.id, signature, exec_duration_ms);
                                        // Assuming actual profit is same as estimated for now
                                        let actual_profit = opp_clone.estimated_profit_usd;
                                        metrics_clone.lock().await.record_trade_attempt( // Using record_trade_attempt to log outcome
                                            &opp_clone.id, &dex_path_strings, &opp_clone.input_token, &opp_clone.output_token,
                                            opp_clone.input_amount, opp_clone.expected_output, opp_clone.profit_pct,
                                            opp_clone.estimated_profit_usd,
                                            TradeOutcome::Success,
                                            actual_profit,
                                            exec_duration_ms,
                                            base_tx_fee_sol, // tx_cost_sol placeholder
                                            Some(base_tx_fee_sol * app_config.sol_price_usd.unwrap_or(100.0)), // fees_paid_usd placeholder
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
                                            None,
                                            exec_duration_ms,
                                            base_tx_fee_sol,
                                            Some(base_tx_fee_sol * app_config.sol_price_usd.unwrap_or(100.0)),
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
             // Handling task completion/panics
            _ = async { threshold_task_handle.await.unwrap_or_else(|e| error!("Dynamic threshold task panicked: {:?}", e)) }, if !threshold_task_handle.is_finished() => {
                info!("Dynamic threshold update task completed or panicked.");
                // Optionally re-spawn if needed, or handle error
            },
            _ = async { congestion_task_handle.await.unwrap_or_else(|e| error!("Congestion update task panicked: {:?}", e)) }, if !congestion_task_handle.is_finished() => {
                info!("Congestion update task completed or panicked.");
            },
            _ = async { health_check_task_handle.await.unwrap_or_else(|e| error!("Health check task panicked: {:?}", e)) }, if !health_check_task_handle.is_finished() => {
                info!("Health check task completed or panicked.");
            }
        }
    }

    info!("Shutting down. Final metrics summary:");
    metrics.lock().await.summary();

    Ok(())
}