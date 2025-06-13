// src/dex/pool_discovery.rs
use crate::cache::Cache; // <<<--- IMPORT CACHE
use crate::dex::quote::PoolDiscoverable;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser, POOL_PARSER_REGISTRY};
use anyhow::{Context, Result};
use futures::future::join_all;
use log::{info, warn, error};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

#[derive(Clone)]
pub struct PoolDiscoveryService {
    pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
    rpc_client: Arc<SolanaRpcClient>,
    cache: Arc<Cache>, // <<<--- ADD CACHE
    config: PoolDiscoveryConfig,
}

#[derive(Debug, Clone)]
pub struct PoolDiscoveryConfig {
    pub batch_size: usize,
    pub batch_delay_ms: u64,
    pub api_cache_ttl_secs: u64, // <<<--- ADD TTL FOR API CACHE
}

impl Default for PoolDiscoveryConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_delay_ms: 100,
            api_cache_ttl_secs: 300, // Default to 5 minutes
        }
    }
}

// ... PoolDiscoveryResult struct remains the same ...
#[derive(Debug, Default)]
pub struct PoolDiscoveryResult {
    pub pools: HashMap<Pubkey, Arc<PoolInfo>>,
    pub total_discovered_from_apis: usize,
    pub pools_enriched_count: usize,
    pub enrichment_failures: usize,
    pub total_duration: Duration,
}


impl PoolDiscoveryService {
    pub fn new(
        pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
        rpc_client: Arc<SolanaRpcClient>,
        cache: Arc<Cache>, // <<<--- ADD CACHE
        config: PoolDiscoveryConfig,
    ) -> Self {
        Self {
            pool_discoverable_clients,
            rpc_client,
            cache,
            config,
        }
    }

    // ... discover_and_enrich_all_pools remains the same ...
    pub async fn discover_and_enrich_all_pools(&self) -> Result<PoolDiscoveryResult> {
        let start_time = Instant::now();
        info!("Starting full pool discovery and enrichment process...");

        let api_pools = self.discover_from_apis().await?;
        let total_discovered_from_apis = api_pools.len();
        info!("Phase 1 complete: Discovered {} unique pools from all DEX APIs.", total_discovered_from_apis);

        let (enriched_pools, enrichment_failures) = self.enrich_with_onchain_data(api_pools).await?;
        let pools_enriched_count = enriched_pools.len();
        info!("Phase 2 complete: Successfully enriched {} pools with live on-chain data.", pools_enriched_count);

        let result = PoolDiscoveryResult {
            pools: enriched_pools,
            total_discovered_from_apis,
            pools_enriched_count,
            enrichment_failures,
            total_duration: start_time.elapsed(),
        };

        info!("Pool discovery finished in {:?}", result.total_duration);
        Ok(result)
    }


    /// Fetches initial pool lists from all DEX clients' APIs concurrently, using a cache.
    async fn discover_from_apis(&self) -> Result<HashMap<Pubkey, PoolInfo>> {
        let mut discovery_futures = Vec::new();
        for client in &self.pool_discoverable_clients {
            let client = client.clone();
            let cache = self.cache.clone(); // <<<--- CLONE CACHE FOR TASK
            let cache_ttl = self.config.api_cache_ttl_secs;

            discovery_futures.push(tokio::spawn(async move {
                let dex_name = client.dex_name();
                let cache_key = format!("api_cache:{}", dex_name);

                // <<< --- START CACHING LOGIC ---
                // 1. Check cache first
                if let Ok(Some(cached_pools_json)) = cache.get(&cache_key).await {
                    info!("Cache HIT for {} pool list.", dex_name);
                    if let Ok(pools) = serde_json::from_str::<Vec<PoolInfo>>(&cached_pools_json) {
                        return Ok(pools);
                    } else {
                        warn!("Failed to deserialize cached pool list for {}. Fetching from API.", dex_name);
                    }
                }

                // 2. If cache miss, fetch from API
                info!("Cache MISS for {}. Fetching from API.", dex_name);
                let pools = client.discover_pools().await?;

                // 3. Save to cache
                if let Ok(pools_json) = serde_json::to_string(&pools) {
                    if let Err(e) = cache.set(&cache_key, &pools_json, Some(cache_ttl as usize)).await {
                        warn!("Failed to cache pool list for {}: {}", dex_name, e);
                    }
                }
                // --- END CACHING LOGIC --- >>>

                Ok(pools)
            }));
        }

        let results = join_all(discovery_futures).await;
        let mut all_pools = HashMap::new();
        for res in results {
            match res {
                Ok(Ok(pools)) => {
                    for pool in pools {
                        all_pools.insert(pool.address, pool);
                    }
                }
                Ok(Err(e)) => error!("A DEX client failed during pool discovery: {}", e),
                Err(e) => error!("A discovery task panicked: {}", e),
            }
        }
        Ok(all_pools)
    }

    // ... enrich_with_onchain_data remains the same ...
    async fn enrich_with_onchain_data(&self, static_pools: HashMap<Pubkey, PoolInfo>) -> Result<(HashMap<Pubkey, Arc<PoolInfo>>, usize)> {
        let pool_addresses: Vec<Pubkey> = static_pools.keys().cloned().collect();
        let mut enriched_pools = HashMap::new();
        let mut failures = 0;

        for chunk in pool_addresses.chunks(self.config.batch_size) {
            info!("Fetching on-chain data for a batch of {} pools...", chunk.len());
            let accounts_data = self.rpc_client.primary_client.get_multiple_accounts(chunk).await
                .context("Failed to get multiple accounts from RPC")?;
            let mut parse_futures = Vec::new();

            for (i, maybe_account) in accounts_data.into_iter().enumerate() {
                let address = chunk[i];
                if let Some(account) = maybe_account {
                    if let Some(parser) = POOL_PARSER_REGISTRY.get(&account.owner) {
                        let parser = parser.clone();
                        let rpc_client = self.rpc_client.clone();
                        let static_pool_info = static_pools.get(&address).cloned().unwrap_or_default();
                        
                        parse_futures.push(tokio::spawn(async move {
                            match parser.parse_pool_data(&account.data, rpc_client.as_ref()).await {
                                Ok(mut live_pool_info) => {
                                    live_pool_info.address = address;
                                    live_pool_info.name = static_pool_info.name;
                                    live_pool_info.token_a.symbol = static_pool_info.token_a.symbol;
                                    live_pool_info.token_b.symbol = static_pool_info.token_b.symbol;
                                    Ok(live_pool_info)
                                }
                                Err(e) => {
                                    warn!("Failed to parse pool data for {}: {}", address, e);
                                    Err(())
                                }
                            }
                        }));
                    } else {
                        warn!("No parser registered for program ID: {}", account.owner);
                        failures += 1;
                    }
                } else {
                    warn!("Account not found for pool address: {}", address);
                    failures += 1;
                }
            }

            let results = join_all(parse_futures).await;
            for res in results {
                if let Ok(Ok(pool_info)) = res {
                    enriched_pools.insert(pool_info.address, Arc::new(pool_info));
                } else {
                    failures += 1;
                }
            }

            if self.config.batch_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.batch_delay_ms)).await;
            }
        }
        Ok((enriched_pools, failures))
    }
}