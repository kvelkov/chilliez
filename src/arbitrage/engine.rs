use crate::arbitrage::calculator::is_profitable;
use crate::dex::pool::{DexType, PoolInfo};
use crate::solana::websocket::{SolanaWebsocketManager, RawAccountUpdate};
use crate::websocket::market_data::CryptoDataProvider;
use crate::websocket::types::AccountUpdate;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ArbOpportunity {
    pub route: Vec<Pubkey>,
    pub profit_percentage: f64,
    pub input_token: String,
    pub output_token: String,
    pub input_amount: f64,
    pub expected_output: f64,
    #[allow(dead_code)]
    pub dex_path: Vec<DexType>,
}

// ArbOpportunity implementation removed in favor of using the canonical is_profitable function

pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: Arc<RwLock<f64>>,
    max_slippage: f64,
    tx_fee_lamports: u64,
    ws_manager: Option<SolanaWebsocketManager>,
    price_provider: Option<Box<dyn CryptoDataProvider + Send + Sync>>,
}

impl ArbitrageEngine {
    pub fn new(
        pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
        min_profit_threshold: f64,
        max_slippage: f64,
        tx_fee_lamports: u64,
        ws_manager: Option<SolanaWebsocketManager>,
        price_provider: Option<Box<dyn CryptoDataProvider + Send + Sync>>,
    ) -> Self {
        Self {
            pools,
            min_profit_threshold: Arc::new(RwLock::new(min_profit_threshold)),
            max_slippage,
            tx_fee_lamports,
            ws_manager,
            price_provider,
        }
    }

    /// Update min_profit_threshold dynamically at runtime
    pub async fn set_min_profit_threshold(&self, new_threshold: f64) {
        let mut guard = self.min_profit_threshold.write().await;
        *guard = new_threshold;
    }

    /// Get the current min_profit_threshold value
    pub async fn get_min_profit_threshold(&self) -> f64 {
        *self.min_profit_threshold.read().await
    }

    pub async fn start_services(&self) {
        if let Some(manager) = &self.ws_manager {
            if let Err(e) = manager.start().await {
                warn!("WebSocket service failed to start: {:?}", e);
            }

            let pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::clone(&self.pools);
            let mut receiver = manager.update_sender().subscribe();
            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    match update {
                        RawAccountUpdate::Account { pubkey, data, timestamp: _ } => {
                            // Directly parse AccountUpdate from data and pubkey
                            if data.len() >= 16 {
                                let reserve_a = u64::from_le_bytes(data[0..8].try_into().unwrap_or_default());
                                let reserve_b = u64::from_le_bytes(data[8..16].try_into().unwrap_or_default());
                                let parsed_update = AccountUpdate { pubkey, reserve_a, reserve_b };
                                let mut guard = pools.write().await;
                                if let Some(pool_arc) = guard.get_mut(&parsed_update.pubkey) {
                                    let mut pool = Arc::make_mut(pool_arc);
                                    pool.token_a.reserve = parsed_update.reserve_a;
                                    pool.token_b.reserve = parsed_update.reserve_b;
                                }
                            }
                        }
                        RawAccountUpdate::Disconnected { .. } => {
                            // handle disconnect if needed
                        }
                        RawAccountUpdate::Error { .. } => {
                            // handle error if needed
                        }
                    }
                }
            });
        }

        if let Some(provider) = &self.price_provider {
            if let Some(price) = provider.get_price("SOL").await {
                info!("Price feed test: SOL = {:.4}", price);
            }
        }
    }
}
