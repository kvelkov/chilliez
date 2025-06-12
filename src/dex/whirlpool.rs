// src/dex/whirlpool.rs
//! API Client for Orca Whirlpools.

use crate::cache::Cache;
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use crate::utils::PoolInfo;
use anyhow::{anyhow, Result as AnyhowResult};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct WhirlpoolClient;

impl WhirlpoolClient {
    pub fn new(_cache: Arc<Cache>, _quote_cache_ttl_secs: Option<u64>) -> Self {
        Self
    }
}

impl DexClient for WhirlpoolClient {
    fn get_name(&self) -> &str {
        "Whirlpool"
    }

    fn calculate_onchain_quote(
        &self,
        _pool: &PoolInfo,
        _input_amount: u64,
    ) -> AnyhowResult<CanonicalQuote> {
        Err(anyhow!("calculate_onchain_quote not implemented"))
    }

    fn get_swap_instruction(
        &self,
        _swap_info: &crate::dex::quote::SwapInfo,
    ) -> AnyhowResult<solana_sdk::instruction::Instruction> {
        Err(anyhow!("get_swap_instruction not implemented"))
    }
}
