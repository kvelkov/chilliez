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

/// Lifinity pool concentration levels
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConcentrationLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "extreme")]
    Extreme,
}

/// Lifinity WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LifinityMessage {
    #[serde(rename = "pool_update")]
    PoolUpdate {
        pool_address: String,
        token_a_mint: String,
        token_b_mint: String,
        token_a_reserve: u64,
        token_b_reserve: u64,
        price: f64,
        oracle_price: Option<f64>,
        concentration: u16,
        concentration_level: ConcentrationLevel,
        liquidity: u128,
        inventory_ratio: f64,
        timestamp: u64,
    },
    #[serde(rename = "oracle_update")]
    OracleUpdate {
        pool_address: String,
        oracle_price: f64,
        confidence: f64,
        timestamp: u64,
    },
    #[serde(rename = "concentration_update")]
    ConcentrationUpdate {
        pool_address: String,
        new_concentration: u16,
        old_concentration: u16,
        reason: String,
        timestamp: u64,
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

/// Lifinity WebSocket feed implementation with proactive market making support
pub struct LifinityWebSocketFeed {
    config: WebSocketConfig,
    status: Arc<RwLock<ConnectionStatus>>,
    metrics: Arc<RwLock<WebSocketMetrics>>,
    websocket: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_pools: Arc<RwLock<HashMap<String, ConcentrationLevel>>>,
    price_sender: Arc<RwLock<Option<broadcast::Sender<PriceUpdate>>>>,
    last_heartbeat: Arc<RwLock<Option<Instant>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
}

impl LifinityWebSocketFeed {
    /// Create new Lifinity WebSocket feed
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

    /// Get the recommended Lifinity WebSocket URL
    pub fn get_lifinity_websocket_url() -> String {
        // Note: Replace with actual Lifinity WebSocket URL when available
        // This is a placeholder URL structure based on common patterns
        "wss://api.lifinity.io/ws".to_string()
    }

    /// Connect with retry logic
    async fn connect_with_retry(&mut self) -> Result<()> {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
        
        for attempt in 0..MAX_RETRIES {
            match self.establish_connection().await {
                Ok(()) => {
                    *self.reconnect_attempts.write().await = 0;
                    info!("✅ Connected to Lifinity WebSocket on attempt {}", attempt + 1);
                    return Ok(());
                }
                Err(e) => {
                    *self.reconnect_attempts.write().await = attempt + 1;
                    let backoff = INITIAL_BACKOFF * 2_u32.pow(attempt);
                    warn!("❌ Lifinity connection attempt {} failed: {}. Retrying in {:?}", 
                          attempt + 1, e, backoff);
                    
                    if attempt < MAX_RETRIES - 1 {
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }
        
        Err(anyhow!("Failed to connect to Lifinity WebSocket after {} attempts", MAX_RETRIES))
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
                debug!("Lifinity WebSocket not connected, stopping message loop");
                break;
            }

            let message_result = {
                let mut websocket_guard = self.websocket.write().await;
                if let Some(ref mut ws) = *websocket_guard {
                    ws.next().await
                } else {
                    warn!("Lifinity WebSocket stream not available");
                    break;
                }
            };

            match message_result {
                Some(Ok(Message::Text(text))) => {
                    if let Err(e) = self.handle_message(&text, &mut price_sender).await {
                        error!("Error handling Lifinity message: {}", e);
                        self.update_metrics_error().await;
                    }
                }
                Some(Ok(Message::Close(_))) => {
                    info!("Lifinity WebSocket connection closed by server");
                    *self.status.write().await = ConnectionStatus::Disconnected;
                    break;
                }
                Some(Err(e)) => {
                    error!("Lifinity WebSocket error: {}", e);
                    *self.status.write().await = ConnectionStatus::Failed;
                    self.update_metrics_error().await;
                    break;
                }
                None => {
                    warn!("Lifinity WebSocket stream ended unexpectedly");
                    *self.status.write().await = ConnectionStatus::Disconnected;
                    break;
                }
                _ => {
                    // Handle other message types (binary, ping, pong) if needed
                    debug!("Received non-text message from Lifinity WebSocket");
                }
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket message
    async fn handle_message(&self, text: &str, price_sender: &mut broadcast::Sender<PriceUpdate>) -> Result<()> {
        let message: LifinityMessage = serde_json::from_str(text)
            .map_err(|e| anyhow!("Failed to parse Lifinity message: {}", e))?;

        match message {
            LifinityMessage::PoolUpdate { 
                pool_address, 
                token_a_mint,
                token_b_mint,
                token_a_reserve,
                token_b_reserve,
                price,
                oracle_price: _,  // Oracle price available but not used in PriceUpdate
                concentration: _,  // Concentration available but not used in PriceUpdate
                concentration_level: _,  // Concentration level available but not used
                liquidity,
                inventory_ratio: _,  // Inventory ratio available but not used
                timestamp,
            } => {
                let price_update = PriceUpdate {
                    pool_address: pool_address.clone(),
                    dex_type: DexType::Lifinity,
                    token_a_reserve,
                    token_b_reserve,
                    token_a_mint,
                    token_b_mint,
                    timestamp,
                    price_a_to_b: price,
                    price_b_to_a: 1.0 / price,
                    liquidity: Some(liquidity),
                    volume_24h: None, // Volume data not provided in this message
                };

                if let Err(e) = price_sender.send(price_update) {
                    warn!("Failed to send Lifinity price update: {}", e);
                }

                self.update_metrics_message().await;
                debug!("📊 Processed Lifinity pool update for {}", pool_address);
            }
            LifinityMessage::OracleUpdate { pool_address, oracle_price, confidence: _, timestamp: _ } => {
                info!("📊 Lifinity oracle update for {}: price={:.6}", pool_address, oracle_price);
                self.update_metrics_message().await;
            }
            LifinityMessage::ConcentrationUpdate { pool_address, new_concentration, old_concentration, reason, timestamp: _ } => {
                info!("🔄 Lifinity concentration update for {}: {} -> {} ({})", 
                      pool_address, old_concentration, new_concentration, reason);
                self.update_metrics_message().await;
            }
            LifinityMessage::SubscriptionAck { pools, status } => {
                info!("✅ Lifinity subscription confirmed for {} pools: {}", pools.len(), status);
                self.update_metrics_message().await;
            }
            LifinityMessage::Error { message, code } => {
                error!("❌ Lifinity WebSocket error {}: {}", code, message);
                self.update_metrics_error().await;
            }
            LifinityMessage::Heartbeat { timestamp: _ } => {
                *self.last_heartbeat.write().await = Some(Instant::now());
                debug!("💓 Lifinity heartbeat received");
            }
        }

        Ok(())
    }

    /// Send subscription message for pools
    async fn send_subscription(&self, pool_addresses: Vec<String>) -> Result<()> {
        let subscription_message = json!({
            "type": "subscribe",
            "pools": pool_addresses,
            "include_oracle_data": true,
            "include_concentration_updates": true,
            "include_inventory_data": true,
        });

        let mut websocket_guard = self.websocket.write().await;
        if let Some(ref mut ws) = *websocket_guard {
            ws.send(Message::Text(subscription_message.to_string())).await
                .map_err(|e| anyhow!("Failed to send subscription: {}", e))?;
            
            info!("📡 Sent Lifinity subscription for {} pools", pool_addresses.len());
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
impl WebSocketFeed for LifinityWebSocketFeed {
    fn dex_type(&self) -> DexType {
        DexType::Lifinity
    }

    async fn connect(&mut self) -> Result<()> {
        info!("🔌 Connecting to Lifinity WebSocket...");
        
        // Set URL if not provided
        if self.config.url.is_empty() {
            self.config.url = Self::get_lifinity_websocket_url();
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
                error!("Lifinity message loop error: {}", e);
            }
        });

        info!("✅ Connected to Lifinity WebSocket successfully");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("🔌 Disconnecting from Lifinity WebSocket...");
        
        *self.status.write().await = ConnectionStatus::Disconnected;
        
        let mut websocket_guard = self.websocket.write().await;
        if let Some(mut ws) = websocket_guard.take() {
            let _ = ws.close(None).await;
        }

        *self.price_sender.write().await = None;
        self.subscribed_pools.write().await.clear();
        
        info!("✅ Disconnected from Lifinity WebSocket");
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
        debug!("📊 Subscribing to {} Lifinity pools", pool_addresses.len());

        // Store subscribed pools (assuming they're all Medium concentration for now)
        {
            let mut pools = self.subscribed_pools.write().await;
            for addr in &pool_addresses {
                pools.insert(addr.clone(), ConcentrationLevel::Medium);
            }
        }

        // Send subscription if connected
        let status = self.status.read().await.clone();
        if matches!(status, ConnectionStatus::Connected) {
            self.send_subscription(pool_addresses).await?;
        } else {
            info!("📊 Lifinity pools queued for subscription when connected");
        }

        Ok(())
    }

    fn parse_message(&self, message: &str) -> Result<Vec<PriceUpdate>> {
        let lifinity_message: LifinityMessage = serde_json::from_str(message)?;

        match lifinity_message {
            LifinityMessage::PoolUpdate { 
                pool_address, 
                token_a_mint,
                token_b_mint,
                token_a_reserve,
                token_b_reserve,
                price,
                oracle_price: _,  // Oracle price available but not used in PriceUpdate
                concentration: _,  // Concentration available but not used in PriceUpdate
                concentration_level: _,  // Concentration level available but not used
                liquidity,
                inventory_ratio: _,  // Inventory ratio available but not used
                timestamp,
            } => {
                let price_update = PriceUpdate {
                    pool_address,
                    dex_type: DexType::Lifinity,
                    token_a_reserve,
                    token_b_reserve,
                    token_a_mint,
                    token_b_mint,
                    timestamp,
                    price_a_to_b: price,
                    price_b_to_a: 1.0 / price,
                    liquidity: Some(liquidity),
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
impl Clone for LifinityWebSocketFeed {
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
    use tokio;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_lifinity_feed_creation() {
        let config = WebSocketConfig {
            url: "wss://api.lifinity.io/ws".to_string(),
            dex_type: DexType::Lifinity,
            ..Default::default()
        };

        let feed = LifinityWebSocketFeed::new(config);
        assert_eq!(feed.dex_type(), DexType::Lifinity);
        assert!(matches!(feed.status(), ConnectionStatus::Disconnected));
    }

    #[test]
    fn test_lifinity_url_generation() {
        let url = LifinityWebSocketFeed::get_lifinity_websocket_url();
        assert!(!url.is_empty());
        assert!(url.starts_with("wss://"));
    }

    #[test]
    fn test_lifinity_message_parsing() {
        let feed = LifinityWebSocketFeed::new(WebSocketConfig {
            url: "wss://test.com".to_string(),
            dex_type: DexType::Lifinity,
            ..Default::default()
        });

        let message = r#"{
            "type": "pool_update",
            "pool_address": "7YttLkHDoNj9wyDur5pM1ejNaAvT9X4eqaYcHQqtj2G5",
            "token_a_mint": "So11111111111111111111111111111111111111112",
            "token_b_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "token_a_reserve": 1000000,
            "token_b_reserve": 2000000,
            "price": 100.5,
            "oracle_price": 101.0,
            "concentration": 5000,
            "concentration_level": "medium",
            "liquidity": 5000000,
            "inventory_ratio": 0.5,
            "timestamp": 1234567890
        }"#;

        let updates = feed.parse_message(message).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].dex_type, DexType::Lifinity);
        assert_eq!(updates[0].price_a_to_b, 100.5);
    }

    #[test]
    fn test_concentration_level_serialization() {
        let level = ConcentrationLevel::High;
        let serialized = serde_json::to_string(&level).unwrap();
        assert_eq!(serialized, r#""high""#);
        
        let deserialized: ConcentrationLevel = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ConcentrationLevel::High);
    }
}
