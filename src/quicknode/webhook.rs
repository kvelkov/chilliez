// src/quicknode/webhook.rs

use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    quicknode::function_client::{ArbitrageOpportunity as QuickNodeArbitrageOpportunity, QuickNodeFunctionClient},
};
use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;
use std::sync::Arc;
use log::{info, error};

// This struct will hold the QuickNode function client and a sender
// to communicate with the arbitrage orchestrator
pub struct QuickNodeWebhookProcessor {
    function_client: QuickNodeFunctionClient,
    // Sender to send processed opportunities to the orchestrator
    opportunity_sender: mpsc::UnboundedSender<MultiHopArbOpportunity>,
}

impl QuickNodeWebhookProcessor {
    pub fn new(
        function_url: String,
        opportunity_sender: mpsc::UnboundedSender<MultiHopArbOpportunity>,
    ) -> Self {
        Self {
            function_client: QuickNodeFunctionClient::new(function_url),
            opportunity_sender,
        }
    }

    // This function would be called by your HTTP server when a webhook is received
    pub async fn process_webhook_data(&self, raw_webhook_payload: Value) -> Result<()> {
        info!("Received webhook payload from QuickNode.");

        // The QuickNode filter returns an object with a 'data' array if opportunities are found
        // You need to extract the 'arbitrageTransactions' from this structure.
        if let Some(data_array) = raw_webhook_payload["data"].as_array() {
            for block_data in data_array {
                if let Some(arbitrage_txs) = block_data["arbitrageTransactions"].as_array() {
                    for arb_tx_value in arbitrage_txs {
                        // Deserialize the individual arbitrage transaction into your struct
                        match serde_json::from_value::<QuickNodeArbitrageOpportunity>(arb_tx_value.clone()) {
                            Ok(qn_opp) => {
                                info!("Successfully parsed arbitrage opportunity from QuickNode Function.");
                                // Convert QuickNode's ArbitrageOpportunity to your bot's MultiHopArbOpportunity
                                // This is a crucial mapping step based on your bot's internal data model
                                let multi_hop_opp = self.convert_qn_opp_to_multihop(&qn_opp)?;
                                if let Err(e) = self.opportunity_sender.send(multi_hop_opp) {
                                    error!("Failed to send opportunity to orchestrator: {:?}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize arbitrage opportunity from QuickNode: {:?}", e);
                            }
                        }
                    }
                }
            }
        } else {
            info!("No arbitrage transactions found in this webhook payload.");
        }
        Ok(())
    }

    // Helper function to convert QuickNode's ArbitrageOpportunity to your bot's MultiHopArbOpportunity
    fn convert_qn_opp_to_multihop(&self, qn_opp: &QuickNodeArbitrageOpportunity) -> Result<MultiHopArbOpportunity> {
        // Map DEX string to DexType
        fn dex_from_str(dex: &str) -> DexType {
            match dex.to_lowercase().as_str() {
                "orca" => DexType::Orca,
                "raydium" => DexType::Raydium,
                "jupiter" => DexType::Jupiter,
                "meteora" => DexType::Meteora,
                "lifinity" => DexType::Lifinity,
                "phoenix" => DexType::Phoenix,
                _ => DexType::Unknown(dex.to_string()),
            }
        }

        // Build hops
        let mut hops = Vec::new();
        let mut dex_path = Vec::new();
        let mut pool_path = Vec::new();
        let mut intermediate_tokens = Vec::new();
        for swap in &qn_opp.dex_swaps {
            let dex = dex_from_str(&swap.dex);
            let pool = swap.program_id.parse::<Pubkey>().unwrap_or(Pubkey::default());
            dex_path.push(dex.clone());
            pool_path.push(pool);
            intermediate_tokens.push(swap.token_out.clone());
            hops.push(crate::arbitrage::opportunity::ArbHop {
                dex,
                pool,
                input_token: swap.token_in.clone(),
                output_token: swap.token_out.clone(),
                input_amount: swap.amount_in,
                expected_output: swap.amount_out,
            });
        }

        // Use first and last swaps for input/output
        let input_token = qn_opp.dex_swaps.first().map(|s| s.token_in.clone()).unwrap_or_default();
        let output_token = qn_opp.dex_swaps.last().map(|s| s.token_out.clone()).unwrap_or_default();
        let input_amount = qn_opp.dex_swaps.first().map(|s| s.amount_in).unwrap_or(0.0);
        let expected_output = qn_opp.dex_swaps.last().map(|s| s.amount_out).unwrap_or(0.0);
        let input_token_mint = qn_opp.dex_swaps.first().and_then(|s| s.token_in.parse().ok()).unwrap_or(Pubkey::default());
        let output_token_mint = qn_opp.dex_swaps.last().and_then(|s| s.token_out.parse().ok()).unwrap_or(Pubkey::default());

        Ok(MultiHopArbOpportunity {
            id: qn_opp.signature.clone(),
            hops,
            total_profit: qn_opp.opportunities.first().and_then(|o| o.estimated_profit).unwrap_or(0.0),
            profit_pct: qn_opp.price_impact, // Use price impact as a proxy for profit pct if needed
            input_token,
            output_token,
            input_amount,
            expected_output,
            dex_path,
            pool_path,
            risk_score: None,
            notes: None,
            estimated_profit_usd: Some(qn_opp.estimated_value_usd),
            input_amount_usd: None,
            output_amount_usd: None,
            intermediate_tokens,
            source_pool: Arc::new(PoolInfo::default()), // Placeholder, can be looked up if needed
            target_pool: Arc::new(PoolInfo::default()), // Placeholder, can be looked up if needed
            input_token_mint,
            output_token_mint,
            intermediate_token_mint: None,
            estimated_gas_cost: None,
            detected_at: None,
        })
    }
}
