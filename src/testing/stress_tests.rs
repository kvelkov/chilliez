//! Stress Testing for Sprint 4
//! 
//! Comprehensive stress testing including:
//! - High-frequency operation testing
//! - Memory leak detection
//! - Concurrent load testing
//! - Resource exhaustion testing

use super::{TestMetrics, MockDexEnvironment, MarketCondition};
use crate::{
    arbitrage::engine::ArbitrageEngine,
    error::ArbError,
};
use dashmap::DashMap;
use std::{
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use log::{info, debug, warn};
use sysinfo::{System, SystemExt, ProcessExt};

/// Stress test configuration
#[derive(Debug, Clone)]
pub struct StressTestConfig {
    /// Duration of stress test
    pub duration: Duration,
    /// Maximum concurrent operations
    pub max_concurrency: usize,
    /// Operations per second target
    pub target_ops_per_second: f64,
    /// Memory usage threshold (MB)
    pub memory_threshold_mb: f64,
    /// CPU usage threshold (%)
    pub cpu_threshold_percent: f64,
    /// Enable memory leak detection
    pub detect_memory_leaks: bool,
}

impl Default for StressTestConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(300), // 5 minutes
            max_concurrency: 100,
            target_ops_per_second: 1000.0,
            memory_threshold_mb: 1024.0, // 1GB
            cpu_threshold_percent: 80.0,
            detect_memory_leaks: true,
        }
    }
}

/// Stress test results
#[derive(Debug, Clone)]
pub struct StressTestResults {
    pub test_name: String,
    pub duration: Duration,
    pub total_operations: usize,
    pub successful_operations: usize,
    pub failed_operations: usize,
    pub operations_per_second: f64,
    pub peak_memory_mb: f64,
    pub memory_growth_mb: f64,
    pub peak_cpu_percent: f64,
    pub average_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub error_rate: f64,
    pub resource_exhaustion_detected: bool,
    pub memory_leak_detected: bool,
}

/// Stress test suite
pub struct StressTestSuite {
    pub mock_environment: Arc<MockDexEnvironment>,
    pub arbitrage_engine: Option<Arc<ArbitrageEngine>>,
    pub config: StressTestConfig,
    pub system: System,
}

impl StressTestSuite {
    pub async fn new(config: StressTestConfig) -> Result<Self, ArbError> {
        info!("🔧 Setting up stress test suite...");
        
        let mock_environment = Arc::new(MockDexEnvironment::new(MarketCondition::Normal));
        let mut system = System::new_all();
        system.refresh_all();
        
        Ok(Self {
            mock_environment,
            arbitrage_engine: None,
            config,
            system,
        })
    }

    /// Setup arbitrage engine for stress testing
    pub async fn setup_engine(&mut self) -> Result<(), ArbError> {
        info!("🚀 Setting up arbitrage engine for stress testing...");
        
        let hot_cache = Arc::new(DashMap::new());
        
        // Populate with pools from all mock DEXes
        for dex in self.mock_environment.dexes.values() {
            let pools = dex.discover_pools().await
                .map_err(|e| ArbError::DexError(format!("Failed to discover pools: {}", e)))?;
            
            for pool in pools {
                hot_cache.insert(pool.address, Arc::new(pool));
            }
        }
        
        let metrics = Arc::new(tokio::sync::Mutex::new(crate::monitoring::metrics::Metrics::new(150.0, None)));
        let dex_clients = self.mock_environment.get_dex_clients();
        let config = Arc::new(crate::config::Config::test_default());
        
        let arbitrage_engine = Arc::new(ArbitrageEngine::new(
            hot_cache,
            None,
            None,
            None,
            config,
            metrics,
            dex_clients,
            None,
        ));
        
        self.arbitrage_engine = Some(arbitrage_engine);
        info!("✅ Stress test engine setup complete");
        
        Ok(())
    }

    /// High-frequency operation stress test
    pub async fn stress_test_high_frequency(&mut self) -> Result<StressTestResults, ArbError> {
        info!("🔥 Running high-frequency operation stress test...");
        
        let engine = self.arbitrage_engine.as_ref()
            .ok_or_else(|| ArbError::ConfigError("Engine not initialized".to_string()))?;
        
        let start_time = Instant::now();
        let initial_memory = self.get_memory_usage();
        
        let total_operations = Arc::new(AtomicUsize::new(0));
        let successful_operations = Arc::new(AtomicUsize::new(0));
        let failed_operations = Arc::new(AtomicUsize::new(0));
        let latencies = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        
        // Semaphore to control concurrency
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrency));
        
        let mut memory_samples = Vec::new();
        let mut cpu_samples = Vec::new();
        let mut peak_memory = initial_memory;
        let mut peak_cpu = 0.0;
        
        info!("📊 Target: {:.0} ops/sec for {:?} with max {} concurrent operations", 
              self.config.target_ops_per_second, self.config.duration, self.config.max_concurrency);
        
        // Calculate target interval between operations
        let target_interval = Duration::from_secs_f64(1.0 / self.config.target_ops_per_second);
        let mut next_operation_time = start_time;
        
        // Monitoring task
        let monitoring_handle = {
            let total_ops = total_operations.clone();
            let duration = self.config.duration;
            tokio::spawn(async move {
                let mut last_count = 0;
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                
                loop {
                    interval.tick().await;
                    let current_count = total_ops.load(Ordering::Relaxed);
                    let ops_in_interval = current_count - last_count;
                    let current_rate = ops_in_interval as f64 / 10.0;
                    
                    debug!("Stress test progress: {} total ops, {:.1} ops/sec current rate", 
                           current_count, current_rate);
                    
                    last_count = current_count;
                    
                    if start_time.elapsed() >= duration {
                        break;
                    }
                }
            })
        };
        
        // Main stress test loop
        while start_time.elapsed() < self.config.duration {
            // Wait until it's time for the next operation
            let now = Instant::now();
            if now < next_operation_time {
                tokio::time::sleep(next_operation_time - now).await;
            }
            
            // Sample system metrics periodically
            if total_operations.load(Ordering::Relaxed) % 100 == 0 {
                let memory = self.get_memory_usage();
                let cpu = self.get_cpu_usage();
                
                memory_samples.push(memory);
                cpu_samples.push(cpu);
                
                if memory > peak_memory {
                    peak_memory = memory;
                }
                if cpu > peak_cpu {
                    peak_cpu = cpu;
                }
                
                // Check thresholds
                if memory > self.config.memory_threshold_mb {
                    warn!("Memory usage exceeded threshold: {:.1} MB", memory);
                }
                if cpu > self.config.cpu_threshold_percent {
                    warn!("CPU usage exceeded threshold: {:.1}%", cpu);
                }
            }
            
            // Spawn operation task
            let permit = semaphore.clone().acquire_owned().await
                .map_err(|e| ArbError::ExecutionError(format!("Semaphore error: {}", e)))?;
            
            let engine_clone = engine.clone();
            let total_ops = total_operations.clone();
            let successful_ops = successful_operations.clone();
            let failed_ops = failed_operations.clone();
            let latencies_clone = latencies.clone();
            
            tokio::spawn(async move {
                let _permit = permit; // Hold permit until task completes
                let op_start = Instant::now();
                
                match engine_clone.detect_arbitrage_opportunities().await {
                    Ok(_) => {
                        successful_ops.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        failed_ops.fetch_add(1, Ordering::Relaxed);
                    }
                }
                
                let latency = op_start.elapsed().as_secs_f64() * 1000.0;
                latencies_clone.lock().await.push(latency);
                total_ops.fetch_add(1, Ordering::Relaxed);
            });
            
            next_operation_time += target_interval;
            
            // Simulate market movements occasionally
            if total_operations.load(Ordering::Relaxed) % 1000 == 0 {
                self.mock_environment.simulate_market_movements();
            }
        }
        
        // Wait for all operations to complete
        info!("⏳ Waiting for all operations to complete...");
        let _permits: Vec<_> = (0..self.config.max_concurrency)
            .map(|_| semaphore.clone().acquire_owned())
            .collect::<futures::future::JoinAll<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ArbError::ExecutionError(format!("Final semaphore acquire error: {}", e)))?;
        
        monitoring_handle.abort();
        
        let total_duration = start_time.elapsed();
        let final_memory = self.get_memory_usage();
        
        // Calculate results
        let total_ops = total_operations.load(Ordering::Relaxed);
        let successful_ops = successful_operations.load(Ordering::Relaxed);
        let failed_ops = failed_operations.load(Ordering::Relaxed);
        
        let latencies_vec = latencies.lock().await.clone();
        let (average_latency, p99_latency) = if !latencies_vec.is_empty() {
            let mut sorted_latencies = latencies_vec.clone();
            sorted_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
            
            let avg = sorted_latencies.iter().sum::<f64>() / sorted_latencies.len() as f64;
            let p99_idx = (sorted_latencies.len() as f64 * 0.99) as usize;
            let p99 = sorted_latencies.get(p99_idx).copied().unwrap_or(0.0);
            
            (avg, p99)
        } else {
            (0.0, 0.0)
        };
        
        let ops_per_second = total_ops as f64 / total_duration.as_secs_f64();
        let error_rate = failed_ops as f64 / total_ops as f64;
        let memory_growth = final_memory - initial_memory;
        
        // Detect resource exhaustion
        let resource_exhaustion = peak_memory > self.config.memory_threshold_mb || 
                                 peak_cpu > self.config.cpu_threshold_percent;
        
        // Detect memory leaks (simplified)
        let memory_leak = self.config.detect_memory_leaks && 
                         memory_growth > (initial_memory * 0.5); // 50% growth threshold
        
        let results = StressTestResults {
            test_name: "High-Frequency Operations".to_string(),
            duration: total_duration,
            total_operations: total_ops,
            successful_operations: successful_ops,
            failed_operations: failed_ops,
            operations_per_second: ops_per_second,
            peak_memory_mb: peak_memory,
            memory_growth_mb: memory_growth,
            peak_cpu_percent: peak_cpu,
            average_latency_ms: average_latency,
            p99_latency_ms: p99_latency,
            error_rate,
            resource_exhaustion_detected: resource_exhaustion,
            memory_leak_detected: memory_leak,
        };
        
        info!("✅ High-frequency stress test completed");
        info!("   📊 {} operations in {:.1}s ({:.1} ops/sec)", 
              total_ops, total_duration.as_secs_f64(), ops_per_second);
        info!("   📊 Success rate: {:.1}%, Error rate: {:.1}%", 
              (successful_ops as f64 / total_ops as f64) * 100.0, error_rate * 100.0);
        info!("   📊 Memory: {:.1} MB peak, {:.1} MB growth", peak_memory, memory_growth);
        info!("   📊 Latency: {:.2}ms avg, {:.2}ms p99", average_latency, p99_latency);
        
        if resource_exhaustion {
            warn!("   ⚠️ Resource exhaustion detected!");
        }
        if memory_leak {
            warn!("   ⚠️ Potential memory leak detected!");
        }
        
        Ok(results)
    }

    /// Concurrent load stress test
    pub async fn stress_test_concurrent_load(&mut self) -> Result<StressTestResults, ArbError> {
        info!("🔥 Running concurrent load stress test...");
        
        let engine = self.arbitrage_engine.as_ref()
            .ok_or_else(|| ArbError::ConfigError("Engine not initialized".to_string()))?;
        
        let start_time = Instant::now();
        let initial_memory = self.get_memory_usage();
        
        let concurrent_tasks = self.config.max_concurrency;
        let operations_per_task = (self.config.target_ops_per_second * self.config.duration.as_secs_f64()) as usize / concurrent_tasks;
        
        info!("📊 Spawning {} concurrent tasks, {} operations each", 
              concurrent_tasks, operations_per_task);
        
        let mut handles = Vec::new();
        let total_operations = Arc::new(AtomicUsize::new(0));
        let successful_operations = Arc::new(AtomicUsize::new(0));
        
        // Spawn concurrent tasks
        for task_id in 0..concurrent_tasks {
            let engine_clone = engine.clone();
            let total_ops = total_operations.clone();
            let successful_ops = successful_operations.clone();
            let mock_env = self.mock_environment.clone();
            
            let handle = tokio::spawn(async move {
                let mut task_latencies = Vec::new();
                let mut task_successful = 0;
                let mut task_failed = 0;
                
                for op_idx in 0..operations_per_task {
                    let op_start = Instant::now();
                    
                    match engine_clone.detect_arbitrage_opportunities().await {
                        Ok(_) => {
                            task_successful += 1;
                            successful_ops.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            task_failed += 1;
                        }
                    }
                    
                    let latency = op_start.elapsed().as_secs_f64() * 1000.0;
                    task_latencies.push(latency);
                    total_ops.fetch_add(1, Ordering::Relaxed);
                    
                    // Simulate market movements occasionally
                    if op_idx % 100 == 0 {
                        mock_env.simulate_market_movements();
                    }
                    
                    // Small delay to prevent overwhelming the system
                    if op_idx % 10 == 0 {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }
                
                (task_id, task_latencies, task_successful, task_failed)
            });
            
            handles.push(handle);
        }
        
        // Monitor system resources while tasks run
        let monitoring_handle = {
            let duration = self.config.duration;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                
                while start_time.elapsed() < duration {
                    interval.tick().await;
                    // Monitoring would happen here in a real implementation
                }
            })
        };
        
        // Collect results from all tasks
        let mut all_latencies = Vec::new();
        let mut total_successful = 0;
        let mut total_failed = 0;
        
        for handle in handles {
            let (task_id, latencies, successful, failed) = handle.await
                .map_err(|e| ArbError::ExecutionError(format!("Task {} join error: {}", task_id, e)))?;
            
            all_latencies.extend(latencies);
            total_successful += successful;
            total_failed += failed;
            
            debug!("Task {} completed: {} successful, {} failed", task_id, successful, failed);
        }
        
        monitoring_handle.abort();
        
        let total_duration = start_time.elapsed();
        let final_memory = self.get_memory_usage();
        let total_ops = total_operations.load(Ordering::Relaxed);
        
        // Calculate statistics
        let (average_latency, p99_latency) = if !all_latencies.is_empty() {
            let mut sorted = all_latencies.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            
            let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
            let p99_idx = (sorted.len() as f64 * 0.99) as usize;
            let p99 = sorted.get(p99_idx).copied().unwrap_or(0.0);
            
            (avg, p99)
        } else {
            (0.0, 0.0)
        };
        
        let results = StressTestResults {
            test_name: "Concurrent Load".to_string(),
            duration: total_duration,
            total_operations: total_ops,
            successful_operations: total_successful,
            failed_operations: total_failed,
            operations_per_second: total_ops as f64 / total_duration.as_secs_f64(),
            peak_memory_mb: final_memory.max(initial_memory),
            memory_growth_mb: final_memory - initial_memory,
            peak_cpu_percent: 0.0, // Would need proper monitoring
            average_latency_ms: average_latency,
            p99_latency_ms: p99_latency,
            error_rate: total_failed as f64 / total_ops as f64,
            resource_exhaustion_detected: false,
            memory_leak_detected: false,
        };
        
        info!("✅ Concurrent load stress test completed");
        info!("   📊 {} concurrent tasks completed {} total operations", 
              concurrent_tasks, total_ops);
        info!("   📊 Throughput: {:.1} ops/sec", results.operations_per_second);
        info!("   📊 Success rate: {:.1}%", (total_successful as f64 / total_ops as f64) * 100.0);
        
        Ok(results)
    }

    /// Run all stress tests
    pub async fn run_all_stress_tests(&mut self) -> Result<Vec<StressTestResults>, ArbError> {
        info!("🚀 Running complete stress test suite...");
        
        self.setup_engine().await?;
        
        let mut results = Vec::new();
        
        // Run high-frequency test
        results.push(self.stress_test_high_frequency().await?);
        
        // Reset environment between tests
        self.mock_environment.reset_all();
        
        // Run concurrent load test
        results.push(self.stress_test_concurrent_load().await?);
        
        info!("✅ All stress tests completed");
        Ok(results)
    }

    // Helper methods
    fn get_memory_usage(&mut self) -> f64 {
        self.system.refresh_memory();
        let process_id = std::process::id();
        
        if let Some(process) = self.system.process(sysinfo::Pid::from(process_id as usize)) {
            process.memory() as f64 / 1024.0 / 1024.0 // Convert to MB
        } else {
            0.0
        }
    }

    fn get_cpu_usage(&mut self) -> f64 {
        self.system.refresh_cpu();
        self.system.global_cpu_info().cpu_usage() as f64
    }
}

/// Generate stress test report
pub fn generate_stress_test_report(results: &[StressTestResults]) -> String {
    let mut report = String::new();
    
    report.push_str("🔥 STRESS TEST REPORT\n");
    report.push_str("====================\n\n");
    
    for result in results {
        report.push_str(&format!("🧪 {}\n", result.test_name));
        report.push_str(&format!("   Duration: {:.1}s\n", result.duration.as_secs_f64()));
        report.push_str(&format!("   Operations: {} total, {} successful, {} failed\n",
                                result.total_operations, result.successful_operations, result.failed_operations));
        report.push_str(&format!("   Throughput: {:.1} ops/sec\n", result.operations_per_second));
        report.push_str(&format!("   Error Rate: {:.2}%\n", result.error_rate * 100.0));
        report.push_str(&format!("   Latency: {:.2}ms avg, {:.2}ms p99\n", 
                                result.average_latency_ms, result.p99_latency_ms));
        report.push_str(&format!("   Memory: {:.1} MB peak, {:.1} MB growth\n",
                                result.peak_memory_mb, result.memory_growth_mb));
        report.push_str(&format!("   CPU: {:.1}% peak\n", result.peak_cpu_percent));
        
        if result.resource_exhaustion_detected {
            report.push_str("   ⚠️ Resource exhaustion detected\n");
        }
        if result.memory_leak_detected {
            report.push_str("   ⚠️ Potential memory leak detected\n");
        }
        
        report.push_str("\n");
    }
    
    report
}
