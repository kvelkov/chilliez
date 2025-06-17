// examples/comprehensive_performance_demo.rs
//! Comprehensive Performance Optimization Demo
//!
//! This demo showcases the complete performance optimization system:
//! - Parallel DEX quote calculations
//! - Concurrent route optimization
//! - Advanced caching with TTL and freshness validation
//! - Real-time performance monitoring
//! - Stress testing under load

use anyhow::Result;
use log::{info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use solana_arb_bot::{
    arbitrage::routing::smart_router::{
        RouteConstraints, RouteRequest, RoutingPriority, SmartRouterConfig,
    },
    performance::{
        PerformanceConfig, PerformanceManager, PoolState, RouteData, RouteInfo, StressTestConfig,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    info!("🚀 Starting Comprehensive Performance Optimization Demo");
    info!("============================================================");

    // Demo 1: Initialize Performance-Optimized System
    demo_performance_initialization().await?;

    // Demo 2: Parallel Processing Capabilities
    demo_parallel_processing().await?;

    // Demo 3: Advanced Caching System
    demo_advanced_caching().await?;

    // Demo 4: Performance-Optimized Smart Router
    demo_smart_router_performance().await?;

    // Demo 5: Real-time Performance Monitoring
    demo_real_time_monitoring().await?;

    // Demo 6: Stress Testing Under Load
    demo_stress_testing().await?;

    // Demo 7: Performance Report Generation
    demo_performance_reporting().await?;

    info!("✅ Comprehensive Performance Demo Completed Successfully!");
    Ok(())
}

async fn demo_performance_initialization() -> Result<()> {
    info!("\n📋 Demo 1: Performance System Initialization");
    info!("---------------------------------------------");

    let performance_config = PerformanceConfig {
        max_concurrent_workers: num_cpus::get().max(8),
        operation_timeout: Duration::from_secs(30),
        pool_cache_ttl: Duration::from_secs(10),
        route_cache_ttl: Duration::from_secs(30),
        quote_cache_ttl: Duration::from_secs(5),
        max_cache_size: 10000,
        metrics_enabled: true,
        benchmark_interval: Duration::from_secs(60),
    };

    info!("🔧 Performance Configuration:");
    info!(
        "   - Max Concurrent Workers: {}",
        performance_config.max_concurrent_workers
    );
    info!(
        "   - Pool Cache TTL: {:?}",
        performance_config.pool_cache_ttl
    );
    info!(
        "   - Route Cache TTL: {:?}",
        performance_config.route_cache_ttl
    );
    info!(
        "   - Quote Cache TTL: {:?}",
        performance_config.quote_cache_ttl
    );
    info!("   - Max Cache Size: {}", performance_config.max_cache_size);

    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);
    performance_manager.start_monitoring().await?;

    info!("✅ Performance system initialized successfully");
    info!("📊 Background monitoring started");

    Ok(())
}

async fn demo_parallel_processing() -> Result<()> {
    info!("\n⚡ Demo 2: Parallel Processing Capabilities");
    info!("-------------------------------------------");

    let performance_config = PerformanceConfig::default();
    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);
    let executor = performance_manager.parallel_executor();

    info!("🔄 Testing concurrent DEX quote calculations...");

    // Simulate DEX quote calculations
    let quote_tasks: Vec<_> = (0..10)
        .map(|i| {
            move || async move {
                // Simulate quote calculation time
                let calculation_time = Duration::from_millis(50 + (i * 10));
                tokio::time::sleep(calculation_time).await;

                // Simulate quote result
                let price = 100.0 + (i as f64 * 0.5);
                let liquidity = 10000 + (i * 1000);

                Ok(format!(
                    "DEX-{}: Price={:.2}, Liquidity={}",
                    i, price, liquidity
                ))
            }
        })
        .collect();

    let start_time = Instant::now();
    let results = executor.execute_concurrent(quote_tasks).await;
    let execution_time = start_time.elapsed();

    info!("⏱️  Parallel Execution Results:");
    info!("   - Tasks Executed: {}", results.len());
    info!("   - Execution Time: {:?}", execution_time);
    info!(
        "   - Successful Tasks: {}",
        results.iter().filter(|r| r.is_ok()).count()
    );

    // Show some results
    for (i, result) in results.iter().take(3).enumerate() {
        match result {
            Ok(quote) => info!("   - Quote {}: {}", i + 1, quote),
            Err(e) => warn!("   - Quote {}: Error: {}", i + 1, e),
        }
    }

    let stats = executor.get_stats().await;
    info!("📈 Executor Statistics:");
    info!("   - Completed Tasks: {}", stats.completed_tasks);
    info!("   - Average Task Duration: {:?}", stats.avg_task_duration);
    info!("   - Active Workers: {}", stats.active_workers);

    Ok(())
}

async fn demo_advanced_caching() -> Result<()> {
    info!("\n💾 Demo 3: Advanced Caching System");
    info!("-----------------------------------");

    let performance_config = PerformanceConfig::default();
    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);
    let cache_manager = performance_manager.cache_manager();

    info!("🔄 Testing cache operations...");

    // Test pool state caching
    info!("📊 Pool State Caching:");
    for i in 0..5 {
        let pool_key = format!("pool_{}", i);
        let pool_state = PoolState {
            pool_address: pool_key.clone(),
            token_a: "SOL".to_string(),
            token_b: "USDC".to_string(),
            reserves_a: 1000000 + i * 100000,
            reserves_b: 50000000 + i * 1000000,
            fee_rate: 0.003,
            liquidity: 10000000 + i * 500000,
            price: 50.0 + i as f64 * 0.5,
            last_updated: 1640000000 + i,
            dex_type: "Raydium".to_string(),
        };

        cache_manager
            .set_pool_state(pool_key.clone(), pool_state.clone())
            .await;
        info!(
            "   - Cached: {} -> Price: {:.2}",
            pool_key, pool_state.price
        );
    }

    // Test cache retrieval
    info!("🔍 Cache Retrieval:");
    for i in 0..3 {
        let pool_key = format!("pool_{}", i);
        if let Some(cached_data) = cache_manager.get_pool_state(&pool_key).await {
            info!(
                "   - Cache HIT: {} -> Price: {:.2}",
                pool_key, cached_data.price
            );
        } else {
            info!("   - Cache MISS: {}", pool_key);
        }
    }

    // Test route caching
    info!("🛣️  Route Caching:");
    let route_info = RouteInfo {
        input_token: "SOL".to_string(),
        output_token: "USDC".to_string(),
        amount: 1000,
        routes: vec![RouteData {
            dex_name: "Raydium".to_string(),
            hops: vec!["SOL".to_string(), "USDC".to_string()],
            estimated_output: 950,
            fees: 50,
            slippage: 0.005,
        }],
        best_route_index: 0,
        total_output: 950,
        price_impact: 0.002,
        execution_time_estimate: Duration::from_millis(100),
    };
    cache_manager
        .set_route("SOL_USDC_1000".to_string(), route_info.clone())
        .await;

    if let Some(cached_routes) = cache_manager.get_route("SOL_USDC_1000").await {
        info!(
            "   - Cached Route: {} -> {} (output: {})",
            cached_routes.input_token, cached_routes.output_token, cached_routes.total_output
        );
    }

    // Show cache statistics
    let cache_stats = cache_manager.get_stats().await;
    info!("📈 Cache Statistics:");
    info!(
        "   - Pool Cache Hit Rate: {:.1}%",
        cache_stats.pool_hit_rate * 100.0
    );
    info!(
        "   - Route Cache Hit Rate: {:.1}%",
        cache_stats.route_hit_rate * 100.0
    );
    info!(
        "   - Quote Cache Hit Rate: {:.1}%",
        cache_stats.quote_hit_rate * 100.0
    );
    info!("   - Total Cache Entries: {}", cache_stats.total_entries);

    Ok(())
}

async fn demo_smart_router_performance() -> Result<()> {
    info!("\n🧠 Demo 4: Performance-Optimized Smart Router");
    info!("----------------------------------------------");

    // Create a mock smart router configuration
    let router_config = SmartRouterConfig {
        enable_intelligent_routing: true,
        enable_cross_dex_routing: true,
        enable_route_caching: true,
        route_cache_ttl: Duration::from_secs(30),
        max_route_computation_time: Duration::from_secs(5),
        min_improvement_threshold: 0.01,
        ..Default::default()
    };

    info!("🔧 Smart Router Configuration:");
    info!(
        "   - Intelligent Routing: {}",
        router_config.enable_intelligent_routing
    );
    info!(
        "   - Cross-DEX Routing: {}",
        router_config.enable_cross_dex_routing
    );
    info!("   - Route Caching: {}", router_config.enable_route_caching);
    info!("   - Cache TTL: {:?}", router_config.route_cache_ttl);

    // Simulate route optimization
    info!("🔄 Simulating route optimization with performance enhancements...");
    let start_time = Instant::now();

    // Mock route request
    let route_request = RouteRequest {
        input_token: "SOL".to_string(),
        output_token: "USDC".to_string(),
        amount: 1000,
        max_slippage: Some(0.005),
        max_price_impact: Some(0.01),
        preferred_dexs: None,
        excluded_dexs: None,
        max_hops: Some(3),
        enable_splitting: true,
        speed_priority: RoutingPriority::CostOptimized,
        timestamp: SystemTime::now(),
        constraints: RouteConstraints {
            execution_deadline: None,
            max_gas_cost: None,
            allowed_dexs: None,
            forbidden_dexs: None,
        },
        min_amount_out: Some(950),
    };

    info!("📋 Route Request:");
    info!(
        "   - Path: {} -> {}",
        route_request.input_token, route_request.output_token
    );
    info!("   - Amount: {}", route_request.amount);
    info!("   - Priority: {:?}", route_request.speed_priority);
    info!("   - Max Hops: {:?}", route_request.max_hops);
    if let Some(slippage) = route_request.max_slippage {
        info!("   - Max Slippage: {:.1}%", slippage * 100.0);
    }

    // Simulate processing time
    tokio::time::sleep(Duration::from_millis(100)).await;
    let processing_time = start_time.elapsed();

    info!("⚡ Performance Results:");
    info!("   - Route Calculation Time: {:?}", processing_time);
    info!("   - Parallel Processing: ✅ Enabled");
    info!("   - Advanced Caching: ✅ Enabled");
    info!("   - Performance Monitoring: ✅ Active");

    Ok(())
}

async fn demo_real_time_monitoring() -> Result<()> {
    info!("\n📊 Demo 5: Real-time Performance Monitoring");
    info!("--------------------------------------------");

    let performance_config = PerformanceConfig::default();
    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);

    // Start monitoring
    performance_manager.start_monitoring().await?;

    info!("🔄 Running monitored operations...");

    // Simulate some operations while monitoring
    for i in 0..5 {
        info!("   Operation {} in progress...", i + 1);

        // Simulate work
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Get metrics snapshot
        let metrics = performance_manager.metrics_collector();
        let metrics_summary = metrics.read().await.get_summary();

        info!("   📈 Current Metrics:");
        info!("      - CPU Usage: {:.1}%", metrics_summary.cpu_usage);
        info!(
            "      - Memory Usage: {:.1}MB",
            metrics_summary.memory_usage_mb
        );
        info!(
            "      - Network Latency: {:.1}ms",
            metrics_summary.network_latency_ms
        );
    }

    info!("✅ Real-time monitoring demonstrated");

    Ok(())
}

async fn demo_stress_testing() -> Result<()> {
    info!("\n🏋️ Demo 6: Stress Testing Under Load");
    info!("-------------------------------------");

    let performance_config = PerformanceConfig {
        max_concurrent_workers: 16,
        ..Default::default()
    };
    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);
    let benchmark_runner = performance_manager.benchmark_runner();

    info!("🔄 Running stress test...");

    let stress_config = StressTestConfig {
        duration: Duration::from_secs(5),
        concurrent_operations: 10,
        operation_interval: Duration::from_millis(100),
        target_ops_per_second: 50.0,
    };

    let start_time = Instant::now();
    let benchmark_results = benchmark_runner.run_stress_test(stress_config).await?;
    let test_duration = start_time.elapsed();

    info!("⚡ Stress Test Results:");
    info!("   - Test Duration: {:?}", test_duration);
    info!(
        "   - Operations Completed: {}",
        benchmark_results.operations_completed
    );
    info!(
        "   - Operations/Second: {:.2}",
        benchmark_results.operations_per_second
    );
    info!(
        "   - Average Latency: {:.2}ms",
        benchmark_results.average_latency.as_millis()
    );
    info!(
        "   - Memory Used: {:.1}MB",
        benchmark_results.memory_used_mb
    );
    info!(
        "   - Success Rate: {:.2}%",
        benchmark_results.success_rate * 100.0
    );

    // System benchmark
    info!("🖥️  Running system benchmark...");
    benchmark_runner.run_system_benchmark().await?;

    Ok(())
}

async fn demo_performance_reporting() -> Result<()> {
    info!("\n📋 Demo 7: Performance Report Generation");
    info!("----------------------------------------");

    let performance_config = PerformanceConfig::default();
    let performance_manager = Arc::new(PerformanceManager::new(performance_config).await?);

    // Generate some activity first
    let executor = performance_manager.parallel_executor();
    let cache_manager = performance_manager.cache_manager();

    // Simulate some operations
    let tasks: Vec<_> = (0..5)
        .map(|i| {
            move || async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(format!("Task {}", i))
            }
        })
        .collect();

    executor.execute_concurrent(tasks).await;

    // Cache some data
    for i in 0..3 {
        let pool_state = PoolState {
            pool_address: format!("pool_{}", i),
            token_a: "SOL".to_string(),
            token_b: "USDC".to_string(),
            reserves_a: 1000000,
            reserves_b: 50000000,
            fee_rate: 0.003,
            liquidity: 10000000,
            price: 50.0,
            last_updated: 1640000000,
            dex_type: "Raydium".to_string(),
        };
        cache_manager
            .set_pool_state(format!("pool_{}", i), pool_state)
            .await;
    }

    // Generate comprehensive report
    let report = performance_manager.get_performance_report().await;

    info!("📊 Comprehensive Performance Report:");
    info!("{}", report.summary());

    info!("✅ Performance reporting demonstrated");

    Ok(())
}
