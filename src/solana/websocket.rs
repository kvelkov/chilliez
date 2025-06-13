// src/solana/websocket.rs

use crate::error::ArbError;
use futures_util::{stream::StreamExt, SinkExt};
use log::{info, error, warn, debug};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use base64::{Engine as _, engine::general_purpose};

#[derive(Debug, Clone)]
pub struct RawAccountUpdate {
    pub pubkey: Pubkey,
    pub data: Vec<u8>,
    pub timestamp: u64,
    pub slot: Option<u64>,
    pub lamports: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct WebSocketMetrics {
    pub total_updates_received: u64,
    pub successful_parses: u64,
    pub failed_parses: u64,
    pub connection_count: u64,
    pub last_update_timestamp: u64,
    pub average_update_latency_ms: f64,
}

pub struct SolanaWebsocketManager {
    ws_url: String,
    ws_sender: Option<tokio::sync::mpsc::Sender<Message>>,
    updates_tx: broadcast::Sender<RawAccountUpdate>,
    _handle: Option<JoinHandle<()>>,
    metrics: Arc<tokio::sync::Mutex<WebSocketMetrics>>,
    subscription_count: Arc<tokio::sync::Mutex<usize>>,
}

impl SolanaWebsocketManager {
    pub fn new(ws_url: String) -> (Self, broadcast::Receiver<RawAccountUpdate>) {
        let (updates_tx, updates_rx) = broadcast::channel(2048); // Increased buffer for high-frequency updates
        
        let metrics = Arc::new(tokio::sync::Mutex::new(WebSocketMetrics {
            total_updates_received: 0,
            successful_parses: 0,
            failed_parses: 0,
            connection_count: 0,
            last_update_timestamp: 0,
            average_update_latency_ms: 0.0,
        }));
        
        (
            Self {
                ws_url,
                ws_sender: None,
                updates_tx,
                _handle: None,
                metrics,
                subscription_count: Arc::new(tokio::sync::Mutex::new(0)),
            },
            updates_rx,
        )
    }

    pub async fn start(&mut self) -> Result<(), ArbError> {
        info!("🌐 Connecting to enhanced WebSocket server: {}", self.ws_url);
        
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .map_err(|e| ArbError::WebSocketError(format!("Failed to connect: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();
        let (mpsc_tx, mut mpsc_rx) = tokio::sync::mpsc::channel::<Message>(256);

        self.ws_sender = Some(mpsc_tx);

        // Update connection metrics
        {
            let mut metrics = self.metrics.lock().await;
            metrics.connection_count += 1;
        }

        // Task to forward messages from our MPSC channel to the WebSocket sink
        tokio::spawn(async move {
            while let Some(msg) = mpsc_rx.recv().await {
                if write.send(msg).await.is_err() {
                    error!("❌ WebSocket write error. Closing connection.");
                    break;
                }
            }
        });

        // Enhanced task to read messages from the WebSocket and broadcast them
        let updates_tx_clone = self.updates_tx.clone();
        let metrics_clone = self.metrics.clone();
        
        self._handle = Some(tokio::spawn(async move {
            info!("🔄 Enhanced WebSocket read loop started with performance monitoring.");
            let mut update_count = 0u64;
            let mut parse_success_count = 0u64;
            let mut parse_failure_count = 0u64;
            
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    update_count += 1;
                    let parse_start = std::time::Instant::now();
                    
                    // Enhanced parsing for account notifications
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            if json["method"] == "accountNotification" {
                                if let Some(params) = json["params"].as_object() {
                                    if let Some(result) = params["result"].as_object() {
                                        if let Some(value) = result["value"].as_object() {
                                            // Extract enhanced data
                                            let pubkey_str = value["pubkey"].as_str();
                                            let data_array = value["data"].as_array();
                                            let lamports = value["lamports"].as_u64();
                                            let slot = result["context"]["slot"].as_u64();
                                            
                                            if let (Some(pubkey_str), Some(data_array)) = (pubkey_str, data_array) {
                                                if let Some(data_str) = data_array[0].as_str() {
                                                    match (pubkey_str.parse::<Pubkey>(), general_purpose::STANDARD.decode(data_str)) {
                                                        (Ok(pubkey), Ok(data)) => {
                                                            let update = RawAccountUpdate {
                                                                pubkey,
                                                                data,
                                                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                                                slot,
                                                                lamports,
                                                            };
                                                            
                                                            if let Err(e) = updates_tx_clone.send(update) {
                                                                warn!("Failed to broadcast account update: {}", e);
                                                            } else {
                                                                parse_success_count += 1;
                                                            }
                                                        }
                                                        _ => {
                                                            parse_failure_count += 1;
                                                            debug!("Failed to parse pubkey or data for update");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            parse_failure_count += 1;
                            debug!("Failed to parse WebSocket message as JSON");
                        }
                    }
                    
                    // Update metrics periodically
                    if update_count % 1000 == 0 {
                        let parse_duration = parse_start.elapsed();
                        let mut metrics = metrics_clone.lock().await;
                        metrics.total_updates_received = update_count;
                        metrics.successful_parses = parse_success_count;
                        metrics.failed_parses = parse_failure_count;
                        metrics.last_update_timestamp = chrono::Utc::now().timestamp_millis() as u64;
                        metrics.average_update_latency_ms = parse_duration.as_millis() as f64;
                        
                        info!("📊 WebSocket metrics: {} updates processed ({} success, {} failed)", 
                              update_count, parse_success_count, parse_failure_count);
                    }
                }
            }
            info!("🔚 Enhanced WebSocket read loop finished after processing {} updates.", update_count);
        }));

        info!("✅ Enhanced WebSocket manager started successfully with performance monitoring.");
        Ok(())
    }

    pub async fn subscribe_to_pools(&self, pool_addresses: Vec<Pubkey>) -> Result<(), ArbError> {
        let sender = self.ws_sender.as_ref().ok_or_else(|| {
            ArbError::WebSocketError("WebSocket sender not available.".to_string())
        })?;
        
        let pool_count = pool_addresses.len();
        info!("📡 Subscribing to {} pool addresses for real-time updates...", pool_count);
        
        let mut successful_subscriptions = 0;
        let mut failed_subscriptions = 0;
        
        // Subscribe in batches to avoid overwhelming the WebSocket
        const BATCH_SIZE: usize = 100;
        for (batch_idx, batch) in pool_addresses.chunks(BATCH_SIZE).enumerate() {
            info!("📡 Processing subscription batch {} ({} pools)", batch_idx + 1, batch.len());
            
            for pubkey in batch {
                let subscribe_msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": format!("pool_sub_{}", pubkey),
                    "method": "accountSubscribe",
                    "params": [
                        pubkey.to_string(),
                        {
                            "encoding": "base64",
                            "commitment": "processed"
                        }
                    ]
                });
                
                match sender.send(Message::Text(subscribe_msg.to_string())).await {
                    Ok(_) => successful_subscriptions += 1,
                    Err(e) => {
                        failed_subscriptions += 1;
                        warn!("Failed to send subscription for {}: {}", pubkey, e);
                    }
                }
            }
            
            // Small delay between batches to avoid rate limiting
            if batch_idx < pool_addresses.chunks(BATCH_SIZE).len() - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        
        // Update subscription count
        {
            let mut count = self.subscription_count.lock().await;
            *count = successful_subscriptions;
        }
        
        info!("✅ Subscription complete: {} successful, {} failed out of {} total pools", 
              successful_subscriptions, failed_subscriptions, pool_count);
        
        if failed_subscriptions > 0 {
            warn!("⚠️  {} subscriptions failed - some pools may not receive real-time updates", 
                  failed_subscriptions);
        }
        
        Ok(())
    }

    pub async fn get_metrics(&self) -> WebSocketMetrics {
        self.metrics.lock().await.clone()
    }

    pub async fn get_subscription_count(&self) -> usize {
        *self.subscription_count.lock().await
    }

    pub async fn stop(&mut self) {
        info!("🛑 Stopping enhanced WebSocket manager...");
        
        // Close the sender to signal shutdown
        if let Some(sender) = self.ws_sender.take() {
            drop(sender);
        }
        
        // Wait for the handle to finish if it exists
        if let Some(handle) = self._handle.take() {
            let _ = handle.await;
        }
        
        info!("✅ Enhanced WebSocket manager stopped gracefully");
    }
}