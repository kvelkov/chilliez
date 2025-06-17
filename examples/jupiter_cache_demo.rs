use solana_arb_bot::{arbitrage::jupiter::CacheConfig, dex::clients::jupiter::JupiterClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("🪐 Jupiter Cache Integration Demo");
    println!("=================================");

    // Create Jupiter client with cache enabled
    let cache_config = CacheConfig {
        enabled: true,
        ttl_seconds: 5,
        max_entries: 100,
        amount_bucket_size: 1_000_000,
        volatility_threshold_pct: 2.0,
        target_hit_rate: 0.7,
    };

    let jupiter_client = JupiterClient::new_with_cache_config(cache_config);

    println!("✅ Jupiter client created with cache enabled");
    println!("   - TTL: 5 seconds");
    println!("   - Max entries: 100");
    println!("   - Amount bucketing: 1M lamports");

    // Example token mints (SOL and USDC)
    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let amount = 1_000_000; // 1 SOL (in lamports)
    let slippage_bps = 50; // 0.5%

    println!("\n🔍 Testing cache behavior...");

    // First request - should be a cache miss
    println!("\n1️⃣ First request (expecting cache MISS):");
    let start = std::time::Instant::now();

    match jupiter_client
        .get_quote_with_fallback(sol_mint, usdc_mint, amount, slippage_bps)
        .await
    {
        Ok(quote) => {
            let duration = start.elapsed();
            println!(
                "   ✅ Quote received: {} SOL -> {} USDC",
                quote.in_amount, quote.out_amount
            );
            println!("   ⏱️  Duration: {:?} (API call + caching)", duration);
        }
        Err(e) => {
            println!("   ❌ Quote failed: {} (this is expected in demo mode)", e);
        }
    }

    // Get cache stats after first request
    if let Some(stats) = jupiter_client.get_cache_stats().await {
        println!(
            "   📊 Cache stats: {} hits, {} misses",
            stats.hits, stats.misses
        );
    }

    // Second request - should be a cache hit (if first succeeded)
    println!("\n2️⃣ Second request (expecting cache HIT if first succeeded):");
    let start = std::time::Instant::now();

    match jupiter_client
        .get_quote_with_fallback(sol_mint, usdc_mint, amount, slippage_bps)
        .await
    {
        Ok(quote) => {
            let duration = start.elapsed();
            println!(
                "   ✅ Quote received: {} SOL -> {} USDC",
                quote.in_amount, quote.out_amount
            );
            println!(
                "   ⏱️  Duration: {:?} (should be much faster from cache)",
                duration
            );
        }
        Err(e) => {
            println!("   ❌ Quote failed: {} (this is expected in demo mode)", e);
        }
    }

    // Get final cache stats
    if let Some(stats) = jupiter_client.get_cache_stats().await {
        println!(
            "   📊 Final cache stats: {} hits, {} misses",
            stats.hits, stats.misses
        );
        if stats.hits + stats.misses > 0 {
            let hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;
            println!("   🎯 Hit rate: {:.1}%", hit_rate * 100.0);
        }
    }

    // Test amount bucketing
    println!("\n3️⃣ Testing amount bucketing (slightly different amount):");
    let bucketed_amount = 1_200_000; // 1.2 SOL - should bucket to same 1M group

    match jupiter_client
        .get_quote_with_fallback(sol_mint, usdc_mint, bucketed_amount, slippage_bps)
        .await
    {
        Ok(quote) => {
            println!(
                "   ✅ Bucketed quote: {} SOL -> {} USDC",
                quote.in_amount, quote.out_amount
            );
            println!("   📦 Amount {} bucketed with {}", bucketed_amount, amount);
        }
        Err(e) => {
            println!(
                "   ❌ Bucketed quote failed: {} (this is expected in demo mode)",
                e
            );
        }
    }

    // Demonstrate cache management
    println!("\n🧹 Cache management demo:");
    println!(
        "   - Cache is enabled: {}",
        jupiter_client.is_cache_enabled()
    );

    // Clear cache
    jupiter_client.clear_cache().await;
    println!("   - Cache cleared manually");

    // Test volatility invalidation
    jupiter_client.invalidate_cache_for_volatility().await;
    println!("   - Volatility check performed");

    println!("\n✨ Jupiter cache integration demo complete!");
    println!("   The cache will automatically:");
    println!("   - Store successful quotes for 5 seconds");
    println!("   - Group similar amounts for better hit rates");
    println!("   - Invalidate during high market volatility");
    println!("   - Use LRU eviction when full");

    Ok(())
}
