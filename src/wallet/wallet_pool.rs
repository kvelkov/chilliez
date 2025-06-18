//! Ephemeral Wallet Pool for Arbitrage Operations
//!
//! This module provides a wallet pool system for generating ephemeral wallets
//! for arbitrage operations, managing their lifecycle, and handling profit sweeps.

use log::{debug, info, warn};
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Configuration for the wallet pool
#[derive(Debug, Clone)]
pub struct WalletPoolConfig {
    /// Time-to-live for ephemeral wallets
    pub wallet_ttl_secs: u64,
    /// Minimum balance to trigger a sweep (in lamports)
    pub sweep_threshold_lamports: u64,
    /// Keep this many lamports for transaction fees
    pub fee_reserve_lamports: u64,
    /// Maximum number of wallets to keep in pool
    pub max_pool_size: usize,
    /// Enable automatic cleanup of expired wallets
    pub auto_cleanup: bool,
}

impl Default for WalletPoolConfig {
    fn default() -> Self {
        Self {
            wallet_ttl_secs: 300,             // 5 minutes
            sweep_threshold_lamports: 10_000, // 0.00001 SOL
            fee_reserve_lamports: 5_000,      // 0.000005 SOL
            max_pool_size: 100,
            auto_cleanup: true,
        }
    }
}

#[derive(Debug)]
pub struct EphemeralWallet {
    pub keypair: Keypair,
    pub created_at: Instant,
    pub last_used: Instant,
    pub trade_count: u32,
}

impl EphemeralWallet {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            keypair: Keypair::new(),
            created_at: now,
            last_used: now,
            trade_count: 0,
        }
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() >= ttl
    }

    pub fn is_stale(&self, stale_duration: Duration) -> bool {
        self.last_used.elapsed() >= stale_duration
    }

    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    pub fn use_wallet(&mut self) {
        self.last_used = Instant::now();
        self.trade_count += 1;
    }

    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// Wallet pool for managing ephemeral wallets used in arbitrage operations
pub struct WalletPool {
    wallets: VecDeque<EphemeralWallet>,
    config: WalletPoolConfig,
    collector: Pubkey,
    total_wallets_created: u64,
    total_sweeps_performed: u64,
}

impl WalletPool {
    /// Create a new wallet pool
    pub fn new(config: WalletPoolConfig, collector: Pubkey) -> Self {
        info!("🏦 Initializing wallet pool with collector: {}", collector);
        Self {
            wallets: VecDeque::new(),
            config,
            collector,
            total_wallets_created: 0,
            total_sweeps_performed: 0,
        }
    }

    /// Create a new wallet pool with default configuration
    pub fn new_with_defaults(collector: Pubkey) -> Self {
        Self::new(WalletPoolConfig::default(), collector)
    }

    /// Get or generate a signing wallet for trading
    pub fn get_signing_wallet(&mut self) -> &mut EphemeralWallet {
        // Clean up expired wallets if auto cleanup is enabled
        if self.config.auto_cleanup {
            self.cleanup_expired_wallets();
        }

        // Check if we can reuse an existing wallet
        let ttl = Duration::from_secs(self.config.wallet_ttl_secs);
        let has_valid_wallet = self.wallets.iter().any(|w| !w.is_expired(ttl));

        if !has_valid_wallet {
            // Generate a new wallet if none available or all expired
            self.generate_new_wallet_internal();
        }

        // Now get the first non-expired wallet (we know at least one exists)
        let ttl = Duration::from_secs(self.config.wallet_ttl_secs);
        for wallet in self.wallets.iter_mut() {
            if !wallet.is_expired(ttl) {
                wallet.use_wallet();
                debug!("♻️ Using wallet: {}", wallet.pubkey());
                return wallet;
            }
        }

        // This should never happen, but just in case
        unreachable!("No valid wallet found after generation")
    }

    /// Generate a fresh ephemeral wallet (internal method)
    fn generate_new_wallet_internal(&mut self) {
        // Enforce pool size limit
        if self.wallets.len() >= self.config.max_pool_size {
            warn!(
                "⚠️ Wallet pool at capacity ({}), removing oldest wallet",
                self.config.max_pool_size
            );
            self.wallets.pop_front();
        }

        let wallet = EphemeralWallet::new();
        self.total_wallets_created += 1;

        info!(
            "🆕 Generated new ephemeral wallet: {} (total: {})",
            wallet.pubkey(),
            self.total_wallets_created
        );

        self.wallets.push_back(wallet);
    }

    /// Get all wallets that have expired
    pub fn get_expired_wallets(&self) -> Vec<&EphemeralWallet> {
        let ttl = Duration::from_secs(self.config.wallet_ttl_secs);
        self.wallets.iter().filter(|w| w.is_expired(ttl)).collect()
    }

    /// Remove expired wallets from the pool
    pub fn cleanup_expired_wallets(&mut self) -> usize {
        let initial_count = self.wallets.len();
        let ttl = Duration::from_secs(self.config.wallet_ttl_secs);

        self.wallets.retain(|w| !w.is_expired(ttl));

        let removed_count = initial_count - self.wallets.len();
        if removed_count > 0 {
            debug!("🧹 Cleaned up {} expired wallets", removed_count);
        }

        removed_count
    }

    /// Check if a wallet should be swept based on balance
    pub fn should_sweep_wallet(&self, balance_lamports: u64) -> bool {
        balance_lamports > self.config.sweep_threshold_lamports
    }

    /// Calculate the amount to sweep (leave some for fees)
    pub fn calculate_sweep_amount(&self, balance_lamports: u64) -> Option<u64> {
        if balance_lamports <= self.config.fee_reserve_lamports {
            return None;
        }

        let sweep_amount = balance_lamports.saturating_sub(self.config.fee_reserve_lamports);
        if sweep_amount < self.config.sweep_threshold_lamports {
            return None;
        }

        Some(sweep_amount)
    }

    /// Generate a transaction to sweep funds from a wallet to collector
    pub fn create_sweep_transaction(
        &mut self,
        wallet: &EphemeralWallet,
        sweep_amount: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let instr = system_instruction::transfer(&wallet.pubkey(), &self.collector, sweep_amount);

        self.total_sweeps_performed += 1;
        info!(
            "💰 Creating sweep transaction: {} lamports from {} to {} (sweep #{})",
            sweep_amount,
            wallet.pubkey(),
            self.collector,
            self.total_sweeps_performed
        );

        Transaction::new_signed_with_payer(
            &[instr],
            Some(&wallet.pubkey()),
            &[&wallet.keypair],
            recent_blockhash,
        )
    }

    /// Get wallet pool statistics
    pub fn get_stats(&self) -> WalletPoolStats {
        let _now = Instant::now();
        let ttl = Duration::from_secs(self.config.wallet_ttl_secs);

        let active_wallets = self.wallets.iter().filter(|w| !w.is_expired(ttl)).count();
        let expired_wallets = self.wallets.len() - active_wallets;

        let avg_age = if !self.wallets.is_empty() {
            self.wallets.iter().map(|w| w.age().as_secs()).sum::<u64>() / self.wallets.len() as u64
        } else {
            0
        };

        WalletPoolStats {
            total_wallets: self.wallets.len(),
            active_wallets,
            expired_wallets,
            total_created: self.total_wallets_created,
            total_sweeps: self.total_sweeps_performed,
            average_age_secs: avg_age,
            collector_address: self.collector,
        }
    }

    /// Get all wallet public keys (for monitoring)
    pub fn get_all_pubkeys(&self) -> Vec<Pubkey> {
        self.wallets.iter().map(|w| w.pubkey()).collect()
    }

    /// Force cleanup of all wallets (useful for testing)
    pub fn clear_all_wallets(&mut self) {
        let count = self.wallets.len();
        self.wallets.clear();
        if count > 0 {
            warn!("🗑️ Force cleared {} wallets from pool", count);
        }
    }

    /// Update collector address
    pub fn update_collector(&mut self, new_collector: Pubkey) {
        info!(
            "🔄 Updating collector address from {} to {}",
            self.collector, new_collector
        );
        self.collector = new_collector;
    }
}

/// Statistics about the wallet pool
#[derive(Debug, Clone)]
pub struct WalletPoolStats {
    pub total_wallets: usize,
    pub active_wallets: usize,
    pub expired_wallets: usize,
    pub total_created: u64,
    pub total_sweeps: u64,
    pub average_age_secs: u64,
    pub collector_address: Pubkey,
}

impl Default for WalletPoolStats {
    fn default() -> Self {
        Self {
            total_wallets: 0,
            active_wallets: 0,
            expired_wallets: 0,
            total_created: 0,
            total_sweeps: 0,
            average_age_secs: 0,
            collector_address: Pubkey::default(),
        }
    }
}
