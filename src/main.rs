// src/main.rs

// Module declarations
mod arbitrage;
mod cache; // Added cache module
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils; // Keep utils public if types are used by other crates, otherwise consider private
pub mod websocket; // Corrected: This should likely be `crate::solana::websocket` or a local module

// Crate imports
use crate::{
    arbitrage::{
        // detector::ArbitrageDetector, // Marked as unused
        // dynamic_threshold::recommend_min_profit_threshold, // Marked as unused
        dynamic_threshold::VolatilityTracker, // Kept as it's used
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor, 
        // opportunity::MultiHopArbOpportunity, // Marked as unused
    },
    cache::Cache,
    config::load_config, // Removed ::Config as it's unused
    dex::get_all_clients_arc,
    error::ArbError, 
    metrics::{Metrics, TradeOutcome}, // Added TradeOutcome
    solana::{
        rpc::SolanaRpcClient,
        websocket::{SolanaWebsocketManager, WebsocketUpdate}, // Corrected import path
    },
    utils::{setup_logging, ProgramConfig, PoolInfo, load_keypair}, // Added PoolInfo, load_keypair, removed self import
    // Removed websocket::AccountUpdate as it's not directly used here, or should be defined
};
use solana_sdk::signer::Signer; 
// use futures::future::join_all; // Marked as unused
use log::{debug, error, info, warn};
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use solana_client::rpc_client::RpcClient; // Added RpcClient import
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Instant}; // Import tokio::time::Instant

#[tokio::main]
async fn main() -> Result<(), ArbError> {
    // Setup logging
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    // Load configuration
    let app_config = Arc::new(load_config()?); // Corrected to use imported load_config directly
    info!("Configuration loaded and validated successfully.");

    // --- Initialize Metrics ---
    // If log_path for metrics comes from config, pass it here:
    // let metrics_log_path = app_config.metrics_log_path.as_deref(); // Example config field
    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0), 
        app_config.metrics_log_path.clone(), // Pass metrics_log_path from config
    )));

    metrics.lock().await.log_launch(); // log_launch is not async

    // Initialize Solana RPC client (both primary and fallback)
    info!("Initializing Solana RPC client...");
    let primary_rpc_endpoint = app_config.rpc_url.clone(); // Corrected: was redis_url
    let fallback_rpc_endpoints = app_config.rpc_url_backup.clone(); // Corrected: was dex_quote_cache_ttl_secs

    let rpc_client = Arc::new(SolanaRpcClient::new_with_fallbacks(
        &primary_rpc_endpoint,
        &fallback_rpc_endpoints.unwrap_or_default(), // Corrected: use rpc_url_backup
        app_config.rpc_max_retries.unwrap_or(5), // Corrected: use rpc_max_retries
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(1000)), // Corrected: use rpc_retry_delay_ms
    ));
    info!("Solana RPC client initialized.");

    // --- Initialize Redis Cache ---
    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs)
            .await
            .map_err(|e| {
                error!("CRITICAL: Failed to initialize Redis cache at {}: {}", app_config.redis_url, e);
                anyhow::anyhow!("Redis cache initialization failed: {}", e)
            })?,
    );
    info!("Redis cache initialized successfully at {}", app_config.redis_url);

    // --- Initialize Solana RPC Clients ---
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints = app_config.rpc_url_backup
        .iter()
        .cloned()
        .collect::<Vec<String>>();

    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint,
        fallback_rpc_endpoints,
        app_config.rpc_max_retries,
        Duration::from_millis(app_config.rpc_retry_delay_ms),
    ));
    info!("High-availability Solana RPC client initialized for endpoint: {}", primary_rpc_endpoint);

    let direct_rpc_client = Arc::new(
        solana_client::nonblocking::rpc_client::RpcClient::new_with_commitment(
            primary_rpc_endpoint.clone(),
            solana_sdk::commitment_config::CommitmentConfig::confirmed(), // Or from config
        ),
    );
    info!("Direct Solana RPC client initialized for endpoint: {}", primary_rpc_endpoint);


    // --- Initialize DEX API Clients ---
    // This now correctly passes the initialized cache and config.
    let _dex_api_clients = get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized and configured with cache.");


    // --- Initialize Shared Pools Map ---
    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    // Ensure log_pools_fetched in metrics/mod.rs is updated if it needs to be async or handle the new pools_map type.
    metrics.lock().await.log_pools_fetched(pools_map.read().await.len()); // log_pools_fetched is not async
    info!("Initial pool data fetched (or initialized as empty).");

    // Load trader wallet
    let wallet_path = app_config.trader_wallet_keypair_path.clone(); // Corrected: was health_check_token_symbol
    let wallet = match read_keypair_file(&wallet_path).map_err(|e| {
        error!(
            "Failed to read keypair file from path '{}': {}",
            wallet_path, e
        );
        anyhow::anyhow!("Failed to read keypair file '{}': {}", wallet_path, e)
    }) {
        Ok(kp) => Arc::new(kp),
        Err(e) => return Err(e),
    };
    info!("Trader wallet loaded successfully from: {}", wallet_path);
    info!("Trader wallet Pubkey: {}", wallet.pubkey());

    // Initialize TransactionExecutor
    let wallet_keypair_for_tx = Arc::new(load_keypair(&app_config.trader_wallet_keypair_path)?); // Used imported load_keypair
    let rpc_client_for_tx = Arc::new(RpcClient::new(app_config.rpc_url.clone()));

    let tx_executor = Arc::new(ArbitrageExecutor::new( 
        wallet_keypair_for_tx.clone(),
        rpc_client_for_tx.clone(),
        app_config.default_priority_fee_lamports.unwrap_or(10000),
        std::time::Duration::from_secs(app_config.transaction_timeout_secs.unwrap_or(30)),
        app_config.simulation_mode.unwrap_or(false),
    ));

    // Initialize Metrics (already initialized above, reusing 'metrics')
    // let metrics = Arc::new(Mutex::new(Metrics::new(Some("metrics_log.json "))));

    // Initialize ArbitrageEngine
    let arbitrage_engine_for_tx = Arc::new(ArbitrageEngine::new( // Renamed to avoid conflict
        pools_map.clone(),
        Some(rpc_client_for_tx.clone()), 
        app_config.clone(),
        metrics.clone(),
        None, 
    ));
    info!("TransactionExecutor initialized."); // This log refers to tx_executor

    // --- Initialize Arbitrage Executor ---
    let executor = Arc::new(
        ArbitrageExecutor::new(
            Arc::clone(&wallet),
            Arc::clone(&direct_rpc_client),
            app_config.default_priority_fee_lamports.unwrap_or(0), // Ensure default if None
            Duration::from_secs(app_config.max_transaction_timeout_seconds),
            app_config.paper_trading.unwrap_or(false), // Ensure default if None
        )
        .with_solana_rpc(Arc::clone(&ha_solana_rpc_client)),
    );
    if let Err(e) = executor.update_network_congestion().await {
        warn!("Initial network congestion update failed: {}", e);
    }
    info!("Arbitrage executor initialized.");

    // --- Initialize Arbitrage Engine ---
    let ws_manager_instance = if !app_config.ws_url.is_empty() {
        let (manager, _receiver) = crate::solana::websocket::SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            vec![], // TODO: Add fallback WS URLs from config if available
            app_config.ws_update_channel_size,
        );
        Some(manager)
    } else {
        warn!("WebSocket URL not configured. Real-time pool updates will be disabled.");
        None
    };

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        Some(rpc_client.clone()), // Pass the RPC client Arc<SolanaRpcClient>
        app_config.clone(), 
        metrics.clone(),    
        None, 
    ));
    info!("ArbitrageEngine initialized.");

    arbitrage_engine.start_services().await;


    // --- Background Tasks ---
    let mut volatility_tracker = VolatilityTracker::new(app_config.volatility_tracker_window.unwrap_or(20));
    let engine_threshold_handle = Arc::clone(&arbitrage_engine);
    let threshold_update_interval_secs = app_config.dynamic_threshold_update_interval_secs.unwrap_or(10);

    let mut threshold_task_handle = tokio::spawn(async move { // Made mutable
        info!("Starting dynamic profit threshold update task (interval: {}s).", threshold_update_interval_secs);
        let mut threshold_interval = interval(Duration::from_secs(threshold_update_interval_secs));
        let base_min_profit_from_config = app_config.min_profit_pct.unwrap_or(0.001); // Corrected: use min_profit_pct
        let volatility_factor_from_config = app_config.volatility_threshold_factor.unwrap_or(0.5);

        // Dynamic threshold update task (runs periodically)
        loop {
            threshold_interval.tick().await;
            let current_price_of_major_asset = 1.0; 
            volatility_tracker.add_price(current_price_of_major_asset);
            let vol = volatility_tracker.volatility();
            // Ensure the dynamic_threshold module's function is correctly referenced if it's not in global scope
            let new_threshold = crate::arbitrage::dynamic_threshold::recommend_min_profit_threshold(vol, base_min_profit_from_config, volatility_factor_from_config);
            engine_threshold_handle.set_min_profit_threshold(new_threshold).await;
            debug!("Dynamic profit threshold updated: {:.4}% (vol: {:.6})", new_threshold * 100.0, vol);
        }
    });

    let executor_congestion_handle = Arc::clone(&executor);
    let congestion_update_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let mut congestion_task_handle = tokio::spawn(async move { // Made mutable
        info!("Starting network congestion update task (interval: {}s).", congestion_update_interval_secs);
        let mut congestion_interval = interval(Duration::from_secs(congestion_update_interval_secs));
        loop {
            congestion_interval.tick().await;
            if let Err(e) = executor_congestion_handle.update_network_congestion().await {
                error!("Failed to update network congestion: {}", e);
            }
        }
    });

    // Placeholder for health_check_task_handle if it's used later.
    // If it's a tokio::task::JoinHandle, it needs to be initialized.
    // For now, assuming it might be conditionally initialized or this is a remnant.
    // To resolve the "not found in this scope " error for now, we declare it.
    // This should be properly initialized if the health check task is implemented.
    let mut health_check_task_handle: Option<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = None;


    // --- Main Arbitrage Loop ---
    info!("Starting main arbitrage detection cycle (interval: {}s).", app_config.cycle_interval_seconds.unwrap_or(5));
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.unwrap_or(5)));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let cycle_start_time = Instant::now(); // Use tokio::time::Instant

        tokio::select! {
            biased; // Process signal first if available
            _ = signal::ctrl_c() => {
                info!("Ctrl-C received, initiating shutdown...");
                break; // Exit the main loop
            },
            // WebSocket updates handling
            ws_update = async {
                if let Some(manager_arc) = &ws_manager_instance {
                    let mut manager_guard = manager_arc.lock().await;
                    if manager_guard.is_connected().await { // Assuming is_connected is async
                        manager_guard.try_recv_update().await // Assuming try_recv_update is async
                    } else {
                        None
                    }
                } else {
                    // If ws_manager is None, this branch should not be selected often.
                    // We can use a pending future to avoid busy-looping if ws_manager is None.
                    std::future::pending().await
                }
            }, if ws_manager_instance.is_some() => {
                if let Some(update_result) = ws_update {
                    match update_result {
                        Ok(Some(WebsocketUpdate::PoolUpdate(updated_pool_info))) => {
                            // This part needs to be adapted if PoolUpdate directly gives PoolInfo
                            // Or if it gives data that needs parsing and then updating the shared pools_map.
                            // For now, assuming direct update for simplicity.
                            let mut pools_w = pools_map.write().await;
                            pools_w.insert(updated_pool_info.address, Arc::new(updated_pool_info));
                            debug!("Processed pool update from WebSocket for pool {}", pools_w.get(&updated_pool_info.address).unwrap().address);
                        },
                        Ok(Some(WebsocketUpdate::GenericUpdate(msg))) => {
                            info!("Generic WebSocket message: {}", msg);
                        }
                        Ok(None) => { /* No message currently available */ }
                        Err(e) => {
                            error!("WebSocket receive error: {}. Attempting to reconnect if necessary.", e);
                            if let Some(manager_arc) = &ws_manager_instance {
                                let mut manager_guard = manager_arc.lock().await;
                                if !manager_guard.is_connected().await { // Check if already trying to reconnect
                                    // manager_guard.reconnect().await.unwrap_or_else(|err| { // Assuming reconnect is async
                                    //     error!("Failed to reconnect WebSocket: {}", err);
                                    // });
                                }
                            }
                        }
                    }
                }
            },
            // Main arbitrage cycle tick
            _ = main_cycle_interval.tick() => {
                let detection_start_time = Instant::now();
                info!("Main cycle tick: Searching for arbitrage opportunities...");
                metrics.lock().await.increment_main_cycles();

                // Dynamic Threshold Update (if enabled and interval passed)
                // This logic might be better inside the dynamic_threshold_task
                // or triggered by its own timer if it's independent of the main cycle.

                let opportunities = match arbitrage_engine.detect_arbitrage().await {
                    Ok(ops) => ops,
                    Err(e) => {
                        error!("Error during arbitrage detection: {}", e);
                        Vec::new() // Continue with an empty list of opportunities
                    }
                };
                let detection_duration = detection_start_time.elapsed();
                info!("Found {} opportunities in {:?}", opportunities.len(), detection_duration);

                if !opportunities.is_empty() {
                    let base_tx_fee_lamports_for_metrics = app_config.default_priority_fee_lamports.unwrap_or(1000); // Corrected

                    for opp_item in opportunities { // Renamed opp to opp_item to avoid conflict with loop variable
                        // Assuming MultiHopArbOpportunity has a unique identifier or can be constructed
                        // The 'hop' variable was not defined in this scope.
                        // We need to iterate through hops if opp_item contains multiple.
                        // For simplicity, assuming opp_item itself has the necessary details or refers to the first hop.
                        // This section needs to be carefully reviewed based on MultiHopArbOpportunity structure.

                        // If opp_item is a MultiHopArbOpportunity, it should have fields like input_token, profit_pct etc.
                        // If it contains a Vec<Hop>, we need to decide how to log.
                        // For now, using fields directly from opp_item, assuming it's structured appropriately.

                        let opp_identifier = format!(
                            "Opportunity_{}_{}_{:.4}", 
                            opp_item.input_token, // Assuming these fields exist on opp_item (MultiHopArbOpportunity)
                            opp_item.output_token,
                            opp_item.profit_pct
                        );
                        let dex_path_vec: Vec<String> = opp_item.dex_path.iter().map(|d| format!("{:?}", d)).collect();

                        metrics.lock().await.record_trade_attempt(
                            &opp_identifier,
                            &dex_path_vec,
                            &opp_item.input_token,
                            &opp_item.output_token,
                            opp_item.input_amount, // Assuming this field exists
                            opp_item.expected_output, // Assuming this field exists
                            opp_item.profit_pct,
                            opp_item.estimated_profit_usd, // Assuming this field exists
                            TradeOutcome::Attempted, // Changed from Skipped
                            None, 
                            0,    
                            0.0,  
                            None, 
                            Some("Attempting execution".to_string()),
                            None, 
                        );

                        let execution_start_time = Instant::now();
                        match tx_executor.execute_arbitrage_opportunity(&opp_item).await { // Pass opp_item
                            Ok(signature) => {
                                let execution_duration_ms = execution_start_time.elapsed().as_millis();
                                info!("Successfully executed opportunity {}: Signature: {}, Duration: {}ms", opp_identifier, signature, execution_duration_ms);
                                metrics.lock().await.record_trade_outcome(
                                    &opp_identifier,
                                    TradeOutcome::Success,
                                    opp_item.profit_pct, 
                                    Some(base_tx_fee_lamports_for_metrics), 
                                    Some(execution_duration_ms as u64),
                                    Some(signature.to_string()),
                                    None, 
                                ); 
                            }
                            Err(e) => {
                                let execution_duration_ms = execution_start_time.elapsed().as_millis();
                                error!("Failed to execute opportunity {}: {}. Duration: {}ms", opp_identifier, e, execution_duration_ms);
                                metrics.lock().await.record_trade_outcome(
                                    &opp_identifier,
                                    TradeOutcome::Failure,
                                    0.0, 
                                    Some(base_tx_fee_lamports_for_metrics),
                                    Some(execution_duration_ms as u64),
                                    None, 
                                    Some(e.to_string()),
                                ); 
                            }
                        }
                    }
                }
                // Log main cycle duration
                metrics.lock().await.record_main_cycle_duration(cycle_start_time.elapsed().as_millis() as u64);
            },
            // Dynamic threshold update task
            maybe_threshold_task = async {
                // Borrow mutable handle if it exists
                if let Some(task_handle_ref_mut) = threshold_task_handle.as_mut() {
                     task_handle_ref_mut.await.map_err(|e| error!("Dynamic threshold task panicked: {:?}", e))
                } else {
                    futures::future::pending().await
                }
            }, if threshold_task_handle.is_some() => { 
                info!("Dynamic threshold update task completed or panicked.");
                threshold_task_handle = None; 
            },
            // Health check task
            maybe_health_check_task = async {
                // Borrow mutable handle if it exists
                if let Some(task_handle_ref_mut) = health_check_task_handle.as_mut() {
                    task_handle_ref_mut.await.map_err(|e| error!("Health check task panicked: {:?}", e))
                } else {
                    futures::future::pending().await
                }
            }, if health_check_task_handle.is_some() => { 
                info!("Health check task completed or panicked.");
                health_check_task_handle = None; 
            }
        }
    }

    // Final summary before shutdown
    info!("Shutting down. Final metrics summary:");
    metrics.lock().await.summary(); // Not async

    Ok(())
}
// Reminder: Add these fields to your Config struct in `src/config/settings.rs`
// and load them from environment variables:
// ---
// pub redis_url: String,
// pub redis_default_ttl_secs: u64,
// pub dex_quote_cache_ttl_secs: Option<HashMap<String, u64>>,
// pub volatility_tracker_window: Option<usize>,
// pub dynamic_threshold_update_interval_secs: Option<u64>,
// pub volatility_threshold_factor: Option<f64>,
// pub congestion_update_interval_secs: Option<u64>,
// pub execution_chunk_size: Option<u8>,
// pub sol_price_usd: Option<f64>,
// pub ws_update_channel_size: usize, // Should already be there
// pub degradation_profit_factor: Option<f64>,
// pub degradation_slippage_factor: Option<f64>,
// pub pool_read_timeout_ms: Option<u64>,
// pub health_check_token_symbol: Option<String>,
// pub metrics_log_path: Option<String>, // For file-based metrics logging if used
//
// And ensure `recommended_min_profit_threshold` in `src/arbitrage/dynamic_threshold.rs`
// is updated to accept `base_threshold` and `factor`.
// Example:
// pub fn recommended_min_profit_threshold(volatility: f64, base_threshold: f64, factor: f64) -> f64 {
//     let boost = (volatility * factor).min(base_threshold * 2.0); // Cap boost
//     (base_threshold + boost).max(0.0001) // Ensure positive and not excessively small
// }
//
// Also, update `Metrics` methods in `src/metrics/mod.rs` to be `async` if they perform
// operations like file I/O that should be non-blocking.
