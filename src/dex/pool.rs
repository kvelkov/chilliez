// src/dex/pool.rs
//! Defines PoolMap and the POOL_PARSER_REGISTRY for associating
//! program IDs with their respective pool data parsers.

use crate::dex::{
    lifinity::LifinityPoolParser,
    meteora::MeteoraPoolParser,
    orca::OrcaPoolParser,
    raydium::RaydiumPoolParser,
};
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser};
use log;
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;

/// Static registry mapping DEX program IDs to their corresponding `PoolParser` instances.
/// This registry allows the dynamic dispatch of parsing logic based on an account's owner program.
pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, Arc<dyn UtilsPoolParser>>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // --- Register Orca Parser for Whirlpools ---
    // The single OrcaPoolParser now handles Whirlpools.
    let orca_parser = Arc::new(OrcaPoolParser);
    let orca_program_id = orca_parser.get_program_id();
    m.insert(orca_program_id, orca_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Orca Whirlpool parser for program ID: {}", orca_program_id);

    // --- Register Raydium Parser ---
    let raydium_parser = Arc::new(RaydiumPoolParser);
    let raydium_program_id = raydium_parser.get_program_id();
    m.insert(raydium_program_id, raydium_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Raydium pool parser for program ID: {}", raydium_program_id);

    // --- Register Lifinity Parser ---
    let lifinity_parser = Arc::new(LifinityPoolParser);
    let lifinity_program_id = lifinity_parser.get_program_id();
    m.insert(lifinity_program_id, lifinity_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Lifinity pool parser for program ID: {}", lifinity_program_id);

    // --- Register Meteora Parser ---
    // NOTE: Meteora has multiple program IDs. The parser itself must handle dispatching.
    // We register it under its primary/default ID.
    let meteora_parser = Arc::new(MeteoraPoolParser);
    let meteora_program_id = meteora_parser.get_program_id();
    m.insert(meteora_program_id, meteora_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Meteora pool parser for program ID: {}", meteora_program_id);

    // The WhirlpoolPoolParser is now removed, as OrcaPoolParser handles this.

    if m.is_empty() {
        log::warn!("POOL_PARSER_REGISTRY is empty. No pool parsers were registered.");
    } else {
        log::info!("POOL_PARSER_REGISTRY initialized with {} unique parsers.", m.len());
    }
    m
});

/// Retrieves the pool parser for a given DEX program ID.
pub fn get_pool_parser_for_program(program_id: &Pubkey) -> Option<Arc<dyn UtilsPoolParser>> {
    POOL_PARSER_REGISTRY.get(program_id).cloned()
}

// --- PoolMap Definition (remains unchanged) ---
pub struct PoolMap {
    pub _pools: HashMap<Pubkey, Arc<PoolInfo>>,
}
impl PoolMap {
    pub fn new() -> Self { Self { _pools: HashMap::new() } }
    pub fn _from_hashmap(pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Self { Self { _pools: pools } }
    pub fn _add_pool(&mut self, pool: Arc<PoolInfo>) { self._pools.insert(pool.address, pool); }
    pub fn _get_pool(&self, address: &Pubkey) -> Option<&Arc<PoolInfo>> { self._pools.get(address) }
    pub fn _candidate_pairs(&self) -> Vec<(Pubkey, Pubkey)> {
        // This function can be improved for performance later, but remains functionally correct.
        let mut pairs = Vec::new();
        let pool_vec: Vec<&Arc<PoolInfo>> = self._pools.values().collect();
        if pool_vec.len() < 2 { return pairs; }
        for (i, pool1_arc) in pool_vec.iter().enumerate() {
            let pool1 = pool1_arc.as_ref();
            for pool2_arc in pool_vec.iter().skip(i + 1) {
                let pool2 = pool2_arc.as_ref();
                if pool1.token_a.mint == pool2.token_a.mint || 
                   pool1.token_a.mint == pool2.token_b.mint || 
                   pool1.token_b.mint == pool2.token_a.mint || 
                   pool1.token_b.mint == pool2.token_b.mint
                {
                    pairs.push((pool1.address, pool2.address));
                }
            }
        }
        log::debug!("Generated {} candidate pool pairs from PoolMap.", pairs.len());
        pairs
    }
}
impl Default for PoolMap { fn default() -> Self { Self::new() } }