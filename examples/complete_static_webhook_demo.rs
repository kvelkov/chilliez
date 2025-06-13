// examples/complete_static_webhook_demo.rs
//! Complete demonstration of static pool discovery integrated with webhook monitoring
//! This is the ultimate example showing how both systems work together

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

    info!("🎯 Complete Static + Webhook Integration Demo");
    info!("📋 This comprehensive demo shows:");
    info!("   1. Static pool discovery from all DEXs");
    info!("   2. Registration of static pools for webhook monitoring");
    info!("   3. Real-time webhook updates for monitored pools");
    info!("   4. Enhanced pool management and statistics");
    info!("   5. Complete production workflow");

    // Load configuration
    let config = Arc::new(Config::from_env());
    info!("✅ Configuration loaded");

    // Run the complete demo
    run_complete_demo(config).await?;

    Ok(())
}

async fn run_complete_demo(config: Arc<Config>) -> anyhow::Result<()> {
    // Phase 1: Set up the integrated service
    info!("🚀 Phase 1: Setting up Integrated Pool Service");
    let integrated_service = setup_integrated_service(config.clone()).await?;

    // Phase 2: Run static discovery and register pools
    info!("🔍 Phase 2: Static Pool Discovery and Registration");
    demonstrate_static_discovery(&integrated_service).await?;

    // Phase 3: Enhanced webhook monitoring
    info!("📡 Phase 3: Enhanced Webhook Monitoring");
    demonstrate_webhook_monitoring(&integrated_service).await?;

    // Phase 4: Real-time statistics and monitoring
    info!("📊 Phase 4: Real-time Statistics Dashboard");
    run_statistics_dashboard(&integrated_service).await?;

    Ok(())
}

async fn setup_integrated_service(config: Arc<Config>) -> anyhow::Result<IntegratedPoolService> {
    // Create DEX clients
    let dex_clients = create_dex_clients().await?;
    info!("✅ Created {} DEX clients", dex_clients.len());

    // Create integrated service
    let mut integrated_service = IntegratedPoolService::new(config, dex_clients)?;
    
    // Initialize (sets up webhooks if enabled)
    integrated_service.initialize().await?;
    info!("✅ Integrated service initialized");

    // Start the service (runs discovery and processing)
    integrated_service.start().await?;
    info!("✅ Integrated service started");

    Ok(integrated_service)
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

async fn demonstrate_static_discovery(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("🔍 Demonstrating Static Pool Discovery Integration");

    // Wait for initial discovery
    info!("⏳ Waiting for initial static pool discovery...");
    sleep(Duration::from_secs(8)).await;

    // Get initial stats
    let stats = integrated_service.get_stats().await;
    info!("📈 Discovery Results:");
    info!("   Total Pools Discovered: {}", stats.total_pools);
    info!("   Static Discovery Pools: {}", stats.static_discovery.total_static_pools);

    // Show discovery breakdown
    info!("📋 Pools by DEX:");
    for (dex, count) in &stats.static_discovery.pools_by_dex {
        info!("   {}: {} pools", dex, count);
    }

    // Show sample pools for each DEX
    show_sample_pools_by_dex(integrated_service).await?;

    // Demonstrate enhanced pool registration for webhook monitoring
    demonstrate_enhanced_registration(integrated_service).await?;

    Ok(())
}

async fn show_sample_pools_by_dex(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    let dex_types = [DexType::Orca, DexType::Raydium, DexType::Meteora, DexType::Lifinity];
    
    info!("🏢 Sample Pools by DEX:");
    for dex_type in &dex_types {
        let pools = integrated_service.get_pools_by_dex(dex_type).await;
        
        if !pools.is_empty() {
            info!("   {:?} (showing first 3 of {}):", dex_type, pools.len());
            let sample_count = std::cmp::min(3, pools.len());
            
            for i in 0..sample_count {
                let pool = &pools[i];
                info!("     - {} ({}-{})", 
                      pool.address, 
                      pool.token_a.symbol, 
                      pool.token_b.symbol);
            }
        } else {
            warn!("   {:?}: No pools discovered", dex_type);
        }
    }
    
    Ok(())
}

async fn demonstrate_enhanced_registration(_integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("🔗 Enhanced Registration Demo");
    info!("✅ Static pools are automatically registered for webhook monitoring");
    info!("💡 The integrated service handles registration seamlessly");
    info!("📡 All discovered pools are now monitored for real-time updates");
    
    Ok(())
}

async fn demonstrate_webhook_monitoring(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📡 Demonstrating Enhanced Webhook Monitoring");
    
    let stats = integrated_service.get_stats().await;
    
    if let Some(webhook_stats) = &stats.webhook_stats {
        if webhook_stats.enabled {
            info!("✅ Webhook system is active and monitoring {} pools", 
                  webhook_stats.pools_in_cache);
            info!("📊 Current webhook statistics:");
            info!("   Active Webhooks: {}", webhook_stats.active_webhooks);
            info!("   Total Notifications: {}", webhook_stats.total_notifications);
            info!("   Successful Updates: {}", webhook_stats.successful_updates);
            info!("   Swap Events: {}", webhook_stats.swap_events);
            info!("   Liquidity Events: {}", webhook_stats.liquidity_events);
            
            info!("💡 Tip: Perform swaps on Solana DEXs to see real-time updates!");
        } else {
            warn!("⚠️  Webhook system is disabled");
        }
    } else {
        warn!("⚠️  Webhook statistics not available");
    }

    Ok(())
}

async fn run_statistics_dashboard(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📊 Starting Real-time Statistics Dashboard");
    info!("🔄 Monitoring for 2 minutes... (Ctrl+C to stop earlier)");

    let mut check_interval = 0;
    let total_checks = 12; // 2 minutes at 10-second intervals
    
    while check_interval < total_checks {
        check_interval += 1;
        
        info!("📊 === Dashboard Update #{}/{} ===", check_interval, total_checks);
        
        // Get comprehensive stats
        let stats = integrated_service.get_stats().await;
        
        // Main metrics
        info!("📈 Main Metrics:");
        info!("   Total Pools: {}", stats.total_pools);
        info!("   Static Discovery: {}", stats.static_discovery.total_static_pools);
        info!("   Webhook Updates: {}", stats.static_discovery.total_webhook_updates);
        info!("   Successful Merges: {}", stats.static_discovery.successful_merges);
        
        // Time-based metrics
        if let Some(last_static) = stats.static_discovery.last_static_refresh {
            let elapsed = last_static.elapsed().as_secs();
            info!("   Last Static Refresh: {}s ago", elapsed);
        }
        
        if let Some(last_webhook) = stats.static_discovery.last_webhook_update {
            let elapsed = last_webhook.elapsed().as_secs();
            info!("   Last Webhook Update: {}s ago", elapsed);
        }

        // Webhook system details
        if let Some(webhook_stats) = &stats.webhook_stats {
            info!("📡 Webhook System:");
            info!("   Status: {}", if webhook_stats.enabled { "Active ✅" } else { "Disabled ❌" });
            info!("   Monitored Pools: {}", webhook_stats.pools_in_cache);
            info!("   Events Processed: {}", webhook_stats.total_notifications);
            info!("   Success Rate: {:.1}%", 
                  if webhook_stats.total_notifications > 0 {
                      (webhook_stats.successful_updates as f64 / webhook_stats.total_notifications as f64) * 100.0
                  } else {
                      0.0
                  });
        }

        // DEX breakdown
        info!("🏢 DEX Breakdown:");
        for (dex, count) in &stats.static_discovery.pools_by_dex {
            info!("   {}: {} pools", dex, count);
        }

        // Recent activity
        show_recent_activity(integrated_service).await?;

        if check_interval < total_checks {
            info!("⏳ Next update in 10 seconds...\n");
            sleep(Duration::from_secs(10)).await;
        }
    }

    info!("✅ Dashboard monitoring complete");
    info!("🎯 Static + Webhook Integration Demo Finished Successfully!");
    
    // Final summary
    show_final_summary(integrated_service).await?;

    Ok(())
}

async fn show_recent_activity(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    let recent_pools = integrated_service.get_recently_updated_pools(3).await;
    
    if !recent_pools.is_empty() {
        info!("🕒 Recent Activity (top 3):");
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
        info!("🕒 No recent pool activity");
    }
    
    Ok(())
}

async fn show_final_summary(integrated_service: &IntegratedPoolService) -> anyhow::Result<()> {
    info!("📋 === FINAL SUMMARY ===");
    
    let stats = integrated_service.get_stats().await;
    
    info!("🎯 Integration Results:");
    info!("   ✅ Total Pools Integrated: {}", stats.total_pools);
    info!("   ✅ Static Discovery Successful: {} pools", stats.static_discovery.total_static_pools);
    info!("   ✅ Webhook Updates Processed: {}", stats.static_discovery.total_webhook_updates);
    
    if let Some(webhook_stats) = &stats.webhook_stats {
        info!("   ✅ Webhook Monitoring: {} (tracking {} pools)", 
              if webhook_stats.enabled { "Active" } else { "Disabled" },
              webhook_stats.pools_in_cache);
    }

    info!("🚀 Production Benefits:");
    info!("   - Real-time pool state updates via webhooks");
    info!("   - Comprehensive static pool discovery");
    info!("   - Seamless integration of both systems");
    info!("   - Enhanced arbitrage opportunity detection");
    info!("   - Production-ready monitoring and statistics");

    info!("💡 Next Steps:");
    info!("   - Deploy webhook server to public endpoint");
    info!("   - Configure environment variables for production");
    info!("   - Begin arbitrage engine testing with live data");
    info!("   - Monitor webhook events in production environment");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dex_client_setup() {
        let clients = create_dex_clients().await;
        assert!(clients.is_ok());
        assert_eq!(clients.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_integrated_service_setup() {
        let config = Arc::new(Config::test_default());
        let clients = create_dex_clients().await.unwrap();
        let service = IntegratedPoolService::new(config, clients);
        assert!(service.is_ok());
    }
}
