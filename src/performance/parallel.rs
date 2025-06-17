// src/performance/parallel.rs
//! Parallel Processing Engine for High-Performance Operations
//!
//! This module provides concurrent execution capabilities for:
//! - DEX quote calculations
//! - Transaction simulations
//! - Route optimization
//! - Pool state updates
//! - Market data analysis

use anyhow::Result;
use futures::{stream::FuturesUnordered, StreamExt};
use log::{debug, info};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::timeout;

/// Configuration for parallel execution
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    pub max_workers: usize,
    pub timeout_ms: u64,
    pub retry_attempts: usize,
    pub backoff_ms: u64,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            max_workers: num_cpus::get().max(4),
            timeout_ms: 30_000,
            retry_attempts: 3,
            backoff_ms: 100,
        }
    }
}

pub struct ParallelExecutor {
    semaphore: Arc<Semaphore>,
    stats: Arc<RwLock<ParallelStats>>,
    max_workers: usize,
    timeout_duration: Duration,
}

/// Statistics for parallel execution
#[derive(Debug, Clone, Default)]
pub struct ParallelStats {
    pub active_workers: usize,
    pub completed_tasks: u64,
    pub failed_tasks: u64,
    pub avg_task_duration: Duration,
    pub max_task_duration: Duration,
    pub min_task_duration: Duration,
    pub total_execution_time: Duration,
    pub queue_length: usize,
}

impl ParallelExecutor {
    /// Create a new parallel executor
    pub async fn new(max_workers: usize) -> Result<Self> {
        info!(
            "🚀 Initializing ParallelExecutor with {} workers",
            max_workers
        );

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(max_workers)),
            stats: Arc::new(RwLock::new(ParallelStats::default())),
            max_workers,
            timeout_duration: Duration::from_secs(30),
        })
    }

    /// Execute multiple futures concurrently with load balancing
    pub async fn execute_concurrent<T, F, Fut>(&self, tasks: Vec<F>) -> Vec<Result<T>>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T>> + Send,
        T: Send + 'static,
    {
        if tasks.is_empty() {
            return Vec::new();
        }

        let start_time = Instant::now();
        let task_count = tasks.len();

        debug!("Executing {} tasks concurrently", task_count);

        // Update queue length
        {
            let mut stats = self.stats.write().await;
            stats.queue_length = task_count;
        }

        let mut futures = FuturesUnordered::new();

        for task in tasks {
            let semaphore = self.semaphore.clone();
            let stats = self.stats.clone();
            let timeout_duration = self.timeout_duration;

            let future = async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| anyhow::anyhow!("Semaphore error: {}", e))?;

                // Update active workers count
                {
                    let mut s = stats.write().await;
                    s.active_workers += 1;
                }

                let task_start = Instant::now();
                let result = timeout(timeout_duration, task()).await;
                let task_duration = task_start.elapsed();

                // Update stats
                {
                    let mut s = stats.write().await;
                    s.active_workers -= 1;

                    match &result {
                        Ok(Ok(_)) => {
                            s.completed_tasks += 1;
                            s.total_execution_time += task_duration;

                            // Update duration stats
                            if s.completed_tasks == 1 {
                                s.min_task_duration = task_duration;
                                s.max_task_duration = task_duration;
                                s.avg_task_duration = task_duration;
                            } else {
                                s.min_task_duration = s.min_task_duration.min(task_duration);
                                s.max_task_duration = s.max_task_duration.max(task_duration);
                                s.avg_task_duration = Duration::from_nanos(
                                    (s.avg_task_duration.as_nanos() as u64
                                        * (s.completed_tasks - 1)
                                        + task_duration.as_nanos() as u64)
                                        / s.completed_tasks,
                                );
                            }
                        }
                        _ => {
                            s.failed_tasks += 1;
                        }
                    }
                }

                match result {
                    Ok(task_result) => task_result,
                    Err(_) => Err(anyhow::anyhow!("Task timeout after {:?}", timeout_duration)),
                }
            };

            futures.push(future);
        }

        let mut results = Vec::with_capacity(task_count);
        while let Some(result) = futures.next().await {
            results.push(result);
        }

        let total_time = start_time.elapsed();
        debug!("Completed {} tasks in {:?}", task_count, total_time);

        // Clear queue length
        {
            let mut stats = self.stats.write().await;
            stats.queue_length = 0;
        }

        results
    }

    /// Execute DEX quotes in parallel across multiple DEXs
    pub async fn execute_parallel_quotes<T>(
        &self,
        quote_functions: Vec<Box<dyn FnOnce() -> tokio::task::JoinHandle<Result<T>> + Send>>,
    ) -> Vec<Result<T>>
    where
        T: Send + 'static,
    {
        let tasks: Vec<_> = quote_functions
            .into_iter()
            .map(|quote_fn| {
                move || async move {
                    let handle = quote_fn();
                    handle
                        .await
                        .map_err(|e| anyhow::anyhow!("Join error: {}", e))?
                }
            })
            .collect();

        self.execute_concurrent(tasks).await
    }

    /// Execute transaction simulations in parallel
    pub async fn execute_parallel_simulations<F, T>(&self, simulations: Vec<F>) -> Vec<Result<T>>
    where
        F: FnOnce() -> tokio::task::JoinHandle<Result<T>> + Send,
        T: Send + 'static,
    {
        let tasks: Vec<_> = simulations
            .into_iter()
            .map(|sim| {
                move || async move {
                    sim()
                        .await
                        .map_err(|e| anyhow::anyhow!("Join error: {}", e))?
                }
            })
            .collect();

        self.execute_concurrent(tasks).await
    }

    /// Execute route optimizations in parallel
    pub async fn execute_parallel_optimizations<F, T>(
        &self,
        optimizations: Vec<F>,
    ) -> Vec<Result<T>>
    where
        F: FnOnce() -> tokio::task::JoinHandle<Result<T>> + Send,
        T: Send + 'static,
    {
        let tasks: Vec<_> = optimizations
            .into_iter()
            .map(|opt| {
                move || async move {
                    opt()
                        .await
                        .map_err(|e| anyhow::anyhow!("Join error: {}", e))?
                }
            })
            .collect();

        self.execute_concurrent(tasks).await
    }

    /// Get current execution statistics
    pub async fn get_stats(&self) -> ParallelStats {
        self.stats.read().await.clone()
    }

    /// Reset statistics
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = ParallelStats::default();
    }

    /// Get worker capacity
    pub fn max_workers(&self) -> usize {
        self.max_workers
    }

    /// Check if executor is at capacity
    pub async fn is_at_capacity(&self) -> bool {
        let stats = self.stats.read().await;
        stats.active_workers >= self.max_workers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_parallel_executor() {
        let executor = ParallelExecutor::new(4).await.unwrap();

        // Create some test tasks
        let tasks: Vec<_> = (0..10)
            .map(|i| {
                move || async move {
                    sleep(Duration::from_millis(100)).await;
                    Ok::<_, anyhow::Error>(i * 2)
                }
            })
            .collect();

        let results = executor.execute_concurrent(tasks).await;
        assert_eq!(results.len(), 10);

        // Check that all tasks completed successfully
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok());
            assert_eq!(result.as_ref().unwrap(), &(i * 2));
        }
    }

    #[tokio::test]
    async fn test_parallel_stats() {
        let executor = ParallelExecutor::new(2).await.unwrap();

        let tasks: Vec<_> = (0..5)
            .map(|i| {
                move || async move {
                    sleep(Duration::from_millis(50)).await;
                    if i == 2 {
                        Err(anyhow::anyhow!("Test error"))
                    } else {
                        Ok::<_, anyhow::Error>(i)
                    }
                }
            })
            .collect();

        let _results = executor.execute_concurrent(tasks).await;
        let stats = executor.get_stats().await;

        assert_eq!(stats.completed_tasks, 4);
        assert_eq!(stats.failed_tasks, 1);
        assert!(stats.avg_task_duration > Duration::from_millis(40));
    }
}
