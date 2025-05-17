use crate::arbitrage::detector::ArbitrageDetector;
use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::metrics::Metrics;
use crate::solana::websocket::{RawAccountUpdate, SolanaWebsocketManager};
use crate::utils::PoolInfo;
use crate::websocket::market_data::CryptoDataProvider;
use crate::websocket::types::AccountUpdate;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

    /// Returns true if the given slippage and fee are within engine's configured limits
    pub fn should_execute_trade(&self, slippage: f64, fee_lamports: u64) -> bool {
        slippage <= self.max_slippage && fee_lamports <= self.tx_fee_lamports
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
                        RawAccountUpdate::Account {
                            pubkey,
                            data,
                            timestamp: _,
                        } => {
                            // Directly parse AccountUpdate from data and pubkey
                            if data.len() >= 16 {
                                let reserve_a =
                                    u64::from_le_bytes(data[0..8].try_into().unwrap_or_default());
                                let reserve_b =
                                    u64::from_le_bytes(data[8..16].try_into().unwrap_or_default());
                                let parsed_update = AccountUpdate {
                                    pubkey,
                                    reserve_a,
                                    reserve_b,
                                };
                                let mut guard = pools.write().await;
                                if let Some(pool_arc) = guard.get_mut(&parsed_update.pubkey) {
                                    let pool = Arc::make_mut(pool_arc);
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

    /// Discover and return all multi-hop arbitrage opportunities using the new detector
    pub async fn discover_multihop_opportunities(&self) -> Vec<MultiHopArbOpportunity> {
        let mut metrics = Metrics::new();
        let min_profit = self.get_min_profit_threshold().await;
        let detector = ArbitrageDetector::new(min_profit);
        let pools_guard = self.pools.read().await;
        detector
            .find_all_multihop_opportunities(&*pools_guard, &mut metrics)
            .await
    }

    /// Given a MultiHopArbOpportunity, return Vec<Arc<PoolInfo>> for all hops (or None if any missing)
    pub async fn resolve_pools_for_opportunity(
        &self,
        opp: &MultiHopArbOpportunity,
    ) -> Option<Vec<Arc<PoolInfo>>> {
        let pools_guard = self.pools.read().await;
        let mut result = Vec::with_capacity(opp.hops.len());
        for hop in &opp.hops {
            if let Some(pool) = pools_guard.get(&hop.pool) {
                result.push(pool.clone());
            } else {
                return None; // If any pool is missing, return None
            }
        }
        Some(result)
    }

    // Example: FeeManager integration for trade evaluation
    #[allow(dead_code)]
    pub fn example_fee_manager_usage(
        pool: &crate::utils::PoolInfo,
        input_amount: &crate::utils::TokenAmount,
        is_a_to_b: bool,
        last_update_timestamp: Option<u64>,
    ) {
        let _fee_breakdown = FeeManager::estimate_pool_swap_integrated(
            pool,
            input_amount,
            is_a_to_b,
            last_update_timestamp,
        );
        let _multi_hop_breakdown = FeeManager::estimate_multi_hop_with_model(
            &[pool],
            &[input_amount.clone()],
            &[is_a_to_b],
            &[(Some(pool.fee_numerator), Some(pool.fee_denominator), last_update_timestamp)],
            &XYKSlippageModel,
        );
        let _fee_in_usdc = FeeManager::convert_fee_to_reference_token(
            _fee_breakdown.expected_fee,
            &pool.token_a.symbol,
            "USDC",
        );
    }
}
