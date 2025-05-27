//! Integration utility to exercise all DEX infrastructure and eliminate unused warnings.

use crate::dex::lifinity::{LifinityClient, LifinityPoolParser, LIFINITY_PROGRAM_ID};
use crate::dex::meteora::MeteoraClient;
use crate::dex::orca::{OrcaClient, OrcaPoolParser, ORCA_SWAP_PROGRAM_ID_V2}; 
use crate::dex::phoenix::{PhoenixClient}; // Removed unused _PHOENIX_PROGRAM_ID import
use crate::dex::pool::get_pool_parser_fn_for_program;
use crate::dex::quote::DexClient;
use crate::dex::raydium::{RaydiumClient, RaydiumPoolParser, RAYDIUM_LIQUIDITY_PROGRAM_V4}; 
use crate::dex::whirlpool_parser::{WhirlpoolPoolParser, ORCA_WHIRLPOOL_PROGRAM_ID}; 
use crate::dex::whirlpool::WhirlpoolClient; 
use crate::utils::{DexType, PoolInfo, PoolToken, PoolParser as UtilsPoolParser}; 
use serde_json;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

// Prefixed as unused by direct calls in lib, should be called by a test runner or main test fn.
pub fn _exercise_parser_registry() {
    let orca_id = Pubkey::from_str(ORCA_SWAP_PROGRAM_ID_V2).unwrap();
    let raydium_id = Pubkey::from_str(RAYDIUM_LIQUIDITY_PROGRAM_V4).unwrap();
    let whirlpool_id = Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).unwrap();
    let lifinity_id = Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap();
    // PHOENIX_PROGRAM_ID is const _PHOENIX_PROGRAM_ID, not used for pool parsing typically
    // let phoenix_id = Pubkey::from_str(_PHOENIX_PROGRAM_ID).unwrap(); 
    let ids = vec![orca_id, raydium_id, whirlpool_id, lifinity_id]; // Removed phoenix_id
    for id in ids {
        let _ = get_pool_parser_fn_for_program(&id);
    }
    let dummy = Pubkey::new_unique();
    let dummy_data = vec![0u8; 700]; // Increased size to pass Whirlpool initial check
    let _ = OrcaPoolParser::parse_pool_data(dummy, &dummy_data);
    let _ = RaydiumPoolParser::parse_pool_data(dummy, &dummy_data);
    let _ = WhirlpoolPoolParser::parse_pool_data(dummy, &dummy_data);
    let _ = LifinityPoolParser::parse_pool_data(dummy, &dummy_data);
    let _ = OrcaPoolParser::get_program_id();
    let _ = RaydiumPoolParser::get_program_id();
    let _ = WhirlpoolPoolParser::get_program_id();
    let _ = LifinityPoolParser::get_program_id();
}

// Prefixed
pub async fn _exercise_dex_clients() {
    use crate::dex::http_utils_shared::log_timed_request;
    use std::sync::Arc; 
    use crate::cache::Cache; 
    use crate::config::settings::Config; 

    let app_config = Arc::new(Config::from_env()); 
    let cache = Arc::new(Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs).await.unwrap());

    let orca = log_timed_request("Initialize Orca Client", async { OrcaClient::new(cache.clone(), None) }).await;
    let raydium =
        log_timed_request("Initialize Raydium Client", async { RaydiumClient::new(cache.clone(), None) }).await;
    let whirlpool = log_timed_request("Initialize Whirlpool Client", async {
        WhirlpoolClient::new(cache.clone(), None)
    })
    .await;
    let lifinity = log_timed_request("Initialize Lifinity Client", async {
        LifinityClient::new(cache.clone(), None)
    })
    .await;
    let meteora =
        log_timed_request("Initialize Meteora Client", async { MeteoraClient::new(cache.clone(), None) }).await;
    let phoenix =
        log_timed_request("Initialize Phoenix Client", async { PhoenixClient::new(cache.clone(), None) }).await;
    
    let _ = orca._get_api_key(); // Use prefixed
    let _ = raydium.get_api_key(); // Fixed: use get_api_key, not _get_api_key
    let _ = whirlpool.get_api_key(); // Fixed: use get_api_key, not _get_api_key
    let _ = lifinity.get_api_key(); // Assuming this one might be used or was missed for prefixing
    let _ = phoenix._get_api_key(); // Use prefixed
    
    let _ = orca.get_supported_pairs();
    let _ = raydium.get_supported_pairs();
    let _ = whirlpool.get_supported_pairs();
    let _ = lifinity.get_supported_pairs();
    let _ = meteora.get_supported_pairs();
    let _ = phoenix.get_supported_pairs();
    
    let _ = orca.get_name();
    let _ = raydium.get_name();
    let _ = whirlpool.get_name();
    let _ = lifinity.get_name();
    let _ = meteora.get_name();
    let _ = phoenix.get_name();
    
    let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // USDC mint
    let sol = "So11111111111111111111111111111111111111112"; // SOL mint
    let _ = orca.get_best_swap_quote(usdc, sol, 1_000_000).await;
    let _ = raydium.get_best_swap_quote(usdc, sol, 1_000_000).await;
    let _ = whirlpool.get_best_swap_quote(usdc, sol, 1_000_000).await;
    let _ = lifinity.get_best_swap_quote(usdc, sol, 1_000_000).await;
    let _ = meteora.get_best_swap_quote(usdc, sol, 1_000_000).await;
    let _ = phoenix.get_best_swap_quote(usdc, sol, 1_000_000).await;
}// Prefixed
pub fn _exercise_serde() {
    let pool = PoolInfo {
        address: Pubkey::new_unique(),
        name: "TestPool".to_string(),
        token_a: PoolToken {
            mint: Pubkey::new_unique(),
            symbol: "A".to_string(),
            decimals: 6,
            reserve: 1_000_000,
        },
        token_b: PoolToken {
            mint: Pubkey::new_unique(),
            symbol: "B".to_string(),
            decimals: 6,
            reserve: 2_000_000,
        },
        fee_numerator: 30,
        fee_denominator: 10000,
        last_update_timestamp: 0,
        dex_type: DexType::Orca,
    };
    let json = serde_json::to_string(&pool).unwrap();
    let _pool2: PoolInfo = serde_json::from_str(&json).unwrap();
}