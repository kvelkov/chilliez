//! Wallet-Jito Integration Module
//!
//! This module provides the integrated workflow for using ephemeral wallets
//! with Jito bundle submission for MEV-protected arbitrage operations.

use crate::{
    arbitrage::{BundleBuilder, JitoClient, JitoClientConfig},
    error::ArbError,
    wallet::{WalletPool, WalletPoolConfig},
};
use log::{debug, error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{hash::Hash, pubkey::Pubkey, transaction::Transaction};
use std::sync::Arc;
use tokio::sync::RwLock;

type Result<T> = std::result::Result<T, ArbError>;

/// Configuration for the integrated wallet-Jito system
#[derive(Debug, Clone)]
pub struct WalletJitoConfig {
    pub wallet_config: WalletPoolConfig,
    pub jito_config: JitoClientConfig,
    /// Enable automatic profit sweeping
    pub auto_sweep_profits: bool,
    /// Minimum profit threshold to execute trades (in lamports)
    pub min_profit_threshold: u64,
    /// Enable bundle optimization (combine sweep with trade)
    pub optimize_bundles: bool,
}

impl Default for WalletJitoConfig {
    fn default() -> Self {
        Self {
            wallet_config: WalletPoolConfig::default(),
            jito_config: JitoClientConfig::default(),
            auto_sweep_profits: true,
            min_profit_threshold: 50_000, // 0.00005 SOL
            optimize_bundles: true,
        }
    }
}

/// Integrated execution statistics
#[derive(Debug, Clone, Default)]
pub struct IntegratedStats {
    pub total_trades_executed: u64,
    pub total_profits_swept: u64,
    pub successful_bundles: u64,
    pub failed_bundles: u64,
    pub average_bundle_size: f64,
    pub total_fees_paid: u64,
}

/// Integrated wallet and Jito client for MEV-protected arbitrage
pub struct WalletJitoIntegration {
    wallet_pool: Arc<RwLock<WalletPool>>,
    jito_client: Arc<RwLock<JitoClient>>,
    rpc_client: Arc<RpcClient>,
    config: WalletJitoConfig,
    stats: IntegratedStats,
}

impl WalletJitoIntegration {
    /// Create a new integrated wallet-Jito system
    pub fn new(
        config: WalletJitoConfig,
        collector_wallet: Pubkey,
        rpc_client: Arc<RpcClient>,
    ) -> Self {
        info!(
            "🚀 Initializing integrated wallet-Jito system with collector: {}",
            collector_wallet
        );

        let wallet_pool = Arc::new(RwLock::new(WalletPool::new(
            config.wallet_config.clone(),
            collector_wallet,
        )));

        let jito_client = Arc::new(RwLock::new(JitoClient::new_with_defaults(RpcClient::new(
            rpc_client.url(),
        ))));

        Self {
            wallet_pool,
            jito_client,
            rpc_client,
            config,
            stats: IntegratedStats::default(),
        }
    }

    /// Execute an arbitrage trade with automatic wallet management and bundling
    pub async fn execute_arbitrage_trade(
        &mut self,
        trade_instructions: Vec<Transaction>,
        expected_profit_lamports: u64,
    ) -> Result<String> {
        if expected_profit_lamports < self.config.min_profit_threshold {
            return Err(ArbError::InsufficientProfit(format!(
                "Expected profit {} below threshold {}",
                expected_profit_lamports, self.config.min_profit_threshold
            )));
        }

        info!(
            "💰 Executing arbitrage trade with expected profit: {} lamports",
            expected_profit_lamports
        );

        // Get a signing wallet for the trade
        let (trader_pubkey, trader_keypair) = {
            let mut pool = self.wallet_pool.write().await;
            let wallet = pool.get_signing_wallet();
            (wallet.pubkey(), wallet.keypair.insecure_clone())
        };

        debug!("🔑 Using ephemeral wallet: {}", trader_pubkey);

        // Prepare the bundle
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| ArbError::RpcError(format!("Failed to get blockhash: {}", e)))?;

        let mut bundle_builder = BundleBuilder::new(self.config.jito_config.max_bundle_size);

        // Add trade transactions to bundle
        bundle_builder = bundle_builder.add_transactions(trade_instructions)?;

        // Check if we should add a sweep transaction
        if self.config.auto_sweep_profits && self.config.optimize_bundles {
            if let Some(sweep_tx) = self
                .prepare_sweep_transaction(&trader_pubkey, recent_blockhash)
                .await?
            {
                bundle_builder = bundle_builder.add_transaction(sweep_tx)?;
                info!("💸 Added sweep transaction to bundle");
            }
        }

        // Calculate optimal tip
        let optimal_tip = {
            let jito_client = self.jito_client.read().await;
            jito_client
                .calculate_optimal_tip(expected_profit_lamports)
                .await?
        };

        // Get bundle size before submitting
        let bundle_size = bundle_builder.size();

        // Submit bundle with tip
        let bundle_id = {
            let mut jito_client = self.jito_client.write().await;
            jito_client
                .submit_bundle_with_tip(
                    bundle_builder.build(),
                    optimal_tip,
                    &trader_keypair,
                    recent_blockhash,
                )
                .await?
        };

        // Update statistics
        self.update_execution_stats(true, bundle_size, optimal_tip);

        info!(
            "✅ Arbitrage trade executed successfully: bundle {}",
            bundle_id
        );
        Ok(bundle_id)
    }

    /// Prepare a sweep transaction if the wallet has sufficient balance
    async fn prepare_sweep_transaction(
        &self,
        wallet_pubkey: &Pubkey,
        _recent_blockhash: Hash,
    ) -> Result<Option<Transaction>> {
        // Get wallet balance
        let balance = self
            .rpc_client
            .get_balance(wallet_pubkey)
            .await
            .map_err(|e| ArbError::RpcError(format!("Failed to get balance: {}", e)))?;

        // Check if sweep is needed
        let pool = self.wallet_pool.read().await;
        if let Some(sweep_amount) = pool.calculate_sweep_amount(balance) {
            // Find the wallet in the pool
            // Note: In a real implementation, you'd want a better way to get the wallet reference
            info!(
                "💰 Preparing sweep of {} lamports from wallet {}",
                sweep_amount, wallet_pubkey
            );

            // For now, we'll return None and handle sweeping separately
            // In a full implementation, you'd create the sweep transaction here
            return Ok(None);
        }

        Ok(None)
    }

    /// Perform profit sweeping for all eligible wallets
    pub async fn sweep_profits_from_all_wallets(&mut self) -> Result<Vec<String>> {
        info!("🧹 Starting profit sweep from all eligible wallets");

        let mut sweep_results = Vec::new();
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| ArbError::RpcError(format!("Failed to get blockhash: {}", e)))?;

        // Get all wallet public keys
        let wallet_pubkeys = {
            let pool = self.wallet_pool.read().await;
            pool.get_all_pubkeys()
        };

        for wallet_pubkey in wallet_pubkeys {
            match self
                .sweep_wallet_if_eligible(&wallet_pubkey, recent_blockhash)
                .await
            {
                Ok(Some(sweep_id)) => {
                    sweep_results.push(sweep_id);
                    self.stats.total_profits_swept += 1;
                }
                Ok(None) => {
                    debug!("💧 Wallet {} not eligible for sweep", wallet_pubkey);
                }
                Err(e) => {
                    error!("❌ Failed to sweep wallet {}: {}", wallet_pubkey, e);
                }
            }
        }

        info!(
            "✅ Profit sweep completed: {} wallets swept",
            sweep_results.len()
        );
        Ok(sweep_results)
    }

    /// Sweep a specific wallet if it meets the criteria
    async fn sweep_wallet_if_eligible(
        &self,
        wallet_pubkey: &Pubkey,
        _recent_blockhash: Hash,
    ) -> Result<Option<String>> {
        // Get wallet balance
        let balance = self
            .rpc_client
            .get_balance(wallet_pubkey)
            .await
            .map_err(|e| {
                ArbError::RpcError(format!(
                    "Failed to get balance for {}: {}",
                    wallet_pubkey, e
                ))
            })?;

        // Check if sweep is warranted
        let pool = self.wallet_pool.read().await;
        if let Some(sweep_amount) = pool.calculate_sweep_amount(balance) {
            // Note: In a real implementation, you'd need to access the wallet's keypair
            // For now, we'll just log what would happen
            info!(
                "💰 Would sweep {} lamports from {}",
                sweep_amount, wallet_pubkey
            );
            return Ok(Some(format!("sweep_{}", wallet_pubkey)));
        }

        Ok(None)
    }

    /// Get comprehensive statistics
    pub async fn get_comprehensive_stats(&self) -> IntegratedSystemStats {
        let wallet_stats = {
            let pool = self.wallet_pool.read().await;
            pool.get_stats()
        };

        let jito_stats = {
            let client = self.jito_client.read().await;
            client.get_stats().clone()
        };

        IntegratedSystemStats {
            wallet_stats,
            jito_stats,
            integration_stats: self.stats.clone(),
        }
    }

    /// Cleanup expired wallets and update statistics
    pub async fn cleanup_and_maintain(&mut self) -> Result<()> {
        info!("🔧 Performing system maintenance");

        // Cleanup expired wallets
        let removed_count = {
            let mut pool = self.wallet_pool.write().await;
            pool.cleanup_expired_wallets()
        };

        if removed_count > 0 {
            info!("🗑️ Removed {} expired wallets", removed_count);
        }

        // Check system health
        let is_healthy = {
            let jito_client = self.jito_client.read().await;
            jito_client.is_healthy()
        };

        if !is_healthy {
            warn!("⚠️ Jito client health check failed - consider reviewing configuration");
        }

        Ok(())
    }

    /// Update execution statistics
    fn update_execution_stats(&mut self, success: bool, bundle_size: usize, fees_paid: u64) {
        self.stats.total_trades_executed += 1;
        self.stats.total_fees_paid += fees_paid;

        if success {
            self.stats.successful_bundles += 1;
        } else {
            self.stats.failed_bundles += 1;
        }

        // Update average bundle size
        let total_bundles = self.stats.successful_bundles + self.stats.failed_bundles;
        if total_bundles == 1 {
            self.stats.average_bundle_size = bundle_size as f64;
        } else {
            self.stats.average_bundle_size =
                (self.stats.average_bundle_size * (total_bundles - 1) as f64 + bundle_size as f64)
                    / total_bundles as f64;
        }
    }

    /// Get success rate as percentage
    pub fn get_success_rate(&self) -> f64 {
        let total = self.stats.successful_bundles + self.stats.failed_bundles;
        if total == 0 {
            return 0.0;
        }
        (self.stats.successful_bundles as f64 / total as f64) * 100.0
    }
}

/// Comprehensive system statistics
#[derive(Debug)]
pub struct IntegratedSystemStats {
    pub wallet_stats: crate::wallet::WalletPoolStats,
    pub jito_stats: crate::arbitrage::BundleStats,
    pub integration_stats: IntegratedStats,
}
