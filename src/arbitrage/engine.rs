// src/arbitrage/engine.rs
use crate::{
    arbitrage::detector::ArbitrageDetector,
    arbitrage::opportunity::MultiHopArbOpportunity,
    config::settings::Config,
    dex::quote::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::PoolInfo,
    websocket::CryptoDataProvider,
};
use log::{error, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout};
use tokio::time::error::Elapsed;

pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: Arc<RwLock<f64>>,
    max_slippage: f64,
    tx_fee_lamports: u64,
    pub(crate) ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub(crate) _price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>,
    pub(crate) rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>,
    pub(crate) _degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    pub(crate) health_check_interval: Duration,
    pub(crate) ws_reconnect_attempts: Arc<AtomicU64>,
    pub(crate) max_ws_reconnect_attempts: u64,
    pub(crate) detector: Arc<Mutex<ArbitrageDetector>>,
    pub(crate) _dex_providers: Vec<Arc<dyn DexClient>>,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        config: Arc<Config>,
        metrics: Arc<Mutex<Metrics>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
        ws_manager_instance: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        dex_api_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        let initial_min_profit_pct = config.min_profit_pct * 100.0;
        let max_slippage_fraction = config.max_slippage_pct;
        let tx_fee_lamports_val = config.default_priority_fee_lamports;
        let health_check_interval_secs = config.health_check_interval_secs.unwrap_or(60);
        let max_ws_reconnect_val = config.max_ws_reconnect_attempts.map_or(5, |v| v as u64);
        let detector = Arc::new(Mutex::new(ArbitrageDetector::new(initial_min_profit_pct)));

        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(initial_min_profit_pct)),
            max_slippage: max_slippage_fraction,
            tx_fee_lamports: tx_fee_lamports_val,
            ws_manager: ws_manager_instance,
            _price_provider: price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            _degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(
                Instant::now() - Duration::from_secs(health_check_interval_secs * 2),
            )),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,
            _dex_providers: dex_api_clients,
        }
    }

    pub async fn start_services(&self) {
        info!("ArbitrageEngine services starting...");
        if let Some(ws_manager_arc) = &self.ws_manager {
            let ws_manager = ws_manager_arc.lock().await;
            if let Err(e) = ws_manager.start().await {
                error!("Failed to start WebSocket Manager: {}", e);
            } else {
                info!("WebSocket Manager started successfully.");
            }
        } else {
            warn!("WebSocket Manager not available, not starting WS services.");
        }
    }
    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn set_min_profit_threshold(&self, threshold_pct: f64) {
        let mut profit_threshold_guard = self.min_profit_threshold.write().await;
        *profit_threshold_guard = threshold_pct;
        info!(
            "ArbitrageEngine min_profit_threshold updated to: {:.4}%",
            threshold_pct
        );
    }

    pub fn _should_execute_trade(
        &self,
        opportunity: &MultiHopArbOpportunity,
        calculated_slippage_fraction: f64,
        estimated_fee_lamports: u64,
    ) -> bool {
        if opportunity.profit_pct < *self.min_profit_threshold.blocking_read() {
            return false;
        }
        if calculated_slippage_fraction > self.max_slippage {
            warn!(
                "Opportunity {} exceeds max slippage: {:.4}% > {:.4}%",
                opportunity.id,
                calculated_slippage_fraction * 100.0,
                self.max_slippage * 100.0
            );
            return false;
        }
        let profit_in_lamports_equivalent = (opportunity.total_profit * 1_000_000_000.0) as u64;
        if profit_in_lamports_equivalent < estimated_fee_lamports {
            warn!(
                "Opportunity {} profit ({}) less than est. fee ({})",
                opportunity.id, profit_in_lamports_equivalent, estimated_fee_lamports
            );
            return false;
        }
        true
    }

    async fn discover_opportunities_internal<F, Fut>(
        &self,
        operation_name: &str,
        detector_call: F,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError>
    where
        F: FnOnce(
            Arc<Mutex<ArbitrageDetector>>,
            HashMap<Pubkey, Arc<PoolInfo>>,
            Arc<Mutex<Metrics>>,
        ) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<MultiHopArbOpportunity>, ArbError>>,
    {
        self.maybe_check_health().await?;

        let guard = timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        .map_err(|_elapsed: Elapsed| {
            warn!("{}: Timeout waiting for pools read lock", operation_name);
            ArbError::TimeoutError(format!("Timeout for pools read lock in {}", operation_name))
        })?;

        let pools_map = guard.deref().clone(); // Clone the HashMap so the closure owns it
        detector_call(
            Arc::clone(&self.detector),
            pools_map,
            Arc::clone(&self.metrics),
        )
        .await
    }

    pub async fn _discover_direct_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal(
            "discover_direct_opportunities",
            |detector, pools_map, metrics_arc| async move {
                let mut metrics_guard = metrics_arc.lock().await;
                detector
                    .lock()
                    .await
                    .find_all_opportunities(&pools_map, &mut *metrics_guard)
                    .await
            },
        )
        .await
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_opportunities_internal(
            "discover_multihop_opportunities",
            |detector, pools_map, metrics_arc| async move {
                let mut metrics_guard = metrics_arc.lock().await;
                detector
                    .lock()
                    .await
                    .find_all_multihop_opportunities(&pools_map, &mut *metrics_guard)
                    .await
            },
        )
        .await
    }

    pub async fn _discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let max_slippage = self.max_slippage;
        let tx_fee_lamports = self.tx_fee_lamports;
        self.discover_opportunities_internal(
            "discover_multihop_opportunities_with_risk",
            move |detector, pools_map, metrics_arc| async move {
                let mut metrics_guard = metrics_arc.lock().await;
                detector
                    .lock()
                    .await
                    .find_all_multihop_opportunities_with_risk(
                        &pools_map,
                        &mut *metrics_guard,
                        max_slippage,
                        tx_fee_lamports,
                    )
                    .await
            },
        )
        .await
    }

    pub async fn _resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        self.with_pool_guard_async(
            "_resolve_pools_for_opportunity",
            false,
            |pools_arc| async move {
                let pools_guard = pools_arc.read().await;
                let mut resolved_pools = Vec::new();
                for hop_pool_address in &opportunity.pool_path {
                    match pools_guard.get(hop_pool_address) {
                        Some(pool_info) => resolved_pools.push(Arc::clone(pool_info)),
                        None => {
                            let hop_details = opportunity
                                .hops
                                .iter()
                                .find(|h| &h.pool == hop_pool_address);
                            let (input_token, output_token) = hop_details
                                .map_or(("N/A".to_string(), "N/A".to_string()), |h| {
                                    (h.input_token.clone(), h.output_token.clone())
                                });
                            warn!(
                                "Pool {} for hop {}->{} not found in local cache.",
                                hop_pool_address, input_token, output_token
                            );
                            return Err(ArbError::PoolNotFound(hop_pool_address.to_string()));
                        }
                    }
                }
                Ok(resolved_pools)
            },
        )
        .await
    }

    async fn with_pool_guard_async<Fut, T>(
        &self,
        operation_name: &str,
        critical: bool,
        closure: impl FnOnce(Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>) -> Fut,
    ) -> Result<T, ArbError>
    where
        Fut: std::future::Future<Output = Result<T, ArbError>>,
    {
        match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            async { Ok::<_, ArbError>(Arc::clone(&self.pools)) },
        )
        .await
        {
            Ok(Ok(pools_arc)) => closure(pools_arc).await,
            Ok(Err(e)) => {
                error!(
                    "{}: Error from closure within with_pool_guard_async: {}",
                    operation_name, e
                );
                Err(e)
            }
            Err(_timeout_error) => {
                if critical {
                    error!(
                        "{}: Timeout in pool guard handling. This is critical.",
                        operation_name
                    );
                    Err(ArbError::TimeoutError(format!(
                        "Critical timeout for pool guard in {}",
                        operation_name
                    )))
                } else {
                    warn!("{}: Timeout in pool guard handling.", operation_name);
                    Err(ArbError::TimeoutError(format!(
                        "Timeout for pool guard in {}",
                        operation_name
                    )))
                }
            }
        }
    }

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        let mut last_check_guard = self.last_health_check.write().await;
        if last_check_guard.elapsed() > self.health_check_interval {
            info!("Performing scheduled health checks...");
            if let Some(rpc_client) = &self.rpc_client {
                if !rpc_client.is_healthy().await {
                    warn!("RPC client is unhealthy during maybe_check_health.");
                }
            }
            *last_check_guard = Instant::now();
            self.metrics.lock().await.set_system_health(true);
        }
        Ok(())
    }

    pub async fn run_health_checks(&self) {
        info!("Health check task running periodical checks...");
        let mut healthy = true;
        if let Some(rpc) = &self.rpc_client {
            if !rpc.is_healthy().await {
                warn!("RPC client reported as unhealthy.");
                healthy = false;
            }
        }
        if let Some(ws_manager_arc) = &self.ws_manager {
            let manager = ws_manager_arc.lock().await;
            if !manager.is_connected().await {
                warn!("WebSocket manager reported as disconnected.");
                healthy = false;
                let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                if attempts < self.max_ws_reconnect_attempts {
                    warn!(
                        "Attempting to reconnect WebSocket (attempt {}/{})",
                        attempts + 1,
                        self.max_ws_reconnect_attempts
                    );
                } else {
                    error!(
                        "Max WebSocket reconnect attempts ({}) reached.",
                        self.max_ws_reconnect_attempts
                    );
                }
            } else {
                self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
            }
        }
        self.metrics.lock().await.set_system_health(healthy);
        *self.last_health_check.write().await = Instant::now();
    }

    pub async fn update_pools(
        &self,
        new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        let mut pools_guard = self.pools.write().await;
        let mut new_count = 0;
        let mut updated_count = 0;
        for (key, pool_info) in new_pools_data {
            if pools_guard.contains_key(&key) {
                updated_count += 1;
            } else {
                new_count += 1;
            }
            pools_guard.insert(key, pool_info);
        }
        let total_count = pools_guard.len();
        info!(
            "Pools updated: {} new, {} updated. Total pools: {}",
            new_count, updated_count, total_count
        );
        self.metrics
            .lock()
            .await
            .log_pools_updated(new_count, updated_count, total_count);
        Ok(())
    }

    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let opportunities = self.discover_multihop_opportunities().await?;
        Ok(opportunities)
    }

    pub async fn run_dynamic_threshold_updates(&self) {
        info!("Dynamic threshold update service started.");
    }

    pub async fn _get_current_status(&self) -> String {
        format!(
            "ArbitrageEngine Status: Min Profit= {:.4}%, Max Slippage= {:.4}%, Degradation Mode= {}",
            *self.min_profit_threshold.read().await,
            self.max_slippage * 100.0,
            self._degradation_mode.load(Ordering::Relaxed)
        )
    }
}