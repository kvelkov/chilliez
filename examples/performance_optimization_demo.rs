// examples/performance_optimization_demo.rs
//! Performance Optimization Demonstration
//!
//! This example showcases all the performance optimization features:
//! - Parallel processing across multiple DEXs
//! - Advanced caching with TTL
//! - Real-time performance monitoring
//! - Comprehensive benchmarking

use anyhow::Result;
use log::{info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use solana_arb_bot::{PerformanceConfig, PerformanceManager};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("🚀 Performance Optimization Demo Starting");
    println!("===========================================");

    // === PART 1: PERFORMANCE MANAGER INITIALIZATION ===
    info!("\n📊 === PERFORMANCE MANAGER SETUP ===");

    let perf_config = PerformanceConfig {
        max_concurrent_workers: 8,
        operation_timeout: Duration::from_secs(30),
        pool_cache_ttl: Duration::from_secs(10),
        route_cache_ttl: Duration::from_secs(30),
        quote_cache_ttl: Duration::from_secs(5),
        max_cache_size: 10000,
        metrics_enabled: true,
        benchmark_interval: Duration::from_secs(30),
    };

    let performance_manager = Arc::new(PerformanceManager::new(perf_config.clone()).await?);
    info!(
        "✅ Performance Manager initialized with {} workers",
        perf_config.max_concurrent_workers
    );

    // Start background monitoring
    performance_manager.start_monitoring().await?;
    info!("✅ Background performance monitoring started");

    // === PART 2: PARALLEL PROCESSING DEMO ===
    info!("\n⚡ === PARALLEL PROCESSING DEMONSTRATION ===");

    let parallel_executor = performance_manager.parallel_executor();

    // Simulate concurrent quote fetching across multiple DEXs
    info!("🔄 Demonstrating parallel quote fetching...");

    let quote_tasks: Vec<_> = (0..20)
        .map(|i| {
            move || async move {
                // Simulate quote fetching with varying latencies
                let latency = Duration::from_millis(50 + (i * 10) % 200);
                sleep(latency).await;

                // Simulate 95% success rate
                if i % 20 == 0 {
                    Err(anyhow::anyhow!("Simulated quote fetch failure"))
                } else {
                    Ok(format!(
                        "Quote result from DEX {}: 1.0 SOL = {:.2} USDC",
                        i % 5,
                        100.0 + (i as f64 * 0.5)
                    ))
                }
            }
        })
        .collect();

    let quote_start = Instant::now();
    let quote_results = parallel_executor.execute_concurrent(quote_tasks).await;
    let quote_duration = quote_start.elapsed();

    let successful_quotes = quote_results.iter().filter(|r| r.is_ok()).count();
    info!(
        "✅ Parallel quotes completed: {}/{} successful in {:?}",
        successful_quotes,
        quote_results.len(),
        quote_duration
    );

    // Demonstrate parallel transaction simulations
    info!("🔄 Demonstrating parallel transaction simulations...");

    let sim_tasks: Vec<_> = (0..15)
        .map(|i| {
            move || async move {
                // Simulate transaction simulation
                let sim_time = Duration::from_millis(20 + (i * 5) % 100);
                sleep(sim_time).await;

                Ok(format!(
                    "Simulation {}: Success, Gas: {} units",
                    i,
                    45000 + (i * 1000)
                ))
            }
        })
        .collect();

    let sim_start = Instant::now();
    let sim_results = parallel_executor.execute_concurrent(sim_tasks).await;
    let sim_duration = sim_start.elapsed();

    let successful_sims = sim_results.iter().filter(|r| r.is_ok()).count();
    info!(
        "✅ Parallel simulations completed: {}/{} successful in {:?}",
        successful_sims,
        sim_results.len(),
        sim_duration
    );

    // === PART 3: CACHING DEMONSTRATION ===
    info!("\n🗄️ === ADVANCED CACHING DEMONSTRATION ===");

    let cache_manager = performance_manager.cache_manager();

    // Demonstrate pool state caching
    info!("📝 Testing pool state caching...");

    use solana_arb_bot::performance::cache::PoolState;
    let pool_state = PoolState {
        pool_address: "RaydiumPoolABC123".to_string(),
        token_a: "So11111111111111111111111111111111111111112".to_string(), // SOL
        token_b: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
        reserves_a: 10_000_000_000,                                         // 10 SOL
        reserves_b: 1_000_000_000,                                          // 1000 USDC
        fee_rate: 0.0025,
        liquidity: 500_000_000,
        price: 100.0,
        last_updated: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        dex_type: "Raydium".to_string(),
    };

    // Cache the pool state
    cache_manager
        .set_pool_state("RaydiumPoolABC123".to_string(), pool_state.clone())
        .await;
    info!("✅ Pool state cached for Raydium pool");

    // Retrieve from cache
    let cached_pool = cache_manager.get_pool_state("RaydiumPoolABC123").await;
    match cached_pool {
        Some(pool) => {
            info!(
                "✅ Pool state retrieved from cache: {} reserves, {:.4} fee rate",
                pool.reserves_a, pool.fee_rate
            );
        }
        None => {
            warn!("❌ Failed to retrieve pool state from cache");
        }
    }

    // Demonstrate route caching
    info!("📝 Testing route caching...");

    use solana_arb_bot::performance::cache::{RouteData, RouteInfo};
    let route_info = RouteInfo {
        input_token: "So11111111111111111111111111111111111111112".to_string(),
        output_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
        amount: 1_000_000_000, // 1 SOL
        routes: vec![
            RouteData {
                dex_name: "Raydium".to_string(),
                hops: vec!["SOL".to_string(), "USDC".to_string()],
                estimated_output: 99_500_000, // 99.5 USDC
                fees: 250_000,                // 0.25 USDC
                slippage: 0.005,
            },
            RouteData {
                dex_name: "Orca".to_string(),
                hops: vec!["SOL".to_string(), "USDC".to_string()],
                estimated_output: 99_750_000, // 99.75 USDC
                fees: 300_000,                // 0.3 USDC
                slippage: 0.0025,
            },
        ],
        best_route_index: 1, // Orca has better output
        total_output: 99_750_000,
        price_impact: 0.0025,
        execution_time_estimate: Duration::from_millis(250),
    };

    let route_key = cache_manager.generate_route_key(
        "So11111111111111111111111111111111111111112",
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        1_000_000_000,
    );

    cache_manager
        .set_route(route_key.clone(), route_info.clone())
        .await;
    info!("✅ Route cached for SOL->USDC 1.0 SOL trade");

    // Retrieve route from cache
    let cached_route = cache_manager.get_route(&route_key).await;
    match cached_route {
        Some(route) => {
            info!(
                "✅ Route retrieved from cache: {} routes, best output: {} USDC",
                route.routes.len(),
                route.total_output as f64 / 1_000_000.0
            );
        }
        None => {
            warn!("❌ Failed to retrieve route from cache");
        }
    }

    // === PART 4: PERFORMANCE METRICS COLLECTION ===
    info!("\n📈 === PERFORMANCE METRICS DEMONSTRATION ===");

    let metrics_collector = performance_manager.metrics_collector();

    // Simulate various operations with metrics collection
    info!("📊 Collecting performance metrics...");

    for i in 0..50 {
        let operation_start = Instant::now();

        // Simulate different types of operations
        match i % 4 {
            0 => {
                // Route calculation
                sleep(Duration::from_millis(20 + (i % 30))).await;
                let duration = operation_start.elapsed();
                let mut collector = metrics_collector.write().await;
                collector.record_operation("route_calculation", duration, i % 10 != 0);
            }
            1 => {
                // Quote fetching
                sleep(Duration::from_millis(50 + (i % 40))).await;
                let duration = operation_start.elapsed();
                let mut collector = metrics_collector.write().await;
                collector.record_operation("quote_fetching", duration, i % 15 != 0);
            }
            2 => {
                // Transaction simulation
                sleep(Duration::from_millis(30 + (i % 20))).await;
                let duration = operation_start.elapsed();
                let mut collector = metrics_collector.write().await;
                collector.record_operation("transaction_simulation", duration, i % 8 != 0);
            }
            _ => {
                // Cache operations
                sleep(Duration::from_millis(5 + (i % 10))).await;
                let duration = operation_start.elapsed();
                let mut collector = metrics_collector.write().await;
                collector.record_operation("cache_operation", duration, true);
            }
        }
    }

    // Update system stats
    {
        let mut collector = metrics_collector.write().await;
        collector.record_system_stats().await;
    }

    info!("✅ Performance metrics collected for 50 operations");

    // === PART 5: BENCHMARKING ===
    info!("\n🏁 === PERFORMANCE BENCHMARKING ===");

    let benchmark_runner = performance_manager.benchmark_runner();

    info!("🔬 Running comprehensive system benchmarks...");
    let benchmark_results = benchmark_runner.run_system_benchmark().await?;

    for result in &benchmark_results {
        info!(
            "📊 Benchmark '{}': {:.1} ops/sec, {:.2}ms avg latency, {:.1}% success rate",
            result.test_name,
            result.operations_per_second,
            result.average_latency.as_millis(),
            result.success_rate * 100.0
        );
    }

    // === PART 6: PERFORMANCE REPORT ===
    info!("\n📋 === COMPREHENSIVE PERFORMANCE REPORT ===");

    let performance_report = performance_manager.get_performance_report().await;

    println!("\n{}", performance_report.summary());

    // Get cache statistics
    let cache_stats = cache_manager.get_stats().await;
    println!("\nCache Performance:");
    println!(
        "- Pool Cache Hit Rate: {:.1}%",
        cache_stats.pool_hit_rate * 100.0
    );
    println!(
        "- Route Cache Hit Rate: {:.1}%",
        cache_stats.route_hit_rate * 100.0
    );
    println!(
        "- Quote Cache Hit Rate: {:.1}%",
        cache_stats.quote_hit_rate * 100.0
    );
    println!("- Total Cache Entries: {}", cache_stats.total_entries);

    // Get parallel execution statistics
    let parallel_stats = parallel_executor.get_stats().await;
    println!("\nParallel Execution:");
    println!("- Active Workers: {}", parallel_stats.active_workers);
    println!("- Completed Tasks: {}", parallel_stats.completed_tasks);
    println!("- Failed Tasks: {}", parallel_stats.failed_tasks);
    println!(
        "- Average Task Duration: {:?}",
        parallel_stats.avg_task_duration
    );

    // === PART 7: STRESS TEST ===
    info!("\n🔥 === STRESS TESTING ===");

    use solana_arb_bot::performance::benchmark::StressTestConfig;
    let stress_config = StressTestConfig {
        duration: Duration::from_secs(10),
        concurrent_operations: 20,
        operation_interval: Duration::from_millis(10),
        target_ops_per_second: 100.0,
    };

    info!("🚨 Running stress test for 10 seconds...");
    let stress_result = benchmark_runner.run_stress_test(stress_config).await?;

    info!(
        "✅ Stress test completed: {:.1} ops/sec, {:.1}% success rate",
        stress_result.operations_per_second,
        stress_result.success_rate * 100.0
    );

    // === PART 8: FINAL METRICS SUMMARY ===
    info!("\n📊 === FINAL PERFORMANCE SUMMARY ===");

    let _final_report = performance_manager.get_performance_report().await;
    let metrics_summary = {
        let collector = metrics_collector.read().await;
        collector.get_summary()
    };

    println!("\n🎯 PERFORMANCE OPTIMIZATION RESULTS:");
    println!("=====================================");
    println!(
        "✅ Parallel Processing: Up to {} concurrent workers",
        perf_config.max_concurrent_workers
    );
    println!(
        "✅ Cache Performance: {:.1}% average hit rate",
        (cache_stats.pool_hit_rate + cache_stats.route_hit_rate + cache_stats.quote_hit_rate) / 3.0
            * 100.0
    );
    println!(
        "✅ System Throughput: {:.1} operations/second",
        metrics_summary.throughput_ops_per_sec
    );
    println!(
        "✅ Average Latency: {:.1}ms",
        metrics_summary.network_latency_ms
    );
    println!(
        "✅ System Health: {:.1}%",
        metrics_summary.system_health_score * 100.0
    );
    println!("✅ Error Rate: {:.2}%", metrics_summary.error_rate * 100.0);

    println!("\n🚀 PERFORMANCE FEATURES DEMONSTRATED:");
    println!("- ⚡ Concurrent quote calculations across DEXs");
    println!("- 🔄 Parallel transaction simulation");
    println!("- 🗄️ Smart caching with TTL validation");
    println!("- 📊 Real-time performance monitoring");
    println!("- 🏁 Comprehensive benchmarking suite");
    println!("- 🔥 System stress testing");
    println!("- 📈 Detailed performance analytics");

    println!("\n✨ OPTIMIZATION IMPACT:");
    println!("- Reduced latency through parallel processing");
    println!("- Improved throughput via intelligent caching");
    println!("- Enhanced reliability with comprehensive monitoring");
    println!("- Better resource utilization through load balancing");

    info!("\n🎉 Performance Optimization Demo Complete!");
    println!("==========================================");

    Ok(())
}
