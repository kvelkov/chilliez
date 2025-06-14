//! Pool management combining pool registry and discovery functionality.
//! This module consolidates pool.rs and pool_discovery.rs for better organization.

use crate::cache::Cache;
use crate::dex::{
    lifinity::LifinityPoolParser,
    meteora::MeteoraPoolParser,
    orca::OrcaPoolParser,
    raydium::RaydiumPoolParser,
    quote::PoolDiscoverable,
};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use futures::future::join_all;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;

use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

// =====================================================================================
// POOL PARSER REGISTRY (from pool.rs)
// =====================================================================================

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

// =====================================================================================
// POOL MAP (from pool.rs)
// =====================================================================================

/// PoolMap Definition (remains unchanged)
pub struct PoolMap {
    pub _pools: HashMap<Pubkey, Arc<PoolInfo>>,
}

impl PoolMap {
    pub fn new() -> Self { 
        Self { _pools: HashMap::new() } 
    }
    
    pub fn _from_hashmap(pools: HashMap<Pubkey, Arc<PoolInfo>>) -> Self { 
        Self { _pools: pools } 
    }
    
    pub fn _add_pool(&mut self, pool: Arc<PoolInfo>) { 
        self._pools.insert(pool.address, pool); 
    }
    
    pub fn _get_pool(&self, address: &Pubkey) -> Option<&Arc<PoolInfo>> { 
        self._pools.get(address) 
    }
    
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

impl Default for PoolMap { 
    fn default() -> Self { 
        Self::new() 
    } 
}

// =====================================================================================
// POOL DISCOVERY SERVICE (from pool_discovery.rs)
// =====================================================================================

#[derive(Clone)]
pub struct PoolDiscoveryService {
    pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
    rpc_client: Arc<SolanaRpcClient>,
    #[allow(dead_code)] // Reserved for future Redis caching integration
    cache: Arc<Cache>,
    config: PoolDiscoveryConfig,
    // New: Hot cache for concurrent access
    hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
}

#[derive(Debug, Clone)]
pub struct PoolDiscoveryConfig {
    pub batch_size: usize,
    pub batch_delay_ms: u64,
    pub api_cache_ttl_secs: u64,
    pub parallel_parsing_threads: usize,
    pub discovery_interval_secs: u64,
    pub max_concurrent_batches: usize,
}

impl Default for PoolDiscoveryConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_delay_ms: 100,
            api_cache_ttl_secs: 300,
            parallel_parsing_threads: num_cpus::get(),
            discovery_interval_secs: 300, // 5 minutes
            max_concurrent_batches: 10,
        }
    }
}

#[derive(Debug, Default)]
pub struct PoolDiscoveryResult {
    pub pools: HashMap<Pubkey, Arc<PoolInfo>>,
    pub total_discovered_from_apis: usize,
    pub pools_enriched_count: usize,
    pub enrichment_failures: usize,
    pub total_duration: Duration,
    pub parsing_duration: Duration,
    pub rpc_duration: Duration,
}

#[derive(Debug)]
pub struct PoolDiscoveryMetrics {
    pub total_pools_discovered: usize,
    pub successful_parses: usize,
    pub failed_parses: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub avg_parse_time_ms: f64,
    pub avg_rpc_time_ms: f64,
}

impl PoolDiscoveryService {
    pub fn new(
        pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
        rpc_client: Arc<SolanaRpcClient>,
        cache: Arc<Cache>,
        config: PoolDiscoveryConfig,
    ) -> Self {
        let hot_cache = Arc::new(DashMap::new());
        
        Self {
            pool_discoverable_clients,
            rpc_client,
            cache,
            config,
            hot_cache,
        }
    }

    /// Get reference to the hot cache for external access
    pub fn get_hot_cache(&self) -> Arc<DashMap<Pubkey, Arc<PoolInfo>>> {
        self.hot_cache.clone()
    }

    /// Run as a persistent background task
    pub async fn run_continuous_discovery_task(self: Arc<Self>) -> Result<()> {
        info!("Starting continuous pool discovery task with {}s intervals", 
              self.config.discovery_interval_secs);
        
        loop {
            match self.discover_and_parse_pools().await {
                Ok(result) => {
                    info!("Pool discovery completed: {} pools discovered in {:?}", 
                          result.pools.len(), result.total_duration);
                }
                Err(e) => {
                    error!("Pool discovery failed: {}", e);
                }
            }
            
            sleep(Duration::from_secs(self.config.discovery_interval_secs)).await;
        }
    }

    /// Main pool discovery and parsing pipeline
    pub async fn discover_and_parse_pools(&self) -> Result<PoolDiscoveryResult> {
        let start_time = Instant::now();
        let mut result = PoolDiscoveryResult::default();

        // Step 1: Discover pools from all APIs
        let discovery_start = Instant::now();
        let pool_addresses = self.discover_pools_from_apis().await?;
        result.total_discovered_from_apis = pool_addresses.len();
        debug!("Discovered {} pool addresses from APIs in {:?}", 
               pool_addresses.len(), discovery_start.elapsed());

        // Step 2: Enrich pools with on-chain data
        let parsing_start = Instant::now();
        let enriched_pools = self.enrich_pools_with_onchain_data(pool_addresses).await?;
        result.parsing_duration = parsing_start.elapsed();
        result.pools_enriched_count = enriched_pools.len();

        // Step 3: Update hot cache
        for (address, pool_info) in &enriched_pools {
            self.hot_cache.insert(*address, pool_info.clone());
        }

        result.pools = enriched_pools;
        result.total_duration = start_time.elapsed();

        info!("Pool discovery completed: {}/{} pools successfully enriched in {:?}",
              result.pools_enriched_count, result.total_discovered_from_apis, result.total_duration);

        Ok(result)
    }

    /// Discover pool addresses from all DEX APIs
    async fn discover_pools_from_apis(&self) -> Result<Vec<Pubkey>> {
        let discovery_tasks: Vec<_> = self.pool_discoverable_clients
            .iter()
            .map(|client| {
                let client = client.clone();
                async move {
                    client.discover_pools().await
                }
            })
            .collect();

        let results = join_all(discovery_tasks).await;
        let mut all_pool_addresses = Vec::new();

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(pools) => {
                    info!("Client {}: discovered {} pools", i, pools.len());
                    // Extract addresses from PoolInfo structs
                    for pool in pools {
                        all_pool_addresses.push(pool.address);
                    }
                }
                Err(e) => {
                    warn!("Client {}: pool discovery failed: {}", i, e);
                }
            }
        }

        // Deduplicate - pools are Pubkey addresses
        all_pool_addresses.sort_unstable();
        all_pool_addresses.dedup();

        Ok(all_pool_addresses)
    }

    /// Enrich pool addresses with on-chain account data
    async fn enrich_pools_with_onchain_data(
        &self,
        pool_addresses: Vec<Pubkey>,
    ) -> Result<HashMap<Pubkey, Arc<PoolInfo>>> {
        if pool_addresses.is_empty() {
            return Ok(HashMap::new());
        }

        info!("Enriching {} pools with on-chain data using {} threads",
              pool_addresses.len(), self.config.parallel_parsing_threads);

        // Process pools individually for now (can be optimized later with batch requests)
        let mut enriched_pools = HashMap::new();

        for address in pool_addresses {
            match self.rpc_client.get_account_data(&address).await {
                Ok(account_data) => {
                    match self.parse_pool_account_data(address, &account_data).await {
                        Ok(pool_info) => {
                            enriched_pools.insert(address, Arc::new(pool_info));
                        }
                        Err(e) => {
                            debug!("Failed to parse pool {}: {}", address, e);
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to fetch account data for pool {}: {}", address, e);
                }
            }

            // Rate limiting
            if self.config.batch_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.batch_delay_ms)).await;
            }
        }

        Ok(enriched_pools)
    }

    /// Parse a single pool account using the appropriate parser
    async fn parse_pool_account_data(
        &self,
        address: Pubkey,
        account_data: &[u8],
    ) -> Result<PoolInfo> {
        // Try each parser to see which one can handle this data
        for parser in POOL_PARSER_REGISTRY.values() {
            if let Ok(pool_info) = parser.parse_pool_data_sync(address, account_data) {
                return Ok(pool_info);
            }
        }
        
        Err(anyhow!("No parser could handle pool account: {}", address))
    }

    /// Get pools from hot cache
    pub fn get_cached_pools(&self) -> HashMap<Pubkey, Arc<PoolInfo>> {
        self.hot_cache.iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    /// Get a specific pool from cache
    pub fn get_cached_pool(&self, address: &Pubkey) -> Option<Arc<PoolInfo>> {
        self.hot_cache.get(address).map(|entry| entry.value().clone())
    }

    /// Clear the hot cache
    pub fn clear_cache(&self) {
        self.hot_cache.clear();
        info!("Pool cache cleared");
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> (usize, usize) {
        (self.hot_cache.len(), self.hot_cache.capacity())
    }
}

// =====================================================================================
// POOL VALIDATION (from utils/pool_validation.rs)
// =====================================================================================

use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for pool validation
#[derive(Debug, Clone)]
pub struct PoolValidationConfig {
    /// Maximum age of pool data before it's considered stale (in seconds)
    pub max_pool_age_secs: u64,
    /// Whether to skip pools with zero liquidity
    pub skip_empty_pools: bool,
    /// Minimum reserve amount required for a pool to be considered active
    pub min_reserve_threshold: u64,
    /// Whether to verify pools exist on-chain (expensive)
    pub verify_on_chain: bool,
}

impl Default for PoolValidationConfig {
    fn default() -> Self {
        Self {
            max_pool_age_secs: 300, // 5 minutes
            skip_empty_pools: true,
            min_reserve_threshold: 1000, // Minimum 1000 tokens in reserve
            verify_on_chain: false, // Disabled by default as it's expensive
        }
    }
}

/// Filters and validates discovered pools based on configuration criteria
/// 
/// This function validates pools for:
/// - Minimum reserve requirements (respects skip_empty_pools and min_reserve_threshold)
/// - Data freshness (uses max_pool_age_secs for timestamp checking)
/// - Required field validation
/// - Optional on-chain verification (respects verify_on_chain setting)
pub async fn validate_pools(
    pools: &[PoolInfo], 
    config: &PoolValidationConfig,
    rpc_client: Option<&Arc<SolanaRpcClient>>,
) -> Result<Vec<PoolInfo>> {
    let mut valid_pools = Vec::with_capacity(pools.len());
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for pool in pools {
        // Check if pool meets minimum reserve requirements (using config)
        if config.skip_empty_pools {
            if pool.token_a.reserve < config.min_reserve_threshold ||
               pool.token_b.reserve < config.min_reserve_threshold {
                warn!("Pool {} validation failed: reserves below threshold (A: {}, B: {}, min: {})", 
                      pool.address, pool.token_a.reserve, pool.token_b.reserve, config.min_reserve_threshold);
                continue;
            }
        }
        
        // Check if pool data is fresh enough (using config)
        if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
            warn!("Pool {} validation failed: data too old ({} seconds old, max: {})", 
                  pool.address, current_time.saturating_sub(pool.last_update_timestamp), config.max_pool_age_secs);
            continue;
        }
        
        // Additional validation: ensure required fields are present
        if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
            warn!("Pool {} validation failed: invalid token mints", pool.address);
            continue;
        }
        
        // Optional: Use RPC client to verify the pool exists on-chain (using config)
        if config.verify_on_chain {
            if let Some(rpc) = rpc_client {
                match rpc.primary_client.get_account(&pool.address).await {
                    Ok(_) => {
                        debug!("Pool {} on-chain verification passed", pool.address);
                    }
                    Err(e) => {
                        warn!("Pool {} validation failed: on-chain verification failed: {}", pool.address, e);
                        continue;
                    }
                }
            } else {
                warn!("Pool {} validation skipped: on-chain verification requested but no RPC client provided", pool.address);
                // Continue anyway since RPC client wasn't provided
            }
        }
        
        valid_pools.push(pool.clone());
    }
    
    info!("Pool validation completed: {}/{} pools passed validation", valid_pools.len(), pools.len());
    Ok(valid_pools)
}

/// Quick validation for pools without RPC calls
/// Useful for basic filtering before expensive operations
pub fn validate_pools_basic(pools: &[PoolInfo], config: &PoolValidationConfig) -> Vec<PoolInfo> {
    let mut valid_pools = Vec::with_capacity(pools.len());
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for pool in pools {
        // Check if pool meets minimum reserve requirements
        if config.skip_empty_pools {
            if pool.token_a.reserve < config.min_reserve_threshold ||
               pool.token_b.reserve < config.min_reserve_threshold {
                warn!("Pool {} skipped: reserves below threshold (A: {}, B: {}, min: {})", 
                      pool.address, pool.token_a.reserve, pool.token_b.reserve, config.min_reserve_threshold);
                continue;
            }
        }
        
        // Check if pool data is fresh enough
        if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
            warn!("Pool {} skipped: data too old ({} seconds old, max: {})", 
                  pool.address, current_time.saturating_sub(pool.last_update_timestamp), config.max_pool_age_secs);
            continue;
        }
        
        // Basic field validation
        if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
            warn!("Pool {} skipped: invalid token mints", pool.address);
            continue;
        }
        
        valid_pools.push(pool.clone());
    }
    
    valid_pools
}

/// Validates a single pool for real-time webhook processing
/// Returns true if pool passes all validation criteria
pub fn validate_single_pool(pool: &PoolInfo, config: &PoolValidationConfig) -> bool {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    // Check reserve requirements
    if config.skip_empty_pools {
        if pool.token_a.reserve < config.min_reserve_threshold ||
           pool.token_b.reserve < config.min_reserve_threshold {
            return false;
        }
    }
    
    // Check data freshness
    if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
        return false;
    }
    
    // Check basic field validity
    if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
        return false;
    }
    
    true
}

/// Validates pools for webhook integration with detailed logging
/// This function is designed to be called from the webhook processing pipeline
pub fn validate_pools_for_webhook(pools: &[PoolInfo], config: &PoolValidationConfig) -> (Vec<PoolInfo>, usize) {
    let total_input = pools.len();
    let valid_pools = validate_pools_basic(pools, config);
    let filtered_count = total_input - valid_pools.len();
    
    if filtered_count > 0 {
        warn!("Pool validation filtered out {} of {} pools ({:.1}% rejection rate)", 
              filtered_count, total_input, (filtered_count as f64 / total_input as f64) * 100.0);
    } else {
        debug!("All {} pools passed validation", total_input);
    }
    
    (valid_pools, filtered_count)
}
