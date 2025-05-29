// src/dex/pool.rs
//! Defines PoolMap and the POOL_PARSER_REGISTRY for associating
//! program IDs with their respective pool data parsers.

use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser};
use crate::dex::{
    orca::OrcaPoolParser,
    raydium::RaydiumPoolParser,
    lifinity::LifinityPoolParser, // Assuming you have this parser defined
    whirlpool_parser::WhirlpoolPoolParser,
}; // Removed unused import: anyhow::Result as AnyhowResult
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use log;
use std::sync::Arc;

/// Static registry mapping DEX program IDs to their corresponding `PoolParser` instances.
/// This registry allows the dynamic dispatch of parsing logic based on an account's owner program.
pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, Arc<dyn UtilsPoolParser>>> = Lazy::new(|| {
    let mut m = HashMap::new();

    let orca_parser = Arc::new(OrcaPoolParser);
    m.insert(orca_parser.get_program_id(), orca_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Orca legacy pool parser for program ID: {}", orca_parser.get_program_id());

    let raydium_parser = Arc::new(RaydiumPoolParser);
    m.insert(raydium_parser.get_program_id(), raydium_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Raydium pool parser for program ID: {}", raydium_parser.get_program_id());

    // Assuming LifinityPoolParser exists and implements PoolParser
    let lifinity_parser = Arc::new(LifinityPoolParser);
    m.insert(lifinity_parser.get_program_id(), lifinity_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Lifinity pool parser for program ID: {}", lifinity_parser.get_program_id());


    let whirlpool_parser = Arc::new(WhirlpoolPoolParser);
    m.insert(whirlpool_parser.get_program_id(), whirlpool_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Whirlpool parser for program ID: {}", whirlpool_parser.get_program_id());

    if m.is_empty() {
        log::warn!("POOL_PARSER_REGISTRY is empty. No pool parsers were registered.");
    } else {
        log::info!("POOL_PARSER_REGISTRY initialized with {} parsers.", m.len());
    }
    m
});

/// Retrieves the pool parser for a given DEX program ID.
pub fn get_pool_parser_for_program(program_id: &Pubkey) -> Option<Arc<dyn UtilsPoolParser>> {
    POOL_PARSER_REGISTRY.get(program_id).cloned()
}

// --- PoolMap Definition ---

/// PoolMap organizes and manages a collection of PoolInfo objects, keyed by their addresses.
/// It is used to store and retrieve pool data efficiently.
pub struct PoolMap {
    pub _pools: HashMap<Pubkey, Arc<PoolInfo>>, // Prefixed with underscore to indicate internal usage.
}

impl PoolMap {
    /// Constructs a new, empty PoolMap.
    pub fn new() -> Self {
        Self {
            _pools: HashMap::new(),
        }
    }

    /// Constructs a PoolMap from an existing HashMap.
    pub fn _from_hashmap(pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Self {
        Self { _pools: pools }
    }

    /// Adds a pool to the PoolMap.
    pub fn _add_pool(&mut self, pool: Arc<PoolInfo>) {
        self._pools.insert(pool.address, pool);
    }

    /// Retrieves a pool by its address.
    pub fn _get_pool(&self, address: &Pubkey) -> Option<&Arc<PoolInfo>> {
        self._pools.get(address)
    }

    /// Generates candidate pool pairs (for arbitrage) based on overlapping token mints.
    /// This function collects all pool pairs that have any token in common.
    pub fn _candidate_pairs(&self) -> Vec<(Pubkey, Pubkey)> {
        let mut pairs = Vec::new();
        let pool_vec: Vec<&Arc<PoolInfo>> = self._pools.values().collect();

        if pool_vec.len() < 2 {
            return pairs;
        }

        // Compare each pair of pools to look for common tokens by mint.
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