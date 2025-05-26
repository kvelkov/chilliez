use base64::Engine;
use futures::StreamExt;
use log::{debug, error, info, warn};
use solana_account_decoder::{UiAccountData, UiAccountEncoding};
use solana_client::nonblocking::pubsub_client::{PubsubClient, PubsubClientError};
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::utils::PoolInfo;

pub type SubscriptionId = u64;

pub type AccountUpdateSender = broadcast::Sender<RawAccountUpdate>;

#[derive(Debug, Clone)]
pub enum RawAccountUpdate {
    Account {
        pubkey: Pubkey,
        data: Vec<u8>,
        timestamp: u64,
    },
    Disconnected {
        pubkey: Pubkey,
        timestamp: u64,
    },
    Error {
        pubkey: Pubkey,
        message: String,
        timestamp: u64,
    },
}

impl RawAccountUpdate {
    pub fn pubkey(&self) -> &Pubkey {
        match self {
            RawAccountUpdate::Account { pubkey, .. }
            | RawAccountUpdate::Disconnected { pubkey, .. }
            | RawAccountUpdate::Error { pubkey, .. } => pubkey,
        }
    }
    pub fn data(&self) -> Option<&Vec<u8>> {
        match self {
            RawAccountUpdate::Account { data, .. } => Some(data),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum WebsocketUpdate {
    PoolUpdate(PoolInfo),
    GenericUpdate(String),
}

pub struct SolanaWebsocketManager {
    ws_url: String,
    fallback_urls: Vec<String>, // TODO: Enable fallback WS logic for resilience
    subscriptions: Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
    update_sender: AccountUpdateSender,
    shutdown_sender: mpsc::Sender<()>, // TODO: Expose graceful shutdown in integration/tests
    heartbeat_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    pubsub_client: Arc<RwLock<Option<Arc<PubsubClient>>>>,
    // Added for main.rs
    ws_update_receiver: Option<mpsc::Receiver<WebsocketUpdate>>,
    ws_update_sender: Option<mpsc::Sender<WebsocketUpdate>>,
}

impl SolanaWebsocketManager {
    /// Create new manager
    pub fn new(
        ws_url: String,
        fallback_urls: Vec<String>,
        update_channel_size: usize,
    ) -> (Self, broadcast::Receiver<RawAccountUpdate>) {
        let (update_sender, update_receiver) = broadcast::channel(update_channel_size);
        let (shutdown_sender, _) = mpsc::channel(1);
        // Added for main.rs
        let (ws_update_sender, ws_update_receiver) = mpsc::channel(update_channel_size);
        (
            Self {
                ws_url,
                fallback_urls,
                subscriptions: Arc::new(RwLock::new(HashMap::new())),
                update_sender,
                shutdown_sender,
                heartbeat_handle: Arc::new(RwLock::new(None)),
                pubsub_client: Arc::new(RwLock::new(None)),
                // Added for main.rs
                ws_update_receiver: Some(ws_update_receiver),
                ws_update_sender: Some(ws_update_sender),
            },
            update_receiver,
        )
    }

    // Added for main.rs
    pub async fn is_connected(&self) -> bool {
        self.pubsub_client.read().await.is_some()
    }

    // Added for main.rs - Placeholder implementation
    pub async fn try_recv_update(&mut self) -> Result<Option<WebsocketUpdate>, mpsc::error::TryRecvError> {
        if let Some(ref mut receiver) = self.ws_update_receiver {
            receiver.try_recv().map(Some)
        } else {
            Ok(None) // Or an appropriate error
        }
    }

    pub fn update_sender(&self) -> &AccountUpdateSender {
        &self.update_sender
    }

    /// Start manager and heartbeat
    pub async fn start(&self) -> Result<(), PubsubClientError> {
        match self.reconnect().await {
            Ok(_) => info!("[WebSocket] Connected to {}", self.ws_url),
            Err(e) => {
                error!(
                    "[WebSocket] Initial connection to {} failed: {}",
                    self.ws_url, e
                );
                return Err(e);
            }
        }
        let subscriptions = self.subscriptions.clone();
        let update_sender = self.update_sender.clone();
        let pubsub_client = self.pubsub_client.clone();
        let ws_url = self.ws_url.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                if let Some(_client) = pubsub_client.read().await.clone() {
                    debug!("[WebSocket] Heartbeat: connection to {} is alive", ws_url);
                } else {
                    warn!(
                        "[WebSocket] Disconnected from {}, attempting reconnect...",
                        ws_url
                    );
                    let keys: Vec<Pubkey> = {
                        let subs = subscriptions.read().await;
                        subs.keys().copied().collect()
                    };
                    for pubkey in keys {
                        match Self::create_account_subscription(
                            Arc::clone(pubsub_client.read().await.as_ref().unwrap()),
                            pubkey,
                            update_sender.clone(),
                        )
                        .await
                        {
                            Ok(sub_id) => {
                                subscriptions.write().await.insert(pubkey, sub_id);
                                info!("[WebSocket] Resubscribed to account {}", pubkey);
                            }
                            Err(e) => error!("[WebSocket] Failed to resubscribe {}: {}", pubkey, e),
                        }
                    }
                }
            }
        });
        *self.heartbeat_handle.write().await = Some(handle);
        Ok(())
    }

    async fn reconnect(&self) -> Result<(), PubsubClientError> {
        match PubsubClient::new(&self.ws_url).await {
            Ok(client) => {
                let client = Arc::new(client);
                *self.pubsub_client.write().await = Some(client);
                info!("[WebSocket] Connected to {}", self.ws_url);
                Ok(())
            }
            Err(e) => {
                error!("[WebSocket] Connection to {} failed: {}", self.ws_url, e);
                Err(e)
            }
        }
    }

    #[allow(dead_code)]
    async fn try_reconnect_and_resubscribe(
        ws_url: &str,
        fallback_urls: &[&str],
        subscriptions: &Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
        update_sender: &AccountUpdateSender,
        pubsub_client: &Arc<RwLock<Option<Arc<PubsubClient>>>>,
    ) -> Result<(), PubsubClientError> {
        for url in std::iter::once(ws_url).chain(fallback_urls.iter().copied()) {
            match PubsubClient::new(url).await {
                Ok(client) => {
                    info!("Reconnected WebSocket to {}", url);
                    let client = Arc::new(client);
                    *pubsub_client.write().await = Some(Arc::clone(&client));
                    // Collect keys to iterate.
                    let keys: Vec<Pubkey> = {
                        let subs = subscriptions.read().await;
                        subs.keys().copied().collect()
                    };
                    for pubkey in keys {
                        if let Ok(sub_id) = Self::create_account_subscription(
                            Arc::clone(&client),
                            pubkey,
                            update_sender.clone(),
                        )
                        .await
                        {
                            subscriptions.write().await.insert(pubkey, sub_id);
                        }
                    }
                    return Ok(());
                }
                Err(err) => {
                    error!("Failed to reconnect to {}: {}", url, err);
                }
            }
        }
        Err(PubsubClientError::ConnectionError(
            std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "Failed to connect to any WebSocket endpoint",
            )
            .into(),
        ))
    }

    /// Stop background tasks
    pub async fn stop(&self) {
        // TODO: Wire stop logic as needed in workflow
        if let Some(handle) = self.heartbeat_handle.write().await.take() {
            handle.abort();
            info!("WebSocket heartbeat task stopped");
        }
    }

    /// Subscribe to account updates
    pub async fn subscribe_to_account(&self, pubkey: Pubkey) -> Result<(), PubsubClientError> {
        let client = match self.pubsub_client.read().await.clone() {
            Some(client) => client,
            None => {
                self.reconnect().await?;
                self.pubsub_client.read().await.clone().unwrap()
            }
        };
        let sub_id =
            Self::create_account_subscription(client, pubkey, self.update_sender.clone()).await?;
        self.subscriptions.write().await.insert(pubkey, sub_id);
        Ok(())
    }

    async fn create_account_subscription(
        client: Arc<PubsubClient>,
        pubkey: Pubkey,
        sender: AccountUpdateSender,
    ) -> Result<SubscriptionId, PubsubClientError> {
        tokio::spawn(async move {
            info!("[WebSocket] Subscribing to account {}", pubkey);
            let (account_stream, _unsubscribe) = match client
                .account_subscribe(
                    &pubkey,
                    Some(RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        commitment: Some(CommitmentConfig::confirmed()),
                        min_context_slot: None,
                        data_slice: None,
                    }),
                )
                .await
            {
                Ok(res) => {
                    info!("[WebSocket] Subscribed to account {}", pubkey);
                    res
                }
                Err(e) => {
                    error!("[WebSocket] Failed to subscribe to {}: {}", pubkey, e);
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let _ = sender.send(RawAccountUpdate::Error {
                        pubkey,
                        message: format!("Subscription failed: {}", e),
                        timestamp,
                    });
                    return;
                }
            };
            let mut stream = account_stream;
            while let Some(response) = stream.next().await {
                debug!("[WebSocket] Received update for {}", pubkey);
                let decoded_data = match &response.value.data {
                    UiAccountData::Binary(encoded, _) => {
                        match base64::engine::general_purpose::STANDARD.decode(encoded) {
                            Ok(data) => {
                                info!(
                                    "[WebSocket] Successfully decoded and parsed data for {}",
                                    pubkey
                                );
                                data
                            }
                            Err(e) => {
                                error!("[WebSocket] Base64 decode error for {}: {}", pubkey, e);
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                let _ = sender.send(RawAccountUpdate::Error {
                                    pubkey,
                                    message: format!("Base64 decode error: {}", e),
                                    timestamp,
                                });
                                continue;
                            }
                        }
                    }
                    _ => {
                        warn!("[WebSocket] Non-binary data for {} ignored", pubkey);
                        continue;
                    }
                };
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let update = RawAccountUpdate::Account {
                    pubkey,
                    data: decoded_data,
                    timestamp,
                };
                match sender.send(update) {
                    Ok(_) => info!("[WebSocket] Broadcasted update for {}", pubkey),
                    Err(e) => error!(
                        "[WebSocket] Failed to broadcast update for {}: {}",
                        pubkey, e
                    ),
                }
            }
            warn!("[WebSocket] Subscription stream for {} ended", pubkey);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let disconnect_update = RawAccountUpdate::Disconnected { pubkey, timestamp };
            let _ = sender.send(disconnect_update);
        });
        Ok(0)
    }

    /// Unsubscribe locally (remote unsub not supported)
    pub async fn unsubscribe(&self, pubkey: &Pubkey) -> Result<(), PubsubClientError> {
        // TODO: Expose in live workflow
        let mut subscriptions = self.subscriptions.write().await;
        if subscriptions.remove(pubkey).is_some() {
            info!("Unsubscribed from account updates for {}", pubkey);
        }
        Ok(())
    }
}
