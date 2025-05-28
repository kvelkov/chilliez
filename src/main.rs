pub mod arbitrage;
mod cache;
mod config;
mod dex;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;
pub mod error;

use crate::{
    arbitrage::{
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor,
        pipeline::ExecutionPipeline,
    },
    cache::Cache,
    config::settings::Config,
    dex::get_all_clients_arc,
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::{RawAccountUpdate, SolanaWebsocketManager},
    },
    utils::{setup_logging, PoolInfo, ProgramConfig}, 
};
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
    let config = Config::from_env();
    config.validate_and_log();
    Arc::new(config)
}

#[tokio::main]
async fn main() -> MainResult<()> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

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
        &primary_rpc_endpoint, fallback_rpc_endpoints,
        app_config.rpc_max_retries.unwrap_or(3) as usize,
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)),
    ));
    info!("High-availability Solana RPC client initialized.");

    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs).await
            .map_err(|e| ArbError::ConfigError(format!("Redis cache init failed: {}", e)))?,
    );
    info!("Redis cache initialized successfully.");

    let dex_api_clients = get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized.");

    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::new(RwLock::new(HashMap::new()));
    metrics.lock().await.log_pools_fetched(pools_map.read().await.len());
    info!("Initial pool data map initialized (current size: {}).", pools_map.read().await.len());

    let wallet_path = app_config.trader_wallet_keypair_path.clone().unwrap_or(app_config.wallet_path.clone());
    let wallet = match read_keypair_file(&wallet_path) {
        Ok(kp) => Arc::new(kp),
        Err(e) => return Err(ArbError::ConfigError(format!("Failed to read wallet keypair '{}': {}", wallet_path, e))),
    };
    info!("Trader wallet loaded: {}", wallet.pubkey());

    let ws_setup_opt: Option<(Arc<Mutex<SolanaWebsocketManager>>, broadcast::Receiver<RawAccountUpdate>)> = if !app_config.ws_url.is_empty() {
        let (manager, raw_updates_rx_for_main) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            app_config.rpc_url_backup
                .as_ref()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_else(Vec::new),
            app_config.ws_update_channel_size.unwrap_or(1024),
        );
        let manager_arc = Arc::new(Mutex::new(manager));
        {
            let mut mgr = manager_arc.lock().await;
            let _ = mgr.start(Some(redis_cache.clone())).await;
        }
        Some((manager_arc, raw_updates_rx_for_main))
    } else {
        warn!("WebSocket URL not configured, WebSocket manager not started.");
        None
    };

    let arbitrage_engine: Arc<ArbitrageEngine> = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        ws_setup_opt.as_ref().map(|(manager_arc, _rx)| manager_arc.clone()),
        None,
        Some(ha_solana_rpc_client.clone()),
        app_config.clone(),
        metrics.clone(),
        dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");

    let simulation_mode_from_env = env::var("SIMULATION_MODE")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    let mut pipeline = ExecutionPipeline::new();
    let event_sender_for_executor = pipeline.get_sender();

    tokio::spawn(async move {
        info!("[Main] Starting ExecutionPipeline listener task.");
        pipeline.start_listener().await;
        info!("[Main] ExecutionPipeline listener task finished.");
    });

    let tx_executor: Arc<ArbitrageExecutor> = Arc::new(ArbitrageExecutor::new(
        wallet.clone(),
        ha_solana_rpc_client.primary_client.clone(), 
        Some(event_sender_for_executor),
        metrics.clone(),
    ));
    info!("ArbitrageExecutor initialized.");

    // Start the main loop of the arbitrage engine.
    // The run_main_loop method handles spawning its own tasks.
    tokio::spawn(async move { arbitrage_engine.run_main_loop().await });

    info!("Starting main arbitrage detection cycle...");
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.unwrap_or(30)));

    loop {
        main_cycle_interval.tick().await;
        info!("Main arbitrage cycle tick.");
        metrics.lock().await.increment_main_cycles();

        // To call detect_arbitrage, we need the engine instance, not the Arc itself
        // if we were calling it directly here. However, run_main_loop already does this internally.
        // The loop below is likely redundant if run_main_loop is active.
        // For now, I'll comment out this direct call as run_main_loop should handle detection.
        /* match arbitrage_engine.detect_arbitrage().await {
            Ok(opportunities) => {
                if !opportunities.is_empty() {
                    info!("Detected {} arbitrage opportunities.", opportunities.len());
                }
            }
            Err(e) => {
                error!("Error during arbitrage detection: {}", e);
            }
        } */
    }
    // If the main function should wait for the engine to finish (which it won't in this setup as it's a loop)
    // you might need to join the spawned task or use a different mechanism to keep main alive.
    // For a continuously running bot, this loop might be replaced by just waiting on the spawned engine task.
}
