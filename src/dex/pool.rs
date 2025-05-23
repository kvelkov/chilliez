// src/dex/pool.rs
//! Defines PoolInfo, PoolMap, and the POOL_PARSER_REGISTRY for associating
//! program IDs with their respective pool data parsers.

use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount, PoolParser as UtilsPoolParser}; // Use aliased PoolParser
use anyhow::Result as AnyhowResult; // Using anyhow::Result for errors
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use log; // For logging
use std::sync::Arc; // Import Arc

// Type alias for the pool parsing function signature.
// It takes a Pubkey (pool address) and raw account data ( &[u8]),
// and returns a Result containing PoolInfo or an error.
pub type PoolParseFn = fn(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo>;

/// Static registry mapping DEX program IDs to their `PoolParseFn`.
/// This allows dynamic dispatch of parsing logic based on the account's owner program.
pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, PoolParseFn>> = Lazy::new(|| {
    let mut m = HashMap::new();
    
    // Register Orca legacy pool parser
    // Ensure OrcaPoolParser is correctly pathed and implements UtilsPoolParser
    let orca_program_id = crate::dex::orca::OrcaPoolParser::get_program_id();
    m.insert(
        orca_program_id,
        crate::dex::orca::OrcaPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Orca legacy pool parser for program ID: {}", orca_program_id);

    // Register Raydium pool parser
    let raydium_program_id = crate::dex::raydium::RaydiumPoolParser::get_program_id();
    m.insert(
        raydium_program_id,
        crate::dex::raydium::RaydiumPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Raydium pool parser for program ID: {}", raydium_program_id);
   

    // Register Lifinity pool parser
    // Ensure LifinityPoolParser is correctly pathed
    let lifinity_program_id = crate::dex::lifinity::LifinityPoolParser::get_program_id();
    m.insert(
        lifinity_program_id,
        crate::dex::lifinity::LifinityPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Lifinity pool parser for program ID: {}", lifinity_program_id);

    // Register the new Whirlpool parser from its dedicated module
    // Ensure WhirlpoolPoolParser is correctly pathed
    let whirlpool_program_id = crate::dex::whirlpool_parser::WhirlpoolPoolParser::get_program_id();
    m.insert(
        whirlpool_program_id,
        crate::dex::whirlpool_parser::WhirlpoolPoolParser::parse_pool_data as PoolParseFn,
    );
    log::info!("Registered Whirlpool parser for program ID: {}", whirlpool_program_id);


    // TODO: Add parsers for Phoenix, Meteora if they have on-chain state that fits this model.
    // Phoenix is an order book, so parsing "pools" might be different or done via API.
    // Meteora has various pool types; each might need its own parser or a more complex one.

    if m.is_empty() {
        log::warn!("POOL_PARSER_REGISTRY is empty. No pool parsers were registered.");
    } else {
        log::info!("POOL_PARSER_REGISTRY initialized with {} parsers.", m.len());
    }
    m
});

/// Retrieves a `PoolParseFn` from the registry for a given DEX program ID.
/// Returns `None` if no parser is registered for the program ID.
pub fn get_pool_parser_fn_for_program(program_id: &Pubkey) -> Option<PoolParseFn> {
    POOL_PARSER_REGISTRY.get(program_id).copied()
}

// The calculate_price function is removed from here as per your decision.
// Price calculations are primarily handled in arbitrage/calculator.rs.
/*
#[allow(dead_code)]
pub fn calculate_price(pool: &PoolInfo) -> f64 {
    // ... implementation ...
}
*/

// calculate_output_amount is already in utils.rs and is more generic.
// If a version specific to this module's PoolInfo (if it were different) was needed, it could stay.
// But since utils::PoolInfo is the canonical one, utils::calculate_output_amount is sufficient.
// Removing this duplicate to avoid confusion.
/*
pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    // ... implementation ...
}
*/


/// `PoolMap` is a container for managing and accessing pools.
/// It's useful for arbitrage candidate discovery and token pair filtering.
/// Note: This struct seems to be a simple wrapper around a HashMap.
/// Consider if its methods are used extensively or if direct HashMap usage is simpler.
#[allow(dead_code)] // Currently, PoolMap and its methods are not used in the main flow.
pub struct PoolMap {
    pub pools: HashMap<Pubkey, Arc<PoolInfo>>, // Changed to Arc<PoolInfo> for shared ownership
}

#[allow(dead_code)]
impl PoolMap {
    /// Creates a new, empty `PoolMap`.
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    /// Creates a `PoolMap` from an existing `HashMap` of pools.
    pub fn from_hashmap(pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Self {
        Self { pools }
    }

    /// Adds a pool to the map. If the pool address already exists, it's updated.
    pub fn add_pool(&mut self, pool: Arc<PoolInfo>) {
        self.pools.insert(pool.address, pool);
    }

    /// Gets an immutable reference to a pool by its address.
    pub fn get_pool(&self, address: &Pubkey) -> Option<&Arc<PoolInfo>> {
        self.pools.get(address)
    }

    /// Returns all candidate pairs of pool addresses for arbitrage analysis.
    /// A candidate pair consists of two distinct pools that share at least one common token,
    /// or trade the same two tokens (potentially in reverse order).
    /// This is a very broad filter; further checks on specific token pairs are needed.
    pub fn candidate_pairs(&self) -> Vec<(Pubkey, Pubkey)> {
        let mut pairs = Vec::new();
        let pool_vec: Vec<&Arc<PoolInfo>> = self.pools.values().collect();

        if pool_vec.len() < 2 {
            return pairs; // Not enough pools to form pairs
        }

        for (i, pool1_arc) in pool_vec.iter().enumerate() {
            let pool1 = pool1_arc.as_ref(); // Deref Arc for easier access
            for pool2_arc in pool_vec.iter().skip(i + 1) { // Simplified loop, j is not needed
                let pool2 = pool2_arc.as_ref();

                // Check if the pools share tokens (potential arbitrage opportunity)
                // This logic identifies pools that could form part of a 2-hop (A-B via pool1, B-A via pool2)
                // or triangular arbitrage if one token is common.
                if (pool1.token_a.mint == pool2.token_a.mint && pool1.token_b.mint == pool2.token_b.mint) || // A/B and A/B
                   (pool1.token_a.mint == pool2.token_b.mint && pool1.token_b.mint == pool2.token_a.mint) || // A/B and B/A
                   (pool1.token_a.mint == pool2.token_a.mint || pool1.token_a.mint == pool2.token_b.mint) || // A is common
                   (pool1.token_b.mint == pool2.token_a.mint || pool1.token_b.mint == pool2.token_b.mint)    // B is common
                {
                    pairs.push((pool1.address, pool2.address));
                }
            }
        }
        log::debug!("Generated {} candidate pool pairs from PoolMap.", pairs.len());
        pairs
    }
}

/// Default implementation, useful for testing or initialization.
impl Default for PoolMap {
    fn default() -> Self {
        Self::new()
    }
}
