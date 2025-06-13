// src/webhooks/types.rs
//! Types and structures for webhook handling

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// Helius webhook notification structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeliusWebhookNotification {
    pub webhook: WebhookInfo,
    #[serde(rename = "txnSignature")]
    pub txn_signature: String,
    pub timestamp: u64,
    #[serde(rename = "slot")]
    pub slot: u64,
    #[serde(rename = "programId")]
    pub program_id: String,
    pub accounts: Vec<String>,
    #[serde(rename = "nativeTransfers")]
    pub native_transfers: Option<Vec<NativeTransfer>>,
    #[serde(rename = "tokenTransfers")]
    pub token_transfers: Option<Vec<TokenTransfer>>,
    #[serde(rename = "accountData")]
    pub account_data: Option<Vec<AccountData>>,
    #[serde(rename = "events")]
    pub events: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookInfo {
    #[serde(rename = "webhookURL")]
    pub webhook_url: String,
    #[serde(rename = "webhookType")]
    pub webhook_type: String,
    #[serde(rename = "accountAddresses")]
    pub account_addresses: Vec<String>,
    #[serde(rename = "programIds")]
    pub program_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativeTransfer {
    #[serde(rename = "fromUserAccount")]
    pub from_user_account: String,
    #[serde(rename = "toUserAccount")]
    pub to_user_account: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenTransfer {
    #[serde(rename = "fromUserAccount")]
    pub from_user_account: String,
    #[serde(rename = "toUserAccount")]
    pub to_user_account: String,
    #[serde(rename = "fromTokenAccount")]
    pub from_token_account: String,
    #[serde(rename = "toTokenAccount")]
    pub to_token_account: String,
    #[serde(rename = "tokenAmount")]
    pub token_amount: u64,
    #[serde(rename = "mint")]
    pub mint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountData {
    pub account: String,
    #[serde(rename = "nativeBalanceChange")]
    pub native_balance_change: i64,
    #[serde(rename = "tokenBalanceChanges")]
    pub token_balance_changes: Option<Vec<TokenBalanceChange>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenBalanceChange {
    #[serde(rename = "userAccount")]
    pub user_account: String,
    #[serde(rename = "tokenAccount")]
    pub token_account: String,
    pub mint: String,
    #[serde(rename = "rawTokenAmount")]
    pub raw_token_amount: RawTokenAmount,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawTokenAmount {
    #[serde(rename = "tokenAmount")]
    pub token_amount: String,
    pub decimals: u8,
}

/// Internal pool update event
#[derive(Debug, Clone)]
pub struct PoolUpdateEvent {
    pub pool_address: Pubkey,
    pub program_id: Pubkey,
    pub signature: String,
    pub timestamp: u64,
    pub slot: u64,
    pub update_type: PoolUpdateType,
    pub token_transfers: Vec<TokenTransfer>,
    pub account_changes: Vec<AccountData>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PoolUpdateType {
    Swap,
    AddLiquidity,
    RemoveLiquidity,
    PriceUpdate,
    Unknown,
}

/// DEX program IDs we monitor
pub struct DexPrograms;

impl DexPrograms {
    // Orca Whirlpool Program
    pub const ORCA_WHIRLPOOL: &'static str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
    
    // Raydium AMM Program
    pub const RAYDIUM_AMM: &'static str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
    
    // Raydium CLMM Program
    pub const RAYDIUM_CLMM: &'static str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
    
    // Meteora DLMM Program
    pub const METEORA_DLMM: &'static str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";
    
    // Meteora Dynamic AMM Program
    pub const METEORA_AMM: &'static str = "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB";
    
    // Lifinity Program
    pub const LIFINITY: &'static str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27RLAAEwqEEKLw";
    
    // Serum DEX Program
    pub const SERUM_DEX: &'static str = "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin";

    /// Get all monitored program IDs
    pub fn all_program_ids() -> Vec<&'static str> {
        vec![
            Self::ORCA_WHIRLPOOL,
            Self::RAYDIUM_AMM,
            Self::RAYDIUM_CLMM,
            Self::METEORA_DLMM,
            Self::METEORA_AMM,
            Self::LIFINITY,
            Self::SERUM_DEX,
        ]
    }

    /// Get program ID by DEX name
    pub fn get_program_id(dex_name: &str) -> Option<&'static str> {
        match dex_name.to_lowercase().as_str() {
            "orca" | "whirlpool" => Some(Self::ORCA_WHIRLPOOL),
            "raydium" | "raydium_amm" => Some(Self::RAYDIUM_AMM),
            "raydium_clmm" => Some(Self::RAYDIUM_CLMM),
            "meteora" | "meteora_dlmm" => Some(Self::METEORA_DLMM),
            "meteora_amm" => Some(Self::METEORA_AMM),
            "lifinity" => Some(Self::LIFINITY),
            "serum" => Some(Self::SERUM_DEX),
            _ => None,
        }
    }
}
