// examples/static_pools_to_webhook_integration.rs
//! Example showing how to integrate static pool discovery with webhook updates
//! This demonstrates the enhancement of the webhook system to include static pools

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("🔗 Static Pools to Webhook Integration Demo");
    info!("📋 This demo shows:");
    info!("   1. Discovering pools from multiple DEXs");
    info!("   2. Integrating static pools with webhook system");
    info!("   3. Real-time updates to the enhanced pool cache");
    info!("   4. Monitoring the combined system");

    // Load configuration
    let config = Arc::new(Config::from_env());
    info!("✅ Configuration loaded");

    // Run the demo
    run_integration_demo(config).await?;

    Ok(())
}

async fn run_integration_demo(config: Arc<Config>) -> anyhow::Result<()> {
    // Step 1: Create DEX clients for static pool discovery
    let dex_clients = create_dex_clients().await?;
    info!("✅ Created {} DEX clients", dex_clients.len());

    // Step 2: Create and initialize integrated service
    let mut integrated_service = IntegratedPoolService::new(
        config.clone(),
        dex_clients,
    )?;

    info!("📡 Initializing integrated service...");
    integrated_service.initialize().await?;
    info!("✅ Integrated service initialized");

    // Step 3: Start the service (runs static discovery + webhook processing)
    info!("🚀 Starting integrated service...");
    integrated_service.start().await?;
    info!("✅ Integrated service started");

    // Step 4: Demonstrate the integration
    demonstrate_integration(&integrated_service).await?;

    Ok(())
}

async fn create_dex_clients() -> anyhow::Result<Vec<Arc<dyn DexClient>>> {
    let mut clients: Vec<Arc<dyn DexClient>> = Vec::new();

    info!("🔵 Creating Orca client...");
    clients.push(Arc::new(OrcaClient::new()));

    info!("🟡 Creating Raydium client...");
    clients.push(Arc::new(RaydiumClient::new()));

    info!("🟣 Creating Meteora client...");
    clients.push(Arc::new(MeteoraClient::new()));

    info!("🟢 Creating Lifinity client...");
    clients.push(Arc::new(LifinityClient::new()));

    Ok(clients)
}

async fn demonstrate_integration(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("🎯 Demonstrating Static Pool + Webhook Integration");

    // Show initial state
    show_initial_state(integrated_service).await?;

    // Wait for initial discovery to complete
    info!("⏳ Waiting for initial static pool discovery...");
    sleep(Duration::from_secs(5)).await;

    // Show state after static discovery
    show_post_discovery_state(integrated_service).await?;

    // Monitor for webhook updates
    monitor_webhook_integration(integrated_service).await?;

    Ok(())
}

async fn show_initial_state(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📊 === Initial State ===");
    
    let stats = integrated_service.get_stats().await;
    info!("📈 Total Pools: {}", stats.total_pools);
    info!("🔍 Static Pools: {}", stats.static_discovery.total_static_pools);
    
    if let Some(webhook_stats) = &stats.webhook_stats {
        info!("📡 Webhook Enabled: {}", webhook_stats.enabled);
        info!("📡 Active Webhooks: {}", webhook_stats.active_webhooks);
    } else {
        warn!("📡 Webhook service not available");
    }

    Ok(())
}

async fn show_post_discovery_state(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📊 === Post-Discovery State ===");
    
    let stats = integrated_service.get_stats().await;
    info!("📈 Total Pools: {}", stats.total_pools);
    info!("🔍 Static Pools: {}", stats.static_discovery.total_static_pools);
    
    // Show pools by DEX
    info!("📋 Pools by DEX:");
    for (dex, count) in &stats.static_discovery.pools_by_dex {
        info!("   {}: {} pools", dex, count);
    }

    // Show some example pools
    show_example_pools(integrated_service).await?;

    Ok(())
}

async fn show_example_pools(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    let dex_types = [DexType::Orca, DexType::Raydium, DexType::Meteora, DexType::Lifinity];
    
    info!("🏢 Example Pools by DEX:");
    
    for dex_type in &dex_types {
        let pools = integrated_service.get_pools_by_dex(dex_type).await;
        
        if !pools.is_empty() {
            info!("   {:?}:", dex_type);
            let sample_count = std::cmp::min(2, pools.len());
            for i in 0..sample_count {
                let pool = &pools[i];
                info!("     - {} ({}-{})", 
                      pool.address, 
                      pool.token_a.symbol, 
                      pool.token_b.symbol);
            }
            if pools.len() > 2 {
                info!("     ... and {} more", pools.len() - 2);
            }
        } else {
            warn!("   {:?}: No pools discovered", dex_type);
        }
    }
    
    Ok(())
}

async fn monitor_webhook_integration(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("🎯 === Monitoring Webhook Integration ===");
    info!("📡 Waiting for webhook updates...");
    info!("💡 Tip: Perform swaps on Solana DEXs to trigger webhook events");

    let mut previous_stats = integrated_service.get_stats().await;
    
    for i in 1..=10 {
        sleep(Duration::from_secs(10)).await;
        
        let current_stats = integrated_service.get_stats().await;
        
        info!("📊 Check #{}/10:", i);
        info!("   Total Pools: {}", current_stats.total_pools);
        info!("   Webhook Updates: {}", current_stats.static_discovery.total_webhook_updates);
        
        // Check for new webhook activity
        let webhook_updates_delta = current_stats.static_discovery.total_webhook_updates 
            - previous_stats.static_discovery.total_webhook_updates;
        
        if webhook_updates_delta > 0 {
            info!("🎉 NEW: {} webhook updates received!", webhook_updates_delta);
            
            if let Some(webhook_stats) = &current_stats.webhook_stats {
                info!("📡 Webhook Stats:");
                info!("   Total Notifications: {}", webhook_stats.total_notifications);
                info!("   Successful Updates: {}", webhook_stats.successful_updates);
                info!("   Swap Events: {}", webhook_stats.swap_events);
                info!("   Liquidity Events: {}", webhook_stats.liquidity_events);
            }
            
            // Show recently updated pools
            let recent_pools = integrated_service.get_recently_updated_pools(3).await;
            if !recent_pools.is_empty() {
                info!("🕒 Recently Updated Pools:");
                for (idx, pool) in recent_pools.iter().enumerate() {
                    info!("   {}. {} ({:?})", idx + 1, pool.address, pool.dex_type);
                }
            }
        } else {
            info!("⏳ No new webhook updates");
        }
        
        previous_stats = current_stats;
    }
    
    info!("✅ Monitoring complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_dex_clients() {
        let clients = create_dex_clients().await;
        assert!(clients.is_ok());
        assert_eq!(clients.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_integrated_service_creation() {
        let config = Arc::new(Config::test_default());
        let clients = create_dex_clients().await.unwrap();
        let service = IntegratedPoolService::new(config, clients);
        assert!(service.is_ok());
    }
}
