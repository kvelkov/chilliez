// src/dex/pool_discovery.rs
use crate::cache::Cache;
use crate::dex::quote::PoolDiscoverable;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::PoolInfo;
use crate::dex::pool::POOL_PARSER_REGISTRY;
use crate::error::ArbError;
use anyhow::{Context, Result};
use dashmap::DashMap;
use futures::future::join_all;
use log::{info, warn, error, debug};
use rayon::prelude::*;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

#[derive(Clone)]
pub struct PoolDiscoveryService {
    pool_discoverable_clients: Vec<Arc<dyn PoolDiscoverable>>,
    rpc_client: Arc<SolanaRpcClient>,
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
        
        let mut interval = tokio::time::interval(
            Duration::from_secs(self.config.discovery_interval_secs)
        );
        
        loop {
            interval.tick().await;
            
            let start_time = Instant::now();
            match self.discover_and_enrich_all_pools().await {
                Ok(result) => {
                    info!("Discovery cycle completed successfully: {} pools enriched in {:?}", 
                          result.pools_enriched_count, result.total_duration);
                    
                    // Update hot cache with new results
                    self.update_hot_cache(result.pools).await;
                }
                Err(e) => {
                    error!("Discovery cycle failed: {}", e);
                }
            }
            
            debug!("Discovery cycle took {:?}", start_time.elapsed());
        }
    }

    /// Enhanced discovery with parallel processing
    pub async fn discover_and_enrich_all_pools(&self) -> Result<PoolDiscoveryResult> {
        let start_time = Instant::now();
        info!("Starting enhanced pool discovery with parallel processing...");

        // Phase 1: Parallel API discovery with caching
        let api_start = Instant::now();
        let api_pools = self.discover_from_apis_parallel().await?;
        let api_duration = api_start.elapsed();
        
        let total_discovered_from_apis = api_pools.len();
        info!("Phase 1 complete: Discovered {} unique pools from all DEX APIs in {:?}", 
              total_discovered_from_apis, api_duration);

        // Phase 2: Massively parallel on-chain enrichment
        let enrich_start = Instant::now();
        let (enriched_pools, enrichment_failures, parsing_duration, rpc_duration) = 
            self.enrich_with_onchain_data_parallel(api_pools).await?;
        let enrich_duration = enrich_start.elapsed();
        
        let pools_enriched_count = enriched_pools.len();
        info!("Phase 2 complete: Successfully enriched {} pools with live on-chain data in {:?}", 
              pools_enriched_count, enrich_duration);

        let result = PoolDiscoveryResult {
            pools: enriched_pools,
            total_discovered_from_apis,
            pools_enriched_count,
            enrichment_failures,
            total_duration: start_time.elapsed(),
            parsing_duration,
            rpc_duration,
        };

        info!("Enhanced pool discovery finished in {:?} (API: {:?}, Enrichment: {:?})", 
              result.total_duration, api_duration, enrich_duration);
        Ok(result)
    }

    /// Parallel API discovery with enhanced caching
    async fn discover_from_apis_parallel(&self) -> Result<HashMap<Pubkey, PoolInfo>> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent_batches));
        
        // Explicitly type the futures vector to resolve ambiguity
        type DiscoveryResult = Result<(Vec<PoolInfo>, bool), ArbError>;
        let mut discovery_futures: Vec<tokio::task::JoinHandle<DiscoveryResult>> = Vec::new();

        for client in &self.pool_discoverable_clients {
            let client = client.clone();
            let cache = self.cache.clone();
            let cache_ttl = self.config.api_cache_ttl_secs;
            let permit = semaphore.clone().acquire_owned().await
                .map_err(|_e| anyhow::anyhow!("Failed to acquire semaphore"))?;

            discovery_futures.push(tokio::spawn(async move {
                let _permit = permit; // Hold permit for duration of task
                let dex_name = client.dex_name();

                // Enhanced caching with metrics
                let cache_key = format!("pool_list_{}", dex_name);
                if let Ok(Some(cached_pools)) = cache.get_json::<Vec<PoolInfo>>("api_cache", &[&cache_key]).await {
                    info!("Cache HIT for {} pool list ({} pools)", dex_name, cached_pools.len());
                    return Ok((cached_pools, true)); // true = cache hit
                }

                info!("Cache MISS for {}. Fetching from API...", dex_name);
                let fetch_start = Instant::now();
                let pools = client.discover_pools().await
                    .map_err(|e| ArbError::DexError(format!("Failed to discover pools from {}: {}", dex_name, e)))?;
                let fetch_duration = fetch_start.elapsed();
                
                info!("Fetched {} pools from {} API in {:?}", pools.len(), dex_name, fetch_duration);

                // Cache the results
                if let Err(e) = cache.set_ex("api_cache", &[&cache_key], &pools, Some(cache_ttl)).await {
                    warn!("Failed to cache pool list for {}: {}", dex_name, e);
                }

                Ok((pools, false)) // false = cache miss
            }));
        }

        let results = join_all(discovery_futures).await;
        let mut all_pools = HashMap::new();
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        for res in results {
            match res {
                Ok(Ok((pools, was_cache_hit))) => {
                    if was_cache_hit {
                        cache_hits += 1;
                    } else {
                        cache_misses += 1;
                    }
                    
                    for pool in pools {
                        all_pools.insert(pool.address, pool);
                    }
                }
                Ok(Err(e)) => error!("A DEX client failed during pool discovery: {}", e),
                Err(e) => error!("A discovery task panicked: {}", e),
            }
        }

        info!("API discovery complete: {} cache hits, {} cache misses, {} total pools", 
              cache_hits, cache_misses, all_pools.len());
        Ok(all_pools)
    }

    /// Massively parallel on-chain data enrichment with CPU-bound parsing
    async fn enrich_with_onchain_data_parallel(
        &self, 
        static_pools: HashMap<Pubkey, PoolInfo>
    ) -> Result<(HashMap<Pubkey, Arc<PoolInfo>>, usize, Duration, Duration)> {
        let pool_addresses: Vec<Pubkey> = static_pools.keys().cloned().collect();
        let mut enriched_pools = HashMap::new();
        let mut failures = 0;
        let mut total_parsing_duration = Duration::from_millis(0);
        let mut total_rpc_duration = Duration::from_millis(0);

        // Process in parallel batches
        let chunks: Vec<_> = pool_addresses.chunks(self.config.batch_size).collect();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent_batches));
        
        // Explicitly type the futures vector to resolve ambiguity
        type BatchResult = Result<(HashMap<Pubkey, Arc<PoolInfo>>, usize, Duration, Duration), ArbError>;
        let mut batch_futures: Vec<tokio::task::JoinHandle<BatchResult>> = Vec::new();

        // Move static_pools into Arc to share across tasks
        let static_pools_arc = Arc::new(static_pools);

        for (batch_idx, chunk) in chunks.into_iter().enumerate() {
            let chunk = chunk.to_vec();
            let rpc_client = self.rpc_client.clone();
            let static_pools_ref = static_pools_arc.clone();
            let permit = semaphore.clone().acquire_owned().await
                .map_err(|_e| anyhow::anyhow!("Failed to acquire semaphore"))?;
            let parsing_threads = self.config.parallel_parsing_threads;

            batch_futures.push(tokio::spawn(async move {
                let _permit = permit;
                info!("Processing batch {} with {} pools...", batch_idx, chunk.len());
                
                // RPC fetch phase
                let rpc_start = Instant::now();
                let accounts_data = rpc_client.primary_client.get_multiple_accounts(&chunk).await
                    .context("Failed to get multiple accounts from RPC")
                    .map_err(|e| ArbError::RpcError(e.to_string()))?;
                let rpc_duration = rpc_start.elapsed();

                // Prepare data for parallel parsing
                let parse_data: Vec<_> = chunk.into_iter()
                    .zip(accounts_data.into_iter())
                    .enumerate()
                    .filter_map(|(i, (address, maybe_account))| {
                        if let Some(account) = maybe_account {
                            if let Some(parser) = POOL_PARSER_REGISTRY.get(&account.owner) {
                                let static_pool_info = static_pools_ref.get(&address).cloned().unwrap_or_default();
                                Some((i, address, account.data, account.owner, parser.clone(), static_pool_info))
                            } else {
                                warn!("No parser registered for program ID: {}", account.owner);
                                None
                            }
                        } else {
                            warn!("Account not found for pool address: {}", address);
                            None
                        }
                    })
                    .collect();

                // CPU-bound parallel parsing using rayon
                let parsing_start = Instant::now();
                let parse_results: Vec<_> = if parse_data.len() > parsing_threads {
                    // Use rayon for CPU-intensive parsing when we have enough work
                    parse_data.into_par_iter()
                        .map(|(i, address, data, _owner, parser, static_pool_info)| {
                            // Note: We can't use async in rayon, so we do synchronous parsing here
                            // For full async parsing, we'd need a different approach
                            match parser.parse_pool_data_sync(address, &data) {
                                Ok(mut live_pool_info) => {
                                    live_pool_info.address = address;
                                    live_pool_info.name = static_pool_info.name;
                                    live_pool_info.token_a.symbol = static_pool_info.token_a.symbol;
                                    live_pool_info.token_b.symbol = static_pool_info.token_b.symbol;
                                    Ok((i, live_pool_info))
                                }
                                Err(e) => {
                                    warn!("Failed to parse pool data for {}: {}", address, e);
                                    Err(())
                                }
                            }
                        })
                        .collect()
                } else {
                    // For smaller batches, use sequential processing to avoid overhead
                    parse_data.into_iter()
                        .map(|(i, address, data, _owner, parser, static_pool_info)| {
                            match parser.parse_pool_data_sync(address, &data) {
                                Ok(mut live_pool_info) => {
                                    live_pool_info.address = address;
                                    live_pool_info.name = static_pool_info.name;
                                    live_pool_info.token_a.symbol = static_pool_info.token_a.symbol;
                                    live_pool_info.token_b.symbol = static_pool_info.token_b.symbol;
                                    Ok((i, live_pool_info))
                                }
                                Err(e) => {
                                    warn!("Failed to parse pool data for {}: {}", address, e);
                                    Err(())
                                }
                            }
                        })
                        .collect()
                };
                let parsing_duration = parsing_start.elapsed();

                // Collect successful results
                let mut batch_pools = HashMap::new();
                let mut batch_failures = 0;
                
                for result in parse_results {
                    match result {
                        Ok((_i, pool_info)) => {
                            batch_pools.insert(pool_info.address, Arc::new(pool_info));
                        }
                        Err(_) => {
                            batch_failures += 1;
                        }
                    }
                }

                info!("Batch {} complete: {} successful, {} failed, RPC: {:?}, Parse: {:?}", 
                      batch_idx, batch_pools.len(), batch_failures, rpc_duration, parsing_duration);

                Ok((batch_pools, batch_failures, parsing_duration, rpc_duration))
            }));
        }

        // Collect all batch results
        let batch_results = join_all(batch_futures).await;
        for res in batch_results {
            match res {
                Ok(Ok((batch_pools, batch_failures, parsing_duration, rpc_duration))) => {
                    enriched_pools.extend(batch_pools);
                    failures += batch_failures;
                    total_parsing_duration += parsing_duration;
                    total_rpc_duration += rpc_duration;
                }
                Ok(Err(e)) => {
                    error!("Batch processing failed: {}", e);
                    failures += self.config.batch_size; // Assume all failed
                }
                Err(e) => {
                    error!("Batch task panicked: {}", e);
                    failures += self.config.batch_size;
                }
            }

            // Add delay between batches if configured
            if self.config.batch_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.batch_delay_ms)).await;
            }
        }

        Ok((enriched_pools, failures, total_parsing_duration, total_rpc_duration))
    }

    /// Update the hot cache with new pool data
    async fn update_hot_cache(&self, pools: HashMap<Pubkey, Arc<PoolInfo>>) {
        let start_time = Instant::now();
        let initial_size = self.hot_cache.len();
        
        // Clear old entries and insert new ones
        self.hot_cache.clear();
        for (pubkey, pool_info) in pools {
            self.hot_cache.insert(pubkey, pool_info);
        }
        
        let final_size = self.hot_cache.len();
        let update_duration = start_time.elapsed();
        
        info!("Hot cache updated: {} -> {} pools in {:?}", 
              initial_size, final_size, update_duration);
    }

    /// Update a single pool in the hot cache (for WebSocket updates)
    pub fn update_pool_in_hot_cache(&self, pubkey: Pubkey, pool_info: Arc<PoolInfo>) {
        self.hot_cache.insert(pubkey, pool_info);
        debug!("Updated pool {} in hot cache", pubkey);
    }

    /// Get pool from hot cache
    pub fn get_pool_from_hot_cache(&self, pubkey: &Pubkey) -> Option<Arc<PoolInfo>> {
        self.hot_cache.get(pubkey).map(|entry| entry.value().clone())
    }

    /// Get metrics about the discovery service
    pub fn get_metrics(&self) -> PoolDiscoveryMetrics {
        PoolDiscoveryMetrics {
            total_pools_discovered: self.hot_cache.len(),
            successful_parses: 0, // Would need to track this
            failed_parses: 0,     // Would need to track this
            cache_hits: 0,        // Would need to track this
            cache_misses: 0,      // Would need to track this
            avg_parse_time_ms: 0.0, // Would need to track this
            avg_rpc_time_ms: 0.0,   // Would need to track this
        }
    }
}