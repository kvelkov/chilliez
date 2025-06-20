use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::interval,
};

use crate::utils::{DexType, PoolInfo};

/// Maximum acceptable latency for price data (100ms as per requirements)
const MAX_PRICE_LATENCY_MS: u64 = 100;

/// WebSocket reconnection settings
const WS_RECONNECT_DELAY_MS: u64 = 1000;
const WS_MAX_RECONNECT_ATTEMPTS: u32 = 10;
const WS_PING_INTERVAL_MS: u64 = 30000;

/// Price update event from WebSocket feeds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    pub pool_address: String,
    pub dex_type: DexType,
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
    pub token_a_mint: String,
    pub token_b_mint: String,
    pub timestamp: u64, // Unix timestamp in milliseconds
    pub price_a_to_b: f64,
    pub price_b_to_a: f64,
    pub liquidity: Option<u128>,
    pub volume_24h: Option<u64>,
}

impl PriceUpdate {
    /// Check if price data is fresh (within latency threshold)
    pub fn is_fresh(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let age_ms = now.saturating_sub(self.timestamp);
        age_ms <= MAX_PRICE_LATENCY_MS
    }

    /// Calculate age of price data in milliseconds
    pub fn age_ms(&self) -> u64 {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        now.saturating_sub(self.timestamp)
    }
}

/// WebSocket connection status
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// WebSocket feed configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    pub url: String,
    pub dex_type: DexType,
    pub reconnect_delay_ms: u64,
    pub max_reconnect_attempts: u32,
    pub ping_interval_ms: u64,
    pub subscription_message: Option<String>,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            dex_type: DexType::Orca,
            reconnect_delay_ms: WS_RECONNECT_DELAY_MS,
            max_reconnect_attempts: WS_MAX_RECONNECT_ATTEMPTS,
            ping_interval_ms: WS_PING_INTERVAL_MS,
            subscription_message: None,
        }
    }
}

/// Trait for DEX-specific WebSocket feed implementations
#[async_trait]
pub trait WebSocketFeed: Send + Sync {
    /// Get the DEX type this feed handles
    fn dex_type(&self) -> DexType;

    /// Connect to the WebSocket and start receiving data
    async fn connect(&mut self) -> Result<()>;

    /// Disconnect from the WebSocket
    async fn disconnect(&mut self) -> Result<()>;

    /// Get current connection status
    fn status(&self) -> ConnectionStatus;

    /// Subscribe to specific pools (if supported by DEX)
    async fn subscribe_to_pools(&mut self, pool_addresses: Vec<String>) -> Result<()>;

    /// Parse incoming WebSocket message into price update
    fn parse_message(&self, message: &str) -> Result<Vec<PriceUpdate>>;

    /// Get health metrics for this feed
    fn get_metrics(&self) -> WebSocketMetrics;
}

/// WebSocket connection metrics
#[derive(Debug, Clone, Default)]
pub struct WebSocketMetrics {
    pub total_messages_received: u64,
    pub total_price_updates: u64,
    pub total_reconnections: u32,
    pub last_message_timestamp: Option<u64>,
    pub connection_uptime_ms: u64,
    pub average_latency_ms: f64,
    pub stale_message_count: u64,
}

/// Centralized WebSocket price feed manager
pub struct PriceFeedManager {
    feeds: HashMap<DexType, Box<dyn WebSocketFeed>>,
    price_cache: Arc<RwLock<HashMap<String, PriceUpdate>>>,
    update_sender: mpsc::UnboundedSender<PriceUpdate>,
    update_receiver: Option<mpsc::UnboundedReceiver<PriceUpdate>>,
    status: Arc<RwLock<HashMap<DexType, ConnectionStatus>>>,
    metrics: Arc<RwLock<HashMap<DexType, WebSocketMetrics>>>,
    shutdown_sender: Option<oneshot::Sender<()>>,
}

impl PriceFeedManager {
    /// Create a new price feed manager
    pub fn new() -> Self {
        let (update_sender, update_receiver) = mpsc::unbounded_channel();

        Self {
            feeds: HashMap::new(),
            price_cache: Arc::new(RwLock::new(HashMap::new())),
            update_sender,
            update_receiver: Some(update_receiver),
            status: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            shutdown_sender: None,
        }
    }

    /// Add a WebSocket feed for a specific DEX
    pub fn add_feed(&mut self, feed: Box<dyn WebSocketFeed>) {
        let dex_type = feed.dex_type();
        self.feeds.insert(dex_type.clone(), feed);

        // Initialize status and metrics
        if let Ok(mut status) = self.status.write() {
            status.insert(dex_type.clone(), ConnectionStatus::Disconnected);
        }
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.insert(dex_type, WebSocketMetrics::default());
        }
    }

    /// Start all WebSocket connections and price monitoring
    pub async fn start(&mut self) -> Result<mpsc::UnboundedReceiver<PriceUpdate>> {
        info!("🚀 Starting WebSocket price feed manager...");

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_sender = Some(shutdown_tx);

        // Connect all feeds
        for (dex_type, feed) in &mut self.feeds {
            info!("📡 Connecting to {} WebSocket feed...", dex_type);

            // Update status to connecting
            if let Ok(mut status) = self.status.write() {
                status.insert(dex_type.clone(), ConnectionStatus::Connecting);
            }

            match feed.connect().await {
                Ok(_) => {
                    info!("✅ Successfully connected to {} WebSocket", dex_type);
                    if let Ok(mut status) = self.status.write() {
                        status.insert(dex_type.clone(), ConnectionStatus::Connected);
                    }
                }
                Err(e) => {
                    error!("❌ Failed to connect to {} WebSocket: {}", dex_type, e);
                    if let Ok(mut status) = self.status.write() {
                        status.insert(dex_type.clone(), ConnectionStatus::Failed);
                    }
                }
            }
        }

        // Start price update monitoring task
        let price_cache = Arc::clone(&self.price_cache);
        let update_sender = self.update_sender.clone();
        tokio::spawn(async move {
            Self::price_monitoring_task(price_cache, update_sender, shutdown_rx).await;
        });

        // Return the receiver for price updates
        self.update_receiver
            .take()
            .ok_or_else(|| anyhow!("Update receiver already taken"))
    }

    /// Stop all WebSocket connections
    pub async fn stop(&mut self) -> Result<()> {
        info!("🛑 Stopping WebSocket price feed manager...");

        // Send shutdown signal
        if let Some(shutdown_tx) = self.shutdown_sender.take() {
            let _ = shutdown_tx.send(());
        }

        // Disconnect all feeds
        for (dex_type, feed) in &mut self.feeds {
            info!("📡 Disconnecting from {} WebSocket feed...", dex_type);
            if let Err(e) = feed.disconnect().await {
                warn!("⚠️ Error disconnecting from {} WebSocket: {}", dex_type, e);
            }
        }

        info!("✅ WebSocket price feed manager stopped");
        Ok(())
    }

    /// Get latest price for a specific pool
    pub fn get_latest_price(&self, pool_address: &str) -> Option<PriceUpdate> {
        self.price_cache.read().ok()?.get(pool_address).cloned()
    }

    /// Get all latest prices
    pub fn get_all_prices(&self) -> HashMap<String, PriceUpdate> {
        self.price_cache
            .read()
            .map(|cache| cache.clone())
            .unwrap_or_default()
    }

    /// Get fresh prices only (within latency threshold)
    pub fn get_fresh_prices(&self) -> HashMap<String, PriceUpdate> {
        self.price_cache
            .read()
            .map(|cache| {
                cache
                    .iter()
                    .filter(|(_, update)| update.is_fresh())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get connection status for all DEXs
    pub fn get_connection_status(&self) -> HashMap<DexType, ConnectionStatus> {
        self.status
            .read()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    /// Get metrics for all feeds
    pub fn get_metrics(&self) -> HashMap<DexType, WebSocketMetrics> {
        self.metrics
            .read()
            .map(|metrics| metrics.clone())
            .unwrap_or_default()
    }

    /// Subscribe to specific pools across all DEXs
    pub async fn subscribe_to_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        // Group pools by DEX type
        let mut pools_by_dex: HashMap<DexType, Vec<String>> = HashMap::new();

        for pool in pools {
            pools_by_dex
                .entry(pool.dex_type)
                .or_default()
                .push(pool.address.to_string());
        }

        // Subscribe each DEX to its pools
        for (dex_type, pool_addresses) in pools_by_dex {
            if let Some(feed) = self.feeds.get_mut(&dex_type) {
                info!(
                    "📋 Subscribing {} to {} pools",
                    dex_type,
                    pool_addresses.len()
                );
                if let Err(e) = feed.subscribe_to_pools(pool_addresses).await {
                    warn!("⚠️ Failed to subscribe {} to pools: {}", dex_type, e);
                }
            }
        }

        Ok(())
    }

    /// Background task for monitoring price updates and cache management
    async fn price_monitoring_task(
        price_cache: Arc<RwLock<HashMap<String, PriceUpdate>>>,
        _update_sender: mpsc::UnboundedSender<PriceUpdate>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) {
        let mut cleanup_interval = interval(Duration::from_secs(60)); // Clean up stale data every minute

        loop {
            tokio::select! {
                _ = cleanup_interval.tick() => {
                    // Remove stale price data
                    if let Ok(mut cache) = price_cache.write() {
                        let before_count = cache.len();
                        cache.retain(|_, update| update.is_fresh());
                        let after_count = cache.len();

                        if before_count != after_count {
                            debug!("🧹 Cleaned up {} stale price entries", before_count - after_count);
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("🛑 Price monitoring task shutting down");
                    break;
                }
            }
        }
    }

    /// Validate price data quality
    pub fn validate_price_quality(&self) -> HashMap<DexType, f64> {
        let mut quality_scores = HashMap::new();
        let prices = self.get_all_prices();

        for dex_type in [
            DexType::Orca,
            DexType::Raydium,
            DexType::Meteora,
            DexType::Lifinity,
            DexType::Phoenix,
        ] {
            let dex_prices: Vec<_> = prices.values().filter(|p| p.dex_type == dex_type).collect();

            if dex_prices.is_empty() {
                quality_scores.insert(dex_type, 0.0);
                continue;
            }

            let fresh_count = dex_prices.iter().filter(|p| p.is_fresh()).count();
            let quality = (fresh_count as f64) / (dex_prices.len() as f64);
            quality_scores.insert(dex_type, quality);
        }

        quality_scores
    }
}

impl Default for PriceFeedManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_update_freshness() {
        let now = chrono::Utc::now().timestamp_millis() as u64;

        // Fresh price update
        let fresh_update = PriceUpdate {
            pool_address: "test_pool".to_string(),
            dex_type: DexType::Orca,
            token_a_reserve: 1000,
            token_b_reserve: 2000,
            token_a_mint: "token_a".to_string(),
            token_b_mint: "token_b".to_string(),
            timestamp: now - 50, // 50ms ago
            price_a_to_b: 2.0,
            price_b_to_a: 0.5,
            liquidity: Some(1000000),
            volume_24h: Some(50000),
        };

        // Stale price update
        let stale_update = PriceUpdate {
            timestamp: now - 150, // 150ms ago
            ..fresh_update.clone()
        };

        assert!(fresh_update.is_fresh());
        assert!(!stale_update.is_fresh());
        assert!(fresh_update.age_ms() <= MAX_PRICE_LATENCY_MS);
        assert!(stale_update.age_ms() > MAX_PRICE_LATENCY_MS);
    }

    #[test]
    fn test_websocket_config_default() {
        let config = WebSocketConfig::default();
        assert_eq!(config.dex_type, DexType::Orca);
        assert_eq!(config.reconnect_delay_ms, WS_RECONNECT_DELAY_MS);
        assert_eq!(config.max_reconnect_attempts, WS_MAX_RECONNECT_ATTEMPTS);
        assert_eq!(config.ping_interval_ms, WS_PING_INTERVAL_MS);
    }

    #[test]
    fn test_price_feed_manager_creation() {
        let manager = PriceFeedManager::new();
        assert!(manager.feeds.is_empty());
        assert!(manager.get_all_prices().is_empty());
    }
}
