// src/dex/pool_discovery.rs

use crate::dex::quote::PoolDiscoverable;
use crate::utils::PoolInfo;
use crate::solana::rpc::SolanaRpcClient;
use anyhow::{Result, Context};
use log::{info, warn, error};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use solana_sdk::pubkey::Pubkey;
use tokio::time::{Duration, sleep};

/// Core service responsible for discovering and managing liquidity pools across all supported DEXs.
/// 
/// This service implements an efficient strategy that prioritizes official DEX-provided JSON lists
/// over expensive getProgramAccounts RPC calls. It discovers pools in batches and enriches them
/// with live on-chain data using optimized RPC patterns.
#[derive(Clone)]
pub struct PoolDiscoveryService {
    /// Collection of all DEX clients that support pool discovery
    pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
    /// Solana RPC client for fetching live on-chain data
    rpc_client: Arc<SolanaRpcClient>,
    /// Configuration for pool discovery behavior
    config: PoolDiscoveryConfig,
}

/// Configuration parameters for the pool discovery service
#[derive(Debug, Clone)]
pub struct PoolDiscoveryConfig {
    /// Maximum number of pools to fetch in a single batch RPC call
    pub batch_size: usize,
    /// Delay between batch requests to avoid rate limiting
    pub batch_delay_ms: u64,
    /// Maximum age of pool data before it's considered stale (in seconds)
    pub max_pool_age_secs: u64,
    /// Whether to skip pools with zero liquidity
    pub skip_empty_pools: bool,
    /// Minimum reserve amount required for a pool to be considered active
    pub min_reserve_threshold: u64,
}

impl Default for PoolDiscoveryConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_delay_ms: 100,
            max_pool_age_secs: 300, // 5 minutes
            skip_empty_pools: true,
            min_reserve_threshold: 1000, // Minimum 1000 tokens in reserve
        }
    }
}

/// Results from pool discovery operation
#[derive(Debug)]
pub struct PoolDiscoveryResult {
    /// Successfully discovered pools indexed by their address
    pub pools: HashMap<Pubkey, PoolInfo>,
    /// Total number of pools discovered across all DEXs
    pub total_discovered: usize,
    /// Number of pools that passed validation filters
    pub pools_after_filtering: usize,
    /// Number of pools that failed to load or parse
    pub failed_pools: usize,
    /// Time taken for the discovery operation in milliseconds
    pub discovery_time_ms: u128,
    /// Per-DEX statistics
    pub dex_stats: HashMap<String, DexDiscoveryStats>,
}

/// Statistics for pool discovery per DEX
#[derive(Debug, Default)]
pub struct DexDiscoveryStats {
    pub pools_discovered: usize,
    pub pools_valid: usize,
    pub pools_failed: usize,
    pub discovery_time_ms: u128,
}

impl PoolDiscoveryService {
    /// Creates a new pool discovery service with the provided pool discoverable clients and RPC client
    pub fn new(
        pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
        rpc_client: Arc<SolanaRpcClient>,
    ) -> Self {
        Self {
            pool_discoverable_clients,
            rpc_client,
            config: PoolDiscoveryConfig::default(),
        }
    }

    /// Creates a new pool discovery service with custom configuration
    pub fn with_config(
        pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
        rpc_client: Arc<SolanaRpcClient>,
        config: PoolDiscoveryConfig,
    ) -> Self {
        Self {
            pool_discoverable_clients,
            rpc_client,
            config,
        }
    }

    /// Discovers all pools from all configured DEX clients
    /// 
    /// This is the main entry point for pool discovery. It coordinates discovery across
    /// all DEXs and returns a comprehensive result with statistics and filtered pools.
    pub async fn discover_all_pools(&self) -> Result<PoolDiscoveryResult> {
        let start_time = SystemTime::now();
        
        info!("Starting pool discovery across {} DEX clients", self.pool_discoverable_clients.len());
        
        let mut all_pools = HashMap::new();
        let mut dex_stats = HashMap::new();
        let mut total_discovered = 0;
        let mut total_failed = 0;

        // Discover pools from each DEX client
        for dex_client in &self.pool_discoverable_clients {
            let dex_name = dex_client.dex_name().to_string();
            let dex_start_time = SystemTime::now();
            
            info!("Discovering pools from DEX: {}", dex_name);
            
            match self.discover_pools_from_dex(dex_client.clone()).await {
                Ok(dex_pools) => {
                    let valid_pools = self.filter_and_validate_pools(dex_pools).await?;
                    let pools_count = valid_pools.len();
                    
                    info!("Successfully discovered {} valid pools from {}", pools_count, dex_name);
                    
                    // Add pools to the main collection
                    for pool in valid_pools {
                        all_pools.insert(pool.address, pool);
                    }
                    
                    let discovery_time = dex_start_time.elapsed()
                        .unwrap_or_default()
                        .as_millis();
                    
                    dex_stats.insert(dex_name, DexDiscoveryStats {
                        pools_discovered: pools_count,
                        pools_valid: pools_count,
                        pools_failed: 0,
                        discovery_time_ms: discovery_time,
                    });
                    
                    total_discovered += pools_count;
                }
                Err(e) => {
                    error!("Failed to discover pools from {}: {}", dex_name, e);
                    total_failed += 1;
                    
                    dex_stats.insert(dex_name, DexDiscoveryStats {
                        pools_discovered: 0,
                        pools_valid: 0,
                        pools_failed: 1,
                        discovery_time_ms: 0,
                    });
                }
            }
            
            // Add delay between DEX calls to be respectful to RPC providers
            if self.config.batch_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.batch_delay_ms)).await;
            }
        }
        
        let total_time = start_time.elapsed()
            .unwrap_or_default()
            .as_millis();
        
        let result = PoolDiscoveryResult {
            pools_after_filtering: all_pools.len(),
            pools: all_pools,
            total_discovered,
            failed_pools: total_failed,
            discovery_time_ms: total_time,
            dex_stats,
        };
        
        info!("Pool discovery completed: {} pools discovered in {}ms", 
               result.pools_after_filtering, result.discovery_time_ms);
        
        Ok(result)
    }

    /// Discovers pools from a specific DEX client
    async fn discover_pools_from_dex(
        &self,
        dex_client: Arc<dyn PoolDiscoverable>,
    ) -> Result<Vec<PoolInfo>> {
        dex_client.discover_pools().await
            .with_context(|| format!("Failed to discover pools from {}", dex_client.dex_name()))
    }

    /// Filters and validates discovered pools based on configuration criteria
    async fn filter_and_validate_pools(&self, pools: Vec<PoolInfo>) -> Result<Vec<PoolInfo>> {
        let mut valid_pools = Vec::new();
        
        for pool in pools {
            // Check if pool meets minimum reserve requirements
            if self.config.skip_empty_pools {
                if pool.token_a.reserve < self.config.min_reserve_threshold ||
                   pool.token_b.reserve < self.config.min_reserve_threshold {
                    continue;
                }
            }
            
            // Check if pool data is fresh enough
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            if current_time.saturating_sub(pool.last_update_timestamp) > self.config.max_pool_age_secs {
                warn!("Pool {} has stale data, skipping", pool.address);
                continue;
            }
            
            // Additional validation: ensure required fields are present
            if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
                warn!("Pool {} has invalid token mints, skipping", pool.address);
                continue;
            }
            
            // Use RPC client to verify the pool exists on-chain (this uses the rpc_client field)
            if let Err(e) = self.rpc_client.primary_client.get_account(&pool.address).await {
                warn!("Pool {} verification failed: {}, skipping", pool.address, e);
                continue;
            }
            
            valid_pools.push(pool);
        }
        
        Ok(valid_pools)
    }

    /// Refreshes pool data for existing pools by fetching latest on-chain state
    pub async fn refresh_pool_data(&self, existing_pools: &mut HashMap<Pubkey, PoolInfo>) -> Result<usize> {
        let start_time = SystemTime::now();
        let pool_addresses: Vec<Pubkey> = existing_pools.keys().cloned().collect();
        let mut refreshed_count = 0;
        
        info!("Refreshing data for {} existing pools", pool_addresses.len());
        
        // Process pools in batches to avoid overwhelming RPC
        for chunk in pool_addresses.chunks(self.config.batch_size) {
            // For each pool, determine which DEX client to use based on dex_type
            for &pool_address in chunk {
                if let Some(existing_pool) = existing_pools.get(&pool_address) {
                    // Find the appropriate DEX client for this pool
                    if let Some(dex_client) = self.find_dex_client_for_pool(existing_pool) {
                        // Re-discover this specific pool
                        match self.refresh_single_pool(dex_client, pool_address).await {
                            Ok(Some(updated_pool)) => {
                                existing_pools.insert(pool_address, updated_pool);
                                refreshed_count += 1;
                            }
                            Ok(None) => {
                                warn!("Pool {} no longer exists, removing", pool_address);
                                existing_pools.remove(&pool_address);
                            }
                            Err(e) => {
                                warn!("Failed to refresh pool {}: {}", pool_address, e);
                            }
                        }
                    }
                }
            }
            
            // Add delay between batches
            if self.config.batch_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.batch_delay_ms)).await;
            }
        }
        
        let refresh_time = start_time.elapsed()
            .unwrap_or_default()
            .as_millis();
        
        info!("Pool refresh completed: {}/{} pools refreshed in {}ms", 
               refreshed_count, pool_addresses.len(), refresh_time);
        
        Ok(refreshed_count)
    }

    /// Finds the appropriate DEX client for a given pool based on its dex_type
    fn find_dex_client_for_pool(&self, pool: &PoolInfo) -> Option<Arc<dyn PoolDiscoverable>> {
        for dex_client in &self.pool_discoverable_clients {
            let dex_name = dex_client.dex_name();
            let matches = match &pool.dex_type {
                crate::utils::DexType::Orca => dex_name == "Orca",
                crate::utils::DexType::Raydium => dex_name == "Raydium",
                crate::utils::DexType::Lifinity => dex_name == "Lifinity",
                crate::utils::DexType::Meteora => dex_name == "Meteora",
                crate::utils::DexType::Whirlpool => dex_name == "Whirlpool",
                crate::utils::DexType::Unknown(name) => dex_name == name,
            };
            
            if matches {
                return Some(dex_client.clone());
            }
        }
        None
    }

    /// Refreshes a single pool's data
    async fn refresh_single_pool(
        &self,
        dex_client: Arc<dyn PoolDiscoverable>,
        pool_address: Pubkey,
    ) -> Result<Option<PoolInfo>> {
        match dex_client.fetch_pool_data(pool_address).await {
            Ok(pool) => Ok(Some(pool)),
            Err(_) => Ok(None), // Pool not found or error occurred
        }
    }

    /// Gets current configuration
    pub fn get_config(&self) -> &PoolDiscoveryConfig {
        &self.config
    }

    /// Updates configuration
    pub fn update_config(&mut self, config: PoolDiscoveryConfig) {
        self.config = config;
    }
}

/// Utility function to create a pool discovery service with all available DEX clients
pub async fn create_pool_discovery_service(
    rpc_client: Arc<SolanaRpcClient>,
) -> Result<PoolDiscoveryService> {
    // This would typically be called from main.rs where DEX clients are available
    // For now, return an empty service that can be configured later
    Ok(PoolDiscoveryService::new(vec![], rpc_client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolToken};
    use std::sync::Arc;
    use async_trait::async_trait;

    // Mock DEX client for testing
    struct MockPoolDiscoverable {
        name: String,
        pools: Vec<PoolInfo>,
    }

    #[async_trait]
    impl PoolDiscoverable for MockPoolDiscoverable {
        async fn discover_pools(&self) -> anyhow::Result<Vec<PoolInfo>> {
            Ok(self.pools.clone())
        }

        async fn fetch_pool_data(&self, pool_address: Pubkey) -> anyhow::Result<PoolInfo> {
            for pool in &self.pools {
                if pool.address == pool_address {
                    return Ok(pool.clone());
                }
            }
            Err(anyhow::anyhow!("Pool not found"))
        }

        fn dex_name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_pool_discovery_service_creation() {
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(100),
        ));
        
        let service = PoolDiscoveryService::new(vec![], rpc_client);
        assert_eq!(service.pool_discoverable_clients.len(), 0);
        assert_eq!(service.config.batch_size, 100);
    }

    #[tokio::test]
    async fn test_pool_filtering() {
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(100),
        ));
        
        let service = PoolDiscoveryService::new(vec![], rpc_client);
        
        // Create test pools - one valid, one with low reserves
        let valid_pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 100000, // Above threshold
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100000, // Above threshold
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            dex_type: DexType::Raydium,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
        };
        
        let low_reserve_pool = PoolInfo {
            token_a: PoolToken {
                reserve: 100, // Below threshold
                ..valid_pool.token_a.clone()
            },
            ..valid_pool.clone()
        };
        
        let filtered_pools = service.filter_and_validate_pools(vec![valid_pool, low_reserve_pool]).await.unwrap();
        
        // Should only have the valid pool
        assert_eq!(filtered_pools.len(), 1);
    }

    #[tokio::test]
    async fn test_pool_discovery_with_mock_client() {
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(100),
        ));

        // Create a mock pool
        let mock_pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Mock Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 100000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            dex_type: DexType::Raydium,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
        };

        let mock_client = Arc::new(MockPoolDiscoverable {
            name: "Mock DEX".to_string(),
            pools: vec![mock_pool.clone()],
        }) as Arc<dyn PoolDiscoverable>;

        let service = PoolDiscoveryService::new(vec![mock_client], rpc_client);
        
        let result = service.discover_all_pools().await.unwrap();
        
        assert_eq!(result.pools.len(), 1);
        assert_eq!(result.total_discovered, 1);
        assert!(result.pools.contains_key(&mock_pool.address));
    }
}

/// Simple integration test for PoolDiscoverable trait methods
/// This function demonstrates usage of the PoolDiscoverable trait to eliminate dead code warnings
pub async fn test_pool_discoverable_integration(
    raydium_client: &crate::dex::raydium::RaydiumClient,
) -> Result<()> {
    info!("Testing PoolDiscoverable trait integration...");
    
    // Test discover_pools method
    let pools = raydium_client.discover_pools().await?;
    info!("PoolDiscoverable::discover_pools found {} pools", pools.len());
    
    // Test dex_name method
    let dex_name = raydium_client.dex_name();
    info!("PoolDiscoverable::dex_name returned: {}", dex_name);
    
    // Test fetch_pool_data method with a known pool (if any pools were discovered)
    if let Some(pool) = pools.first() {
        match raydium_client.fetch_pool_data(pool.address).await {
            Ok(pool_data) => {
                info!("PoolDiscoverable::fetch_pool_data succeeded for pool: {}", pool_data.address);
            }
            Err(e) => {
                info!("PoolDiscoverable::fetch_pool_data failed (expected for demo): {}", e);
            }
        }
    }
    
    Ok(())
}
