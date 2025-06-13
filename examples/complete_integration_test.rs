// examples/complete_integration_test.rs
//! Complete integration test showing static pool discovery + real-time webhook updates
//! This demonstrates the full production setup combining both approaches

use solana_arb_bot::{
    config::Config,
    webhooks::pool_integration::IntegratedPoolService,
    dex::{
        orca::OrcaClient,
        raydium::RaydiumClient,
        meteora::MeteoraClient,
        lifinity::LifinityClient,
        quote::DexClient,
    },
    utils::DexType,
};
use std::sync::Arc;
use log::{info, warn};
use tokio::time::{sleep, Duration};
use chrono;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("🚀 Starting Complete Integration Test: Static Pool Discovery + Webhook Updates");
    info!("📋 This test demonstrates:");
    info!("   1. Static pool discovery from multiple DEXs");
    info!("   2. Real-time webhook integration with Helius");
    info!("   3. Combined pool management and updates");
    info!("   4. Statistics and monitoring");

    // Load configuration
    let config = Arc::new(Config::from_env());
    info!("✅ Configuration loaded");

    // Create DEX clients for static pool discovery
    let dex_clients = create_dex_clients(&config).await?;
    info!("✅ Created {} DEX clients", dex_clients.len());

    // Create and initialize the integrated pool service
    let mut integrated_service = IntegratedPoolService::new(
        config.clone(),
        dex_clients,
    )?;
    
    info!("📡 Initializing integrated pool service...");
    integrated_service.initialize().await?;
    info!("✅ Integrated pool service initialized");

    // Start the service (this runs static discovery and webhook processing)
    info!("🚀 Starting integrated pool service...");
    integrated_service.start().await?;
    info!("✅ Integrated pool service started");

    // Run the monitoring loop
    run_monitoring_loop(&integrated_service).await?;

    Ok(())
}

/// Create all available DEX clients for pool discovery
async fn create_dex_clients(_config: &Config) -> anyhow::Result<Vec<Arc<dyn DexClient>>> {
    let mut clients: Vec<Arc<dyn DexClient>> = Vec::new();

    // Create Orca client
    info!("🔵 Creating Orca client...");
    let orca_client: Arc<dyn DexClient> = Arc::new(OrcaClient::new());
    clients.push(orca_client);

    // Create Raydium client
    info!("🟡 Creating Raydium client...");
    let raydium_client: Arc<dyn DexClient> = Arc::new(RaydiumClient::new());
    clients.push(raydium_client);

    // Create Meteora client
    info!("🟣 Creating Meteora client...");
    let meteora_client: Arc<dyn DexClient> = Arc::new(MeteoraClient::new());
    clients.push(meteora_client);

    // Create Lifinity client
    info!("🟢 Creating Lifinity client...");
    let lifinity_client: Arc<dyn DexClient> = Arc::new(LifinityClient::new());
    clients.push(lifinity_client);

    info!("✅ Created {} DEX clients successfully", clients.len());
    Ok(clients)
}

/// Run monitoring loop to display statistics and pool information
async fn run_monitoring_loop(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📊 Starting monitoring loop...");
    
    let mut check_count = 0;
    
    loop {
        check_count += 1;
        
        // Get current statistics
        let stats = integrated_service.get_stats().await;
        
        info!("📊 === Monitoring Check #{} ===", check_count);
        info!("📈 Total Pools: {}", stats.total_pools);
        info!("🔍 Static Pools: {}", stats.static_discovery.total_static_pools);
        info!("📡 Webhook Updates: {}", stats.static_discovery.total_webhook_updates);
        info!("✅ Successful Merges: {}", stats.static_discovery.successful_merges);
        info!("❌ Failed Merges: {}", stats.static_discovery.failed_merges);
        
        // Display last update times
        if let Some(last_static) = stats.static_discovery.last_static_refresh {
            let elapsed = last_static.elapsed().as_secs();
            info!("🕐 Last Static Refresh: {}s ago", elapsed);
        }
        
        if let Some(last_webhook) = stats.static_discovery.last_webhook_update {
            let elapsed = last_webhook.elapsed().as_secs();
            info!("🕐 Last Webhook Update: {}s ago", elapsed);
        }

        // Display pools by DEX
        info!("📋 Pools by DEX:");
        for (dex, count) in &stats.static_discovery.pools_by_dex {
            info!("   {}: {} pools", dex, count);
        }

        // Show webhook statistics if available
        if let Some(webhook_stats) = &stats.webhook_stats {
            info!("📡 Webhook Statistics:");
            info!("   Active Webhooks: {}", webhook_stats.active_webhooks);
            info!("   Total Notifications: {}", webhook_stats.total_notifications);
            info!("   Successful Updates: {}", webhook_stats.successful_updates);
            info!("   Failed Updates: {}", webhook_stats.failed_updates);
            info!("   Swap Events: {}", webhook_stats.swap_events);
            info!("   Liquidity Events: {}", webhook_stats.liquidity_events);
        }

        // Show recent pool activity
        show_recent_pool_activity(integrated_service).await?;

        // Show pools by DEX breakdown
        show_dex_breakdown(integrated_service).await?;

        info!("💤 Sleeping for 30 seconds before next check...\n");
        sleep(Duration::from_secs(30)).await;
    }
}

/// Display recent pool activity
async fn show_recent_pool_activity(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    let recent_pools = integrated_service.get_recently_updated_pools(5).await;
    
    if !recent_pools.is_empty() {
        info!("🕒 Recent Pool Activity (top 5):");
        for (i, pool) in recent_pools.iter().enumerate() {
            let current_time = chrono::Utc::now().timestamp() as u64;
            let elapsed = if pool.last_update_timestamp > 0 {
                current_time.saturating_sub(pool.last_update_timestamp)
            } else {
                0
            };
            info!("   {}. {} ({:?}) - {}s ago", 
                  i + 1, 
                  pool.address, 
                  pool.dex_type, 
                  elapsed);
        }
    } else {
        warn!("⚠️  No recent pool activity detected");
    }
    
    Ok(())
}

/// Display breakdown of pools by DEX
async fn show_dex_breakdown(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("🏢 DEX Breakdown:");
    
    let dex_types = [DexType::Orca, DexType::Raydium, DexType::Meteora, DexType::Lifinity];
    
    for dex_type in &dex_types {
        let pools = integrated_service.get_pools_by_dex(dex_type).await;
        let count = pools.len();
        
        if count > 0 {
            info!("   {:?}: {} pools", dex_type, count);
            
            // Show some sample pool addresses
            if count > 0 {
                let sample_count = std::cmp::min(3, count);
                info!("     Sample pools:");
                for i in 0..sample_count {
                    info!("       - {}", pools[i].address);
                }
                if count > 3 {
                    info!("       ... and {} more", count - 3);
                }
            }
        } else {
            warn!("   {:?}: No pools found", dex_type);
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn test_dex_client_creation() {
        let config = Config::test_default();
        let clients = create_dex_clients(&config).await;
        assert!(clients.is_ok());
        
        let clients = clients.unwrap();
        assert!(clients.len() >= 4); // At least Orca, Raydium, Meteora, Lifinity
    }

    #[tokio::test] 
    async fn test_integrated_service_creation() {
        let config = Arc::new(Config::test_default());
        let clients = create_dex_clients(&config).await.unwrap();
        let service = IntegratedPoolService::new(config, clients);
        assert!(service.is_ok());
    }
}
