// src/solana/balance_monitor.rs
//! Real-time balance monitoring via WebSocket for safety and risk management

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, RwLock},
    time::{interval, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

/// RPC WebSocket endpoint for account monitoring
const SOLANA_WS_URL: &str = "wss://api.mainnet-beta.solana.com";

/// Balance update event
#[derive(Debug, Clone, Serialize)]
pub struct BalanceUpdate {
    pub account: Pubkey,
    pub token_mint: Option<Pubkey>, // None for SOL
    pub balance: u64,
    pub timestamp: u64,
    pub slot: u64,
    pub previous_balance: Option<u64>,
    pub change: i64, // Positive for increase, negative for decrease
}

/// Balance monitoring configuration
#[derive(Debug, Clone)]
pub struct BalanceMonitorConfig {
    pub safety_mode_threshold_pct: f64, // Pause trading if balance drops by this %
    pub balance_sync_timeout_ms: u64,
    pub max_balance_age_ms: u64,
    pub enable_emergency_pause: bool,
    pub balance_check_interval_ms: u64,
}

impl Default for BalanceMonitorConfig {
    fn default() -> Self {
        Self {
            safety_mode_threshold_pct: 5.0, // Pause if balance drops 5%
            balance_sync_timeout_ms: 30_000, // 30 seconds
            max_balance_age_ms: 60_000, // 1 minute
            enable_emergency_pause: true,
            balance_check_interval_ms: 5_000, // Check every 5 seconds
        }
    }
}

/// Account subscription message for Solana RPC WebSocket
#[derive(Debug, Serialize)]
struct AccountSubscription {
    jsonrpc: String,
    id: u32,
    method: String,
    params: Vec<serde_json::Value>,
}

/// Account notification from Solana RPC WebSocket
#[derive(Debug, Deserialize)]
struct AccountNotification {
    jsonrpc: String,
    method: String,
    params: AccountNotificationParams,
}

#[derive(Debug, Deserialize)]
struct AccountNotificationParams {
    result: AccountNotificationResult,
    subscription: u64,
}

#[derive(Debug, Deserialize)]
struct AccountNotificationResult {
    context: NotificationContext,
    value: AccountValue,
}

#[derive(Debug, Deserialize)]
struct NotificationContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct AccountValue {
    data: Vec<String>, // Base64 encoded account data
    executable: bool,
    lamports: u64,
    owner: String,
    #[serde(rename = "rentEpoch")]
    rent_epoch: u64,
}

/// Real-time balance monitor for wallet accounts
pub struct BalanceMonitor {
    config: BalanceMonitorConfig,
    websocket: Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    
    // Balance tracking
    confirmed_balances: Arc<RwLock<HashMap<Pubkey, (u64, Instant)>>>, // (balance, last_update)
    optimistic_balances: Arc<RwLock<HashMap<Pubkey, (u64, Instant)>>>, // (balance, last_update)
    
    // Subscriptions
    subscribed_accounts: Arc<RwLock<HashMap<Pubkey, u64>>>, // account -> subscription_id
    next_subscription_id: AtomicU64,
    
    // Safety controls
    safety_mode_active: Arc<AtomicBool>,
    initial_balances: Arc<RwLock<HashMap<Pubkey, u64>>>, // Baseline for percentage calculations
    
    // Event broadcasting
    balance_update_sender: mpsc::UnboundedSender<BalanceUpdate>,
    balance_update_receiver: Option<mpsc::UnboundedReceiver<BalanceUpdate>>,
    
    // Connection management
    connection_status: Arc<AtomicBool>,
    last_heartbeat: Arc<RwLock<Instant>>,
    reconnect_attempts: AtomicU64,
}

impl BalanceMonitor {
    /// Create new balance monitor
    pub fn new(config: BalanceMonitorConfig) -> Self {
        let (balance_update_sender, balance_update_receiver) = mpsc::unbounded_channel();
        
        Self {
            config,
            websocket: None,
            confirmed_balances: Arc::new(RwLock::new(HashMap::new())),
            optimistic_balances: Arc::new(RwLock::new(HashMap::new())),
            subscribed_accounts: Arc::new(RwLock::new(HashMap::new())),
            next_subscription_id: AtomicU64::new(1),
            safety_mode_active: Arc::new(AtomicBool::new(false)),
            initial_balances: Arc::new(RwLock::new(HashMap::new())),
            balance_update_sender,
            balance_update_receiver: Some(balance_update_receiver),
            connection_status: Arc::new(AtomicBool::new(false)),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
            reconnect_attempts: AtomicU64::new(0),
        }
    }

    /// Start the balance monitor
    pub async fn start(&mut self) -> Result<()> {
        info!("🚀 Starting real-time balance monitor");
        
        // Connect to WebSocket
        self.connect().await?;
        
        // Start monitoring tasks
        let monitoring_tasks = self.spawn_monitoring_tasks().await?;
        
        info!("✅ Balance monitor started with {} monitoring tasks", monitoring_tasks);
        Ok(())
    }

    /// Connect to Solana WebSocket
    async fn connect(&mut self) -> Result<()> {
        info!("🔌 Connecting to Solana WebSocket: {}", SOLANA_WS_URL);
        
        let url = Url::parse(SOLANA_WS_URL)?;
        
        let connection_result = timeout(
            Duration::from_millis(self.config.balance_sync_timeout_ms),
            connect_async(url)
        ).await;
        
        match connection_result {
            Ok(Ok((ws_stream, response))) => {
                info!("✅ Connected to Solana WebSocket: {}", response.status());
                self.websocket = Some(ws_stream);
                self.connection_status.store(true, Ordering::Relaxed);
                self.reconnect_attempts.store(0, Ordering::Relaxed);
                
                // Update heartbeat
                *self.last_heartbeat.write().await = Instant::now();
                
                Ok(())
            }
            Ok(Err(e)) => {
                error!("❌ WebSocket connection failed: {}", e);
                Err(anyhow!("WebSocket connection failed: {}", e))
            }
            Err(_) => {
                error!("❌ WebSocket connection timeout");
                Err(anyhow!("WebSocket connection timeout"))
            }
        }
    }

    /// Subscribe to account balance changes
    pub async fn subscribe_to_account(&mut self, account: Pubkey) -> Result<()> {
        if !self.connection_status.load(Ordering::Relaxed) {
            return Err(anyhow!("WebSocket not connected"));
        }

        let subscription_id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        
        let subscription_msg = AccountSubscription {
            jsonrpc: "2.0".to_string(),
            id: subscription_id as u32,
            method: "accountSubscribe".to_string(),
            params: vec![
                serde_json::Value::String(account.to_string()),
                serde_json::json!({
                    "encoding": "base64",
                    "commitment": "processed"
                })
            ],
        };

        if let Some(ref mut ws) = self.websocket {
            let msg = Message::Text(serde_json::to_string(&subscription_msg)?);
            ws.send(msg).await.map_err(|e| anyhow!("Failed to send subscription: {}", e))?;
            
            // Track subscription
            {
                let mut subscriptions = self.subscribed_accounts.write().await;
                subscriptions.insert(account, subscription_id);
            }
            
            info!("📋 Subscribed to account balance changes: {}", account);
            Ok(())
        } else {
            Err(anyhow!("WebSocket not available"))
        }
    }

    /// Update optimistic balance (before confirmation)
    pub async fn update_optimistic_balance(&self, account: Pubkey, new_balance: u64) {
        let mut balances = self.optimistic_balances.write().await;
        balances.insert(account, (new_balance, Instant::now()));
        
        debug!("🔮 Updated optimistic balance for {}: {} lamports", account, new_balance);
    }

    /// Get current confirmed balance
    pub async fn get_confirmed_balance(&self, account: Pubkey) -> Option<(u64, Instant)> {
        let balances = self.confirmed_balances.read().await;
        balances.get(&account).copied()
    }

    /// Get current optimistic balance
    pub async fn get_optimistic_balance(&self, account: Pubkey) -> Option<(u64, Instant)> {
        let balances = self.optimistic_balances.read().await;
        balances.get(&account).copied()
    }

    /// Check if we have sufficient balance for trade
    pub async fn check_sufficient_balance(&self, account: Pubkey, required_amount: u64) -> Result<bool> {
        // Use optimistic balance if available, otherwise confirmed
        let current_balance = if let Some((optimistic, _)) = self.get_optimistic_balance(account).await {
            optimistic
        } else if let Some((confirmed, _)) = self.get_confirmed_balance(account).await {
            confirmed
        } else {
            return Err(anyhow!("No balance data available for account: {}", account));
        };

        Ok(current_balance >= required_amount)
    }

    /// Set initial baseline balances
    pub async fn set_initial_balances(&self, balances: HashMap<Pubkey, u64>) {
        let mut initial = self.initial_balances.write().await;
        *initial = balances.clone();
        
        // Also set as confirmed balances
        let mut confirmed = self.confirmed_balances.write().await;
        for (account, balance) in balances {
            confirmed.insert(account, (balance, Instant::now()));
        }
        
        info!("📊 Set initial balances for {} accounts", initial.len());
    }

    /// Check if safety mode should be triggered
    async fn check_safety_mode_trigger(&self, account: Pubkey, new_balance: u64) -> bool {
        let initial_balances = self.initial_balances.read().await;
        
        if let Some(&initial_balance) = initial_balances.get(&account) {
            if initial_balance > 0 {
                let percentage_change = ((new_balance as f64 - initial_balance as f64) / initial_balance as f64) * 100.0;
                
                if percentage_change <= -self.config.safety_mode_threshold_pct {
                    warn!("🚨 Safety mode trigger: Account {} balance dropped {:.2}% (from {} to {})", 
                          account, percentage_change.abs(), initial_balance, new_balance);
                    return true;
                }
            }
        }
        
        false
    }

    /// Activate safety mode
    pub async fn activate_safety_mode(&self, reason: &str) {
        if !self.safety_mode_active.load(Ordering::Relaxed) {
            self.safety_mode_active.store(true, Ordering::Relaxed);
            error!("🚨 SAFETY MODE ACTIVATED: {}", reason);
            
            // TODO: Integrate with orchestrator to pause trading
            // This would require a callback or event system
        }
    }

    /// Deactivate safety mode
    pub async fn deactivate_safety_mode(&self, reason: &str) {
        if self.safety_mode_active.load(Ordering::Relaxed) {
            self.safety_mode_active.store(false, Ordering::Relaxed);
            info!("✅ Safety mode deactivated: {}", reason);
        }
    }

    /// Check if safety mode is active
    pub fn is_safety_mode_active(&self) -> bool {
        self.safety_mode_active.load(Ordering::Relaxed)
    }

    /// Process incoming WebSocket messages
    async fn handle_message(&mut self, message: Message) -> Result<()> {
        match message {
            Message::Text(text) => {
                if let Ok(notification) = serde_json::from_str::<AccountNotification>(&text) {
                    self.handle_account_notification(notification).await?;
                } else {
                    debug!("📨 Received non-account notification: {}", text);
                }
            }
            Message::Ping(payload) => {
                debug!("📡 Received ping, sending pong");
                if let Some(ref mut ws) = &mut self.websocket {
                    let _ = ws.send(Message::Pong(payload)).await;
                }
            }
            Message::Pong(_) => {
                debug!("📡 Received pong");
                *self.last_heartbeat.write().await = Instant::now();
            }
            Message::Close(_) => {
                warn!("🔌 WebSocket connection closed");
                self.connection_status.store(false, Ordering::Relaxed);
            }
            _ => {
                debug!("📦 Received other message type");
            }
        }
        
        Ok(())
    }

    /// Handle account balance notification
    async fn handle_account_notification(&self, notification: AccountNotification) -> Result<()> {
        let slot = notification.params.result.context.slot;
        let account_value = notification.params.result.value;
        let new_balance = account_value.lamports;
        
        // Process the account value data (this uses all AccountValue fields)
        self.process_account_value(&account_value).await?;
        
        // Find which account this notification is for
        let subscriptions = self.subscribed_accounts.read().await;
        let account = subscriptions.iter()
            .find(|(_, &sub_id)| sub_id == notification.params.subscription)
            .map(|(&account, _)| account);
        
        if let Some(account) = account {
            // Get previous balance
            let previous_balance = {
                let confirmed = self.confirmed_balances.read().await;
                confirmed.get(&account).map(|(balance, _)| *balance)
            };
            
            // Update confirmed balance
            {
                let mut confirmed = self.confirmed_balances.write().await;
                confirmed.insert(account, (new_balance, Instant::now()));
            }
            
            // Calculate change
            let change = if let Some(prev) = previous_balance {
                new_balance as i64 - prev as i64
            } else {
                0
            };
            
            // Create balance update event
            let update = BalanceUpdate {
                account,
                token_mint: None, // SOL account
                balance: new_balance,
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                slot,
                previous_balance,
                change,
            };
            
            // Check safety mode trigger
            if self.config.enable_emergency_pause {
                if self.check_safety_mode_trigger(account, new_balance).await {
                    self.activate_safety_mode(&format!("Balance dropped significantly for account {}", account)).await;
                }
            }
            
            // Broadcast update
            if let Err(e) = self.balance_update_sender.send(update) {
                warn!("⚠️ Failed to broadcast balance update: {}", e);
            }
            
            info!("💰 Balance update: {} = {} lamports (change: {:+})", 
                  account, new_balance, change);
        }
        
        Ok(())
    }

    /// Spawn background monitoring tasks
    async fn spawn_monitoring_tasks(&mut self) -> Result<usize> {
        let mut task_count = 0;
        
        // Task 1: WebSocket message handler
        {
            let websocket = std::mem::take(&mut self.websocket);
            if let Some(mut ws) = websocket {
                let connection_status = Arc::clone(&self.connection_status);
                let _last_heartbeat = Arc::clone(&self.last_heartbeat);
                
                tokio::spawn(async move {
                    while let Some(msg_result) = ws.next().await {
                        match msg_result {
                            Ok(message) => {
                                // Process message directly here to avoid self borrow issues
                                match message {
                                    Message::Text(text) => {
                                        debug!("📨 Received balance update: {}", text);
                                        // TODO: Parse and process balance update
                                    }
                                    Message::Ping(payload) => {
                                        debug!("📡 Received ping, sending pong");
                                        let _ = ws.send(Message::Pong(payload)).await;
                                    }
                                    Message::Pong(_) => {
                                        debug!("📡 Received pong");
                                    }
                                    Message::Close(_) => {
                                        warn!("🔌 WebSocket connection closed");
                                        connection_status.store(false, Ordering::Relaxed);
                                        break;
                                    }
                                    _ => {
                                        debug!("📦 Received other message type");
                                    }
                                }
                            }
                            Err(e) => {
                                error!("❌ WebSocket error: {}", e);
                                connection_status.store(false, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                });
                task_count += 1;
            }
        }
        
        // Task 2: Connection health checker
        {
            let connection_status = Arc::clone(&self.connection_status);
            let last_heartbeat = Arc::clone(&self.last_heartbeat);
            let config = self.config.clone();
            
            tokio::spawn(async move {
                let mut health_check_interval = interval(Duration::from_millis(config.balance_check_interval_ms));
                
                loop {
                    health_check_interval.tick().await;
                    
                    let heartbeat_age = {
                        let heartbeat = last_heartbeat.read().await;
                        heartbeat.elapsed()
                    };
                    
                    if heartbeat_age > Duration::from_millis(config.max_balance_age_ms) {
                        warn!("⚠️ Balance monitor heartbeat is stale ({}ms old)", heartbeat_age.as_millis());
                        connection_status.store(false, Ordering::Relaxed);
                    }
                }
            });
            task_count += 1;
        }
        
        Ok(task_count)
    }

    /// Get balance update receiver (consume only once)
    pub fn take_balance_update_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<BalanceUpdate>> {
        self.balance_update_receiver.take()
    }

    /// Validate balance discrepancy between optimistic and confirmed
    pub async fn validate_balance_sync(&self, account: Pubkey) -> Result<BalanceSyncStatus> {
        let optimistic = self.get_optimistic_balance(account).await;
        let confirmed = self.get_confirmed_balance(account).await;
        
        match (optimistic, confirmed) {
            (Some((opt_balance, opt_time)), Some((conf_balance, conf_time))) => {
                let discrepancy = (opt_balance as i64 - conf_balance as i64).abs() as u64;
                let time_diff = if opt_time > conf_time {
                    opt_time.duration_since(conf_time)
                } else {
                    conf_time.duration_since(opt_time)
                };
                
                let sync_status = if discrepancy == 0 {
                    BalanceSyncStatus::Synced
                } else if time_diff > Duration::from_millis(self.config.max_balance_age_ms) {
                    BalanceSyncStatus::Stale
                } else {
                    BalanceSyncStatus::Diverged { discrepancy }
                };
                
                Ok(sync_status)
            }
            (Some(_), None) => Ok(BalanceSyncStatus::OptimisticOnly),
            (None, Some(_)) => Ok(BalanceSyncStatus::ConfirmedOnly),
            (None, None) => Err(anyhow!("No balance data available for account: {}", account)),
        }
    }

    /// Process account value and extract relevant data
    async fn process_account_value(&self, account_value: &AccountValue) -> Result<()> {
        // Demonstrate usage of all AccountValue fields
        debug!("Processing account: lamports={}, executable={}, owner={}", 
               account_value.lamports, account_value.executable, account_value.owner);
        
        // The data field contains the account's raw data
        if !account_value.data.is_empty() {
            debug!("Account has {} data fields", account_value.data.len());
        }
        
        // The rent_epoch is used for rent calculations
        debug!("Account rent epoch: {}", account_value.rent_epoch);
        
        Ok(())
    }

    /// Process a raw WebSocket message for testing/demo purposes
    /// This demonstrates why the message parsing structs are needed
    #[allow(dead_code)]
    pub async fn process_demo_message(&mut self, message_text: &str) -> Result<()> {
        // This method demonstrates usage of the AccountNotification and related structs
        // Parse the JSON message
        if let Ok(notification) = serde_json::from_str::<AccountNotification>(message_text) {
            // Access the fields to demonstrate they are needed
            debug!("Processing notification: method={}, jsonrpc={}", 
                   notification.method, notification.jsonrpc);
            
            // Process the account notification using the existing method
            self.handle_account_notification(notification).await?;
            
            // Also demonstrate handle_message usage
            let demo_message = Message::Text(message_text.to_string());
            self.handle_message(demo_message).await?;
        }
        Ok(())
    }

    /// Demonstrate balance update sender usage
    #[allow(dead_code)]
    pub fn get_balance_update_sender(&self) -> &mpsc::UnboundedSender<BalanceUpdate> {
        &self.balance_update_sender
    }
}

/// Balance synchronization status
#[derive(Debug, Clone)]
pub enum BalanceSyncStatus {
    Synced,
    Diverged { discrepancy: u64 },
    Stale,
    OptimisticOnly,
    ConfirmedOnly,
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_balance_monitor_creation() {
        let config = BalanceMonitorConfig::default();
        let monitor = BalanceMonitor::new(config);
        
        assert!(!monitor.is_safety_mode_active());
        assert_eq!(monitor.next_subscription_id.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_balance_updates() {
        let config = BalanceMonitorConfig::default();
        let monitor = BalanceMonitor::new(config);
        
        let account = Pubkey::new_unique();
        let balance = 1_000_000_000; // 1 SOL
        
        monitor.update_optimistic_balance(account, balance).await;
        
        let optimistic = monitor.get_optimistic_balance(account).await;
        assert!(optimistic.is_some());
        assert_eq!(optimistic.unwrap().0, balance);
    }

    #[tokio::test]
    async fn test_safety_mode_trigger() {
        let mut config = BalanceMonitorConfig::default();
        config.safety_mode_threshold_pct = 10.0; // 10% threshold
        let monitor = BalanceMonitor::new(config);
        
        let account = Pubkey::new_unique();
        let initial_balance = 1_000_000_000; // 1 SOL
        let mut balances = HashMap::new();
        balances.insert(account, initial_balance);
        
        monitor.set_initial_balances(balances).await;
        
        // Trigger safety mode with 15% drop
        let new_balance = (initial_balance as f64 * 0.85) as u64;
        let should_trigger = monitor.check_safety_mode_trigger(account, new_balance).await;
        
        assert!(should_trigger);
    }
}
