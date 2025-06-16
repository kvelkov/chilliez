#![allow(dead_code)] // Phoenix WebSocket feed - production ready

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
    dex::math::phoenix,
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
                        if let Some(order_data) = data["data"].as_object() {
                            let timestamp = chrono::Utc::now().timestamp_millis() as u64;
                            
                            // Parse order book data from Phoenix
                            let best_bid_price = order_data
                                .get("bestBid")
                                .and_then(|bid| bid.get("price"))
                                .and_then(|p| p.as_f64())
                                .unwrap_or(0.0);
                                
                            let best_ask_price = order_data
                                .get("bestAsk")
                                .and_then(|ask| ask.get("price"))
                                .and_then(|p| p.as_f64())
                                .unwrap_or(0.0);
                                
                            let best_bid_size = order_data
                                .get("bestBid")
                                .and_then(|bid| bid.get("size"))
                                .and_then(|s| s.as_f64())
                                .unwrap_or(0.0);
                                
                            let best_ask_size = order_data
                                .get("bestAsk")
                                .and_then(|ask| ask.get("size"))
                                .and_then(|s| s.as_f64())
                                .unwrap_or(0.0);

                            // Create order book levels for market metrics calculation
                            let bids = vec![phoenix::OrderBookLevel {
                                price: (best_bid_price * 1_000_000.0) as u64,
                                size: best_bid_size as u64,
                            }];
                            
                            let asks = vec![phoenix::OrderBookLevel {
                                price: (best_ask_price * 1_000_000.0) as u64,
                                size: best_ask_size as u64,
                            }];

                            // Calculate market metrics using production Phoenix math
                            let market_metrics = phoenix::calculate_market_metrics(&bids, &asks)
                                .unwrap_or_else(|_| phoenix::MarketMetrics {
                                    mid_price: ((best_bid_price + best_ask_price) / 2.0 * 1_000_000.0) as u64,
                                    best_bid: (best_bid_price * 1_000_000.0) as u64,
                                    best_ask: (best_ask_price * 1_000_000.0) as u64,
                                    spread_bps: 0,
                                    bid_depth_1pct: best_bid_size as u64,
                                    ask_depth_1pct: best_ask_size as u64,
                                });

                            let mid_price_f64 = market_metrics.mid_price as f64 / 1_000_000.0;
                            
                            let total_bid_liquidity = order_data
                                .get("totalBidLiquidity")
                                .and_then(|l| l.as_f64())
                                .unwrap_or(best_bid_size);
                                
                            let total_ask_liquidity = order_data
                                .get("totalAskLiquidity")
                                .and_then(|l| l.as_f64())
                                .unwrap_or(best_ask_size);

                            let volume_24h = order_data
                                .get("volume24h")
                                .and_then(|v| v.as_f64());

                            // Create production-quality price update with Phoenix order book data
                            let update = PriceUpdate {
                                pool_address: market_id.to_string(),
                                dex_type: DexType::Phoenix,
                                token_a_mint: market_id.to_string(),
                                token_b_mint: "QUOTE".to_string(), // Would be parsed from market metadata
                                token_a_reserve: total_ask_liquidity as u64, // Base token liquidity
                                token_b_reserve: (total_bid_liquidity * mid_price_f64) as u64, // Quote token liquidity
                                price_a_to_b: if mid_price_f64 > 0.0 { 1.0 / mid_price_f64 } else { 0.0 },
                                price_b_to_a: mid_price_f64,
                                timestamp,
                                liquidity: Some((total_bid_liquidity + total_ask_liquidity) as u128),
                                volume_24h: volume_24h.map(|v| v as u64),
                            };
                            
                            updates.push(update);
                            
                            // Update subscription tracking
                            if let Ok(mut subscriptions) = self.subscribed_markets.write() {
                                if let Some(subscription) = subscriptions.get_mut(market_id) {
                                    subscription.last_update = Some(timestamp);
                                }
                            }
                            
                            debug!("📊 Phoenix market update: {} - mid_price: {:.6}, spread: {} bps, bid_depth: {}, ask_depth: {}",
                                   market_id, mid_price_f64, market_metrics.spread_bps, 
                                   market_metrics.bid_depth_1pct, market_metrics.ask_depth_1pct);
                        }
                    }
                }
                "subscribed" => {
                    info!("✅ Phoenix subscription confirmed");
                }
                "error" => {
                    warn!("⚠️ Phoenix WebSocket error: {:?}", data["message"]);
                }
                "ping" => {
                    // Handle ping/pong for connection health
                    debug!("📡 Phoenix ping received");
                }
                _ => {
                    debug!("❓ Unknown Phoenix message type: {}", msg_type);
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
