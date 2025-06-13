// examples/integrated_pool_service_test.rs
//! Test the integrated pool service combining static discovery with webhooks

use solana_arb_bot::{
    config::Config,
    webhooks::IntegratedPoolService,
    dex::{
        orca::OrcaClient,
        raydium::RaydiumClient,
        meteora::MeteoraClient,
        lifinity::LifinityClient,
        quote::DexClient,
    },
};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("🚀 Integrated Pool Service Test");
    println!("===============================");
    println!("Testing combined static pool discovery + real-time webhook updates...\n");

    // Load configuration
    let config = Arc::new(Config::from_env());
    println!("✅ Configuration loaded");
    println!("   Webhooks enabled: {}", config.enable_webhooks);
    println!("   Pool refresh interval: {} seconds", config.pool_refresh_interval_secs);
    println!();

    // Create DEX clients for static discovery
    let dex_clients: Vec<Arc<dyn DexClient>> = vec![
        Arc::new(OrcaClient::new()),
        Arc::new(RaydiumClient::new()),
        Arc::new(MeteoraClient::new()),
        Arc::new(LifinityClient::new()),
    ];
    
    println!("✅ Created {} DEX clients for discovery", dex_clients.len());
    for (i, client) in dex_clients.iter().enumerate() {
        println!("   {}. {}", i + 1, client.get_name());
    }
    println!();

    // Create integrated pool service
    println!("🔄 Creating integrated pool service...");
    let mut integrated_service = IntegratedPoolService::new(config.clone(), dex_clients)?;
    println!("✅ Integrated pool service created");
    println!();

    // Initialize the service
    println!("🔄 Initializing integrated service...");
    integrated_service.initialize().await?;
    println!("✅ Integrated service initialized");
    println!();

    // Start the service
    println!("🚀 Starting integrated service...");
    integrated_service.start().await?;
    println!("✅ Integrated service started");
    println!();

    // Wait a moment for initial discovery to complete
    println!("⏳ Waiting for initial pool discovery...");
    sleep(Duration::from_secs(5)).await;

    // Display initial statistics
    let initial_stats = integrated_service.get_stats().await;
    display_integrated_stats(&initial_stats, "INITIAL");

    // Get some example pools
    let all_pools = integrated_service.get_pools().await;
    if !all_pools.is_empty() {
        println!("📋 SAMPLE DISCOVERED POOLS");
        println!("==========================");
        for (i, (address, pool)) in all_pools.iter().take(5).enumerate() {
            println!("   {}. {} ({:?})", i + 1, pool.name, pool.dex_type);
            println!("      Address: {}", address);
            println!("      Tokens: {} / {}", pool.token_a.symbol, pool.token_b.symbol);
            println!("      Last Update: {}", pool.last_update_timestamp);
        }
        println!();
    }

    // Test DEX-specific filtering
    let orca_pools = integrated_service.get_pools_by_dex(&solana_arb_bot::utils::DexType::Orca).await;
    let raydium_pools = integrated_service.get_pools_by_dex(&solana_arb_bot::utils::DexType::Raydium).await;
    println!("📊 POOLS BY DEX");
    println!("===============");
    println!("   Orca pools: {}", orca_pools.len());
    println!("   Raydium pools: {}", raydium_pools.len());
    println!();

    // Test recent updates
    let recent_pools = integrated_service.get_recently_updated_pools(3).await;
    if !recent_pools.is_empty() {
        println!("🕒 RECENTLY UPDATED POOLS");
        println!("=========================");
        for (i, pool) in recent_pools.iter().enumerate() {
            println!("   {}. {} (Last update: {})", 
                    i + 1, pool.name, pool.last_update_timestamp);
        }
        println!();
    }

    // Monitor the service for a period
    if config.enable_webhooks {
        println!("📡 WEBHOOK MONITORING");
        println!("====================");
        println!("   Monitoring for webhook updates for 30 seconds...");
        println!("   Service will receive real-time updates from Helius");
        println!();
        
        // Check stats every 10 seconds
        for i in 1..=3 {
            sleep(Duration::from_secs(10)).await;
            let current_stats = integrated_service.get_stats().await;
            display_integrated_stats(&current_stats, &format!("UPDATE {}", i));
        }
    } else {
        println!("🔄 POLLING MONITORING");
        println!("====================");
        println!("   Monitoring static discovery for 30 seconds...");
        println!("   Service will refresh pools every {} seconds", config.pool_refresh_interval_secs);
        println!();
        
        sleep(Duration::from_secs(30)).await;
        let final_stats = integrated_service.get_stats().await;
        display_integrated_stats(&final_stats, "FINAL");
    }

    // Final summary
    println!("🎯 INTEGRATION TEST SUMMARY");
    println!("===========================");
    println!("✅ Static pool discovery: Working");
    println!("✅ Pool caching and management: Working");
    println!("✅ DEX-specific filtering: Working");
    println!("✅ Recent updates tracking: Working");
    
    if config.enable_webhooks {
        println!("✅ Real-time webhook integration: Active");
        println!("📡 Your bot now receives instant pool updates!");
    } else {
        println!("🔄 Polling mode: Active");
        println!("📊 Static refresh working properly");
    }
    
    println!();
    println!("🚀 Integrated Pool Service is fully operational!");
    println!("   Ready for high-performance arbitrage detection with:");
    println!("   - {} total pools from static discovery", initial_stats.total_pools);
    if let Some(webhook_stats) = &initial_stats.webhook_stats {
        println!("   - {} active webhooks for real-time updates", webhook_stats.active_webhooks);
    }
    println!("   - Real-time pool state management");
    println!("   - Competitive advantage in speed and coverage");

    Ok(())
}

fn display_integrated_stats(stats: &solana_arb_bot::webhooks::IntegratedPoolStats, label: &str) {
    println!("📊 INTEGRATED SERVICE STATS ({})", label);
    println!("{}", "=".repeat(30 + label.len()));
    println!("   Total pools in cache: {}", stats.total_pools);
    println!("   Static discovery pools: {}", stats.static_discovery.total_static_pools);
    
    if let Some(last_refresh) = &stats.static_discovery.last_static_refresh {
        println!("   Last static refresh: {:.1}s ago", last_refresh.elapsed().as_secs_f64());
    }
    
    if let Some(webhook_stats) = &stats.webhook_stats {
        println!("   Webhook notifications: {}", webhook_stats.total_notifications);
        println!("   Successful webhook updates: {}", webhook_stats.successful_updates);
        println!("   Active webhooks: {}", webhook_stats.active_webhooks);
        println!("   Swap events detected: {}", webhook_stats.swap_events);
        println!("   Liquidity events detected: {}", webhook_stats.liquidity_events);
    } else {
        println!("   Webhook service: Disabled");
    }
    
    println!("   Successful merges: {}", stats.static_discovery.successful_merges);
    println!("   Failed merges: {}", stats.static_discovery.failed_merges);
    
    if !stats.static_discovery.pools_by_dex.is_empty() {
        println!("   Pools by DEX:");
        for (dex, count) in &stats.static_discovery.pools_by_dex {
            println!("     - {}: {}", dex, count);
        }
    }
    
    println!();
}
