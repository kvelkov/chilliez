// src/dex/pool_discovery.rs
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

/// Core service responsible for discovering and managing liquidity pools across all supported DEXs.
///
/// This service implements an efficient strategy that prioritizes official DEX-provided JSON lists
/// over expensive getProgramAccounts RPC calls. It discovers pools in batches and enriches them

/// with live on-chain data using optimized RPC patterns.
#[derive(Clone)]
pub struct PoolDiscoveryService {
    /// Collection of all DEX clients that support pool discovery.
    pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
    /// Solana RPC client for fetching live on-chain data.
    rpc_client: Arc<SolanaRpcClient>,
    /// Configuration for pool discovery behavior.
    config: PoolDiscoveryConfig,
}

/// Configuration parameters for the pool discovery service.
#[derive(Debug, Clone)]
pub struct PoolDiscoveryConfig {
    /// Maximum number of pools to fetch in a single batch RPC call.
    pub batch_size: usize,
    /// Delay between batch requests to avoid rate limiting.
    pub batch_delay_ms: u64,
}

impl Default for PoolDiscoveryConfig {
    fn default() -> Self {
        Self {
            batch_size: 100, // Solana's getMultipleAccounts has a limit of 100
            batch_delay_ms: 100,
        }
    }
}

/// Results from the complete pool discovery and enrichment operation.
#[derive(Debug, Default)]
pub struct PoolDiscoveryResult {
    /// Successfully discovered and enriched pools, indexed by their address.
    pub pools: HashMap<Pubkey, Arc<PoolInfo>>,
    /// Total number of unique pool addresses found from all DEX APIs.
    pub total_discovered_from_apis: usize,
    /// Number of pools successfully enriched with live on-chain data.
    pub pools_enriched_count: usize,
    /// Number of pools that failed during on-chain data fetching or parsing.
    pub enrichment_failures: usize,
    /// Time taken for the entire operation.
    pub total_duration: Duration,
}

impl PoolDiscoveryService {
    /// Creates a new pool discovery service.
    pub fn new(
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

    /// Discovers all pools from all configured DEXs, enriches them with live on-chain data,
    /// and returns a complete map of valid, tradeable pools.
    pub async fn discover_and_enrich_all_pools(&self) -> Result<PoolDiscoveryResult> {
        let start_time = Instant::now();
        info!("Starting full pool discovery and enrichment process...");

        // --- Phase 1: Discover all pool addresses and static data from DEX APIs concurrently ---
        let api_pools = self.discover_from_apis().await?;
        let total_discovered_from_apis = api_pools.len();
        info!("Phase 1 complete: Discovered {} unique pools from all DEX APIs.", total_discovered_from_apis);

        // --- Phase 2: Enrich pools with live on-chain data in batches ---
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

    /// Fetches initial pool lists from all DEX clients' APIs concurrently.
    async fn discover_from_apis(&self) -> Result<HashMap<Pubkey, PoolInfo>> {
        let mut discovery_futures = Vec::new();
        for client in &self.pool_discoverable_clients {
            let client = client.clone();
            discovery_futures.push(tokio::spawn(async move {
                client.discover_pools().await
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

    /// Takes a map of pools with static data and enriches them with live on-chain data.
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
                    // Get the corresponding parser from the registry
                    if let Some(parser) = POOL_PARSER_REGISTRY.get(&account.owner) {
                        let parser = parser.clone();
                        let rpc_client = self.rpc_client.clone();
                        let static_pool_info = static_pools.get(&address).cloned().unwrap_or_default();
                        
                        parse_futures.push(tokio::spawn(async move {
                            match parser.parse_pool_data(&account.data, rpc_client.as_ref()).await {
                                Ok(mut live_pool_info) => {
                                    // Combine static API data with live on-chain data
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