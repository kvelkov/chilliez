// examples/test_pool_discovery.rs
// Test to verify that the pool discovery integration works end-to-end

use anyhow::Result;
use env_logger;
use log::{info, error};
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();
    
    info!("🧪 Testing Pool Discovery Integration");
    
    // Test 1: Create DEX Clients
    let raydium_client = solana_arb_bot::dex::raydium::RaydiumClient::new();
    let orca_client = solana_arb_bot::dex::orca::OrcaClient::new();
    
    let dex_clients: Vec<Arc<dyn solana_arb_bot::dex::quote::DexClient>> = vec![
        Arc::new(raydium_client),
        Arc::new(orca_client),
    ];
    
    info!("Created {} DEX clients", dex_clients.len());
    
    // Test 2: Create Pool Discovery Service
    let (pool_sender, mut pool_receiver) = mpsc::channel::<Vec<solana_arb_bot::utils::PoolInfo>>(100);
    let discovery_service = solana_arb_bot::discovery::PoolDiscoveryService::new(
        dex_clients,
        pool_sender,
        100, // batch_size
        300, // max_pool_age_secs
        100, // delay_between_batches_ms
    );
    
    info!("Created PoolDiscoveryService with {} clients: {:?}", 
          discovery_service.client_count(),
          discovery_service.get_client_names());
    
    // Test 3: Try running discovery cycle (only if network is available)
    if std::env::var("ENABLE_NETWORK_TEST").is_ok() {
        info!("Running discovery cycle...");
        
        // Clone service for the async task
        let service_clone = discovery_service;
        
        // Run discovery in a task
        let discovery_handle = tokio::spawn(async move {
            service_clone.run_discovery_cycle().await
        });
        
        // Also listen for received pools
        let receiver_handle = tokio::spawn(async move {
            if let Some(pools) = pool_receiver.recv().await {
                info!("Received {} pools from discovery service", pools.len());
                
                // Show first few pools as examples
                for (i, pool) in pools.iter().take(3).enumerate() {
                    info!("Pool {}: {} ({}-{})", 
                          i + 1, 
                          pool.address, 
                          pool.token_a.symbol, 
                          pool.token_b.symbol);
                }
                
                Ok::<(), anyhow::Error>(())
            } else {
                Err(anyhow::anyhow!("No pools received"))
            }
        });
        
        // Wait for both tasks
        match discovery_handle.await {
            Ok(Ok(pools)) => {
                info!("Discovery cycle completed successfully with {} pools", pools.len());
                for pool in pools.iter().take(3) {
                    info!("Sample pool: {} ({}-{})", pool.address, pool.token_a.symbol, pool.token_b.symbol);
                }
            },
            Ok(Err(e)) => error!("Discovery cycle failed: {}", e),
            Err(e) => error!("Discovery task panicked: {}", e),
        }
        
        match receiver_handle.await {
            Ok(Ok(())) => info!("Pool receiver completed successfully"),
            Ok(Err(e)) => error!("Pool receiver failed: {}", e),
            Err(e) => error!("Pool receiver task panicked: {}", e),
        }
    } else {
        info!("Network test disabled. Set ENABLE_NETWORK_TEST=1 to run actual discovery.");
        info!("Basic integration test completed successfully!");
    }
    
    info!("✅ Pool Discovery Integration Test Complete");
    Ok(())
}
