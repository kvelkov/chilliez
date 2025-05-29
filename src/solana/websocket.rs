use crate::{
    cache::Cache,
    dex::pool::get_pool_parser_for_program,
    solana::rpc::SolanaRpcClient,
    utils::PoolInfo,
};
use base64::Engine;
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
use solana_client::{
    nonblocking::pubsub_client::{PubsubClient, PubsubClientError},
    rpc_config::RpcAccountInfoConfig,
    rpc_response,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::{
    collections::HashMap,
    io::{Error as IoError, ErrorKind},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tungstenite::error::Error as WsError;

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
    pub fn _data(&self) -> Option<&Vec<u8>> {
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
    fallback_urls: Vec<String>,
    subscriptions: Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
    update_sender: AccountUpdateSender,
    shutdown_sender: mpsc::Sender<()>,
    shutdown_receiver: Arc<Mutex<mpsc::Receiver<()>>>,
    heartbeat_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    pubsub_client: Arc<RwLock<Option<Arc<PubsubClient>>>>,
    processed_update_sender: mpsc::Sender<WebsocketUpdate>,
    processed_update_receiver: Arc<Mutex<mpsc::Receiver<WebsocketUpdate>>>,
    rpc_client: Arc<SolanaRpcClient>,
    cache: Option<Arc<Cache>>,
}

impl SolanaWebsocketManager {
    pub fn new(
        ws_url: String,
        fallback_urls: Vec<String>,
        update_channel_size: usize,
        rpc_client: Arc<SolanaRpcClient>,
        cache: Option<Arc<Cache>>,
    ) -> (Self, broadcast::Receiver<RawAccountUpdate>) {
        let (raw_update_sender, raw_update_receiver) = broadcast::channel(update_channel_size);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (processed_update_tx, processed_update_rx) = mpsc::channel(update_channel_size);

        (
            Self {
                ws_url,
                fallback_urls,
                subscriptions: Arc::new(RwLock::new(HashMap::new())),
                update_sender: raw_update_sender,
                shutdown_sender: shutdown_tx,
                shutdown_receiver: Arc::new(Mutex::new(shutdown_rx)),
                heartbeat_handle: Arc::new(RwLock::new(None)),
                pubsub_client: Arc::new(RwLock::new(None)),
                processed_update_sender: processed_update_tx,
                processed_update_receiver: Arc::new(Mutex::new(processed_update_rx)),
                rpc_client,
                cache,
            },
            raw_update_receiver,
        )
    }

    pub async fn is_connected(&self) -> bool {
        self.pubsub_client.read().await.is_some()
    }

    pub async fn try_recv_update(&self) -> Result<Option<WebsocketUpdate>, mpsc::error::TryRecvError> {
        match self.processed_update_receiver.lock().await.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn start(&self) -> Result<(), PubsubClientError> {
        match self.reconnect().await {
            Ok(_) => info!("[WebSocket] Connected to {}", self.ws_url),
            Err(e) => {
                error!("[WebSocket] Initial connection to {} failed: {}", self.ws_url, e);
                return Err(e);
            }
        }

        let subscriptions_clone = self.subscriptions.clone();
        let raw_update_sender_clone = self.update_sender.clone();
        let pubsub_client_clone = self.pubsub_client.clone();
        let ws_url_clone = self.ws_url.clone();
        let fallback_urls_clone = self.fallback_urls.clone();
        let rpc_client_clone = self.rpc_client.clone();
        let cache_clone = self.cache.clone();
        let processed_update_sender_clone = self.processed_update_sender.clone();
        let shutdown_receiver_arc_clone = Arc::clone(&self.shutdown_receiver);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if pubsub_client_clone.read().await.is_some() {
                            debug!("[WebSocket] Heartbeat: connection to {} is alive", ws_url_clone);
                        } else {
                            warn!("[WebSocket] Disconnected from {}, attempting reconnect and resubscribe...", ws_url_clone);
                            let fallback_urls_str_slices: Vec<&str> = fallback_urls_clone.iter().map(|s| s.as_str()).collect();
                            if let Err(e) = Self::try_reconnect_and_resubscribe(
                                &ws_url_clone,
                                &fallback_urls_str_slices,
                                &subscriptions_clone,
                                &raw_update_sender_clone,
                                &pubsub_client_clone,
                                &rpc_client_clone,
                                cache_clone.clone(),
                                processed_update_sender_clone.clone(),
                            ).await {
                                error!("[WebSocket] Failed to reconnect and resubscribe: {}", e);
                            }
                        }
                    }
                    _ = async { shutdown_receiver_arc_clone.lock().await.recv().await }, if true => {
                        info!("[WebSocket] Heartbeat task received shutdown signal. Exiting.");
                        break;
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
                *self.pubsub_client.write().await = Some(Arc::new(client));
                info!("[WebSocket] Connected to primary URL: {}", self.ws_url);
                return Ok(());
            }
            Err(e) => {
                error!("[WebSocket] Failed to connect to primary URL {}: {}", self.ws_url, e);
            }
        }

        for fallback_url in &self.fallback_urls {
            warn!("[WebSocket] Attempting connection to fallback URL: {}", fallback_url);
            match PubsubClient::new(fallback_url).await {
                Ok(client) => {
                    *self.pubsub_client.write().await = Some(Arc::new(client));
                    info!("[WebSocket] Connected to fallback URL: {}", fallback_url);
                    return Ok(());
                }
                Err(e) => {
                    error!("[WebSocket] Failed to connect to fallback URL {}: {}", fallback_url, e);
                }
            }
        }
        let custom_err = IoError::new(ErrorKind::Other, "Failed to connect to any WebSocket endpoint");
        Err(PubsubClientError::ConnectionError(WsError::Io(custom_err)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn try_reconnect_and_resubscribe(
        ws_url: &str,
        fallback_urls: &[&str],
        subscriptions_map: &Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
        raw_update_sender_channel: &AccountUpdateSender,
        pubsub_client_arc_option: &Arc<RwLock<Option<Arc<PubsubClient>>>>,
        rpc_client: &Arc<SolanaRpcClient>,
        cache: Option<Arc<Cache>>,
        processed_update_sender_channel: mpsc::Sender<WebsocketUpdate>,
    ) -> Result<(), PubsubClientError> {
        let mut connected_client: Option<Arc<PubsubClient>> = None;

        for url_attempt in std::iter::once(ws_url).chain(fallback_urls.iter().copied()) {
            match PubsubClient::new(url_attempt).await {
                Ok(new_client) => {
                    info!("[WebSocket] Reconnected to {}", url_attempt);
                    connected_client = Some(Arc::new(new_client));
                    break;
                }
                Err(err) => {
                    error!("[WebSocket] Failed to reconnect to {}: {}", url_attempt, err);
                }
            }
        }

        if let Some(new_client_arc) = connected_client {
            *pubsub_client_arc_option.write().await = Some(Arc::clone(&new_client_arc));
            let keys_to_resubscribe: Vec<Pubkey> = { subscriptions_map.read().await.keys().copied().collect() };

            for pubkey_to_resub in keys_to_resubscribe {
                info!("[WebSocket] Attempting to resubscribe to {}", pubkey_to_resub);
                match Self::create_account_subscription(
                    Arc::clone(&new_client_arc),
                    pubkey_to_resub,
                    raw_update_sender_channel.clone(),
                    rpc_client.clone(),
                    cache.clone(),
                    processed_update_sender_channel.clone(),
                )
                .await
                {
                    Ok(sub_id) => {
                        subscriptions_map.write().await.insert(pubkey_to_resub, sub_id);
                        info!("[WebSocket] Successfully resubscribed to {}", pubkey_to_resub);
                    }
                    Err(e) => {
                        error!("[WebSocket] Failed to resubscribe to {}: {}", pubkey_to_resub, e);
                        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let _ = raw_update_sender_channel.send(RawAccountUpdate::Error {
                            pubkey: pubkey_to_resub,
                            message: format!("Resubscription failed: {}", e),
                            timestamp,
                        });
                    }
                }
            }
            Ok(())
        } else {
            let custom_err = IoError::new(ErrorKind::Other, "Failed to reconnect to any WebSocket endpoint after multiple attempts");
            Err(PubsubClientError::ConnectionError(WsError::Io(custom_err)))
        }
    }

    pub async fn stop(&self) {
        info!("[WebSocket] Attempting to stop WebSocket manager...");
        if self.shutdown_sender.send(()).await.is_err() {
            error!("[WebSocket] Failed to send shutdown signal to heartbeat task.");
        } else {
            info!("[WebSocket] Shutdown signal sent to heartbeat task.");
        }

        if let Some(handle) = self.heartbeat_handle.write().await.take() {
            info!("[WebSocket] Waiting for heartbeat task to complete...");
            if let Err(e) = handle.await {
                error!("[WebSocket] Heartbeat task panicked or encountered an error during shutdown: {:?}", e);
            } else {
                info!("[WebSocket] Heartbeat task stopped gracefully.");
            }
        }

        if let Some(client_arc) = self.pubsub_client.write().await.take() {
            info!("[WebSocket] PubSub client instance dropped. Connections should close.");
            drop(client_arc);
        }
        info!("[WebSocket] Manager stopped.");
    }

    pub async fn subscribe_to_account(&self, pubkey: Pubkey) -> Result<(), PubsubClientError> {
        let client_arc = { self.pubsub_client.read().await.clone() };

        let client_to_use = match client_arc {
            Some(client) => client,
            None => {
                warn!("[WebSocket] No active client, attempting to reconnect before subscribing to {}", pubkey);
                self.reconnect().await?;
                self.pubsub_client.read().await.clone().ok_or_else(|| {
                    let custom_err = IoError::new(ErrorKind::Other, "Failed to reconnect for subscription");
                    PubsubClientError::ConnectionError(WsError::Io(custom_err))
                })?
            }
        };

        let sub_id = Self::create_account_subscription(
            client_to_use,
            pubkey,
            self.update_sender.clone(),
            self.rpc_client.clone(),
            self.cache.clone(),
            self.processed_update_sender.clone(),
        )
        .await?;

        self.subscriptions.write().await.insert(pubkey, sub_id);
        info!("[WebSocket] Successfully initiated subscription for {}", pubkey);
        Ok(())
    }

    async fn create_account_subscription(
        client: Arc<PubsubClient>,
        pubkey: Pubkey,
        raw_sender: AccountUpdateSender,
        rpc_client: Arc<SolanaRpcClient>,
        cache: Option<Arc<Cache>>,
        processed_sender: mpsc::Sender<WebsocketUpdate>,
    ) -> Result<SubscriptionId, PubsubClientError> {
        info!("[WebSocket] Creating account subscription for {}", pubkey);

        // Only use one Arc clone and move it into the spawned task.
        let client_for_task = Arc::clone(&client);

        // Move the subscribe call into an async block so the Arc is owned by the task.
        let subscription_id = 0;

        tokio::spawn(async move {
            // Create the stream and unsubscribe_fn inside the task so the Arc lives long enough.
            let (mut account_stream, unsubscribe_fn) = client_for_task
                .account_subscribe(
                    &pubkey,
                    Some(RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        commitment: Some(CommitmentConfig::confirmed()),
                        data_slice: None,
                        min_context_slot: None,
                    }),
                )
                .await
                .expect("Failed to subscribe to account");

            let first_response_result = tokio::time::timeout(Duration::from_secs(30), account_stream.next()).await;

            let first_response: rpc_response::Response<UiAccount> = match first_response_result {
                Ok(Some(resp)) => resp,
                Ok(None) => {
                    let custom_err = IoError::new(ErrorKind::ConnectionAborted, format!("Stream ended before first message for {}", pubkey));
                    error!("[WebSocket] Stream ended before first message for {}: {}", pubkey, custom_err);
                    return;
                }
                Err(_elapsed) => {
                    let custom_err = IoError::new(ErrorKind::TimedOut, format!("Timeout on first message for {}", pubkey));
                    error!("[WebSocket] Timeout waiting for the first message for {}: {}", pubkey, custom_err);
                    return;
                }
            };

            let mut current_response_option: Option<rpc_response::Response<UiAccount>> = Some(first_response);

            loop {
                let response_result: Result<rpc_response::Response<UiAccount>, PubsubClientError> = if let Some(res) = current_response_option.take() {
                    Ok(res)
                } else {
                    match tokio::time::timeout(Duration::from_secs(60), account_stream.next()).await {
                        Ok(Some(inner_result)) => Ok(inner_result),
                        Ok(None) => {
                            warn!("[WebSocket] Subscription stream for {} (ID: {}) ended.", pubkey, subscription_id);
                            break;
                        }
                        Err(_elapsed) => {
                            debug!("[WebSocket] Timeout on stream for {} (ID: {}).", pubkey, subscription_id);
                            continue;
                        }
                    }
                };

                match response_result {
                    Ok(response) => {
                        debug!("[WebSocket] Received update for {}", pubkey);
                        let decoded_data = match &response.value.data {
                            UiAccountData::Binary(encoded, _) => {
                                match base64::engine::general_purpose::STANDARD.decode(encoded) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        error!("[WebSocket] Base64 decode error for {}: {}", pubkey, e);
                                        let _ = raw_sender.send(RawAccountUpdate::Error {
                                            pubkey,
                                            message: format!("Base64 decode error: {}", e),
                                            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
                                        });
                                        continue;
                                    }
                                }
                            }
                            _ => {
                                warn!("[WebSocket] Received non-binary data for {}, skipping.", pubkey);
                                continue;
                            }
                        };

                        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let owner_program_id_str = &response.value.owner;

                        if raw_sender.send(RawAccountUpdate::Account {
                                pubkey,
                                data: decoded_data.clone(),
                                timestamp,
                            }).is_err() {
                            error!("[WebSocket] Failed to broadcast raw update for {}.", pubkey);
                        }

                        match Pubkey::from_str(owner_program_id_str) {
                            Ok(owner_pubkey) => {
                                if let Some(parser) = get_pool_parser_for_program(&owner_pubkey) {
                                    match parser.parse_pool_data(pubkey, &decoded_data, &rpc_client).await {
                                        Ok(pool_info) => {
                                            debug!("[WebSocket] Successfully parsed pool data for {} (owner: {})", pubkey, owner_pubkey);
                                            if let Some(ref cache_instance) = cache {
                                                if let Err(e) = cache_instance.update_pool_cache(&pubkey.to_string(), &pool_info, None).await {
                                                    warn!("[WebSocket] Failed to update cache for {}: {}", pubkey, e);
                                                }
                                            }
                                            if processed_sender.send(WebsocketUpdate::PoolUpdate(pool_info)).await.is_err() {
                                                warn!("[WebSocket] Failed to send PoolUpdate for {} to internal channel.", pubkey);
                                            }
                                        }
                                        Err(e) => {
                                            debug!("[WebSocket] Parser for owner {} failed for {}: {}. Treating as generic.", owner_pubkey, pubkey, e);
                                            if processed_sender.send(WebsocketUpdate::GenericUpdate(format!("Parser error for {}: {}", pubkey, e))).await.is_err() {
                                                 warn!("[WebSocket] Failed to send GenericUpdate (parser error) for {} to internal channel.", pubkey);
                                            }
                                        }
                                    }
                                } else {
                                    debug!("[WebSocket] No parser for owner {}. Update for {} treated as generic.", owner_pubkey, pubkey);
                                     if processed_sender.send(WebsocketUpdate::GenericUpdate(format!("No parser for owner {}, account {}", owner_pubkey, pubkey))).await.is_err() {
                                         warn!("[WebSocket] Failed to send GenericUpdate (no parser) for {} to internal channel.", pubkey);
                                     }
                                }
                            }
                            Err(e) => {
                                error!("[WebSocket] Invalid owner Pubkey string '{}' for {}: {}", owner_program_id_str, pubkey, e);
                                if processed_sender.send(WebsocketUpdate::GenericUpdate(format!("Invalid owner string for {}: {}", pubkey, owner_program_id_str))).await.is_err() {
                                     warn!("[WebSocket] Failed to send GenericUpdate (invalid owner) for {} to internal channel.", pubkey);
                                }
                            }
                        }
                    }
                    Err(client_error) => {
                        error!("[WebSocket] Subscription stream for {} (ID: {}) error: {}. Ending task.", pubkey, subscription_id, client_error);
                        let _ = raw_sender.send(RawAccountUpdate::Error {
                            pubkey,
                            message: format!("Subscription stream error: {}", client_error),
                            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
                        });
                        break;
                    }
                }
            }
            warn!("[WebSocket] Subscription task for {} (ID: {}) stopped.", pubkey, subscription_id);
            let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            let _ = raw_sender.send(RawAccountUpdate::Disconnected { pubkey, timestamp });
            // The future returned by unsubscribe_fn must be awaited.
            // Assuming it's an async fn that might perform I/O for unsubscription.
            // If it's a simple closure returning (), this await might not be strictly necessary
            // but the compiler warning `unused_must_use` suggests it returns a Future.
            unsubscribe_fn().await;
            info!("[WebSocket] Unsubscribe function called for {} (ID: {})", pubkey, subscription_id);
        });

        Ok(subscription_id)
    }

    pub async fn unsubscribe(&self, pubkey: &Pubkey) -> Result<(), PubsubClientError> {
        let mut subs = self.subscriptions.write().await;
        if let Some(sub_id) = subs.remove(pubkey) {
            info!(
                "[WebSocket] Removed {} (ID: {}) from local subscriptions. Unsubscribing from client.",
                pubkey, sub_id
            );
            if let Some(_client_arc) = self.pubsub_client.read().await.clone() {
            } else {
                warn!("[WebSocket] No active client to perform explicit unsubscribe for {}.", pubkey);
            }
        } else {
            warn!("[WebSocket] Attempted to unsubscribe from {}, but it was not found.", pubkey);
        }
        Ok(())
    }
}