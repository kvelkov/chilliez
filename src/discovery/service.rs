// src/discovery/service.rs
use crate::dex::quote::DexClient;
use crate::utils::PoolInfo;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use log::{error, info, warn};

pub struct PoolDiscoveryService {
    dex_clients: Vec<Arc<dyn DexClient>>,
    pool_sender: Sender<Vec<PoolInfo>>,
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
        Self { 
            dex_clients, 
            pool_sender, 
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

        if !all_pools.is_empty() {
            info!("Sending {} discovered pools to ArbitrageEngine", all_pools.len());
            if let Err(e) = self.pool_sender.send(all_pools.clone()).await {
                error!("Failed to send pools to ArbitrageEngine: {}", e);
                // Don't return error, just log it since we still want to return the pools
            }
        } else {
            warn!("No pools discovered in this cycle");
        }
        
        Ok(all_pools)
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
