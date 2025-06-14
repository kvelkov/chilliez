//! Flamegraph Profiling Demo for Sprint 4
//!
//! Demonstrates how to use cargo flamegraph for performance profiling
//!
//! To run this demo with flamegraph:
//! 1. Install flamegraph: `cargo install flamegraph`
//! 2. Run with profiling: `cargo flamegraph --example flamegraph_profiling_demo`
//! 3. Open the generated flamegraph.svg in a browser

use solana_arb_bot::{
    testing::{MockDexEnvironment, MarketCondition},
    arbitrage::engine::ArbitrageEngine,
    metrics::Metrics,
    config::settings::Config,
    dex::quote::DexClient,
};
use dashmap::DashMap;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use log::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("🔥 Starting Flamegraph Profiling Demo");

    let profiler = FlamegraphProfiler::new().await?;
    profiler.run_profiling_workload().await?;

    info!("🎉 Profiling demo completed!");
    info!("📊 Check flamegraph.svg for performance analysis");
    Ok(())
}

struct FlamegraphProfiler {
    mock_environment: Arc<MockDexEnvironment>,
    arbitrage_engine: Arc<ArbitrageEngine>,
}

impl FlamegraphProfiler {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("🔧 Setting up profiling environment...");
        let mock_environment = Arc::new(MockDexEnvironment::new(MarketCondition::Normal));
        let hot_cache = Arc::new(DashMap::new());

        for dex in mock_environment.dexes.values() {
            match DexClient::discover_pools(&**dex).await {
                Ok(pools) => {
                    for pool in pools {
                        hot_cache.insert(pool.address, Arc::new(pool));
                    }
                }
                Err(err) => {
                    warn!("Failed to discover pools for a DEX: {}", err);
                }
            }
        }

        let metrics = Arc::new(Mutex::new(Metrics::new(150.0, None)));
        let dex_clients = mock_environment.get_dex_clients();
        let config = Arc::new(Config::test_default());

        let arbitrage_engine = Arc::new(ArbitrageEngine::new(
            hot_cache.clone(),
            None,
            None,
            None,
            config,
            metrics,
            dex_clients,
            None,
        ));

        info!("✅ Profiling environment ready with {} pools", arbitrage_engine.hot_cache.len());

        Ok(Self {
            mock_environment,
            arbitrage_engine,
        })
    }

    async fn run_profiling_workload(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Run each workload directly to avoid function pointer type issues
        info!("📊 Running workload: Intensive opportunity detection");
        let start = Instant::now();
        self.workload_opportunity_detection().await?;
        info!("⏱ Completed 'Intensive opportunity detection' in {:?}", start.elapsed());

        info!("📊 Running workload: Hot cache operations");
        let start = Instant::now();
        self.workload_cache_operations().await?;
        info!("⏱ Completed 'Hot cache operations' in {:?}", start.elapsed());

        info!("📊 Running workload: Concurrent operations");
        let start = Instant::now();
        self.workload_concurrent_operations().await?;
        info!("⏱ Completed 'Concurrent operations' in {:?}", start.elapsed());

        info!("📊 Running workload: Memory allocation patterns");
        let start = Instant::now();
        self.workload_memory_patterns().await?;
        info!("⏱ Completed 'Memory allocation patterns' in {:?}", start.elapsed());

        Ok(())
    }

    async fn workload_opportunity_detection(&self) -> Result<(), Box<dyn std::error::Error>> {
        for i in 0..1000 {
            let _ = self.arbitrage_engine.detect_arbitrage_opportunities().await?;
            if i % 100 == 0 {
                self.mock_environment.simulate_market_movements();
            }
        }
        Ok(())
    }

    async fn workload_cache_operations(&self) -> Result<(), Box<dyn std::error::Error>> {
        let addresses: Vec<_> = self.arbitrage_engine.hot_cache.iter()
            .map(|entry| *entry.key())
            .collect();

        for i in 0..10_000 {
            for (j, &addr) in addresses.iter().enumerate() {
                if (i + j) % 10 == 0 {
                    if let Some(pool) = self.arbitrage_engine.hot_cache.get(&addr) {
                        self.arbitrage_engine.hot_cache.insert(addr, pool.clone());
                    }
                } else {
                    let _ = self.arbitrage_engine.hot_cache.get(&addr);
                }
            }
        }
        Ok(())
    }

    async fn workload_concurrent_operations(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut handles = Vec::new();

        for _ in 0..10 {
            let engine = self.arbitrage_engine.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..100 {
                    let _ = engine.detect_arbitrage_opportunities().await;
                }
            }));
        }

        for handle in handles {
            handle.await?;
        }

        Ok(())
    }

    async fn workload_memory_patterns(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut temp_data = Vec::new();

        for i in 0..10_000 {
            let mut nested = HashMap::new();
            for j in 0..100 {
                let key = format!("alloc-key-{}-{}", i, j);
                nested.insert(key, vec![format!("payload-{}", i); 5]);
            }
            temp_data.push(nested);

            if i % 1000 == 0 {
                temp_data.clear();
            }
        }

        Ok(())
    }
}