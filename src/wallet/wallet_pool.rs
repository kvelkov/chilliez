// src/wallet/wallet_pool.rs

use solana_sdk::{
    signature::{Keypair, Signer},
    pubkey::Pubkey,
    system_instruction,
    transaction::Transaction,
    // Removed CommitmentConfig as it's not used in this file directly
    // It's used in the example, but the module itself doesn't need it.
};
use std::time::{Duration, Instant};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct EphemeralWallet {
    pub keypair: Keypair,
    pub created_at: Instant,
}

impl EphemeralWallet {
    pub fn new() -> Self {
        Self {
            keypair: Keypair::new(),
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() >= ttl
    }

    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }
}

pub struct WalletPool {
    pub wallets: VecDeque<EphemeralWallet>,
    pub ttl: Duration,
    pub collector: Pubkey,
}

impl WalletPool {
    pub fn new(ttl_secs: u64, collector: Pubkey) -> Self {
        Self {
            wallets: VecDeque::new(),
            ttl: Duration::from_secs(ttl_secs),
            collector,
        }
    }

    pub fn generate_wallet(&mut self) -> &EphemeralWallet {
        let wallet = EphemeralWallet::new();
        self.wallets.push_back(wallet);
        self.wallets.back().unwrap()
    }

    pub fn expired_wallets(&self) -> Vec<&EphemeralWallet> {
        self.wallets
            .iter()
            .filter(|w| w.is_expired(self.ttl))
            .collect()
    }

    pub fn remove_expired(&mut self) {
        self.wallets
            .retain(|w| !w.is_expired(self.ttl));
    }

    /// Generate a transaction to sweep funds from a wallet to collector
    pub fn create_sweep_transaction(
        &self,
        wallet: &EphemeralWallet,
        lamports: u64,
        recent_blockhash: solana_sdk::hash::Hash,
    ) -> Transaction {
        let instr = system_instruction::transfer(
            &wallet.pubkey(),
            &self.collector,
            lamports,
        );

        Transaction::new_signed_with_payer(
            &[instr],
            Some(&wallet.pubkey()),
            &[&wallet.keypair],
            recent_blockhash,
        )
    }
}