// src/webhooks/processor.rs
//! Pool update processor for handling real-time webhook notifications

use crate::utils::PoolInfo;
use crate::webhooks::types::{QuickNodeWebhookPayload, QuickNodeArbitrageTransaction};
use crate::arbitrage::opportunity::{MultiHopArbOpportunity, ArbHop};
use anyhow::Result as AnyhowResult;
use solana_sdk::pubkey::Pubkey;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use crate::webhooks::types::{PoolUpdateEvent, PoolUpdateType};

/// Processes webhook notifications and updates pool information
pub struct PoolUpdateProcessor {
    callbacks: Arc<Mutex<Vec<Box<dyn Fn(PoolUpdateEvent) + Send + Sync>>>>,
}

#[derive(Debug, Default, Clone)]
pub struct PoolUpdateProcessorStats {
    pub total_notifications: u64,
    pub successful_updates: u64,
}

impl PoolUpdateProcessor {
    /// Create a new pool update processor
    pub fn new() -> Self {
        Self {
            callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_update_callback<F>(&self, callback: F)
    where
        F: Fn(PoolUpdateEvent) + Send + Sync + 'static,
    {
        self.callbacks.lock().unwrap().push(Box::new(callback));
    }

    fn notify_callbacks(&self, event: PoolUpdateEvent) {
        for cb in self.callbacks.lock().unwrap().iter() {
            cb(event.clone());
        }
    }

    /// Process a QuickNode webhook payload and send opportunities to the orchestrator
    pub async fn process_quicknode_opportunity(
        payload: QuickNodeWebhookPayload,
        sender: &UnboundedSender<MultiHopArbOpportunity>,
        processor: &PoolUpdateProcessor,
    ) -> AnyhowResult<()> {
        for block in payload.data {
            for tx in block.arbitrage_transactions {
                let opp = Self::map_quicknode_tx_to_multihop(tx)?;
                sender.send(opp.clone())?;
                // Map to PoolUpdateEvent and notify callbacks
                let event = PoolUpdateEvent {
                    pool_address: Pubkey::default(), // TODO: map from tx
                    program_id: Pubkey::default(),   // TODO: map from tx
                    signature: opp.id.clone(),
                    timestamp: 0, // TODO: map from tx
                    slot: 0,      // TODO: map from tx
                    update_type: PoolUpdateType::Swap, // TODO: map from tx
                    token_transfers: vec![], // TODO: map from tx
                    account_changes: vec![], // TODO: map from tx
                };
                processor.notify_callbacks(event);
            }
        }
        Ok(())
    }

    fn map_quicknode_tx_to_multihop(tx: QuickNodeArbitrageTransaction) -> AnyhowResult<MultiHopArbOpportunity> {
        // Map DEX string to DexType
        fn dex_from_str(dex: &str) -> crate::utils::DexType {
            match dex.to_lowercase().as_str() {
                "orca" => crate::utils::DexType::Orca,
                "raydium" => crate::utils::DexType::Raydium,
                "jupiter" => crate::utils::DexType::Jupiter,
                "meteora" => crate::utils::DexType::Meteora,
                "lifinity" => crate::utils::DexType::Lifinity,
                "phoenix" => crate::utils::DexType::Phoenix,
                _ => crate::utils::DexType::Unknown(dex.to_string()),
            }
        }
        let mut hops = Vec::new();
        let mut dex_path = Vec::new();
        let mut pool_path = Vec::new();
        let mut intermediate_tokens = Vec::new();
        for swap in &tx.dex_swaps {
            let dex = dex_from_str(&swap.dex);
            let pool = swap.program_id.parse::<Pubkey>().unwrap_or(Pubkey::default());
            dex_path.push(dex.clone());
            pool_path.push(pool);
            intermediate_tokens.push(swap.token_out.clone());
            hops.push(ArbHop {
                dex,
                pool,
                input_token: swap.token_in.clone(),
                output_token: swap.token_out.clone(),
                input_amount: swap.amount_in,
                expected_output: swap.amount_out,
            });
        }
        let input_token = tx.dex_swaps.first().map(|s| s.token_in.clone()).unwrap_or_default();
        let output_token = tx.dex_swaps.last().map(|s| s.token_out.clone()).unwrap_or_default();
        let input_amount = tx.dex_swaps.first().map(|s| s.amount_in).unwrap_or(0.0);
        let expected_output = tx.dex_swaps.last().map(|s| s.amount_out).unwrap_or(0.0);
        let input_token_mint = tx.dex_swaps.first().and_then(|s| s.token_in.parse().ok()).unwrap_or(Pubkey::default());
        let output_token_mint = tx.dex_swaps.last().and_then(|s| s.token_out.parse().ok()).unwrap_or(Pubkey::default());
        Ok(MultiHopArbOpportunity {
            id: tx.signature.clone(),
            hops,
            total_profit: tx.opportunities.first().and_then(|o| o.estimated_profit).unwrap_or(0.0),
            profit_pct: tx.price_impact,
            input_token,
            output_token,
            input_amount,
            expected_output,
            dex_path,
            pool_path,
            risk_score: None,
            notes: None,
            estimated_profit_usd: Some(tx.estimated_value_usd),
            input_amount_usd: None,
            output_amount_usd: None,
            intermediate_tokens,
            source_pool: Arc::new(PoolInfo::default()),
            target_pool: Arc::new(PoolInfo::default()),
            input_token_mint,
            output_token_mint,
            intermediate_token_mint: None,
            estimated_gas_cost: None,
            detected_at: None,
        })
    }

    pub async fn get_stats(&self) -> PoolUpdateProcessorStats {
        // For now, return dummy stats (extend as needed)
        PoolUpdateProcessorStats {
            total_notifications: 0,
            successful_updates: 0,
        }
    }
    pub async fn increment_successful_updates(&self) {
        // No-op for now; implement if you add tracking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_processor_creation() {
        let processor = PoolUpdateProcessor::new();
        let stats = processor.get_stats().await;
        assert_eq!(stats.total_notifications, 0);
    }

    #[tokio::test]
    async fn test_stats_update() {
        let processor = PoolUpdateProcessor::new();
        processor.increment_successful_updates().await;

        let stats = processor.get_stats().await;
        assert_eq!(stats.successful_updates, 1);
    }
}
