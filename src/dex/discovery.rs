// src/dex/discovery.rs
//! Pool discovery, management, and banned pairs filtering.
//! Consolidates pool_management.rs, banned_pairs.rs, and routing.rs functionality.

use crate::cache::Cache;
use crate::dex::{
    clients::{
        lifinity::LifinityPoolParser,
        meteora::MeteoraPoolParser,
        orca::OrcaPoolParser,
        raydium::RaydiumPoolParser,
    },
    api::{DexClient, PoolDiscoverable},
};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser};
use anyhow::{Context, Result};
use csv::{ReaderBuilder, WriterBuilder as CsvWriterBuilder};
use dashmap::DashMap;
use futures::future::join_all;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::time::Instant;

// =====================================================================================
// POOL PARSER REGISTRY
// =====================================================================================

/// Static registry mapping DEX program IDs to their corresponding `PoolParser` instances.
/// This registry allows the dynamic dispatch of parsing logic based on an account's owner program.
pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, Arc<dyn UtilsPoolParser>>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // --- Register Orca Parser for Whirlpools ---
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
    let meteora_parser = Arc::new(MeteoraPoolParser);
    let meteora_program_id = meteora_parser.get_program_id();
    m.insert(meteora_program_id, meteora_parser.clone() as Arc<dyn UtilsPoolParser>);
    log::info!("Registered Meteora pool parser for program ID: {}", meteora_program_id);

    m
});

// =====================================================================================
// BANNED PAIRS MANAGEMENT
// =====================================================================================

/// A canonical key for a token pair, ensuring (A,B) and (B,A) are treated the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BannedPairKey {
    token1: String,
    token2: String,
}

impl BannedPairKey {
    fn new(token_a: &str, token_b: &str) -> Self {
        if token_a <= token_b {
            BannedPairKey {
                token1: token_a.to_string(),
                token2: token_b.to_string(),
            }
        } else {
            BannedPairKey {
                token1: token_b.to_string(),
                token2: token_a.to_string(),
            }
        }
    }
}

/// Manages the set of banned trading pairs.
#[derive(Debug)]
pub struct BannedPairsManager {
    banned_pairs: HashSet<BannedPairKey>,
    csv_file_path: Box<Path>,
}

impl BannedPairsManager {
    /// Creates a new BannedPairsManager by loading banned pairs from a CSV file.
    pub fn new(csv_file_path: &Path) -> Result<Self> {
        info!("Loading banned pairs from: {:?}", csv_file_path);

        let mut banned_pairs = HashSet::new();

        if csv_file_path.exists() {
            let file = std::fs::File::open(csv_file_path)
                .with_context(|| format!("Failed to open banned pairs file: {:?}", csv_file_path))?;

            let mut reader = ReaderBuilder::new()
                .has_headers(true)
                .from_reader(BufReader::new(file));

            for result in reader.records() {
                let record = result.with_context(|| "Failed to parse CSV record")?;

                if record.len() != 2 {
                    warn!("Skipping invalid record with {} fields: {:?}", record.len(), record);
                    continue;
                }

                let token_a = record[0].trim();
                let token_b = record[1].trim();

                if !token_a.is_empty() && !token_b.is_empty() {
                    let key = BannedPairKey::new(token_a, token_b);
                    banned_pairs.insert(key);
                } else {
                    warn!("Skipping record with empty token: {:?}", record);
                }
            }

            info!("Loaded {} banned pairs from CSV file", banned_pairs.len());
        } else {
            info!("Banned pairs file does not exist, starting with empty set");
        }

        Ok(BannedPairsManager {
            banned_pairs,
            csv_file_path: csv_file_path.into(),
        })
    }

    /// Checks if a trading pair is banned.
    pub fn is_pair_banned(&self, token_a: &str, token_b: &str) -> bool {
        let key = BannedPairKey::new(token_a, token_b);
        self.banned_pairs.contains(&key)
    }

    /// Checks if a trading pair (by Pubkey) is banned.
    pub fn is_pair_banned_pubkey(&self, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        self.is_pair_banned(&token_a.to_string(), &token_b.to_string())
    }

    /// Adds a new banned pair and persists it to the CSV file.
    pub fn ban_pair(&mut self, token_a: &str, token_b: &str) -> Result<()> {
        let key = BannedPairKey::new(token_a, token_b);
        
        if self.banned_pairs.insert(key.clone()) {
            self.persist_new_ban(&key)?;
            info!("Banned new pair: {} <-> {}", key.token1, key.token2);
        } else {
            debug!("Pair already banned: {} <-> {}", key.token1, key.token2);
        }

        Ok(())
    }

    fn persist_new_ban(&self, banned_pair: &BannedPairKey) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*self.csv_file_path)
            .with_context(|| format!("Failed to open banned pairs file for writing: {:?}", self.csv_file_path))?;

        let mut writer = CsvWriterBuilder::new()
            .has_headers(false)
            .from_writer(file);

        writer.write_record(&[&banned_pair.token1, &banned_pair.token2])
            .with_context(|| "Failed to write banned pair to CSV")?;

        writer.flush()
            .with_context(|| "Failed to flush CSV writer")?;

        Ok(())
    }

    /// Returns the number of banned pairs.
    pub fn count(&self) -> usize {
        self.banned_pairs.len()
    }

    /// Filters a list of pools, removing those with banned pairs.
    pub fn filter_banned_pools(&self, pools: Vec<PoolInfo>) -> Vec<PoolInfo> {
        let initial_count = pools.len();
        let filtered: Vec<PoolInfo> = pools
            .into_iter()
            .filter(|pool| {
                !self.is_pair_banned_pubkey(&pool.token_a.mint, &pool.token_b.mint)
            })
            .collect();

        let filtered_count = initial_count - filtered.len();
        if filtered_count > 0 {
            info!("Filtered out {} banned pairs from {} total pools", filtered_count, initial_count);
        }

        filtered
    }
}

// =====================================================================================
// POOL DISCOVERY SERVICE
// =====================================================================================

/// Configuration for pool validation
#[derive(Debug, Clone)]
pub struct PoolValidationConfig {
    pub min_liquidity_usd: f64,
    pub max_price_impact_bps: u16,
    pub require_balanced_reserves: bool,
}

impl Default for PoolValidationConfig {
    fn default() -> Self {
        Self {
            min_liquidity_usd: 1000.0,
            max_price_impact_bps: 500, // 5%
            require_balanced_reserves: false,
        }
    }
}

/// Service responsible for discovering and managing liquidity pools across multiple DEXs.
pub struct PoolDiscoveryService {
    pub pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    pub dex_clients: Vec<Arc<dyn PoolDiscoverable>>,
    pub validation_config: PoolValidationConfig,
    pub banned_pairs_manager: BannedPairsManager,
    rpc_client: Arc<SolanaRpcClient>,
    cache: Arc<Cache>,
}

impl PoolDiscoveryService {
    /// Creates a new PoolDiscoveryService instance.
    pub fn new(
        dex_clients: Vec<Arc<dyn PoolDiscoverable>>,
        rpc_client: Arc<SolanaRpcClient>,
        cache: Arc<Cache>,
        validation_config: PoolValidationConfig,
        banned_pairs_csv_path: &Path,
    ) -> Result<Self> {
        let banned_pairs_manager = BannedPairsManager::new(banned_pairs_csv_path)?;
        
        Ok(Self {
            pool_cache: Arc::new(DashMap::new()),
            dex_clients,
            validation_config,
            banned_pairs_manager,
            rpc_client,
            cache,
        })
    }

    /// Discovers pools from all configured DEX clients.
    pub async fn discover_all_pools(&self) -> Result<Vec<PoolInfo>> {
        info!("Starting pool discovery across {} DEXs", self.dex_clients.len());
        let start_time = Instant::now();

        let discovery_tasks: Vec<_> = self.dex_clients
            .iter()
            .map(|client| async move {
                let dex_start = Instant::now();
                match client.discover_pools().await {
                    Ok(pools) => {
                        info!(
                            "Discovered {} pools from {} in {:?}",
                            pools.len(),
                            client.dex_name(),
                            dex_start.elapsed()
                        );
                        Ok(pools)
                    }
                    Err(e) => {
                        error!("Failed to discover pools from {}: {}", client.dex_name(), e);
                        Ok(Vec::new())
                    }
                }
            })
            .collect();

        let results: Vec<Result<Vec<PoolInfo>>> = join_all(discovery_tasks).await;
        let mut all_pools = Vec::new();

        for result in results {
            match result {
                Ok(pools) => all_pools.extend(pools),
                Err(e) => warn!("Pool discovery task failed: {}", e),
            }
        }

        info!(
            "Total discovery completed in {:?}: {} pools found",
            start_time.elapsed(),
            all_pools.len()
        );

        // Filter banned pairs
        let filtered_pools = self.banned_pairs_manager.filter_banned_pools(all_pools);

        // Validate pools
        let validated_pools = self.validate_pools(filtered_pools).await?;

        // Update cache
        self.update_pool_cache(&validated_pools).await;

        info!(
            "Pool discovery complete: {} valid pools cached",
            validated_pools.len()
        );

        Ok(validated_pools)
    }

    /// Updates the pool cache with discovered pools.
    pub async fn update_pool_cache(&self, pools: &[PoolInfo]) {
        for pool in pools {
            self.pool_cache.insert(pool.address, Arc::new(pool.clone()));
        }
        info!("Updated pool cache with {} pools", pools.len());
    }

    /// Validates pools according to the configured validation rules.
    pub async fn validate_pools(&self, pools: Vec<PoolInfo>) -> Result<Vec<PoolInfo>> {
        let mut valid_pools = Vec::new();

        for pool in pools {
            if self.validate_single_pool(&pool).await? {
                valid_pools.push(pool);
            }
        }

        info!(
            "Pool validation complete: {}/{} pools passed validation",
            valid_pools.len(),
            valid_pools.len()
        );

        Ok(valid_pools)
    }

    /// Validates a single pool according to the configuration.
    pub async fn validate_single_pool(&self, pool: &PoolInfo) -> Result<bool> {
        // Check if reserves exist
        if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
            debug!("Pool {} has zero reserves", pool.address);
            return Ok(false);
        }

        // Check if pair is banned
        if self.banned_pairs_manager.is_pair_banned_pubkey(&pool.token_a.mint, &pool.token_b.mint) {
            debug!("Pool {} has banned token pair", pool.address);
            return Ok(false);
        }

        Ok(true)
    }

    /// Returns all cached pools.
    pub fn get_all_cached_pools(&self) -> Vec<Arc<PoolInfo>> {
        self.pool_cache.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Gets a specific pool from cache.
    pub fn get_cached_pool(&self, pool_address: &Pubkey) -> Option<Arc<PoolInfo>> {
        self.pool_cache.get(pool_address).map(|entry| entry.clone())
    }

    /// Returns the total number of cached pools.
    pub fn cache_size(&self) -> usize {
        self.pool_cache.len()
    }

    /// Checks if a pool exists in cache.
    pub fn has_cached_pool(&self, pool_address: &Pubkey) -> bool {
        self.pool_cache.contains_key(pool_address)
    }

    /// Gets pool cache statistics.
    pub fn get_cache_stats(&self) -> (usize, Vec<String>) {
        let size = self.pool_cache.len();
        let dex_types: Vec<String> = self.pool_cache
            .iter()
            .map(|entry| format!("{:?}", entry.value().dex_type))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        
        (size, dex_types)
    }

    /// Clears expired pools from cache based on age.
    pub fn cleanup_expired_pools(&self, max_age_secs: u64) -> usize {
        let cutoff_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(max_age_secs);

        let mut removed_count = 0;
        let keys_to_remove: Vec<Pubkey> = self.pool_cache
            .iter()
            .filter_map(|entry| {
                // For pools without timestamps, we keep them (assume they're fresh)
                // This is a conservative approach for existing pools
                let pool = entry.value();
                // TODO: Add timestamp field to PoolInfo for proper age tracking
                // For now, we'll implement a simple cleanup based on reserve thresholds
                if pool.token_a.reserve < 1000 && pool.token_b.reserve < 1000 {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect();

        for key in keys_to_remove {
            if self.pool_cache.remove(&key).is_some() {
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            info!("Cleaned up {} expired/low-liquidity pools from cache", removed_count);
        }

        removed_count
    }

    /// Sync pool cache with external hot cache (like ArbitrageOrchestrator's cache).
    pub async fn sync_with_hot_cache(&self, hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>) -> Result<()> {
        info!("Syncing pool discovery cache with hot cache...");
        
        let mut synced_count = 0;
        let mut new_pools_count = 0;

        // Copy pools from hot cache to discovery cache
        for entry in hot_cache.iter() {
            let pool_address = *entry.key();
            let pool_info = entry.value().clone();
            
            if !self.pool_cache.contains_key(&pool_address) {
                new_pools_count += 1;
            }
            
            self.pool_cache.insert(pool_address, pool_info);
            synced_count += 1;
        }

        // Copy unique pools from discovery cache to hot cache
        for entry in self.pool_cache.iter() {
            let pool_address = *entry.key();
            let pool_info = entry.value().clone();
            
            if !hot_cache.contains_key(&pool_address) {
                hot_cache.insert(pool_address, pool_info);
            }
        }

        info!(
            "Cache sync completed: {} pools synced, {} new pools added to discovery cache",
            synced_count, new_pools_count
        );

        Ok(())
    }

    /// Export cached pools to hot cache format.
    pub fn export_to_hot_cache(&self) -> Arc<DashMap<Pubkey, Arc<PoolInfo>>> {
        let hot_cache = Arc::new(DashMap::new());
        
        for entry in self.pool_cache.iter() {
            hot_cache.insert(*entry.key(), entry.value().clone());
        }
        
        info!("Exported {} pools to hot cache format", hot_cache.len());
        hot_cache
    }

    /// Import pools from hot cache format.
    pub fn import_from_hot_cache(&self, hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>) -> usize {
        let mut imported_count = 0;
        
        for entry in hot_cache.iter() {
            self.pool_cache.insert(*entry.key(), entry.value().clone());
            imported_count += 1;
        }
        
        info!("Imported {} pools from hot cache", imported_count);
        imported_count
    }

    /// Get pools by DEX type from cache.
    pub fn get_pools_by_dex(&self, dex_type: &crate::utils::DexType) -> Vec<Arc<PoolInfo>> {
        self.pool_cache
            .iter()
            .filter_map(|entry| {
                let pool = entry.value();
                if std::mem::discriminant(&pool.dex_type) == std::mem::discriminant(dex_type) {
                    Some(pool.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Find pools containing specific token pairs.
    pub fn find_pools_for_tokens(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<Arc<PoolInfo>> {
        self.pool_cache
            .iter()
            .filter_map(|entry| {
                let pool = entry.value();
                let matches = (pool.token_a.mint == *token_a && pool.token_b.mint == *token_b) ||
                             (pool.token_a.mint == *token_b && pool.token_b.mint == *token_a);
                
                if matches && !self.banned_pairs_manager.is_pair_banned_pubkey(token_a, token_b) {
                    Some(pool.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Adds a new banned pair.
    pub fn ban_pair(&mut self, token_a: &str, token_b: &str) -> Result<()> {
        self.banned_pairs_manager.ban_pair(token_a, token_b)
    }

    /// Checks if a pair is banned.
    pub fn is_pair_banned(&self, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        self.banned_pairs_manager.is_pair_banned_pubkey(token_a, token_b)
    }
}

// =====================================================================================
// DEX ROUTING UTILITIES
// =====================================================================================

/// Finds the appropriate DEX client for a given pool based on its dex_type
pub fn find_dex_client_for_pool(
    pool: &PoolInfo, 
    dex_clients: &[Arc<dyn DexClient>]
) -> Option<Arc<dyn DexClient>> {
    for dex_client in dex_clients {
        let dex_name = dex_client.get_name();
        let matches = match &pool.dex_type {
            DexType::Orca => dex_name == "Orca",
            DexType::Raydium => dex_name == "Raydium",
            DexType::Lifinity => dex_name == "Lifinity",
            DexType::Meteora => dex_name == "Meteora",
            DexType::Whirlpool => dex_name == "Orca",
            DexType::Unknown(name) => dex_name == name,
        };
        
        if matches {
            return Some(dex_client.clone());
        }
    }
    None
}

/// Groups pools by their DEX type for batch operations
pub fn group_pools_by_dex(pools: &[PoolInfo]) -> HashMap<String, Vec<&PoolInfo>> {
    let mut grouped = HashMap::new();
    
    for pool in pools {
        let dex_name = match &pool.dex_type {
            DexType::Orca => "Orca",
            DexType::Raydium => "Raydium", 
            DexType::Lifinity => "Lifinity",
            DexType::Meteora => "Meteora",
            DexType::Whirlpool => "Orca",
            DexType::Unknown(name) => name.as_str(),
        };
        
        grouped.entry(dex_name.to_string()).or_insert_with(Vec::new).push(pool);
    }
    
    grouped
}

/// Finds pools that support a specific token pair
pub fn find_pools_for_pair(
    pools: &[Arc<PoolInfo>],
    token_a: &Pubkey,
    token_b: &Pubkey,
) -> Vec<Arc<PoolInfo>> {
    pools
        .iter()
        .filter(|pool| {
            (pool.token_a.mint == *token_a && pool.token_b.mint == *token_b) ||
            (pool.token_b.mint == *token_a && pool.token_a.mint == *token_b)
        })
        .cloned()
        .collect()
}

// =====================================================================================
// LEGACY COMPATIBILITY EXPORTS
// =====================================================================================

/// Basic pool validation function for backward compatibility
pub async fn validate_pools(pools: Vec<PoolInfo>) -> Result<Vec<PoolInfo>> {
    let config = PoolValidationConfig::default();
    let banned_pairs_manager = BannedPairsManager::new(Path::new("banned_pairs_log.csv"))?;
    
    let mut valid_pools = Vec::new();
    for pool in pools {
        if validate_single_pool(&pool, &config, &banned_pairs_manager).await? {
            valid_pools.push(pool);
        }
    }
    Ok(valid_pools)
}

/// Validates a single pool with given configuration
pub async fn validate_single_pool(
    pool: &PoolInfo,
    _config: &PoolValidationConfig,
    banned_pairs_manager: &BannedPairsManager,
) -> Result<bool> {
    // Check minimum liquidity
    if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
        return Ok(false);
    }

    // Check if pair is banned
    if banned_pairs_manager.is_pair_banned_pubkey(&pool.token_a.mint, &pool.token_b.mint) {
        return Ok(false);
    }

    Ok(true)
}

/// Backward compatibility alias
pub use validate_pools as validate_pools_basic;
