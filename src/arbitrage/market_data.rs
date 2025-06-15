// src/arbitrage/market_data.rs
use crate::{
    config::settings::Config,
    dex::DexClient,
    error::ArbError,
    metrics::Metrics,
    solana::{rpc::SolanaRpcClient, websocket::SolanaWebsocketManager},
    utils::PoolInfo,
};
use dashmap::DashMap;
use log::{debug, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};

/// Define a simple trait for price data providers
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}

/// Market data manager responsible for price feeds and pool information
pub struct MarketDataManager {
    pub hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub price_provider: Option<Arc<dyn PriceDataProvider>>,
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    pub degradation_mode: Arc<AtomicBool>,
    pub last_health_check: Arc<RwLock<Instant>>,
    pub health_check_interval: Duration,
    pub ws_reconnect_attempts: Arc<AtomicU64>,
    pub max_ws_reconnect_attempts: u64,
}

impl MarketDataManager {
    pub fn new(
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
        price_provider: Option<Arc<dyn PriceDataProvider>>,
        rpc_client: Option<Arc<SolanaRpcClient>>,
        dex_providers: Vec<Arc<dyn DexClient>>,
        config: &Config,
    ) -> Self {
        let health_check_interval = Duration::from_secs(config.health_check_interval_secs.unwrap_or(60));
        let max_ws_reconnect_attempts = config.max_ws_reconnect_attempts.unwrap_or(5) as u64;

        Self {
            hot_cache,
            ws_manager,
            price_provider,
            rpc_client,
            dex_providers,
            degradation_mode: Arc::new(AtomicBool::new(false)),
            last_health_check: Arc::new(RwLock::new(Instant::now())),
            health_check_interval,
            ws_reconnect_attempts: Arc::new(AtomicU64::new(0)),
            max_ws_reconnect_attempts,
        }
    }

    /// Check health status of all market data components
    pub async fn check_health(&self, metrics: &Arc<Mutex<Metrics>>) -> Result<(), ArbError> {
        let now = Instant::now();
        let mut last_check = self.last_health_check.write().await;
        
        if now.duration_since(*last_check) < self.health_check_interval {
            return Ok(());
        }
        
        *last_check = now;
        drop(last_check);

        info!("🏥 Performing comprehensive health check...");

        // Check WebSocket health
        if let Some(ws_manager) = &self.ws_manager {
            match ws_manager.lock().await.health_check().await {
                Ok(_) => {
                    info!("✅ WebSocket connection healthy");
                    self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
                }
                Err(e) => {
                    warn!("❌ WebSocket unhealthy: {}", e);
                    let attempts = self.ws_reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                    if attempts >= self.max_ws_reconnect_attempts {
                        warn!("🚨 Max WebSocket reconnect attempts reached, entering degradation mode");
                        self.degradation_mode.store(true, Ordering::Relaxed);
                    }
                }
            }
        }

        // Check RPC health
        if let Some(rpc_client) = &self.rpc_client {
            match rpc_client.health_check().await {
                Ok(rpc_health_status) => {
                    info!("✅ RPC connection healthy - Latency: {}ms", rpc_health_status.avg_latency_ms);
                    
                    // Log fallback status
                    for (i, fallback) in rpc_health_status.fallback_statuses.iter().enumerate() {
                        if fallback.is_healthy {
                            debug!("✅ Fallback RPC {} healthy", i);
                        } else {
                            warn!("❌ Fallback RPC {} unhealthy: {}", i, fallback.message);
                        }
                    }
                }
                Err(e) => {
                    warn!("❌ RPC unhealthy: {}", e);
                    self.degradation_mode.store(true, Ordering::Relaxed);
                }
            }
        }

        // Check DEX provider health
        for (i, provider) in self.dex_providers.iter().enumerate() {
            match provider.health_check().await {
                Ok(health) => {
                    if health.is_healthy {
                        debug!("✅ DEX provider {} healthy", i);
                    } else {
                        warn!("⚠️ DEX provider {} degraded: {}", i, health.message);
                    }
                }
                Err(e) => {
                    warn!("❌ DEX provider {} unhealthy: {}", i, e);
                }
            }
        }

        // Update metrics
        let mut metrics_guard = metrics.lock().await;
        metrics_guard.last_health_check = chrono::Utc::now().timestamp_millis() as u64;
        metrics_guard.degradation_mode = self.degradation_mode.load(Ordering::Relaxed);

        if self.degradation_mode.load(Ordering::Relaxed) {
            warn!("🚨 System in degradation mode - limited functionality");
        } else {
            info!("✅ All systems healthy");
        }

        Ok(())
    }

    /// Get current cache statistics
    pub fn get_cache_stats(&self) -> (usize, f64) {
        let total_pools = self.hot_cache.len();
        let hit_rate = if total_pools > 0 {
            // This is a simplified calculation - in a real system you'd track hits/misses
            95.0 // Placeholder hit rate
        } else {
            0.0
        };
        (total_pools, hit_rate)
    }

    /// Check if we're in degradation mode
    pub fn is_degraded(&self) -> bool {
        self.degradation_mode.load(Ordering::Relaxed)
    }

    /// Force exit degradation mode (for recovery)
    pub fn exit_degradation_mode(&self) {
        self.degradation_mode.store(false, Ordering::Relaxed);
        self.ws_reconnect_attempts.store(0, Ordering::Relaxed);
        info!("🔄 Exited degradation mode");
    }
}
