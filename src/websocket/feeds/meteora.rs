use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
use url::Url;

use crate::{
    utils::DexType,
    websocket::price_feeds::{
        ConnectionStatus, PriceUpdate, WebSocketFeed, WebSocketConfig,
        WebSocketMetrics,
    },
};

/// Meteora pool types and their characteristics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MeteoraPoolType {
    /// Dynamic AMM pools with concentrated liquidity
    #[serde(rename = "dynamic")]
    Dynamic,
    /// DLMM (Dynamic Liquidity Market Maker) pools
    #[serde(rename = "dlmm")]
    Dlmm,
    /// Legacy pools
    #[serde(rename = "legacy")]
    Legacy,
}

/// Meteora WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MeteoraMessage {
    #[serde(rename = "pool_update")]
    PoolUpdate {
        pool_address: String,
        pool_type: MeteoraPoolType,
        token_a_mint: String,
        token_b_mint: String,
        token_a_reserve: u64,
        token_b_reserve: u64,
        price: f64,
        fee_rate: f64,
        timestamp: u64,
        #[serde(flatten)]
        pool_specific: PoolSpecificData,
    },
    #[serde(rename = "subscription_ack")]
    SubscriptionAck {
        pools: Vec<String>,
        status: String,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
        code: u32,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat {
        timestamp: u64,
    },
}

/// Pool-specific data that varies by pool type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PoolSpecificData {
    Dynamic {
        current_bin: Option<i32>,
        active_bin_range: Option<(i32, i32)>,
        total_liquidity: Option<u64>,
    },
    Dlmm {
        active_bin_id: Option<i32>,
        bin_step: Option<u16>,
        liquidity_per_bin: Option<HashMap<String, u64>>,
    },
    Legacy {
        curve_type: Option<String>,
        amp_factor: Option<u64>,
    },
}

impl Default for PoolSpecificData {
    fn default() -> Self {
        Self::Dynamic {
            current_bin: None,
            active_bin_range: None,
            total_liquidity: None,
        }
    }
}

impl PoolSpecificData {
    /// Get total liquidity value regardless of pool type
    pub fn total_liquidity(&self) -> Option<u64> {
        match self {
            PoolSpecificData::Dynamic { total_liquidity, .. } => *total_liquidity,
            PoolSpecificData::Dlmm { liquidity_per_bin, .. } => {
                liquidity_per_bin.as_ref().map(|bins| {
                    bins.values().sum()
                })
            }
            PoolSpecificData::Legacy { .. } => None,
        }
    }
}

/// Meteora WebSocket feed implementation with DLMM and Dynamic AMM support
pub struct MeteoraWebSocketFeed {
    config: WebSocketConfig,
    status: Arc<RwLock<ConnectionStatus>>,
    metrics: Arc<RwLock<WebSocketMetrics>>,
    websocket: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_pools: Arc<RwLock<HashMap<String, MeteoraPoolType>>>,
    price_sender: Arc<RwLock<Option<broadcast::Sender<PriceUpdate>>>>,
    last_heartbeat: Arc<RwLock<Option<Instant>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
}

impl MeteoraWebSocketFeed {
    /// Create new Meteora WebSocket feed
    pub fn new(config: WebSocketConfig) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            metrics: Arc::new(RwLock::new(WebSocketMetrics::default())),
            websocket: Arc::new(RwLock::new(None)),
            subscribed_pools: Arc::new(RwLock::new(HashMap::new())),
            price_sender: Arc::new(RwLock::new(None)),
            last_heartbeat: Arc::new(RwLock::new(None)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the recommended Meteora WebSocket URL
    pub fn get_meteora_websocket_url() -> String {
        // Note: Replace with actual Meteora WebSocket URL when available
        // This is a placeholder URL structure based on common patterns
        "wss://api.meteora.ag/ws".to_string()
    }

    /// Connect with retry logic
    async fn connect_with_retry(&mut self) -> Result<()> {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
        
        for attempt in 0..MAX_RETRIES {
            match self.establish_connection().await {
                Ok(()) => {
                    *self.reconnect_attempts.write().await = 0;
                    info!("✅ Connected to Meteora WebSocket on attempt {}", attempt + 1);
                    return Ok(());
                }
                Err(e) => {
                    *self.reconnect_attempts.write().await = attempt + 1;
                    let backoff = INITIAL_BACKOFF * 2_u32.pow(attempt);
                    warn!("❌ Meteora connection attempt {} failed: {}. Retrying in {:?}", 
                          attempt + 1, e, backoff);
                    
                    if attempt < MAX_RETRIES - 1 {
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }
        
        Err(anyhow!("Failed to connect to Meteora WebSocket after {} attempts", MAX_RETRIES))
    }

    /// Establish the WebSocket connection
    async fn establish_connection(&mut self) -> Result<()> {
        let url = Url::parse(&self.config.url)
            .map_err(|e| anyhow!("Invalid WebSocket URL: {}", e))?;
        
        let (ws_stream, _) = connect_async(url).await
            .map_err(|e| anyhow!("WebSocket connection failed: {}", e))?;
        
        *self.websocket.write().await = Some(ws_stream);
        *self.status.write().await = ConnectionStatus::Connected;
        *self.last_heartbeat.write().await = Some(Instant::now());
        
        Ok(())
    }

    /// Start the message handling loop
    async fn start_message_loop(&self, mut price_sender: broadcast::Sender<PriceUpdate>) -> Result<()> {
        loop {
            let status = self.status.read().await.clone();
            if !matches!(status, ConnectionStatus::Connected) {
                debug!("Meteora WebSocket not connected, stopping message loop");
                break;
            }

            let message_result = {
                let mut websocket_guard = self.websocket.write().await;
                if let Some(ref mut ws) = *websocket_guard {
                    ws.next().await
                } else {
                    warn!("Meteora WebSocket stream not available");
                    break;
                }
            };

            match message_result {
                Some(Ok(Message::Text(text))) => {
                    if let Err(e) = self.handle_message(&text, &mut price_sender).await {
                        error!("Error handling Meteora message: {}", e);
                        self.update_metrics_error().await;
                    }
                }
                Some(Ok(Message::Close(_))) => {
                    info!("Meteora WebSocket connection closed by server");
                    *self.status.write().await = ConnectionStatus::Disconnected;
                    break;
                }
                Some(Err(e)) => {
                    error!("Meteora WebSocket error: {}", e);
                    *self.status.write().await = ConnectionStatus::Failed;
                    self.update_metrics_error().await;
                    break;
                }
                None => {
                    warn!("Meteora WebSocket stream ended unexpectedly");
                    *self.status.write().await = ConnectionStatus::Disconnected;
                    break;
                }
                _ => {
                    // Handle other message types (binary, ping, pong) if needed
                    debug!("Received non-text message from Meteora WebSocket");
                }
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket message
    async fn handle_message(&self, text: &str, price_sender: &mut broadcast::Sender<PriceUpdate>) -> Result<()> {
        let message: MeteoraMessage = serde_json::from_str(text)
            .map_err(|e| anyhow!("Failed to parse Meteora message: {}", e))?;

        match message {
            MeteoraMessage::PoolUpdate { 
                pool_address, 
                pool_type: _,  // Pool type stored but not used in PriceUpdate
                token_a_mint,
                token_b_mint,
                token_a_reserve,
                token_b_reserve,
                price,
                fee_rate: _,  // Fee rate available but not stored in PriceUpdate
                timestamp,
                pool_specific,
            } => {
                let price_update = PriceUpdate {
                    pool_address: pool_address.clone(),
                    dex_type: DexType::Meteora,
                    token_a_reserve,
                    token_b_reserve,
                    token_a_mint,
                    token_b_mint,
                    timestamp,
                    price_a_to_b: price,
                    price_b_to_a: 1.0 / price,
                    liquidity: pool_specific.total_liquidity().map(|l| l as u128),
                    volume_24h: None, // Volume data not provided in this message
                };

                if let Err(e) = price_sender.send(price_update) {
                    warn!("Failed to send Meteora price update: {}", e);
                }

                self.update_metrics_message().await;
                debug!("📊 Processed Meteora pool update for {}", pool_address);
            }
            MeteoraMessage::SubscriptionAck { pools, status } => {
                info!("✅ Meteora subscription confirmed for {} pools: {}", pools.len(), status);
                self.update_metrics_message().await;
            }
            MeteoraMessage::Error { message, code } => {
                error!("❌ Meteora WebSocket error {}: {}", code, message);
                self.update_metrics_error().await;
            }
            MeteoraMessage::Heartbeat { timestamp: _ } => {
                *self.last_heartbeat.write().await = Some(Instant::now());
                debug!("💓 Meteora heartbeat received");
            }
        }

        Ok(())
    }

    /// Send subscription message for pools
    async fn send_subscription(&self, pool_addresses: Vec<String>) -> Result<()> {
        let subscription_message = json!({
            "type": "subscribe",
            "pools": pool_addresses,
            "include_pool_data": true,
            "include_price_updates": true,
        });

        let mut websocket_guard = self.websocket.write().await;
        if let Some(ref mut ws) = *websocket_guard {
            ws.send(Message::Text(subscription_message.to_string())).await
                .map_err(|e| anyhow!("Failed to send subscription: {}", e))?;
            
            info!("📡 Sent Meteora subscription for {} pools", pool_addresses.len());
            Ok(())
        } else {
            Err(anyhow!("WebSocket not connected"))
        }
    }

    /// Update metrics for successful message processing
    async fn update_metrics_message(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.total_messages_received += 1;
        metrics.total_price_updates += 1;
        metrics.last_message_timestamp = Some(chrono::Utc::now().timestamp_millis() as u64);
    }

    /// Update metrics for errors
    async fn update_metrics_error(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.stale_message_count += 1;
    }

    /// Check connection health
    #[allow(dead_code)] // For future health monitoring
    async fn is_connection_healthy(&self) -> bool {
        let status = self.status.read().await.clone();
        if !matches!(status, ConnectionStatus::Connected) {
            return false;
        }

        // Check heartbeat
        if let Some(last_heartbeat) = *self.last_heartbeat.read().await {
            last_heartbeat.elapsed() < Duration::from_secs(60) // 60 seconds timeout
        } else {
            false
        }
    }
}

#[async_trait]
impl WebSocketFeed for MeteoraWebSocketFeed {
    fn dex_type(&self) -> DexType {
        DexType::Meteora
    }

    async fn connect(&mut self) -> Result<()> {
        info!("🔌 Connecting to Meteora WebSocket...");
        
        // Set URL if not provided
        if self.config.url.is_empty() {
            self.config.url = Self::get_meteora_websocket_url();
        }

        // Attempt connection with retry
        self.connect_with_retry().await?;

        // Set up message handling
        let (price_sender, _) = broadcast::channel(1000);
        *self.price_sender.write().await = Some(price_sender.clone());

        // Start message loop in background
        let feed_clone = self.clone();
        tokio::spawn(async move {
            if let Err(e) = feed_clone.start_message_loop(price_sender).await {
                error!("Meteora message loop error: {}", e);
            }
        });

        info!("✅ Connected to Meteora WebSocket successfully");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("🔌 Disconnecting from Meteora WebSocket...");
        
        *self.status.write().await = ConnectionStatus::Disconnected;
        
        let mut websocket_guard = self.websocket.write().await;
        if let Some(mut ws) = websocket_guard.take() {
            let _ = ws.close(None).await;
        }

        *self.price_sender.write().await = None;
        self.subscribed_pools.write().await.clear();
        
        info!("✅ Disconnected from Meteora WebSocket");
        Ok(())
    }

    fn status(&self) -> ConnectionStatus {
        // This is a blocking call, but we need it for the trait
        // In a real implementation, you might want to cache the status
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.status.read().await.clone()
            })
        })
    }

    async fn subscribe_to_pools(&mut self, pool_addresses: Vec<String>) -> Result<()> {
        debug!("📊 Subscribing to {} Meteora pools", pool_addresses.len());

        // Store subscribed pools (assuming they're all Dynamic type for now)
        {
            let mut pools = self.subscribed_pools.write().await;
            for addr in &pool_addresses {
                pools.insert(addr.clone(), MeteoraPoolType::Dynamic);
            }
        }

        // Send subscription if connected
        let status = self.status.read().await.clone();
        if matches!(status, ConnectionStatus::Connected) {
            self.send_subscription(pool_addresses).await?;
        } else {
            info!("📊 Meteora pools queued for subscription when connected");
        }

        Ok(())
    }

    fn parse_message(&self, message: &str) -> Result<Vec<PriceUpdate>> {
        let meteora_message: MeteoraMessage = serde_json::from_str(message)?;

        match meteora_message {
            MeteoraMessage::PoolUpdate { 
                pool_address, 
                pool_type: _,  // Pool type stored but not used in PriceUpdate
                token_a_mint,
                token_b_mint,
                token_a_reserve,
                token_b_reserve,
                price,
                fee_rate: _,  // Fee rate available but not stored in PriceUpdate
                timestamp,
                pool_specific,
            } => {
                let price_update = PriceUpdate {
                    pool_address,
                    dex_type: DexType::Meteora,
                    token_a_reserve,
                    token_b_reserve,
                    token_a_mint,
                    token_b_mint,
                    timestamp,
                    price_a_to_b: price,
                    price_b_to_a: 1.0 / price,
                    liquidity: pool_specific.total_liquidity().map(|l| l as u128),
                    volume_24h: None, // Volume data not provided in this message
                };
                Ok(vec![price_update])
            }
            _ => Ok(Vec::new()), // Non-price update messages
        }
    }

    fn get_metrics(&self) -> WebSocketMetrics {
        // This is a blocking call for the trait
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.metrics.read().await.clone()
            })
        })
    }
}

// Implement Clone for the feed (needed for spawning tasks)
impl Clone for MeteoraWebSocketFeed {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            status: Arc::clone(&self.status),
            metrics: Arc::clone(&self.metrics),
            websocket: Arc::clone(&self.websocket),
            subscribed_pools: Arc::clone(&self.subscribed_pools),
            price_sender: Arc::clone(&self.price_sender),
            last_heartbeat: Arc::clone(&self.last_heartbeat),
            reconnect_attempts: Arc::clone(&self.reconnect_attempts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio; // Add Tokio for async runtime

    #[tokio::test(flavor = "multi_thread")]
    async fn test_meteora_feed_creation() {
        let config = WebSocketConfig {
            url: "wss://api.meteora.ag/ws".to_string(),
            dex_type: DexType::Meteora,
            ..Default::default()
        };

        let feed = MeteoraWebSocketFeed::new(config);
        assert_eq!(feed.dex_type(), DexType::Meteora);
        assert!(matches!(feed.status(), ConnectionStatus::Disconnected));
    }

    #[test]
    fn test_meteora_url_generation() {
        let url = MeteoraWebSocketFeed::get_meteora_websocket_url();
        assert!(!url.is_empty());
        assert!(url.starts_with("wss://"));
    }

    #[test]
    fn test_meteora_message_parsing() {
        let feed = MeteoraWebSocketFeed::new(WebSocketConfig {
            url: "wss://test.com".to_string(),
            dex_type: DexType::Meteora,
            ..Default::default()
        });

        let message = r#"{
            "type": "pool_update",
            "pool_address": "7YttLkHDoNj9wyDur5pM1ejNaAvT9X4eqaYcHQqtj2G5",
            "pool_type": "dynamic",
            "token_a_mint": "So11111111111111111111111111111111111111112",
            "token_b_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "token_a_reserve": 1000000,
            "token_b_reserve": 2000000,
            "price": 100.5,
            "fee_rate": 0.003,
            "timestamp": 1234567890,
            "current_bin": 123,
            "active_bin_range": [120, 126],
            "total_liquidity": 5000000
        }"#;

        let updates = feed.parse_message(message).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].dex_type, DexType::Meteora);
        assert_eq!(updates[0].price_a_to_b, 100.5);
    }

    #[test]
    fn test_pool_type_serialization() {
        let pool_type = MeteoraPoolType::Dlmm;
        let serialized = serde_json::to_string(&pool_type).unwrap();
        assert_eq!(serialized, r#""dlmm""#);
        
        let deserialized: MeteoraPoolType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, MeteoraPoolType::Dlmm);
    }
}
