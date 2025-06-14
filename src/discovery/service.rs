// src/discovery/service.rs
use crate::dex::quote::DexClient;
use crate::dex::{PoolValidationConfig, validate_pools_basic};
use crate::utils::PoolInfo;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use log::{error, info, warn};

pub struct PoolDiscoveryService {
    dex_clients: Vec<Arc<dyn DexClient>>,
    pool_sender: Sender<Vec<PoolInfo>>,
    validation_config: PoolValidationConfig,
    #[allow(dead_code)]
    batch_size: usize,
    #[allow(dead_code)]
    max_pool_age_secs: u64,
    #[allow(dead_code)]
    delay_between_batches_ms: u64,
}

impl PoolDiscoveryService {
    pub fn new(
        dex_clients: Vec<Arc<dyn DexClient>>,
        pool_sender: Sender<Vec<PoolInfo>>,
        batch_size: usize,
        max_pool_age_secs: u64,
        delay_between_batches_ms: u64,
    ) -> Self {
        let validation_config = PoolValidationConfig {
            max_pool_age_secs,
            skip_empty_pools: true,
            min_reserve_threshold: 1000, // Minimum 1000 tokens in reserve
            verify_on_chain: false, // Disabled by default for performance
        };
        
        Self { 
            dex_clients, 
            pool_sender, 
            validation_config,
            batch_size, 
            max_pool_age_secs, 
            delay_between_batches_ms 
        }
    }

    /// Runs a single discovery cycle, fetching pools from all registered DEX clients.
    pub async fn run_discovery_cycle(&self) -> anyhow::Result<Vec<PoolInfo>> {
        info!("Starting pool discovery cycle with {} DEX clients", self.dex_clients.len());
        
        let mut all_pools = Vec::new();
        let mut successful_clients = 0;
        let mut failed_clients = 0;

        for client in &self.dex_clients {
            let client_name = client.get_name();
            info!("Discovering pools from DEX: {}", client_name);
            
            match client.discover_pools().await {
                Ok(pools) => {
                    let pool_count = pools.len();
                    info!("Successfully discovered {} pools from {}", pool_count, client_name);
                    all_pools.extend(pools);
                    successful_clients += 1;
                }
                Err(e) => {
                    error!("Failed to discover pools from {}: {}", client_name, e);
                    failed_clients += 1;
                }
            }
        }

        info!("Discovery cycle completed: {} successful, {} failed clients. Total pools: {}", 
              successful_clients, failed_clients, all_pools.len());

        // Apply pool validation before sending to ArbitrageEngine
        if !all_pools.is_empty() {
            info!("Validating {} discovered pools before sending to ArbitrageEngine", all_pools.len());
            let valid_pools = validate_pools_basic(&all_pools, &self.validation_config);
            let filtered_count = all_pools.len() - valid_pools.len();
            
            if filtered_count > 0 {
                warn!("Pool validation filtered out {} of {} pools ({:.1}% rejection rate)", 
                      filtered_count, all_pools.len(), (filtered_count as f64 / all_pools.len() as f64) * 100.0);
            }
            
            if !valid_pools.is_empty() {
                info!("Sending {} validated pools to ArbitrageEngine", valid_pools.len());
                if let Err(e) = self.pool_sender.send(valid_pools).await {
                    error!("Failed to send validated pools to ArbitrageEngine: {}", e);
                    // Don't return error, just log it since we still want to return the pools
                }
            } else {
                warn!("No pools passed validation - skipping send to ArbitrageEngine");
            }
            
            // Return all discovered pools (including those that didn't pass validation) for reporting
            Ok(all_pools)
        } else {
            warn!("No pools discovered in this cycle");
            Ok(all_pools)
        }
    }

    /// Runs continuous discovery cycles with specified interval
    pub async fn run_continuous_discovery(&self, interval_secs: u64) -> anyhow::Result<()> {
        info!("Starting continuous pool discovery with {}s intervals", interval_secs);
        
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        
        loop {
            interval.tick().await;
            
            match self.run_discovery_cycle().await {
                Ok(pools) => {
                    info!("Discovery cycle completed successfully with {} pools", pools.len());
                }
                Err(e) => {
                    error!("Discovery cycle failed: {}", e);
                    // Continue with next cycle even if this one failed
                }
            }
        }
    }

    /// Get the number of registered DEX clients
    pub fn client_count(&self) -> usize {
        self.dex_clients.len()
    }

    /// Get the names of all registered DEX clients
    pub fn get_client_names(&self) -> Vec<String> {
        self.dex_clients.iter().map(|client| client.get_name().to_string()).collect()
    }
}
