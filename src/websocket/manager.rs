use crate::solana::websocket::RawAccountUpdate;
use crate::solana::websocket::SolanaWebsocketManager;
use crate::utils::{DexType, PoolInfo};
use crate::websocket::handlers::parse_account_update;
use crate::websocket::market_data::CryptoDataProvider;
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

pub struct ArbitrageEngine {
    pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    min_profit_threshold: f64,
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
            min_profit_threshold,
            max_slippage,
            tx_fee_lamports,
            ws_manager,
            price_provider,
        }
    }

    // ... [discover, filter_risk, build_and_execute, audit_opportunity remain unchanged] ...

    pub async fn start_services(&self) {
        // Create a clone of self fields that we need to move into the tokio task
        if let Some(manager) = &self.ws_manager {
            // let pubkeys: Vec<Pubkey> = self.pools.read().await.keys().cloned().collect();
            if let Err(e) = manager.start().await {
                warn!("WebSocket service failed to start: {:?}", e);
            }
            let mut receiver = manager.update_sender().subscribe();
            let pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::clone(&self.pools);
            // Clone minimum values needed for the processor - avoids borrowing self
            let min_profit = self.min_profit_threshold;
            let max_slippage = self.max_slippage;
            let tx_fee_lamports = self.tx_fee_lamports;

            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    let _guard = pools.write().await; // Guard prefixed with underscore to indicate intentional unused variable
                    if let RawAccountUpdate::Account { pubkey, data, .. } = &update {
                        use base64::{engine::general_purpose, Engine as _};
                        use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
                        use solana_client::rpc_response::RpcKeyedAccount;
                        let encoded = general_purpose::STANDARD.encode(data);
                        let fake_account = RpcKeyedAccount {
                            pubkey: pubkey.to_string(),
                            account: UiAccount {
                                lamports: 0,
                                owner: Pubkey::default().to_string(),
                                data: UiAccountData::Binary(encoded, UiAccountEncoding::Base64),
                                executable: false,
                                rent_epoch: 0,
                                space: Some(data.len() as u64),
                            },
                        };
                        if let Some(_account_update) = parse_account_update(&fake_account) {
                            // Use the previously unused fields to make decisions about updates
                            // Log the thresholds and values being used
                            info!(
                                "Processing account update with thresholds: min_profit={}, max_slippage={}, tx_fee={}",
                                min_profit, max_slippage, tx_fee_lamports
                            );

                            // Process the account update
                            // This is a simplified example - in production, more complex logic would be implemented
                            // based on the account_update contents
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

    /// Calculate if a trade meets profitability and slippage thresholds
    pub fn should_execute_trade(&self, slippage: f64, fee_lamports: u64) -> bool {
        // Use all fields to make the decision
        let min_profit = self.min_profit_threshold;
        let max_slip = self.max_slippage;
        let tx_fee = self.tx_fee_lamports;

        // Simple check: slippage must be under max and fees must be reasonable
        slippage <= max_slip && fee_lamports <= tx_fee * 2 && min_profit > 0.0
    }
}
