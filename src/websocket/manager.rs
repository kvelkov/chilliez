use crate::dex::pool::{DexType, PoolInfo};
use crate::solana::websocket::SolanaWebsocketManager;
use crate::websocket::handlers::parse_account_update;
use crate::websocket::market_data::CryptoDataProvider;
use crate::solana::websocket::RawAccountUpdate;
use crate::arbitrage::calculator::is_profitable;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use base64::Engine;

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
        if let Some(manager) = &self.ws_manager {
            // let pubkeys: Vec<Pubkey> = self.pools.read().await.keys().cloned().collect();
            if let Err(e) = manager.start().await {
                warn!("WebSocket service failed to start: {:?}", e);
            }
            let mut receiver = manager.update_sender().subscribe();
            let pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::clone(&self.pools);

            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    let mut guard = pools.write().await;
                    if let RawAccountUpdate::Account { pubkey, data, .. } = &update {
                        use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
                        use solana_client::rpc_response::RpcKeyedAccount;
                        use base64::{engine::general_purpose, Engine as _};
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
                        if let Some(account_update) = parse_account_update(&fake_account) {
                            // Directly use account_update, no PoolUpdate variant
                            // Update logic here if needed
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
