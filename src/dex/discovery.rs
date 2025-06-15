// src/dex/discovery.rs
//! Pool discovery, management, and banned pairs filtering.
//! Consolidates pool_management.rs, banned_pairs.rs, and routing.rs functionality.

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
#[allow(dead_code)] // Used by pool discovery systems
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

/// Represents a token pair for banned pairs management
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BannedPairKey {
    pub token_a: String,
    pub token_b: String,
}

impl BannedPairKey {
    pub fn new(token_a: String, token_b: String) -> Self {
        // Ensure consistent ordering to treat (A, B) and (B, A) as the same pair
        if token_a <= token_b {
            Self { token_a, token_b }
        } else {
            Self { token_a: token_b, token_b: token_a }
        }
    }
}

/// Manages banned trading pairs
pub struct BannedPairsManager {
    banned_pairs: HashSet<BannedPairKey>,
    csv_file_path: String,
}

impl BannedPairsManager {
    /// Create a new BannedPairsManager and load banned pairs from CSV
    pub fn new(csv_file_path: String) -> Result<Self> {
        let mut manager = Self {
            banned_pairs: HashSet::new(),
            csv_file_path,
        };
        manager.load_from_csv()?;
        Ok(manager)
    }

    /// Load banned pairs from CSV file
    fn load_from_csv(&mut self) -> Result<()> {
        if !Path::new(&self.csv_file_path).exists() {
            info!("Banned pairs CSV file does not exist, starting with empty list: {}", self.csv_file_path);
            return Ok(());
        }

        let file = std::fs::File::open(&self.csv_file_path)
            .with_context(|| format!("Failed to open banned pairs CSV: {}", self.csv_file_path))?;
        
        let mut reader = ReaderBuilder::new().has_headers(true).from_reader(BufReader::new(file));
        
        for result in reader.records() {
            let record = result.with_context(|| "Failed to read CSV record")?;
            if record.len() >= 2 {
                let token_a = record[0].to_string();
                let token_b = record[1].to_string();
                let pair = BannedPairKey::new(token_a, token_b);
                self.banned_pairs.insert(pair);
            }
        }

        info!("Loaded {} banned pairs from CSV", self.banned_pairs.len());
        Ok(())
    }

    /// Save banned pairs to CSV file
    fn save_to_csv(&self) -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.csv_file_path)
            .with_context(|| format!("Failed to create banned pairs CSV: {}", self.csv_file_path))?;

        let mut writer = CsvWriterBuilder::new().has_headers(true).from_writer(file);
        writer.write_record(&["token_a", "token_b"])?;

        for pair in &self.banned_pairs {
            writer.write_record(&[&pair.token_a, &pair.token_b])?;
        }

        writer.flush()?;
        Ok(())
    }

    /// Check if a token pair is banned
    pub fn is_pair_banned(&self, token_a: &str, token_b: &str) -> bool {
        let pair = BannedPairKey::new(token_a.to_string(), token_b.to_string());
        self.banned_pairs.contains(&pair)
    }

    /// Check if a token pair (by Pubkey) is banned
    pub fn is_pair_banned_by_pubkey(&self, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        self.is_pair_banned(&token_a.to_string(), &token_b.to_string())
    }

    /// Ban a token pair
    pub fn ban_pair(&mut self, token_a: String, token_b: String) -> Result<()> {
        let pair = BannedPairKey::new(token_a, token_b);
        self.banned_pairs.insert(pair);
        self.save_to_csv()?;
        info!("Banned pair added and saved to CSV");
        Ok(())
    }

    /// Get the number of banned pairs
    pub fn banned_count(&self) -> usize {
        self.banned_pairs.len()
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
            min_liquidity_usd: 1000.0, // $1000 minimum liquidity
            max_price_impact_bps: 1000, // 10% max price impact
            require_balanced_reserves: false,
        }
    }
}

/// Pool discovery and management service
pub struct PoolDiscoveryService {
    pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    dex_clients: Vec<Arc<dyn PoolDiscoverable>>,
    validation_config: PoolValidationConfig,
    banned_pairs_manager: Arc<tokio::sync::Mutex<BannedPairsManager>>,
    _rpc_client: Arc<SolanaRpcClient>,
}

impl PoolDiscoveryService {
    /// Create a new PoolDiscoveryService
    pub fn new(
        dex_clients: Vec<Arc<dyn PoolDiscoverable>>,
        validation_config: PoolValidationConfig,
        banned_pairs_csv_path: String,
        rpc_client: Arc<SolanaRpcClient>,
    ) -> Result<Self> {
        let banned_pairs_manager = BannedPairsManager::new(banned_pairs_csv_path)?;
        
        Ok(Self {
            pool_cache: Arc::new(DashMap::new()),
            dex_clients,
            validation_config,
            banned_pairs_manager: Arc::new(tokio::sync::Mutex::new(banned_pairs_manager)),
            _rpc_client: rpc_client,
        })
    }

    /// Discover pools from all DEX clients
    pub async fn discover_all_pools(&self) -> Result<usize> {
        let start_time = Instant::now();
        info!("Starting pool discovery across {} DEX clients", self.dex_clients.len());

        let mut discovery_tasks = Vec::new();
        
        for dex_client in &self.dex_clients {
            let client = dex_client.clone();
            let task = tokio::spawn(async move {
                match client.discover_pools().await {
                    Ok(pools) => {
                        info!("Discovered {} pools from {}", pools.len(), client.dex_name());
                        Ok((client.dex_name().to_string(), pools))
                    }
                    Err(e) => {
                        warn!("Failed to discover pools from {}: {}", client.dex_name(), e);
                        Err(e)
                    }
                }
            });
            discovery_tasks.push(task);
        }

        let results = join_all(discovery_tasks).await;
        let mut total_discovered = 0;
        let mut all_pools = Vec::new();

        for result in results {
            match result {
                Ok(Ok((dex_name, pools))) => {
                    total_discovered += pools.len();
                    all_pools.extend(pools);
                    debug!("Successfully processed pools from {}", dex_name);
                }
                Ok(Err(e)) => {
                    warn!("DEX discovery error: {}", e);
                }
                Err(e) => {
                    error!("Task join error: {}", e);
                }
            }
        }

        // Filter banned pairs
        let banned_manager = self.banned_pairs_manager.lock().await;
        let filtered_pools: Vec<PoolInfo> = all_pools
            .into_iter()
            .filter(|pool| {
                !banned_manager.is_pair_banned_by_pubkey(&pool.token_a.mint, &pool.token_b.mint)
            })
            .collect();
        drop(banned_manager);

        let banned_count = total_discovered - filtered_pools.len();
        if banned_count > 0 {
            info!("Filtered out {} banned pairs", banned_count);
        }

        // Validate pools
        let filtered_pools_len = filtered_pools.len();
        let validated_pools = self.validate_pools(filtered_pools).await;
        let validation_filtered = filtered_pools_len - validated_pools.len();
        if validation_filtered > 0 {
            info!("Filtered out {} pools during validation", validation_filtered);
        }

        // Update cache
        self.update_pool_cache(validated_pools).await;

        let discovery_time = start_time.elapsed();
        info!(
            "Pool discovery completed in {:.2}s: {} total discovered, {} in cache",
            discovery_time.as_secs_f64(),
            total_discovered,
            self.pool_cache.len()
        );

        Ok(self.pool_cache.len())
    }

    /// Update the pool cache with discovered pools
    async fn update_pool_cache(&self, pools: Vec<PoolInfo>) {
        for pool in pools {
            self.pool_cache.insert(pool.address, Arc::new(pool));
        }
    }

    /// Validate pools according to configuration
    async fn validate_pools(&self, pools: Vec<PoolInfo>) -> Vec<PoolInfo> {
        let mut validated = Vec::new();
        
        for pool in pools {
            if self.validate_single_pool(&pool).await {
                validated.push(pool);
            }
        }
        
        validated
    }

    /// Validate a single pool
    async fn validate_single_pool(&self, pool: &PoolInfo) -> bool {
        // Check minimum liquidity (simplified - would need price data for accurate USD calculation)
        if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
            return false;
        }

        // Check balanced reserves if required
        if self.validation_config.require_balanced_reserves {
            let ratio = pool.token_a.reserve as f64 / pool.token_b.reserve as f64;
            if ratio < 0.1 || ratio > 10.0 {
                return false;
            }
        }

        true
    }

    /// Get all cached pools
    pub fn get_all_cached_pools(&self) -> Vec<Arc<PoolInfo>> {
        self.pool_cache.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get a specific cached pool
    pub fn get_cached_pool(&self, address: &Pubkey) -> Option<Arc<PoolInfo>> {
        self.pool_cache.get(address).map(|entry| entry.value().clone())
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.pool_cache.len()
    }

    /// Check if pool is cached
    pub fn has_cached_pool(&self, address: &Pubkey) -> bool {
        self.pool_cache.contains_key(address)
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        let mut dex_counts = HashMap::new();

        for entry in self.pool_cache.iter() {
            let dex_name = match &entry.value().dex_type {
                DexType::Orca => "Orca",
                DexType::Raydium => "Raydium",
                DexType::Lifinity => "Lifinity",
                DexType::Meteora => "Meteora",
                DexType::Phoenix => "Phoenix",
                DexType::Jupiter => "Jupiter",
                DexType::Whirlpool => "Whirlpool",
                DexType::Unknown(name) => name.as_str(),
            };
            *dex_counts.entry(dex_name.to_string()).or_insert(0) += 1;
        }

        stats.insert("total".to_string(), self.pool_cache.len());
        for (dex, count) in dex_counts {
            stats.insert(dex, count);
        }

        stats
    }

    /// Clean up expired pools (placeholder implementation)
    pub async fn cleanup_expired_pools(&self, _max_age_seconds: u64) {
        // Implementation would check pool timestamps and remove old entries
        // For now, this is a placeholder
        debug!("Pool cleanup completed");
    }

    /// Export pools to hot cache
    pub async fn export_to_hot_cache(&self, hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>) {
        for entry in self.pool_cache.iter() {
            hot_cache.insert(*entry.key(), entry.value().clone());
        }
        info!("Exported {} pools to hot cache", self.pool_cache.len());
    }

    /// Import pools from hot cache
    pub async fn import_from_hot_cache(&self, hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>) {
        for entry in hot_cache.iter() {
            self.pool_cache.insert(*entry.key(), entry.value().clone());
        }
        info!("Imported {} pools from hot cache", hot_cache.len());
    }

    /// Get pools by DEX type
    pub fn get_pools_by_dex(&self, dex_type: &DexType) -> Vec<Arc<PoolInfo>> {
        self.pool_cache
            .iter()
            .filter(|entry| &entry.value().dex_type == dex_type)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Find pools for specific token pair
    pub fn find_pools_for_tokens(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<Arc<PoolInfo>> {
        self.pool_cache
            .iter()
            .filter(|entry| {
                let pool = entry.value();
                (pool.token_a.mint == *token_a && pool.token_b.mint == *token_b) ||
                (pool.token_a.mint == *token_b && pool.token_b.mint == *token_a)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Check if a pair is banned
    pub async fn is_pair_banned(&self, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        let banned_manager = self.banned_pairs_manager.lock().await;
        banned_manager.is_pair_banned_by_pubkey(token_a, token_b)
    }
}

// =====================================================================================
// DEX ROUTING UTILITIES
// =====================================================================================

/// Finds the appropriate DEX client for a given pool based on its dex_type
#[allow(dead_code)] // Used by routing systems
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
            DexType::Phoenix => dex_name == "Phoenix",
            DexType::Jupiter => dex_name == "Jupiter",
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
            DexType::Phoenix => "Phoenix",
            DexType::Jupiter => "Jupiter",
            DexType::Whirlpool => "Orca",
            DexType::Unknown(name) => name.as_str(),
        };
        
        grouped.entry(dex_name.to_string()).or_insert_with(Vec::new).push(pool);
    }
    
    grouped
}

/// Find pools that support a specific token pair
pub fn find_pools_for_pair(
    pools: &[Arc<PoolInfo>], 
    token_a: &Pubkey, 
    token_b: &Pubkey
) -> Vec<Arc<PoolInfo>> {
    pools
        .iter()
        .filter(|pool| {
            (pool.token_a.mint == *token_a && pool.token_b.mint == *token_b) ||
            (pool.token_a.mint == *token_b && pool.token_b.mint == *token_a)
        })
        .cloned()
        .collect()
}

// =====================================================================================
// LEGACY COMPATIBILITY EXPORTS
// =====================================================================================

/// Basic pool validation function for backward compatibility
pub fn validate_pools(pools: Vec<PoolInfo>, _config: &PoolValidationConfig) -> Vec<PoolInfo> {
    pools
        .into_iter()
        .filter(|pool| {
            // Basic validation
            pool.token_a.reserve > 0 && pool.token_b.reserve > 0
        })
        .collect()
}

/// Validate a single pool with given configuration
pub fn validate_single_pool(pool: &PoolInfo, config: &PoolValidationConfig) -> bool {
    if pool.token_a.reserve == 0 || pool.token_b.reserve == 0 {
        return false;
    }

    if config.require_balanced_reserves {
        let ratio = pool.token_a.reserve as f64 / pool.token_b.reserve as f64;
        if ratio < 0.1 || ratio > 10.0 {
            return false;
        }
    }

    true
}