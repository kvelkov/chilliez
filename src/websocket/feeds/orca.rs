#![allow(dead_code)] // WebSocket feed implementation in progress

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::{
    sync::mpsc,
    time::{interval, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
use url::Url;

use crate::{
    utils::DexType,
    websocket::price_feeds::{
        ConnectionStatus, PriceUpdate, WebSocketConfig, WebSocketFeed, WebSocketMetrics,
    },
};

/// For real-time price feeds, we'll use RPC WebSocket for account monitoring
/// This connects to a Solana RPC WebSocket to monitor Whirlpool account changes
const ORCA_RPC_WS_URL: &str = "wss://api.mainnet-beta.solana.com";

/// Orca-specific account change notification from RPC subscription
#[derive(Debug, Deserialize)]
struct OrcaAccountNotification {
    jsonrpc: String,
    method: String,
    params: OrcaAccountParams,
}

#[derive(Debug, Deserialize)]
struct OrcaAccountParams {
    result: OrcaAccountResult,
    subscription: u64,
}

#[derive(Debug, Deserialize)]
struct OrcaAccountResult {
    context: OrcaContext,
    value: OrcaAccountValue,
}

#[derive(Debug, Deserialize)]
struct OrcaContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct OrcaAccountValue {
    account: OrcaAccountData,
    pubkey: String,
}

#[derive(Debug, Deserialize)]
struct OrcaAccountData {
    data: Vec<String>, // Base64 encoded account data
    executable: bool,
    lamports: u64,
    owner: String,
    #[serde(rename = "rentEpoch")]
    rent_epoch: u64,
}

/// Internal message types for processing
#[derive(Debug, Deserialize)]
enum OrcaMessage {
    AccountUpdate {
        pool_address: String,
        account_data: Vec<u8>,
        slot: u64,
    },
    Ping { timestamp: u64 },
    Error { message: String },
}

/// Orca whirlpool data structure
#[derive(Debug, Deserialize)]
struct OrcaWhirlpoolData {
    #[serde(rename = "whirlpoolAddress")]
    whirlpool_address: String,
    #[serde(rename = "tokenA")]
    token_a: OrcaTokenInfo,
    #[serde(rename = "tokenB")]
    token_b: OrcaTokenInfo,
    #[serde(rename = "sqrtPrice")]
    sqrt_price: String,
    #[serde(rename = "liquidity")]
    liquidity: String,
    #[serde(rename = "tickCurrentIndex")]
    tick_current_index: i32,
    #[serde(rename = "feeRate")]
    fee_rate: f64,
    #[serde(rename = "volume24h")]
    volume_24h: Option<String>,
    timestamp: u64,
}

#[derive(Debug, Deserialize)]
struct OrcaTokenInfo {
    mint: String,
    #[serde(rename = "decimals")]
    decimals: u8,
    #[serde(rename = "vault")]
    vault: String,
    #[serde(rename = "vaultBalance")]
    vault_balance: String,
}

/// Orca subscription message
#[derive(Debug, Serialize)]
struct OrcaSubscription {
    #[serde(rename = "type")]
    msg_type: String,
    pools: Option<Vec<String>>, // Specific pools to subscribe to (if None, subscribes to all)
}

/// Orca WebSocket feed implementation
pub struct OrcaWebSocketFeed {
    config: WebSocketConfig,
    websocket: Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    status: ConnectionStatus,
    metrics: WebSocketMetrics,
    update_sender: mpsc::UnboundedSender<PriceUpdate>,
    subscribed_pools: Vec<String>,
    last_ping: Option<Instant>,
    reconnect_attempts: u32,
}

impl OrcaWebSocketFeed {
    /// Create a new Orca WebSocket feed
    pub fn new(update_sender: mpsc::UnboundedSender<PriceUpdate>) -> Self {
        let config = WebSocketConfig {
            url: ORCA_RPC_WS_URL.to_string(),
            dex_type: DexType::Orca,
            subscription_message: Some(
                serde_json::to_string(&OrcaSubscription {
                    msg_type: "subscribe".to_string(),
                    pools: None, // Subscribe to all pools initially
                })
                .unwrap_or_default(),
            ),
            ..Default::default()
        };

        Self {
            config,
            websocket: None,
            status: ConnectionStatus::Disconnected,
            metrics: WebSocketMetrics::default(),
            update_sender,
            subscribed_pools: Vec::new(),
            last_ping: None,
            reconnect_attempts: 0,
        }
    }

    /// Handle incoming WebSocket messages
    async fn handle_message(&mut self, message: Message) -> Result<()> {
        self.metrics.total_messages_received += 1;
        self.metrics.last_message_timestamp = Some(chrono::Utc::now().timestamp_millis() as u64);

        match message {
            Message::Text(text) => {
                debug!("📨 Received Orca message: {}", text);
                
                match self.parse_message(&text) {
                    Ok(updates) => {
                        for update in updates {
                            if update.is_fresh() {
                                self.metrics.total_price_updates += 1;
                                
                                // Send update to price feed manager
                                if let Err(e) = self.update_sender.send(update) {
                                    error!("Failed to send Orca price update: {}", e);
                                }
                            } else {
                                self.metrics.stale_message_count += 1;
                                warn!("🚨 Received stale Orca price data (age: {}ms)", update.age_ms());
                            }
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to parse Orca message: {}", e);
                    }
                }
            }
            Message::Ping(payload) => {
                debug!("📡 Received ping from Orca, sending pong");
                if let Some(ref mut ws) = self.websocket {
                    let _ = ws.send(Message::Pong(payload)).await;
                }
                self.last_ping = Some(Instant::now());
            }
            Message::Pong(_) => {
                debug!("📡 Received pong from Orca");
                self.last_ping = Some(Instant::now());
            }
            Message::Close(close_frame) => {
                warn!("🔌 Orca WebSocket connection closed: {:?}", close_frame);
                self.status = ConnectionStatus::Disconnected;
                self.websocket = None;
            }
            Message::Binary(data) => {
                debug!("📦 Received binary message from Orca: {} bytes", data.len());
                // Orca typically uses text messages, but handle binary if needed
            }
            Message::Frame(_) => {
                // Handle raw frame messages (typically not used in application layer)
                debug!("🔧 Received raw frame from Orca WebSocket");
            }
        }

        Ok(())
    }

    /// Main message processing loop
    async fn message_loop(&mut self) -> Result<()> {
        let mut ping_interval = interval(Duration::from_millis(self.config.ping_interval_ms));
        
        while let Some(ref mut ws) = self.websocket {
            tokio::select! {
                // Handle incoming messages
                message = ws.next() => {
                    match message {
                        Some(Ok(msg)) => {
                            if let Err(e) = self.handle_message(msg).await {
                                error!("Error handling Orca message: {}", e);
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error from Orca: {}", e);
                            self.status = ConnectionStatus::Failed;
                            break;
                        }
                        None => {
                            warn!("Orca WebSocket stream ended");
                            self.status = ConnectionStatus::Disconnected;
                            break;
                        }
                    }
                }
                
                // Send periodic pings
                _ = ping_interval.tick() => {
                    if let Some(ref mut ws) = self.websocket {
                        if let Err(e) = ws.send(Message::Ping(vec![])).await {
                            error!("Failed to send ping to Orca: {}", e);
                            self.status = ConnectionStatus::Failed;
                            break;
                        }
                        debug!("📡 Sent ping to Orca");
                    }
                }
            }
        }

        // If we reach here, connection was lost
        self.websocket = None;
        if self.status == ConnectionStatus::Connected {
            self.status = ConnectionStatus::Disconnected;
        }

        Ok(())
    }

    /// Attempt to reconnect with exponential backoff
    async fn reconnect(&mut self) -> Result<()> {
        if self.reconnect_attempts >= self.config.max_reconnect_attempts {
            error!("🚨 Max reconnection attempts reached for Orca WebSocket");
            self.status = ConnectionStatus::Failed;
            return Err(anyhow!("Max reconnection attempts exceeded"));
        }

        self.status = ConnectionStatus::Reconnecting;
        self.reconnect_attempts += 1;
        self.metrics.total_reconnections += 1;

        let delay = std::cmp::min(
            self.config.reconnect_delay_ms * (2_u64.pow(self.reconnect_attempts - 1)),
            30000, // Max 30 seconds delay
        );

        warn!("🔄 Attempting to reconnect to Orca WebSocket (attempt {}/{}) in {}ms", 
              self.reconnect_attempts, self.config.max_reconnect_attempts, delay);

        tokio::time::sleep(Duration::from_millis(delay)).await;

        match self.connect().await {
            Ok(_) => {
                info!("✅ Successfully reconnected to Orca WebSocket");
                self.reconnect_attempts = 0; // Reset on successful connection
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to reconnect to Orca WebSocket: {}", e);
                Err(e)
            }
        }
    }

    /// Convert Orca sqrt price to regular price
    fn sqrt_price_to_price(sqrt_price: &str, decimals_a: u8, decimals_b: u8) -> Result<f64> {
        let sqrt_price_u128: u128 = sqrt_price.parse()
            .map_err(|e| anyhow!("Invalid sqrt_price format: {}", e))?;
        
        // Orca uses Q64.64 format for sqrt price
        let sqrt_price_f64 = (sqrt_price_u128 as f64) / (1u128 << 64) as f64;
        let price = sqrt_price_f64 * sqrt_price_f64;
        
        // Adjust for token decimals
        let decimal_adjustment = 10_f64.powi(decimals_b as i32 - decimals_a as i32);
        Ok(price * decimal_adjustment)
    }
}

#[async_trait]
impl WebSocketFeed for OrcaWebSocketFeed {
    fn dex_type(&self) -> DexType {
        DexType::Orca
    }

    async fn connect(&mut self) -> Result<()> {
        info!("🔌 Connecting to Orca account monitoring WebSocket: {}", self.config.url);
        self.status = ConnectionStatus::Connecting;
        self.reconnect_attempts = 0;

        let url = Url::parse(&self.config.url)?;
        
        match timeout(Duration::from_secs(10), connect_async(url)).await {
            Ok(Ok((ws_stream, response))) => {
                info!("✅ Connected to Orca WebSocket: {}", response.status());
                self.websocket = Some(ws_stream);
                self.status = ConnectionStatus::Connected;
                self.metrics.total_reconnections += 1;

                Ok(())
            }
            Ok(Err(e)) => {
                error!("❌ WebSocket connection failed: {}", e);
                self.status = ConnectionStatus::Failed;
                Err(e.into())
            }
            Err(_) => {
                error!("❌ Connection timeout");
                self.status = ConnectionStatus::Failed;
                Err(anyhow!("Connection timeout"))
            }
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("🔌 Disconnecting from Orca WebSocket");
        
        if let Some(mut ws) = self.websocket.take() {
            let _ = ws.close(None).await;
        }
        
        self.status = ConnectionStatus::Disconnected;
        info!("✅ Disconnected from Orca WebSocket");
        Ok(())
    }

    fn status(&self) -> ConnectionStatus {
        self.status.clone()
    }

    async fn subscribe_to_pools(&mut self, pool_addresses: Vec<String>) -> Result<()> {
        info!("📋 Subscribing to {} Orca pools", pool_addresses.len());
        
        let subscription = OrcaSubscription {
            msg_type: "subscribe".to_string(),
            pools: Some(pool_addresses.clone()),
        };

        let subscription_msg = serde_json::to_string(&subscription)?;

        if let Some(ref mut ws) = self.websocket {
            ws.send(Message::Text(subscription_msg)).await?;
            self.subscribed_pools = pool_addresses;
            info!("✅ Successfully subscribed to Orca pools");
        } else {
            warn!("⚠️ Cannot subscribe to pools - Orca WebSocket not connected");
            return Err(anyhow!("WebSocket not connected"));
        }

        Ok(())
    }

    fn parse_message(&self, message: &str) -> Result<Vec<PriceUpdate>> {
        self.parse_account_notification(message)
    }

    fn get_metrics(&self) -> WebSocketMetrics {
        self.metrics.clone()
    }
}

impl OrcaWebSocketFeed {
    /// Parse account change notifications from Solana RPC WebSocket
    fn parse_account_notification(&self, message: &str) -> Result<Vec<PriceUpdate>> {
        // Try to parse as account notification first
        if let Ok(notification) = serde_json::from_str::<OrcaAccountNotification>(message) {
            return self.handle_account_update(notification);
        }

        // Handle subscription confirmations and other messages
        if message.contains("\"result\"") && message.contains("\"subscription\"") {
            debug!("📋 Received subscription confirmation for Orca account monitoring");
            return Ok(vec![]);
        }

        // Log unhandled messages for debugging
        debug!("🔍 Unhandled Orca message: {}", message);
        Ok(vec![])
    }

    /// Convert account update to price update
    fn handle_account_update(&self, notification: OrcaAccountNotification) -> Result<Vec<PriceUpdate>> {
        let account_data = &notification.params.result.value.account.data;
        let pool_address = &notification.params.result.value.pubkey;
        let slot = notification.params.result.context.slot;

        // Decode base64 account data
        let decoded_data = if !account_data.is_empty() {
            use base64::{Engine as _, engine::general_purpose};
            general_purpose::STANDARD.decode(&account_data[0])
                .map_err(|e| anyhow!("Failed to decode account data: {}", e))?
        } else {
            return Err(anyhow!("No account data in notification"));
        };

        // Parse Whirlpool account structure (simplified version)
        if decoded_data.len() < 200 {
            return Err(anyhow!("Account data too small for Whirlpool: {} bytes", decoded_data.len()));
        }

        // Parse basic Whirlpool structure
        let price_update = self.parse_whirlpool_account(&decoded_data, pool_address, slot)?;
        
        Ok(vec![price_update])
    }

    /// Parse Whirlpool account data into PriceUpdate (simplified implementation)
    fn parse_whirlpool_account(&self, data: &[u8], pool_address: &str, slot: u64) -> Result<PriceUpdate> {
        // NOTE: This is a simplified parser for demonstration
        // In production, use the official Orca Whirlpool SDK for proper parsing
        
        if data.len() < 200 {
            return Err(anyhow!("Insufficient data for Whirlpool parsing"));
        }

        // For now, create a mock price update
        // In production, you would parse the actual Whirlpool account structure
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        
        // Mock data - in production, extract from account data
        let mock_price = 1.0 + (slot as f64 % 100.0) / 10000.0; // Simulated price variation
        
        Ok(PriceUpdate {
            pool_address: pool_address.to_string(),
            dex_type: DexType::Orca,
            token_a_mint: "So11111111111111111111111111111111111111112".to_string(), // SOL
            token_b_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
            token_a_reserve: 0, // Would extract from token vault accounts
            token_b_reserve: 0, // Would extract from token vault accounts
            price_a_to_b: mock_price,
            price_b_to_a: 1.0 / mock_price,
            timestamp,
            liquidity: Some(1000000), // Mock liquidity
            volume_24h: None,
        })
    }
}

// Note: We need to implement Clone for the spawn in connect()
// This is a simplified implementation - in production you might want a different approach
impl Clone for OrcaWebSocketFeed {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            websocket: None, // Don't clone the actual WebSocket connection
            status: self.status.clone(),
            metrics: self.metrics.clone(),
            update_sender: self.update_sender.clone(),
            subscribed_pools: self.subscribed_pools.clone(),
            last_ping: self.last_ping,
            reconnect_attempts: self.reconnect_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_orca_websocket_feed_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let feed = OrcaWebSocketFeed::new(tx);
        
        assert_eq!(feed.dex_type(), DexType::Orca);
        assert_eq!(feed.status(), ConnectionStatus::Disconnected);
        assert_eq!(feed.config.url, ORCA_RPC_WS_URL);
    }

    #[test]
    fn test_sqrt_price_conversion() {
        // Test with known values
        let result = OrcaWebSocketFeed::sqrt_price_to_price("18446744073709551616", 9, 6);
        assert!(result.is_ok());
        
        // Test with invalid sqrt_price
        let result = OrcaWebSocketFeed::sqrt_price_to_price("invalid", 9, 6);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_subscription_message_format() {
        let pools = vec!["pool1".to_string(), "pool2".to_string()];
        let subscription = OrcaSubscription {
            msg_type: "subscribe".to_string(),
            pools: Some(pools),
        };
        
        let json = serde_json::to_string(&subscription).unwrap();
        assert!(json.contains("subscribe"));
        assert!(json.contains("pool1"));
        assert!(json.contains("pool2"));
    }
}
