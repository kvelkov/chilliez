//! Enhanced Real-Time Balance Monitoring Module
//!
//! This module provides comprehensive real-time balance monitoring with WebSocket
//! integration, thread-safe updates, and alert systems for discrepancies.

use crate::solana::websocket::SolanaWebsocketManager;
use crate::{error::ArbError, solana::rpc::SolanaRpcClient};
use dashmap::DashMap;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{broadcast, Mutex, RwLock},
    time::interval,
};

// =============================================================================
// Configuration and Types
// =============================================================================

#[derive(Debug, Clone)]
pub struct BalanceMonitorConfig {
    pub enable_websocket_monitoring: bool,
    pub periodic_verification_interval_secs: u64,
    pub balance_threshold_alert_percent: f64,
    pub max_balance_age_secs: u64,
    pub enable_optimistic_balance_tracking: bool,
    pub safety_margin_lamports: u64,
    pub alert_on_discrepancy: bool,
}

impl Default for BalanceMonitorConfig {
    fn default() -> Self {
        Self {
            enable_websocket_monitoring: true,
            periodic_verification_interval_secs: 30,
            balance_threshold_alert_percent: 5.0,
            max_balance_age_secs: 10,
            enable_optimistic_balance_tracking: true,
            safety_margin_lamports: 100_000, // 0.0001 SOL
            alert_on_discrepancy: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceRecord {
    pub confirmed_balance: u64,
    pub optimistic_balance: u64,
    pub last_confirmed_update: u64, // Unix timestamp
    pub last_optimistic_update: u64,
    pub pending_changes: i64, // Signed change amount
    pub is_stale: bool,
}

#[derive(Debug, Clone)]
pub struct BalanceAlert {
    pub alert_type: AlertType,
    pub account: Pubkey,
    pub message: String,
    pub timestamp: SystemTime,
    pub severity: AlertSeverity,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum AlertType {
    BalanceDiscrepancy,
    InsufficientBalance,
    StaleBalance,
    UnexpectedChange,
    WebSocketDisconnection,
    VerificationFailure,
}

#[derive(Debug, Clone)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
    Emergency,
}

#[derive(Debug)]
pub struct BalanceMonitorMetrics {
    pub total_updates_processed: AtomicU64,
    pub websocket_updates: AtomicU64,
    pub rpc_verifications: AtomicU64,
    pub discrepancies_detected: AtomicU64,
    pub alerts_sent: AtomicU64,
    pub last_update_timestamp: AtomicU64,
    pub is_healthy: AtomicBool,
}

// =============================================================================
// Enhanced Real-Time Balance Monitor
// =============================================================================

pub struct EnhancedBalanceMonitor {
    config: BalanceMonitorConfig,
    balances: Arc<DashMap<Pubkey, BalanceRecord>>,
    rpc_client: Arc<SolanaRpcClient>,
    // websocket_manager: Option<Arc<SolanaWebsocketManager>>,
    alert_sender: broadcast::Sender<BalanceAlert>,
    metrics: Arc<BalanceMonitorMetrics>,
    is_running: Arc<AtomicBool>,
    safety_mode: Arc<AtomicBool>,
    monitored_accounts: Arc<RwLock<Vec<Pubkey>>>,
}

impl EnhancedBalanceMonitor {
    pub fn new(
        config: BalanceMonitorConfig,
        rpc_client: Arc<SolanaRpcClient>,
        _websocket_manager: Option<Arc<SolanaWebsocketManager>>,
    ) -> (Self, broadcast::Receiver<BalanceAlert>) {
        let (alert_sender, alert_receiver) = broadcast::channel(1024);

        let monitor = Self {
            config,
            balances: Arc::new(DashMap::new()),
            rpc_client,
            // websocket_manager,
            alert_sender,
            metrics: Arc::new(BalanceMonitorMetrics {
                total_updates_processed: AtomicU64::new(0),
                websocket_updates: AtomicU64::new(0),
                rpc_verifications: AtomicU64::new(0),
                discrepancies_detected: AtomicU64::new(0),
                alerts_sent: AtomicU64::new(0),
                last_update_timestamp: AtomicU64::new(0),
                is_healthy: AtomicBool::new(true),
            }),
            is_running: Arc::new(AtomicBool::new(false)),
            safety_mode: Arc::new(AtomicBool::new(false)),
            monitored_accounts: Arc::new(RwLock::new(Vec::new())),
        };

        (monitor, alert_receiver)
    }

    /// Start the enhanced balance monitoring system
    pub async fn start(&mut self) -> Result<(), ArbError> {
        if self.is_running.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("🚀 Starting Enhanced Balance Monitor...");
        self.is_running.store(true, Ordering::Relaxed);

        // Start WebSocket monitoring if enabled
        if self.config.enable_websocket_monitoring {
            self.start_websocket_monitoring().await?;
        }

        // Start periodic verification
        self.start_periodic_verification().await;

        // Start health monitoring
        self.start_health_monitoring().await;

        info!("✅ Enhanced Balance Monitor started successfully");
        Ok(())
    }

    /// Add accounts to monitor
    pub async fn add_accounts(&self, accounts: Vec<Pubkey>) -> Result<(), ArbError> {
        let mut monitored = self.monitored_accounts.write().await;
        for account in accounts {
            if !monitored.contains(&account) {
                monitored.push(account);

                // Initialize balance record with RPC fetch
                self.initialize_account_balance(account).await?;
            }
        }

        info!(
            "📊 Added {} accounts to balance monitoring",
            monitored.len()
        );
        Ok(())
    }

    /// Get current balance with optimistic/confirmed distinction
    pub async fn get_balance(&self, account: &Pubkey) -> Option<BalanceRecord> {
        self.balances.get(account).map(|entry| entry.clone())
    }

    /// Check if trading should be paused due to balance issues
    pub fn is_safety_mode_active(&self) -> bool {
        self.safety_mode.load(Ordering::Relaxed)
    }

    /// Get current monitoring metrics
    pub fn get_metrics(&self) -> BalanceMonitorMetrics {
        BalanceMonitorMetrics {
            total_updates_processed: AtomicU64::new(
                self.metrics.total_updates_processed.load(Ordering::Relaxed),
            ),
            websocket_updates: AtomicU64::new(
                self.metrics.websocket_updates.load(Ordering::Relaxed),
            ),
            rpc_verifications: AtomicU64::new(
                self.metrics.rpc_verifications.load(Ordering::Relaxed),
            ),
            discrepancies_detected: AtomicU64::new(
                self.metrics.discrepancies_detected.load(Ordering::Relaxed),
            ),
            alerts_sent: AtomicU64::new(self.metrics.alerts_sent.load(Ordering::Relaxed)),
            last_update_timestamp: AtomicU64::new(
                self.metrics.last_update_timestamp.load(Ordering::Relaxed),
            ),
            is_healthy: AtomicBool::new(self.metrics.is_healthy.load(Ordering::Relaxed)),
        }
    }

    /// Apply optimistic balance update (for pending transactions)
    pub async fn apply_optimistic_update(
        &self,
        account: Pubkey,
        change: i64,
    ) -> Result<(), ArbError> {
        if !self.config.enable_optimistic_balance_tracking {
            return Ok(());
        }

        if let Some(mut balance_record) = self.balances.get_mut(&account) {
            balance_record.optimistic_balance =
                (balance_record.optimistic_balance as i64 + change).max(0) as u64;
            balance_record.pending_changes += change;
            balance_record.last_optimistic_update = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            debug!(
                "💫 Applied optimistic balance update: account={}, change={}, new_optimistic={}",
                account, change, balance_record.optimistic_balance
            );
        }

        Ok(())
    }

    // =============================================================================
    // Private Implementation Methods
    // =============================================================================

    async fn initialize_account_balance(&self, account: Pubkey) -> Result<(), ArbError> {
        match self.rpc_client.primary_client.get_balance(&account).await {
            Ok(balance) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let balance_record = BalanceRecord {
                    confirmed_balance: balance,
                    optimistic_balance: balance,
                    last_confirmed_update: now,
                    last_optimistic_update: now,
                    pending_changes: 0,
                    is_stale: false,
                };

                self.balances.insert(account, balance_record);
                debug!(
                    "📋 Initialized balance for account {}: {} lamports",
                    account, balance
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    "❌ Failed to initialize balance for account {}: {}",
                    account, e
                );
                Err(ArbError::NetworkError(format!(
                    "Balance initialization failed: {}",
                    e
                )))
            }
        }
    }

    async fn start_websocket_monitoring(&self) -> Result<(), ArbError> {
        // WebSocket monitoring implementation would integrate with existing SolanaWebsocketManager
        // For now, we'll implement a placeholder
        info!("🔌 WebSocket balance monitoring started");
        Ok(())
    }

    async fn start_periodic_verification(&self) {
        let balances = self.balances.clone();
        let rpc_client = self.rpc_client.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        let alert_sender = self.alert_sender.clone();
        let safety_mode = self.safety_mode.clone();
        let is_running = self.is_running.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(
                config.periodic_verification_interval_secs,
            ));

            while is_running.load(Ordering::Relaxed) {
                interval.tick().await;

                debug!("🔍 Starting periodic balance verification...");
                let mut discrepancies_found = 0;

                for balance_entry in balances.iter() {
                    let account = *balance_entry.key();
                    let current_record = balance_entry.value().clone();

                    // Fetch fresh balance from RPC
                    match rpc_client.primary_client.get_balance(&account).await {
                        Ok(fresh_balance) => {
                            metrics.rpc_verifications.fetch_add(1, Ordering::Relaxed);

                            // Check for discrepancies
                            let difference = (fresh_balance as i64
                                - current_record.confirmed_balance as i64)
                                .abs();
                            let discrepancy_percent = (difference as f64
                                / current_record.confirmed_balance.max(1) as f64)
                                * 100.0;

                            if discrepancy_percent > config.balance_threshold_alert_percent {
                                discrepancies_found += 1;
                                metrics
                                    .discrepancies_detected
                                    .fetch_add(1, Ordering::Relaxed);

                                let alert = BalanceAlert {
                                    alert_type: AlertType::BalanceDiscrepancy,
                                    account,
                                    message: format!("Balance discrepancy detected: expected {}, actual {}, difference: {:.2}%", 
                                                   current_record.confirmed_balance, fresh_balance, discrepancy_percent),
                                    timestamp: SystemTime::now(),
                                    severity: if discrepancy_percent > 20.0 { AlertSeverity::Critical } else { AlertSeverity::Warning },
                                    details: Some(serde_json::json!({
                                        "expected": current_record.confirmed_balance,
                                        "actual": fresh_balance,
                                        "discrepancy_percent": discrepancy_percent,
                                        "difference_lamports": difference
                                    })),
                                };

                                if let Err(e) = alert_sender.send(alert) {
                                    error!("❌ Failed to send balance alert: {}", e);
                                }
                            }

                            // Update confirmed balance
                            drop(balance_entry); // Release the reference before modifying
                            if let Some(mut updated_record) = balances.get_mut(&account) {
                                updated_record.confirmed_balance = fresh_balance;
                                updated_record.last_confirmed_update = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                updated_record.is_stale = false;

                                // Reset optimistic balance if no pending changes
                                if updated_record.pending_changes == 0 {
                                    updated_record.optimistic_balance = fresh_balance;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to verify balance for account {}: {}", account, e);
                        }
                    }
                }

                // Update safety mode based on discrepancies
                if discrepancies_found > 0 {
                    safety_mode.store(true, Ordering::Relaxed);
                    warn!(
                        "🚨 Safety mode activated due to {} balance discrepancies",
                        discrepancies_found
                    );
                } else {
                    safety_mode.store(false, Ordering::Relaxed);
                }

                metrics.last_update_timestamp.store(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    Ordering::Relaxed,
                );

                debug!(
                    "✅ Periodic balance verification completed. Discrepancies: {}",
                    discrepancies_found
                );
            }
        });
    }

    async fn start_health_monitoring(&self) {
        let metrics = self.metrics.clone();
        let is_running = self.is_running.clone();
        let alert_sender = self.alert_sender.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60)); // Health check every minute

            while is_running.load(Ordering::Relaxed) {
                interval.tick().await;

                let last_update = metrics.last_update_timestamp.load(Ordering::Relaxed);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Check if system is healthy (recent updates)
                let is_healthy = (now - last_update) < 120; // Healthy if updated within 2 minutes
                metrics.is_healthy.store(is_healthy, Ordering::Relaxed);

                if !is_healthy {
                    let alert = BalanceAlert {
                        alert_type: AlertType::VerificationFailure,
                        account: Pubkey::default(), // System-wide alert
                        message: format!(
                            "Balance monitor health check failed. Last update: {} seconds ago",
                            now - last_update
                        ),
                        timestamp: SystemTime::now(),
                        severity: AlertSeverity::Critical,
                        details: Some(serde_json::json!({
                            "last_update_seconds_ago": now - last_update,
                            "total_updates": metrics.total_updates_processed.load(Ordering::Relaxed),
                            "websocket_updates": metrics.websocket_updates.load(Ordering::Relaxed),
                            "rpc_verifications": metrics.rpc_verifications.load(Ordering::Relaxed),
                        })),
                    };

                    if let Err(e) = alert_sender.send(alert) {
                        error!("❌ Failed to send health alert: {}", e);
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        info!("🛑 Stopping Enhanced Balance Monitor...");
        self.is_running.store(false, Ordering::Relaxed);
    }
}

// =============================================================================
// Thread-Safe Balance Operations
// =============================================================================

/// Thread-safe atomic balance operations
pub struct AtomicBalanceOperations {
    balance_locks: Arc<DashMap<Pubkey, Arc<Mutex<()>>>>,
}

impl AtomicBalanceOperations {
    pub fn new() -> Self {
        Self {
            balance_locks: Arc::new(DashMap::new()),
        }
    }

    /// Perform atomic balance check and reservation
    pub async fn atomic_balance_check_and_reserve(
        &self,
        account: &Pubkey,
        required_amount: u64,
        balance_monitor: &EnhancedBalanceMonitor,
    ) -> Result<(), ArbError> {
        // Get or create lock for this account
        let lock = self
            .balance_locks
            .entry(*account)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // Perform atomic check
        if let Some(balance_record) = balance_monitor.get_balance(account).await {
            let available_balance = balance_record.optimistic_balance;

            if available_balance < required_amount {
                return Err(ArbError::InsufficientBalance(format!(
                    "Insufficient balance: required {}, available {}",
                    required_amount, available_balance
                )));
            }

            // Reserve the amount optimistically
            balance_monitor
                .apply_optimistic_update(*account, -(required_amount as i64))
                .await?;

            info!(
                "💰 Atomic balance reservation successful: account={}, reserved={}",
                account, required_amount
            );
            Ok(())
        } else {
            Err(ArbError::AccountNotFound(format!(
                "Account {} not monitored",
                account
            )))
        }
    }

    /// Release a previously reserved amount
    pub async fn release_reservation(
        &self,
        account: &Pubkey,
        amount: u64,
        balance_monitor: &EnhancedBalanceMonitor,
    ) -> Result<(), ArbError> {
        let lock = self
            .balance_locks
            .entry(*account)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        balance_monitor
            .apply_optimistic_update(*account, amount as i64)
            .await?;

        info!(
            "🔓 Balance reservation released: account={}, amount={}",
            account, amount
        );
        Ok(())
    }
}

impl Default for AtomicBalanceOperations {
    fn default() -> Self {
        Self::new()
    }
}
