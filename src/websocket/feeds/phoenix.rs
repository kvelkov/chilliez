#![allow(dead_code)] // WebSocket feed implementation in progress

use anyhow::{anyhow, Result};
use futures_util::SinkExt;
use log::{debug, error, info, warn};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Instant,
};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};
use url::Url;

use crate::{
    utils::DexType,
    websocket::price_feeds::{
        ConnectionStatus, PriceUpdate, WebSocketFeed, WebSocketConfig,
        WebSocketMetrics,
    },
};

/// Phoenix-specific WebSocket feed implementation
pub struct PhoenixWebSocketFeed {
    config: WebSocketConfig,
    websocket: Option<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    update_sender: mpsc::UnboundedSender<PriceUpdate>,
    status: ConnectionStatus,
    metrics: WebSocketMetrics,
    subscribed_markets: Arc<RwLock<HashMap<String, PhoenixSubscription>>>,
    last_ping: Option<Instant>,
}

/// Phoenix subscription tracking
#[derive(Debug, Clone)]
struct PhoenixSubscription {
    market_address: String,
    pair_name: String,
    subscribed_at: u64,
    last_update: Option<u64>,
}

/// Phoenix WebSocket message format
#[derive(Debug, Deserialize)]
struct PhoenixMessage {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(rename = "marketId")]
    market_id: Option<String>,
    data: Option<PhoenixMarketData>,
}

/// Phoenix order book market data
#[derive(Debug, Deserialize)]
struct PhoenixMarketData {
    #[serde(rename = "bestBid")]
    best_bid: Option<PhoenixLevel>,
    #[serde(rename = "bestAsk")]
    best_ask: Option<PhoenixLevel>,
    #[serde(rename = "lastPrice")]
    last_price: Option<f64>,
    #[serde(rename = "volume24h")]
    volume_24h: Option<f64>,
    #[serde(rename = "priceChange24h")]
    price_change_24h: Option<f64>,
    #[serde(rename = "totalBidLiquidity")]
    total_bid_liquidity: Option<f64>,
    #[serde(rename = "totalAskLiquidity")]
    total_ask_liquidity: Option<f64>,
    timestamp: u64,
}

/// Phoenix order book level
#[derive(Debug, Deserialize)]
struct PhoenixLevel {
    price: f64,
    size: f64,
}

impl PhoenixWebSocketFeed {
    /// Create new Phoenix WebSocket feed
    pub fn new(
        config: WebSocketConfig,
        update_sender: mpsc::UnboundedSender<PriceUpdate>,
    ) -> Self {
        Self {
            config,
            websocket: None,
            update_sender,
            status: ConnectionStatus::Disconnected,
            metrics: WebSocketMetrics::default(),
            subscribed_markets: Arc::new(RwLock::new(HashMap::new())),
            last_ping: None,
        }
    }

    /// Subscribe to a specific Phoenix market
    pub async fn subscribe_to_market(&mut self, market_address: &str, pair_name: &str) -> Result<()> {
        if !matches!(self.status, ConnectionStatus::Connected) {
            return Err(anyhow!("Not connected to Phoenix WebSocket"));
        }

        let subscription_message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "marketSubscribe",
            "params": [market_address]
        });

        if let Some(ref mut ws) = self.websocket {
            ws.send(Message::Text(subscription_message.to_string())).await?;
            
            // Track subscription
            let subscription = PhoenixSubscription {
                market_address: market_address.to_string(),
                pair_name: pair_name.to_string(),
                subscribed_at: chrono::Utc::now().timestamp_millis() as u64,
                last_update: None,
            };

            {
                let mut subscriptions = self.subscribed_markets.write().unwrap();
                subscriptions.insert(market_address.to_string(), subscription);
            }

            info!("📊 Subscribed to Phoenix market: {} ({})", pair_name, market_address);
        }

        Ok(())
    }

}

#[async_trait::async_trait]
impl WebSocketFeed for PhoenixWebSocketFeed {
    fn dex_type(&self) -> DexType {
        DexType::Phoenix
    }

    async fn connect(&mut self) -> Result<()> {
        info!("🔌 Connecting to Phoenix WebSocket...");
        self.status = ConnectionStatus::Connecting;

        let url = Url::parse(&self.config.url)?;
        
        match connect_async(url).await {
            Ok((ws_stream, _)) => {
                self.websocket = Some(ws_stream);
                self.status = ConnectionStatus::Connected;
                self.metrics.total_reconnections += 1;
                self.metrics.last_message_timestamp = Some(chrono::Utc::now().timestamp_millis() as u64);
                
                info!("✅ Connected to Phoenix WebSocket");
                Ok(())
            }
            Err(e) => {
                self.status = ConnectionStatus::Failed;
                self.metrics.stale_message_count += 1;
                error!("❌ Failed to connect to Phoenix WebSocket: {}", e);
                Err(anyhow!("Connection failed: {}", e))
            }
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut ws) = self.websocket.take() {
            let _ = ws.close(None).await;
        }
        
        self.status = ConnectionStatus::Disconnected;
        self.subscribed_markets.write().unwrap().clear();
        
        info!("🔌 Disconnected from Phoenix WebSocket");
        Ok(())
    }

    fn status(&self) -> ConnectionStatus {
        self.status.clone()
    }

    async fn subscribe_to_pools(&mut self, pool_addresses: Vec<String>) -> Result<()> {
        if !matches!(self.status, ConnectionStatus::Connected) {
            return Err(anyhow!("Not connected to Phoenix WebSocket"));
        }

        for address in pool_addresses {
            // For Phoenix, subscribe to order book updates for the market
            let subscribe_msg = format!(
                r#"{{"type":"subscribe","channel":"orderbook","market":"{}"}}"#,
                address
            );
            
            if let Some(ref mut ws) = self.websocket {
                ws.send(Message::Text(subscribe_msg)).await?;
                
                // Track subscription
                let subscription = PhoenixSubscription {
                    market_address: address.clone(),
                    pair_name: format!("UNKNOWN/{}", address), // Would need market info to get proper pair name
                    subscribed_at: chrono::Utc::now().timestamp_millis() as u64,
                    last_update: None,
                };
                
                self.subscribed_markets.write().unwrap().insert(address, subscription);
            }
        }

        Ok(())
    }

    fn parse_message(&self, message: &str) -> Result<Vec<PriceUpdate>> {
        let data: serde_json::Value = serde_json::from_str(message)?;
        let mut updates = Vec::new();

        // Handle different message types from Phoenix
        if let Some(msg_type) = data["type"].as_str() {
            match msg_type {
                "orderbook" | "marketUpdate" => {
                    if let Some(market_id) = data["market"].as_str() {
                        if let Some(_order_data) = data["data"].as_object() {
                            let timestamp = chrono::Utc::now().timestamp_millis() as u64;
                            
                            // Create placeholder update - Phoenix has complex order book structure
                            // In a real implementation, we'd need to parse the order book data properly
                            let update = PriceUpdate {
                                pool_address: market_id.to_string(),
                                dex_type: DexType::Phoenix,
                                token_a_mint: market_id.to_string(),
                                token_b_mint: "QUOTE".to_string(), // Would need to parse market info
                                token_a_reserve: 0, // Would calculate from order book
                                token_b_reserve: 0, // Would calculate from order book
                                price_a_to_b: 0.0, // Would get from best ask
                                price_b_to_a: 0.0, // Would get from best bid
                                timestamp,
                                liquidity: None,
                                volume_24h: None,
                            };
                            
                            updates.push(update);
                        }
                    }
                }
                "subscribed" => {
                    debug!("Phoenix subscription confirmed");
                }
                "error" => {
                    warn!("Phoenix WebSocket error: {:?}", data["message"]);
                }
                _ => {
                    debug!("Unknown Phoenix message type: {}", msg_type);
                }
            }
        }

        Ok(updates)
    }

    fn get_metrics(&self) -> WebSocketMetrics {
        self.metrics.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_phoenix_feed_creation() {
        let config = WebSocketConfig {
            url: "wss://phoenix.example.com/ws".to_string(),
            dex_type: DexType::Phoenix,
            reconnect_delay_ms: 1000,
            max_reconnect_attempts: 3,
            ping_interval_ms: 30000,
            subscription_message: None,
        };

        let (tx, _rx) = mpsc::unbounded_channel();
        let feed = PhoenixWebSocketFeed::new(config, tx);
        
        assert_eq!(feed.dex_type(), DexType::Phoenix);
        assert_eq!(feed.status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_phoenix_message_parsing() {
        let config = WebSocketConfig {
            url: "wss://phoenix.example.com/ws".to_string(),
            dex_type: DexType::Phoenix,
            reconnect_delay_ms: 1000,
            max_reconnect_attempts: 3,
            ping_interval_ms: 30000,
            subscription_message: None,
        };

        let (tx, _rx) = mpsc::unbounded_channel();
        let feed = PhoenixWebSocketFeed::new(config, tx);
        
        let test_message = r#"{
            "type": "marketUpdate",
            "marketId": "ABC123",
            "data": {
                "bestBid": {"price": 1.98, "size": 1000.0},
                "bestAsk": {"price": 2.02, "size": 800.0},
                "lastPrice": 2.0,
                "volume24h": 50000.0,
                "priceChange24h": 0.05,
                "totalBidLiquidity": 15000.0,
                "totalAskLiquidity": 12000.0,
                "timestamp": 1640995200000
            }
        }"#;

        // This would need a subscription to be set up first in a real test
        let result = feed.parse_message(test_message);
        assert!(result.is_ok());
    }
}
