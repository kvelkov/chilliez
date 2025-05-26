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
use log::{error, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicU64},
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
    ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    metrics: Arc<Mutex<Metrics>>,
    rpc_client: Option<Arc<SolanaRpcClient>>,
    config: Arc<Config>,
    degradation_mode: Arc<AtomicBool>,
    last_health_check: Arc<RwLock<Instant>>,
    pub(crate) health_check_interval: Duration,
    ws_reconnect_attempts: Arc<AtomicU64>,
    max_ws_reconnect_attempts: u64,
    pub(crate) detector: Arc<Mutex<ArbitrageDetector>>,
    dex_providers: Vec<Arc<dyn DexClient>>,
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
            price_provider,
            metrics,
            rpc_client,
            config: Arc::clone(&config),
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(
                Instant::now() - Duration::from_secs(health_check_interval_secs * 2),
            )),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts: max_ws_reconnect_val,
            detector,
            dex_providers: dex_api_clients,
        }
    }

    pub async fn start_services(&self) { /* ... */
    }
    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }
    pub async fn set_min_profit_threshold(&self, _threshold_pct: f64) { /* ... */
    }
    pub fn should_execute_trade(
        &self,
        _calculated_slippage_fraction: f64,
        _estimated_fee_lamports: u64,
    ) -> bool {
        false
    }

    async fn discover_opportunities_internal<F, Fut>(
        &self,
        operation_name: &str,
        detector_call: F,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError>
    where
        F: FnOnce(
            Arc<Mutex<ArbitrageDetector>>,
            &HashMap<Pubkey, Arc<PoolInfo>>,
            &mut Metrics,
        ) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<MultiHopArbOpportunity>, ArbError>>,
    {
        self.maybe_check_health().await?;

        // Step 1: Handle Timeout (Tokio async locks do not return poison errors)
        let guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        {
            Ok(g) => g, // Successfully acquired lock
            Err(_elapsed) => {
                warn!("{}: Timeout waiting for pools read lock", operation_name);
                return Err(ArbError::TimeoutError(format!(
                    "Timeout for pools read lock in {}",
                    operation_name
                )));
            }
        };

        let mut metrics_guard = self.metrics.lock().await;
        detector_call(
            Arc::clone(&self.detector),
            guard.deref(),
            &mut *metrics_guard,
        )
        .await
    }

    pub async fn discover_direct_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        {
            Ok(g) => g,
            Err(_elapsed) => {
                warn!("discover_direct_opportunities: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError(
                    "Timeout for pools read lock in discover_direct_opportunities".to_string(),
                ));
            }
        };
        let mut metrics_guard = self.metrics.lock().await;
        self.detector
            .lock()
            .await
            .find_all_opportunities(guard.deref(), &mut *metrics_guard)
            .await
    }

    pub async fn discover_multihop_opportunities(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        {
            Ok(g) => g,
            Err(_elapsed) => {
                warn!("discover_multihop_opportunities: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError(
                    "Timeout for pools read lock in discover_multihop_opportunities".to_string(),
                ));
            }
        };
        let mut metrics_guard = self.metrics.lock().await;
        self.detector
            .lock()
            .await
            .find_all_multihop_opportunities(guard.deref(), &mut *metrics_guard)
            .await
    }

    pub async fn discover_multihop_opportunities_with_risk(
        &self,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.maybe_check_health().await?;
        let guard = match timeout(
            Duration::from_millis(self.config.pool_read_timeout_ms.unwrap_or(1000)),
            self.pools.read(),
        )
        .await
        {
            Ok(g) => g,
            Err(_elapsed) => {
                warn!("discover_multihop_opportunities_with_risk: Timeout waiting for pools read lock");
                return Err(ArbError::TimeoutError(
                    "Timeout for pools read lock in discover_multihop_opportunities_with_risk".to_string(),
                ));
            }
        };
        let mut metrics_guard = self.metrics.lock().await;
        let max_slippage = self.max_slippage;
        let tx_fee_lamports = self.tx_fee_lamports;
        self.detector
            .lock()
            .await
            .find_all_multihop_opportunities_with_risk(
                guard.deref(),
                &mut *metrics_guard,
                max_slippage,
                tx_fee_lamports,
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

    pub async fn resolve_pools_for_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        self.with_pool_guard_async(
            "resolve_pools_for_opportunity",
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

    pub async fn maybe_check_health(&self) -> Result<(), ArbError> {
        Ok(())
    }
    pub async fn run_health_checks(&self) { /* ... */
    }
    pub async fn update_pools(
        &self,
        _new_pools_data: HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<(), ArbError> {
        Ok(())
    }

    pub async fn detect_arbitrage(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        self.discover_multihop_opportunities().await
    }

    pub async fn run_dynamic_threshold_updates(&self) { /* ... */
    }
    pub async fn get_current_status(&self) -> String {
        String::new()
    }
}
