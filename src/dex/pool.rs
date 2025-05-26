// src/dex/pool.rs
//! Defines PoolInfo, PoolMap, and the POOL_PARSER_REGISTRY for associating
//! program IDs with their respective pool data parsers.

// Removed DexType, PoolToken, TokenAmount from this specific import line as they are not directly used in this file.
// PoolInfo (used in PoolParseFn) and UtilsPoolParser are used.
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser}; 
use anyhow::Result as AnyhowResult; 
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use log; 
use std::sync::Arc;

// This type alias IS USED by POOL_PARSER_REGISTRY.
// If POOL_PARSER_REGISTRY is flagged as unused, this might be transitively flagged.
// The registry's usage is critical and needs to be wired into the WebSocket processing logic.
pub type PoolParseFn = fn(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo>;

/// Static registry mapping DEX program IDs to their `PoolParseFn`.
/// This allows dynamic dispatch of parsing logic based on the account's owner program.
/// CRITICAL: This registry should be used by the Solana WebSocket message handler
/// (e.g., within SolanaWebsocketManager or a dedicated data processing service)
/// to parse incoming raw account data for subscribed pools.
/// The parsed PoolInfo should then be sent to ArbitrageEngine::update_pools.
pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, PoolParseFn>> = Lazy::new(|| {
    let mut m = HashMap::new();
    
    let orca_program_id = crate::dex::orca::OrcaPoolParser::get_program_id();
    m.insert(
        orca_program_id,
        crate::dex::orca::OrcaPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Orca legacy pool parser for program ID: {}", orca_program_id);

    let raydium_program_id = crate::dex::raydium::RaydiumPoolParser::get_program_id();
    m.insert(
        raydium_program_id,
        crate::dex::raydium::RaydiumPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Raydium pool parser for program ID: {}", raydium_program_id);

    let lifinity_program_id = crate::dex::lifinity::LifinityPoolParser::get_program_id();
    m.insert(
        lifinity_program_id,
        crate::dex::lifinity::LifinityPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Lifinity pool parser for program ID: {}", lifinity_program_id);

    let whirlpool_program_id = crate::dex::whirlpool_parser::WhirlpoolPoolParser::get_program_id();
    m.insert(
        whirlpool_program_id,
        crate::dex::whirlpool_parser::WhirlpoolPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Whirlpool parser for program ID: {}", whirlpool_program_id);

    if m.is_empty() {
        log::warn!("POOL_PARSER_REGISTRY is empty. No pool parsers were registered.");
    } else {
        log::info!("POOL_PARSER_REGISTRY initialized with {} parsers.", m.len());
    }
    m
});

/// Retrieves a `PoolParseFn` from the registry for a given DEX program ID.
/// CRITICAL: This function is the intended interface to the POOL_PARSER_REGISTRY
/// and should be called by the WebSocket data processing logic.
pub fn get_pool_parser_fn_for_program(program_id: &Pubkey) -> Option<PoolParseFn> {
    POOL_PARSER_REGISTRY.get(program_id).copied()
}

pub struct PoolMap {
    pub _pools: HashMap<Pubkey, Arc<PoolInfo>>, // Prefixed
}
impl PoolMap {
    pub fn new() -> Self {
        Self {
            _pools: HashMap::new(),
        }
    }

    pub fn _from_hashmap(pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Self { // Prefixed
        Self { _pools: pools }
    }

    pub fn _add_pool(&mut self, pool: Arc<PoolInfo>) { // Prefixed
        self._pools.insert(pool.address, pool);
    }

    pub fn _get_pool(&self, address: &Pubkey) -> Option<&Arc<PoolInfo>> { // Prefixed
        self._pools.get(address)
    }

    pub fn _candidate_pairs(&self) -> Vec<(Pubkey, Pubkey)> { // Prefixed
        let mut pairs = Vec::new();
        let pool_vec: Vec<&Arc<PoolInfo>> = self._pools.values().collect();

        if pool_vec.len() < 2 {
            return pairs; 
        }

        for (i, pool1_arc) in pool_vec.iter().enumerate() {
            let pool1 = pool1_arc.as_ref(); 
            for pool2_arc in pool_vec.iter().skip(i + 1) { 
                let pool2 = pool2_arc.as_ref();
                if (pool1.token_a.mint == pool2.token_a.mint && pool1.token_b.mint == pool2.token_b.mint) || 
                   (pool1.token_a.mint == pool2.token_b.mint && pool1.token_b.mint == pool2.token_a.mint) || 
                   (pool1.token_a.mint == pool2.token_a.mint || pool1.token_a.mint == pool2.token_b.mint) ||
                   (pool1.token_b.mint == pool2.token_a.mint || pool1.token_b.mint == pool2.token_b.mint)
                {
                    pairs.push((pool1.address, pool2.address));
                }
            }
        }
        log::debug!("Generated {} candidate pool pairs from PoolMap.", pairs.len());
        pairs
    }
}

impl Default for PoolMap {
    fn default() -> Self {
        Self::new()
    }
}