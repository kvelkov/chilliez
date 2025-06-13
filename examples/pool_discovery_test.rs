// examples/pool_discovery_test.rs
//! Simple test to verify pool discovery is working with real DEX APIs

use solana_arb_bot::{
    dex::{
        orca::OrcaClient,
        raydium::RaydiumClient,
        meteora::MeteoraClient,
        lifinity::LifinityClient,
        quote::DexClient,
    },
};
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("🔍 Pool Discovery Test");
    println!("=====================");
    println!("Testing real pool discovery from DEX APIs...\n");

    // Test each DEX client individually
    let mut total_pools = 0;

    // Test Orca
    println!("🌊 Testing Orca pool discovery...");
    let orca_client = OrcaClient::new();
    match orca_client.discover_pools().await {
        Ok(pools) => {
            println!("   ✅ Orca: {} pools discovered", pools.len());
            total_pools += pools.len();
            
            // Show a few examples
            if !pools.is_empty() {
                println!("   📋 Sample pools:");
                for (i, pool) in pools.iter().take(3).enumerate() {
                    println!("      {}. {} ({})", i+1, pool.name, pool.address);
                }
            }
        }
        Err(e) => println!("   ❌ Orca discovery failed: {}", e),
    }
    println!();

    // Test Raydium
    println!("⚡ Testing Raydium pool discovery...");
    let raydium_client = RaydiumClient::new();
    match raydium_client.discover_pools().await {
        Ok(pools) => {
            println!("   ✅ Raydium: {} pools discovered", pools.len());
            total_pools += pools.len();
            
            // Show a few examples
            if !pools.is_empty() {
                println!("   📋 Sample pools:");
                for (i, pool) in pools.iter().take(3).enumerate() {
                    println!("      {}. {} ({})", i+1, pool.name, pool.address);
                }
            }
        }
        Err(e) => println!("   ❌ Raydium discovery failed: {}", e),
    }
    println!();

    // Test Meteora
    println!("🌟 Testing Meteora pool discovery...");
    let meteora_client = MeteoraClient::new();
    match meteora_client.discover_pools().await {
        Ok(pools) => {
            println!("   ✅ Meteora: {} pools discovered", pools.len());
            total_pools += pools.len();
            
            // Show a few examples
            if !pools.is_empty() {
                println!("   📋 Sample pools:");
                for (i, pool) in pools.iter().take(3).enumerate() {
                    println!("      {}. {} ({})", i+1, pool.name, pool.address);
                }
            }
        }
        Err(e) => println!("   ❌ Meteora discovery failed: {}", e),
    }
    println!();

    // Test Lifinity
    println!("🔥 Testing Lifinity pool discovery...");
    let lifinity_client = LifinityClient::new();
    match lifinity_client.discover_pools().await {
        Ok(pools) => {
            println!("   ✅ Lifinity: {} pools discovered", pools.len());
            total_pools += pools.len();
            
            // Show a few examples
            if !pools.is_empty() {
                println!("   📋 Sample pools:");
                for (i, pool) in pools.iter().take(3).enumerate() {
                    println!("      {}. {} ({})", i+1, pool.name, pool.address);
                }
            }
        }
        Err(e) => println!("   ❌ Lifinity discovery failed: {}", e),
    }
    println!();

    // Test Whirlpool - skip for now (requires cache setup)
    println!("🌀 Skipping Whirlpool (requires additional setup)...");
    println!();

    // Summary
    println!("📊 DISCOVERY SUMMARY");
    println!("====================");
    println!("🎯 Total pools discovered: {}", total_pools);
    
    if total_pools > 0 {
        println!("✅ Pool discovery is working successfully!");
        println!("🚀 Ready for arbitrage opportunity detection!");
    } else {
        println!("⚠️  No pools discovered - check API endpoints and network connectivity");
    }

    Ok(())
}
