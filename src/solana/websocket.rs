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
use tokio::sync::{broadcast, mpsc, RwLock, Mutex};
use crate::dex::pool::get_pool_parser_fn_for_program; // Added for pool parsing
use std::str::FromStr; // Added for Pubkey::from_str
use crate::cache::Cache;
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
    fallback_urls: Vec<String>,
    subscriptions: Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
    update_sender: AccountUpdateSender,
    shutdown_sender: mpsc::Sender<()>,
    shutdown_receiver: Arc<Mutex<mpsc::Receiver<()>>>,
    heartbeat_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    pubsub_client: Arc<RwLock<Option<Arc<PubsubClient>>>>,
    ws_update_receiver: Option<mpsc::Receiver<WebsocketUpdate>>,
    ws_update_sender: Option<mpsc::Sender<WebsocketUpdate>>,
}

impl SolanaWebsocketManager {
    pub fn new(
        ws_url: String,
        fallback_urls: Vec<String>,
        update_channel_size: usize,
    ) -> (Self, broadcast::Receiver<RawAccountUpdate>) {
        let (update_sender, update_receiver) = broadcast::channel(update_channel_size);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (ws_update_sender, ws_update_receiver) = mpsc::channel(update_channel_size);
        (
            Self {
                ws_url,
                fallback_urls,
                subscriptions: Arc::new(RwLock::new(HashMap::new())),
                update_sender,
                shutdown_sender: shutdown_tx,
                shutdown_receiver: Arc::new(Mutex::new(shutdown_rx)),
                heartbeat_handle: Arc::new(RwLock::new(None)),
                pubsub_client: Arc::new(RwLock::new(None)),
                ws_update_receiver: Some(ws_update_receiver),
                ws_update_sender: Some(ws_update_sender),
            },
            update_receiver,
        )
    }

    pub async fn is_connected(&self) -> bool {
        self.pubsub_client.read().await.is_some()
    }

    pub async fn try_recv_update(
        &mut self,
    ) -> Result<Option<WebsocketUpdate>, mpsc::error::TryRecvError> {
        if let Some(ref mut receiver) = self.ws_update_receiver {
            receiver.try_recv().map(Some)
        } else {
            Ok(None)
        }
    }

    // TODO: This method is currently unused externally. Kept for potential future use.
    #[allow(dead_code)]
    pub fn _update_sender(&self) -> &AccountUpdateSender {
        &self.update_sender
    }

    pub async fn start(&self, cache: Option<Arc<Cache>>) -> Result<(), PubsubClientError> {
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
        let subscriptions_clone = self.subscriptions.clone();
        let update_sender_clone = self.update_sender.clone();
        let pubsub_client_clone = self.pubsub_client.clone();
        let ws_url_clone = self.ws_url.clone();
        let fallback_urls_clone = self.fallback_urls.clone();
        let cache_clone = cache.clone();
        let ws_update_sender_clone = self.ws_update_sender.clone();
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
                                &update_sender_clone,
                                &pubsub_client_clone,
                                cache_clone.clone(),
                                ws_update_sender_clone.clone(),
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
                let client_arc = Arc::new(client);
                *self.pubsub_client.write().await = Some(client_arc);
                info!("[WebSocket] Connected to {}", self.ws_url);
                Ok(())
            }
            Err(e) => {
                error!(
                    "[WebSocket] Connection to {} failed: {}",
                    self.ws_url, e
                );
                for fallback_url in &self.fallback_urls {
                    warn!(
                        "[WebSocket] Attempting connection to fallback URL: {}",
                        fallback_url
                    );
                    match PubsubClient::new(fallback_url).await {
                        Ok(client) => {
                            let client_arc = Arc::new(client);
                            *self.pubsub_client.write().await = Some(client_arc);
                            info!("[WebSocket] Connected to fallback {}", fallback_url);
                            return Ok(());
                        }
                        Err(fe) => {
                            error!(
                                "[WebSocket] Fallback connection to {} failed: {}",
                                fallback_url, fe
                            );
                        }
                    }
                }
                Err(e)
            }
        }
    }

    async fn try_reconnect_and_resubscribe(
        ws_url: &str,
        fallback_urls: &[&str],
        subscriptions_map: &Arc<RwLock<HashMap<Pubkey, SubscriptionId>>>,
        update_sender_channel: &AccountUpdateSender,
        pubsub_client_arc_option: &Arc<RwLock<Option<Arc<PubsubClient>>>>,
        cache_instance: Option<Arc<Cache>>,
        ws_update_sender_channel: Option<mpsc::Sender<WebsocketUpdate>>,
    ) -> Result<(), PubsubClientError> {
        for url_attempt in std::iter::once(ws_url).chain(fallback_urls.iter().copied()) {
            match PubsubClient::new(url_attempt).await {
                Ok(new_client) => {
                    info!("[WebSocket] Reconnected to {}", url_attempt);
                    let new_client_arc = Arc::new(new_client);
                    *pubsub_client_arc_option.write().await = Some(Arc::clone(&new_client_arc));
                    let keys_to_resubscribe: Vec<Pubkey> = {
                        let subs_guard = subscriptions_map.read().await;
                        subs_guard.keys().copied().collect()
                    };
                    for pubkey_to_resub in keys_to_resubscribe {
                        info!("[WebSocket] Attempting to resubscribe to {}", pubkey_to_resub);
                        match Self::create_account_subscription(
                            Arc::clone(&new_client_arc),
                            pubkey_to_resub,
                            update_sender_channel.clone(),
                            cache_instance.clone(),
                            ws_update_sender_channel.clone(),
                        ).await {
                            Ok(sub_id) => {
                                subscriptions_map.write().await.insert(pubkey_to_resub, sub_id);
                                info!("[WebSocket] Successfully resubscribed to {}", pubkey_to_resub);
                            }
                            Err(e) => {
                                error!("[WebSocket] Failed to resubscribe to {}: {}", pubkey_to_resub, e);
                                let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                                let _ = update_sender_channel.send(RawAccountUpdate::Error {
                                    pubkey: pubkey_to_resub,
                                    message: format!("Resubscription failed: {}", e),
                                    timestamp,
                                });
                            }
                        }
                    }
                    return Ok(());
                }
                Err(err) => {
                    error!("[WebSocket] Failed to reconnect to {}: {}", url_attempt, err);
                }
            }
        }
        Err(PubsubClientError::ConnectionError(
            std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "Failed to connect to any WebSocket endpoint after multiple attempts",
            )
            .into(),
        ))
    }

    pub async fn stop(&self) {
        if self.shutdown_sender.send(()).await.is_err() {
            error!("[WebSocket] Failed to send shutdown signal to heartbeat task.");
        } else {
            info!("[WebSocket] Shutdown signal sent to heartbeat task.");
        }
        if let Some(handle) = self.heartbeat_handle.write().await.take() {
            if let Err(e) = handle.await {
                error!(
                    "[WebSocket] Heartbeat task panicked or encountered an error during shutdown: {:?}",
                    e
                );
            } else {
                info!("[WebSocket] Heartbeat task stopped gracefully.");
            }
        }
        if let Some(client_arc) = self.pubsub_client.write().await.take() {
            info!("[WebSocket] Pubsub client instance dropped.");
            drop(client_arc);
        }
    }

    pub async fn subscribe_to_account(
        &self,
        pubkey: Pubkey,
        cache: Option<Arc<Cache>>,
    ) -> Result<(), PubsubClientError> {
        let client_arc = {
            let guard = self.pubsub_client.read().await;
            guard.clone()
        };
        let client_to_use = match client_arc {
            Some(client) => client,
            None => {
                self.reconnect().await?;
                self.pubsub_client
                    .read()
                    .await
                    .clone()
                    .ok_or_else(|| {
                        PubsubClientError::ConnectionError(
                            std::io::Error::new(
                                std::io::ErrorKind::NotConnected,
                                "Failed to reconnect",
                            )
                            .into(),
                        )
                    })?
            }
        };
        let sub_id = Self::create_account_subscription(
            client_to_use,
            pubkey,
            self.update_sender.clone(),
            cache,
            self.ws_update_sender.clone(),
        )
        .await?;
        self.subscriptions.write().await.insert(pubkey, sub_id);
        info!("[WebSocket] Successfully initiated subscription for {}", pubkey);
        Ok(())
    }

    async fn create_account_subscription(
        client: Arc<PubsubClient>,
        pubkey: Pubkey,
        sender: AccountUpdateSender,
        cache: Option<Arc<Cache>>,
        ws_update_sender: Option<mpsc::Sender<WebsocketUpdate>>,
    ) -> Result<SubscriptionId, PubsubClientError> {
        info!("[WebSocket] Creating account subscription for {}", pubkey);
        // Move the account_subscribe call *inside* the spawned task to ensure the client Arc lives long enough.
        let client_for_task = Arc::clone(&client);

        let subscription_id_placeholder: SubscriptionId = 0;

        tokio::spawn(async move {
            // Move the subscribe call here
            let (mut account_stream, subscription_meta_closure) = match client_for_task
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
                Ok(res) => res,
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

            info!("[WebSocket] Subscription task started for {}", pubkey);
            loop {
                match tokio::time::timeout(Duration::from_secs(60), account_stream.next()).await {
                    Ok(Some(response)) => {
                        // response is Response<UiAccount>
                        debug!("[WebSocket] Received update for {}", pubkey);
                        let decoded_data = match &response.value.data {
                            UiAccountData::Binary(encoded, _) => {
                                match base64::engine::general_purpose::STANDARD.decode(encoded) {
                                    Ok(data) => data,
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
                                warn!("[WebSocket] Received non-binary data for {}, skipping.", pubkey);
                                continue;
                            }
                        };
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        // Get owner program ID to find the correct parser
                        let owner_program_id_str = &response.value.owner;
                        match Pubkey::from_str(owner_program_id_str) {
                            Ok(owner_pubkey) => {
                                if let Some(parser_fn) = get_pool_parser_fn_for_program(&owner_pubkey) {
                                    match parser_fn(pubkey, &decoded_data) {
                                        Ok(pool_info) => {
                                            debug!("[WebSocket] Successfully parsed pool data for {} using parser for program {}", pubkey, owner_pubkey);
                                            if let Some(ref cache_instance) = cache {
                                                if let Err(e) = cache_instance
                                                    .update_pool_cache(&pubkey.to_string(), &pool_info, None)
                                                    .await
                                                {
                                                    warn!(
                                                        "[WebSocket] Failed to update pool cache for {}: {}",
                                                        pubkey, e
                                                    );
                                                }
                                            }
                                            if let Some(ref sender_channel) = ws_update_sender {
                                                if sender_channel
                                                    .send(WebsocketUpdate::PoolUpdate(pool_info))
                                                    .await
                                                    .is_err()
                                                {
                                                    warn!(
                                                        "[WebSocket] Failed to send PoolUpdate for {} to internal channel.",
                                                        pubkey
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!("[WebSocket] Parser for program {} failed for account {}: {}. Data len: {}. Treating as generic.", owner_pubkey, pubkey, e, decoded_data.len());
                                            // Potentially send a more specific error or just a generic update
                                            if let Some(ref sender_channel) = ws_update_sender {
                                                if sender_channel.send(WebsocketUpdate::GenericUpdate(format!(
                                                    "Parser error for pubkey {} (owner {}): {}, data length {}",
                                                    pubkey, owner_pubkey, e, decoded_data.len()
                                                ))).await.is_err() {
                                                    warn!("[WebSocket] Failed to send GenericUpdate (parser error) for {} to internal channel.", pubkey);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    debug!("[WebSocket] No parser registered for program ID: {}. Account update for {} (len: {}) treated as generic.", owner_pubkey, pubkey, decoded_data.len());
                                    if let Some(ref sender_channel) = ws_update_sender {
                                        if sender_channel.send(WebsocketUpdate::GenericUpdate(format!(
                                            "No parser for owner {}, pubkey {} at {}, data length {}",
                                            owner_pubkey, pubkey, timestamp, decoded_data.len()
                                        ))).await.is_err() {
                                            warn!("[WebSocket] Failed to send GenericUpdate (no parser) for {} to internal channel.", pubkey);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("[WebSocket] Failed to parse owner string '{}' as Pubkey for account {}: {}", owner_program_id_str, pubkey, e);
                                if let Some(ref sender_channel) = ws_update_sender {
                                     if sender_channel.send(WebsocketUpdate::GenericUpdate(format!(
                                        "Invalid owner pubkey string for account {}, owner string: '{}', data length {}",
                                        pubkey, owner_program_id_str, decoded_data.len()
                                    ))).await.is_err() {
                                        warn!("[WebSocket] Failed to send GenericUpdate (invalid owner) for {} to internal channel.", pubkey);
                                    }
                                }
                            }
                        }

                        let update = RawAccountUpdate::Account {
                            pubkey,
                            data: decoded_data,
                            timestamp,
                        };
                        match sender.send(update) {
                            Ok(_) => debug!("[WebSocket] Broadcasted update for {}", pubkey),
                            Err(e) => error!(
                                "[WebSocket] Failed to broadcast update for {}: {}",
                                pubkey, e
                            ),
                        }
                    }
                    Ok(None) => {
                        warn!(
                            "[WebSocket] Subscription stream for {} ended (None received).",
                            pubkey
                        );
                        break;
                    }
                    Err(_elapsed) => {
                        debug!(
                            "[WebSocket] Timeout waiting for update from {}, assuming connection is alive.",
                            pubkey
                        );
                    }
                }
            }
            warn!("[WebSocket] Subscription task for {} stopped.", pubkey);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let disconnect_update = RawAccountUpdate::Disconnected { pubkey, timestamp };
            if sender.send(disconnect_update).is_err() {
                error!(
                    "[WebSocket] Failed to send Disconnected update for {} after stream ended.",
                    pubkey
                );
            }
            subscription_meta_closure().await;
            info!("[WebSocket] Unsubscribe function called for {}", pubkey);
        });

        Ok(subscription_id_placeholder)
    }

    pub async fn unsubscribe(&self, pubkey: &Pubkey) -> Result<(), PubsubClientError> {
        let mut subs = self.subscriptions.write().await;
        if let Some(_sub_id) = subs.remove(pubkey) {
            info!(
                "[WebSocket] Removed {} from local subscriptions. Actual unsubscription relies on stream closure.",
                pubkey
            );
        } else {
            warn!(
                "[WebSocket] Attempted to unsubscribe from {}, but it was not found in local subscriptions.",
                pubkey
            );
        }
        Ok(())
    }
}
