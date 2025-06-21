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
    monitoring::performance::{
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
    // ...existing code...
    Ok(())
}
