//! Jupiter Multi-Route Optimization Demo
//! 
//! This example demonstrates how to use Jupiter's multi-route optimization
//! capabilities through the JupiterFallbackManager.

use std::sync::Arc;
use tokio::sync::Mutex;
use solana_arb_bot::{
    arbitrage::jupiter::{
        JupiterFallbackManager,
        integration::JupiterIntegrationConfig,
        routes::{RouteOptimizationConfig, RouteScoringConfig, RouteCacheConfig},
        cache::CacheConfig,
    },
    dex::clients::jupiter::JupiterClient,
    local_metrics::Metrics,
    config::settings::Config,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("🛣️  Jupiter Multi-Route Optimization Demo");
    println!("==========================================");

    // Create configuration with route optimization enabled
    let mut config = Config::test_default();
    config.jupiter_route_optimization_enabled = true;
    config.jupiter_max_parallel_routes = 5;
    config.jupiter_max_alternative_routes = 10;
    config.jupiter_route_evaluation_timeout_ms = 3000;
    config.jupiter_min_route_improvement_pct = 0.1;

    // Create Jupiter integration config
    let jupiter_config = JupiterIntegrationConfig {
        fallback_enabled: true,
        min_profit_pct: 0.001,
        max_slippage_bps: 50,
        
        // Cache configuration
        cache: CacheConfig {
            enabled: true,
            ttl_seconds: 5,
            max_entries: 1000,
            amount_bucket_size: 1_000_000,
            volatility_threshold_pct: 2.0,
            target_hit_rate: 0.7,
        },
        
        // Route optimization configuration
        route_optimization: RouteOptimizationConfig {
            enabled: true,
            max_parallel_routes: 5,
            max_alternative_routes: 10,
            route_evaluation_timeout_ms: 3000,
            min_route_improvement_pct: 0.1,
            scoring: RouteScoringConfig {
                output_amount_weight: 0.4,
                price_impact_weight: 0.3,
                hop_count_weight: 0.1,
                reliability_weight: 0.15,
                gas_cost_weight: 0.05,
            },
            caching: RouteCacheConfig {
                enabled: true,
                route_ttl_seconds: 30,
                max_cached_route_sets: 500,
                market_movement_threshold_pct: 1.0,
            },
        },
        
        monitoring: Default::default(),
    };

    // Create metrics and Jupiter client
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    let jupiter_client = Arc::new(JupiterClient::new());

    // Create the fallback manager with route optimization
    let fallback_manager = JupiterFallbackManager::new(
        jupiter_client,
        jupiter_config,
        Arc::clone(&metrics),
    );

    println!("✅ Created JupiterFallbackManager with route optimization enabled");
    println!("   📊 Max parallel routes: {}", config.jupiter_max_parallel_routes);
    println!("   🔀 Max alternative routes: {}", config.jupiter_max_alternative_routes);
    println!("   ⏱️  Route evaluation timeout: {}ms", config.jupiter_route_evaluation_timeout_ms);
    println!("   📈 Min route improvement: {}%", config.jupiter_min_route_improvement_pct);

    // Check if route optimization is enabled
    if fallback_manager.is_route_optimization_enabled() {
        println!("\n🛣️  Route optimization is ENABLED");
        
        // Example token pairs for demonstration
        let examples = vec![
            ("USDC", "So11111111111111111111111111111111111111112", "SOL"), // USDC -> SOL
            ("SOL", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "USDC"), // SOL -> USDC
        ];

        for (input_name, input_mint, output_name) in examples {
            println!("\n📊 Testing route optimization for {} -> {}", input_name, output_name);
            
            let amount = if input_name == "USDC" { 1_000_000 } else { 1_000_000_000 }; // 1 USDC or 1 SOL
            
            // Try to get optimal route (this would normally make real API calls)
            println!("   🔍 Discovering optimal routes for {} {}", 
                     amount as f64 / if input_name == "USDC" { 1_000_000.0 } else { 1_000_000_000.0 }, 
                     input_name);
            
            // In a real environment, this would evaluate multiple routes
            match fallback_manager.get_optimal_route(
                input_mint,
                "So11111111111111111111111111111111111111112", // SOL mint as example
                amount,
                Some(50), // 0.5% slippage
            ).await {
                Ok(result) => {
                    println!("   ✅ Route optimization completed:");
                    println!("      📈 Routes evaluated: {}", result.routes_evaluated);
                    println!("      ❌ Routes failed: {}", result.routes_failed);
                    println!("      ⏱️  Evaluation time: {:?}", result.total_evaluation_time);
                    println!("      🏆 Selection reason: {}", result.selection_reason);
                    println!("      💰 Best output: {} units", result.best_route.quote.out_amount);
                }
                Err(e) => {
                    println!("   ⚠️  Route optimization failed: {} (expected in demo mode)", e);
                }
            }
            
            // Also try cached single quote for comparison
            println!("   📡 Trying cached single quote for comparison...");
            match fallback_manager.get_quote_with_cache(
                input_mint,
                "So11111111111111111111111111111111111111112",
                amount,
                Some(50),
            ).await {
                Ok(quote) => {
                    println!("   ✅ Cached quote: {} units output", quote.out_amount);
                }
                Err(e) => {
                    println!("   ⚠️  Cached quote failed: {} (expected in demo mode)", e);
                }
            }
        }

        // Show route optimization statistics
        if let Some(stats) = fallback_manager.get_route_optimization_stats().await {
            println!("\n📊 Route Optimization Statistics:");
            println!("   {}", stats);
        }

        // Show cache statistics
        let cache_stats = fallback_manager.get_cache_statistics().await;
        println!("\n💾 Cache Statistics:");
        println!("   Hit rate: {:.1}%", cache_stats.hit_rate * 100.0);
        println!("   Total requests: {}", cache_stats.total_requests);
        println!("   Cache hits: {}", cache_stats.cache_hits);
        println!("   Cache misses: {}", cache_stats.cache_misses);

    } else {
        println!("\n❌ Route optimization is DISABLED");
        println!("   This would fall back to single-route mode with caching");
    }

    println!("\n🎯 Multi-route optimization demo completed!");
    println!("   In production, this would provide:");
    println!("   • Parallel evaluation of multiple route options");
    println!("   • Intelligent scoring based on output, price impact, and reliability");  
    println!("   • Route caching for improved performance");
    println!("   • Automatic fallback to single routes when optimization fails");
    println!("   • Comprehensive monitoring and analytics");

    Ok(())
}
