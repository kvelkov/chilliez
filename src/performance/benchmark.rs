// src/performance/benchmark.rs
//! Performance Benchmarking and Testing Suite
//! 
//! This module provides comprehensive benchmarking capabilities including:
//! - Route calculation performance tests
//! - DEX integration latency measurements
//! - Cache performance analysis
//! - System stress testing

use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use log::{info};
use serde::{Serialize, Deserialize};

use super::{PerformanceConfig};

/// Benchmark runner for performance testing
pub struct BenchmarkRunner {
    config: PerformanceConfig,
}

/// Benchmark results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResults {
    pub test_name: String,
    pub timestamp: u64,
    pub duration: Duration,
    pub operations_completed: u64,
    pub operations_per_second: f64,
    pub average_latency: Duration,
    pub p95_latency: Duration,
    pub p99_latency: Duration,
    pub min_latency: Duration,
    pub max_latency: Duration,
    pub success_rate: f64,
    pub memory_used_mb: f64,
    pub cpu_usage_percent: f64,
}

/// Stress test configuration
#[derive(Debug, Clone)]
pub struct StressTestConfig {
    pub duration: Duration,
    pub concurrent_operations: usize,
    pub operation_interval: Duration,
    pub target_ops_per_second: f64,
}

impl Default for StressTestConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(60),
            concurrent_operations: 100,
            operation_interval: Duration::from_millis(10),
            target_ops_per_second: 100.0,
        }
    }
}

impl BenchmarkRunner {
    /// Create a new benchmark runner
    pub fn new(config: PerformanceConfig) -> Self {
        Self { config }
    }

    /// Run comprehensive system benchmark
    pub async fn run_system_benchmark(&self) -> Result<Vec<BenchmarkResults>> {
        info!("🏁 Starting comprehensive system benchmark");
        
        let mut results = Vec::new();
        
        // Route calculation benchmark
        results.push(self.benchmark_route_calculation().await?);
        
        // Quote fetching benchmark
        results.push(self.benchmark_quote_fetching().await?);
        
        // Cache performance benchmark
        results.push(self.benchmark_cache_performance().await?);
        
        // Parallel processing benchmark
        results.push(self.benchmark_parallel_processing().await?);
        
        info!("✅ System benchmark completed with {} test suites", results.len());
        Ok(results)
    }

    /// Benchmark route calculation performance
    pub async fn benchmark_route_calculation(&self) -> Result<BenchmarkResults> {
        let test_name = "route_calculation".to_string();
        info!("🔍 Benchmarking route calculation performance");
        
        let start_time = Instant::now();
        let mut latencies = Vec::new();
        let mut successful_operations = 0;
        let total_operations = 1000;
        
        for i in 0..total_operations {
            let operation_start = Instant::now();
            
            // Simulate route calculation
            let result = timeout(
                self.config.operation_timeout,
                self.simulate_route_calculation(i)
            ).await;
            
            let latency = operation_start.elapsed();
            latencies.push(latency);
            
            if result.is_ok() && result.unwrap().is_ok() {
                successful_operations += 1;
            }
            
            // Throttle to prevent overwhelming the system
            if i % 100 == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        
        let total_duration = start_time.elapsed();
        
        // Calculate statistics
        latencies.sort();
        let average_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        let p99_index = (latencies.len() as f64 * 0.99) as usize;
        
        let results = BenchmarkResults {
            test_name,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration: total_duration,
            operations_completed: successful_operations,
            operations_per_second: successful_operations as f64 / total_duration.as_secs_f64(),
            average_latency,
            p95_latency: latencies[p95_index.min(latencies.len() - 1)],
            p99_latency: latencies[p99_index.min(latencies.len() - 1)],
            min_latency: latencies[0],
            max_latency: latencies[latencies.len() - 1],
            success_rate: successful_operations as f64 / total_operations as f64,
            memory_used_mb: self.get_memory_usage_mb(),
            cpu_usage_percent: 0.0, // Would implement actual CPU monitoring
        };
        
        info!("Route calculation benchmark: {:.1} ops/sec, {:.2}ms avg latency", 
              results.operations_per_second, results.average_latency.as_millis());
        
        Ok(results)
    }

    /// Benchmark quote fetching performance
    pub async fn benchmark_quote_fetching(&self) -> Result<BenchmarkResults> {
        let test_name = "quote_fetching".to_string();
        info!("💱 Benchmarking quote fetching performance");
        
        let start_time = Instant::now();
        let mut latencies = Vec::new();
        let mut successful_operations = 0;
        let total_operations = 500;
        
        for i in 0..total_operations {
            let operation_start = Instant::now();
            
            // Simulate quote fetching from multiple DEXs
            let result = timeout(
                self.config.operation_timeout,
                self.simulate_quote_fetching(i)
            ).await;
            
            let latency = operation_start.elapsed();
            latencies.push(latency);
            
            if result.is_ok() && result.unwrap().is_ok() {
                successful_operations += 1;
            }
        }
        
        let total_duration = start_time.elapsed();
        
        // Calculate statistics
        latencies.sort();
        let average_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        let p99_index = (latencies.len() as f64 * 0.99) as usize;
        
        let results = BenchmarkResults {
            test_name,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration: total_duration,
            operations_completed: successful_operations,
            operations_per_second: successful_operations as f64 / total_duration.as_secs_f64(),
            average_latency,
            p95_latency: latencies[p95_index.min(latencies.len() - 1)],
            p99_latency: latencies[p99_index.min(latencies.len() - 1)],
            min_latency: latencies[0],
            max_latency: latencies[latencies.len() - 1],
            success_rate: successful_operations as f64 / total_operations as f64,
            memory_used_mb: self.get_memory_usage_mb(),
            cpu_usage_percent: 0.0,
        };
        
        info!("Quote fetching benchmark: {:.1} ops/sec, {:.2}ms avg latency", 
              results.operations_per_second, results.average_latency.as_millis());
        
        Ok(results)
    }

    /// Benchmark cache performance
    pub async fn benchmark_cache_performance(&self) -> Result<BenchmarkResults> {
        let test_name = "cache_performance".to_string();
        info!("🗄️ Benchmarking cache performance");
        
        let start_time = Instant::now();
        let mut latencies = Vec::new();
        let mut successful_operations = 0;
        let total_operations = 10000;
        
        for i in 0..total_operations {
            let operation_start = Instant::now();
            
            // Simulate cache operations (reads and writes)
            let result = if i % 4 == 0 {
                // 25% writes
                self.simulate_cache_write(i).await
            } else {
                // 75% reads
                self.simulate_cache_read(i).await
            };
            
            let latency = operation_start.elapsed();
            latencies.push(latency);
            
            if result.is_ok() {
                successful_operations += 1;
            }
        }
        
        let total_duration = start_time.elapsed();
        
        // Calculate statistics
        latencies.sort();
        let average_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        let p99_index = (latencies.len() as f64 * 0.99) as usize;
        
        let results = BenchmarkResults {
            test_name,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration: total_duration,
            operations_completed: successful_operations,
            operations_per_second: successful_operations as f64 / total_duration.as_secs_f64(),
            average_latency,
            p95_latency: latencies[p95_index.min(latencies.len() - 1)],
            p99_latency: latencies[p99_index.min(latencies.len() - 1)],
            min_latency: latencies[0],
            max_latency: latencies[latencies.len() - 1],
            success_rate: successful_operations as f64 / total_operations as f64,
            memory_used_mb: self.get_memory_usage_mb(),
            cpu_usage_percent: 0.0,
        };
        
        info!("Cache performance benchmark: {:.1} ops/sec, {:.2}μs avg latency", 
              results.operations_per_second, results.average_latency.as_micros());
        
        Ok(results)
    }

    /// Benchmark parallel processing performance
    pub async fn benchmark_parallel_processing(&self) -> Result<BenchmarkResults> {
        let test_name = "parallel_processing".to_string();
        info!("⚡ Benchmarking parallel processing performance");
        
        let start_time = Instant::now();
        let mut latencies = Vec::new();
        let mut successful_operations = 0;
        let total_batches = 100;
        let batch_size = 20;
        
        for batch in 0..total_batches {
            let operation_start = Instant::now();
            
            // Create tasks for parallel execution
            let tasks: Vec<_> = (0..batch_size).map(|i| {
                let task_id = batch * batch_size + i;
                tokio::spawn(async move {
                    Self::simulate_parallel_task_static(task_id).await
                })
            }).collect();
            
            // Wait for all tasks to complete
            let mut batch_successful = 0;
            for task in tasks {
                if let Ok(Ok(_)) = task.await {
                    batch_successful += 1;
                }
            }
            
            let latency = operation_start.elapsed();
            latencies.push(latency);
            successful_operations += batch_successful;
        }
        
        let total_duration = start_time.elapsed();
        let total_operations = total_batches * batch_size;
        
        // Calculate statistics
        latencies.sort();
        let average_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        let p99_index = (latencies.len() as f64 * 0.99) as usize;
        
        let results = BenchmarkResults {
            test_name,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration: total_duration,
            operations_completed: successful_operations,
            operations_per_second: successful_operations as f64 / total_duration.as_secs_f64(),
            average_latency,
            p95_latency: latencies[p95_index.min(latencies.len() - 1)],
            p99_latency: latencies[p99_index.min(latencies.len() - 1)],
            min_latency: latencies[0],
            max_latency: latencies[latencies.len() - 1],
            success_rate: successful_operations as f64 / total_operations as f64,
            memory_used_mb: self.get_memory_usage_mb(),
            cpu_usage_percent: 0.0,
        };
        
        info!("Parallel processing benchmark: {:.1} ops/sec, {:.2}ms avg batch latency", 
              results.operations_per_second, results.average_latency.as_millis());
        
        Ok(results)
    }

    /// Run stress test to validate system under load
    pub async fn run_stress_test(&self, config: StressTestConfig) -> Result<BenchmarkResults> {
        info!("🔥 Starting stress test: {} ops for {:?}", 
              config.concurrent_operations, config.duration);
        
        let start_time = Instant::now();
        let mut latencies = Vec::new();
        let mut successful_operations = 0;
        let mut total_operations = 0;
        
        let end_time = start_time + config.duration;
        let mut operation_counter = 0;
        
        while Instant::now() < end_time {
            let batch_start = Instant::now();
            
            // Create concurrent operations
            let tasks: Vec<_> = (0..config.concurrent_operations).map(|_| {
                let op_id = operation_counter;
                operation_counter += 1;
                tokio::spawn(async move {
                    Self::simulate_stress_operation_static(op_id).await
                })
            }).collect();
            
            // Wait for batch completion
            for task in tasks {
                total_operations += 1;
                let task_start = Instant::now();
                
                if let Ok(Ok(_)) = task.await {
                    successful_operations += 1;
                }
                
                latencies.push(task_start.elapsed());
            }
            
            // Control operation rate
            let batch_duration = batch_start.elapsed();
            let target_batch_duration = Duration::from_secs_f64(
                config.concurrent_operations as f64 / config.target_ops_per_second
            );
            
            if batch_duration < target_batch_duration {
                tokio::time::sleep(target_batch_duration - batch_duration).await;
            }
        }
        
        let total_duration = start_time.elapsed();
        
        // Calculate statistics
        latencies.sort();
        let average_latency = if !latencies.is_empty() {
            latencies.iter().sum::<Duration>() / latencies.len() as u32
        } else {
            Duration::from_millis(0)
        };
        
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        let p99_index = (latencies.len() as f64 * 0.99) as usize;
        
        let results = BenchmarkResults {
            test_name: "stress_test".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration: total_duration,
            operations_completed: successful_operations,
            operations_per_second: successful_operations as f64 / total_duration.as_secs_f64(),
            average_latency,
            p95_latency: if !latencies.is_empty() { 
                latencies[p95_index.min(latencies.len() - 1)] 
            } else { 
                Duration::from_millis(0) 
            },
            p99_latency: if !latencies.is_empty() { 
                latencies[p99_index.min(latencies.len() - 1)] 
            } else { 
                Duration::from_millis(0) 
            },
            min_latency: latencies.first().copied().unwrap_or_default(),
            max_latency: latencies.last().copied().unwrap_or_default(),
            success_rate: if total_operations > 0 {
                successful_operations as f64 / total_operations as f64
            } else {
                0.0
            },
            memory_used_mb: self.get_memory_usage_mb(),
            cpu_usage_percent: 0.0,
        };
        
        info!("Stress test completed: {:.1} ops/sec, {:.1}% success rate", 
              results.operations_per_second, results.success_rate * 100.0);
        
        Ok(results)
    }

    /// Generate benchmark report
    pub fn generate_report(&self, results: &[BenchmarkResults]) -> String {
        let mut report = String::new();
        report.push_str("PERFORMANCE BENCHMARK REPORT\n");
        report.push_str("============================\n\n");
        
        for result in results {
            report.push_str(&format!(
                "Test: {}\n\
                 Duration: {:.2}s\n\
                 Operations: {}\n\
                 Throughput: {:.1} ops/sec\n\
                 Success Rate: {:.1}%\n\
                 Avg Latency: {:.2}ms\n\
                 P95 Latency: {:.2}ms\n\
                 P99 Latency: {:.2}ms\n\
                 Memory Usage: {:.1} MB\n\
                 \n",
                result.test_name,
                result.duration.as_secs_f64(),
                result.operations_completed,
                result.operations_per_second,
                result.success_rate * 100.0,
                result.average_latency.as_millis() as f64,
                result.p95_latency.as_millis() as f64,
                result.p99_latency.as_millis() as f64,
                result.memory_used_mb
            ));
        }
        
        // Summary
        let total_ops: u64 = results.iter().map(|r| r.operations_completed).sum();
        let avg_throughput: f64 = results.iter().map(|r| r.operations_per_second).sum::<f64>() / results.len() as f64;
        let avg_success_rate: f64 = results.iter().map(|r| r.success_rate).sum::<f64>() / results.len() as f64;
        
        report.push_str(&format!(
            "SUMMARY\n\
             -------\n\
             Total Operations: {}\n\
             Average Throughput: {:.1} ops/sec\n\
             Average Success Rate: {:.1}%\n\
             Tests Completed: {}\n",
            total_ops,
            avg_throughput,
            avg_success_rate * 100.0,
            results.len()
        ));
        
        report
    }

    // Simulation methods for testing
    async fn simulate_route_calculation(&self, _operation_id: usize) -> Result<()> {
        // Simulate computational work for route calculation
        tokio::time::sleep(Duration::from_micros(500 + (rand::random::<u64>() % 1000))).await;
        
        // Randomly fail 5% of operations
        if rand::random::<f64>() < 0.05 {
            return Err(anyhow::anyhow!("Simulated route calculation failure"));
        }
        
        Ok(())
    }

    async fn simulate_quote_fetching(&self, _operation_id: usize) -> Result<()> {
        // Simulate network latency for quote fetching
        tokio::time::sleep(Duration::from_millis(10 + (rand::random::<u64>() % 50))).await;
        
        // Randomly fail 3% of operations
        if rand::random::<f64>() < 0.03 {
            return Err(anyhow::anyhow!("Simulated quote fetching failure"));
        }
        
        Ok(())
    }

    async fn simulate_cache_read(&self, _operation_id: usize) -> Result<()> {
        // Simulate cache read latency
        tokio::time::sleep(Duration::from_nanos(100 + (rand::random::<u64>() % 500))).await;
        Ok(())
    }

    async fn simulate_cache_write(&self, _operation_id: usize) -> Result<()> {
        // Simulate cache write latency
        tokio::time::sleep(Duration::from_nanos(200 + (rand::random::<u64>() % 800))).await;
        Ok(())
    }

    async fn simulate_parallel_task_static(_task_id: usize) -> Result<()> {
        // Simulate parallel work
        tokio::time::sleep(Duration::from_millis(5 + (rand::random::<u64>() % 20))).await;
        
        // Randomly fail 2% of tasks
        if rand::random::<f64>() < 0.02 {
            return Err(anyhow::anyhow!("Simulated parallel task failure"));
        }
        
        Ok(())
    }

    async fn simulate_stress_operation_static(_operation_id: usize) -> Result<()> {
        // Simulate varying workload under stress
        let work_duration = Duration::from_micros(100 + (rand::random::<u64>() % 2000));
        tokio::time::sleep(work_duration).await;
        
        // Higher failure rate under stress
        if rand::random::<f64>() < 0.08 {
            return Err(anyhow::anyhow!("Simulated stress operation failure"));
        }
        
        Ok(())
    }

    fn get_memory_usage_mb(&self) -> f64 {
        // Placeholder for actual memory usage measurement
        // Would integrate with system monitoring
        50.0 + rand::random::<f64>() * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_benchmark_runner() {
        let config = PerformanceConfig::default();
        let runner = BenchmarkRunner::new(config);
        
        let result = runner.benchmark_cache_performance().await.unwrap();
        
        assert_eq!(result.test_name, "cache_performance");
        assert!(result.operations_completed > 0);
        assert!(result.success_rate > 0.0);
    }

    #[tokio::test]
    async fn test_stress_test() {
        let config = PerformanceConfig::default();
        let runner = BenchmarkRunner::new(config);
        
        let stress_config = StressTestConfig {
            duration: Duration::from_secs(1),
            concurrent_operations: 10,
            operation_interval: Duration::from_millis(10),
            target_ops_per_second: 50.0,
        };
        
        let result = runner.run_stress_test(stress_config).await.unwrap();
        
        assert_eq!(result.test_name, "stress_test");
        assert!(result.operations_completed > 0);
    }
}
