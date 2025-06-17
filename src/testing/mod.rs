//! Sprint 4 Testing Infrastructure
//!
//! Comprehensive testing framework for the arbitrage bot including:
//! - Mock DEX environment
//! - Integration test suites
//! - Performance benchmarks
//! - Stress testing utilities

pub mod mock_dex;

pub use mock_dex::{
    EnvironmentStats, MarketCondition, MockDex, MockDexConfig, MockDexEnvironment,
    MockOpportunityGenerator, TransactionStats,
};

use crate::error::ArbError;
use log::info;
use std::time::{Duration, Instant};

/// Test result for tracking test outcomes
#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub success: bool,
    pub duration: Duration,
    pub error_message: Option<String>,
    pub metrics: Option<TestMetrics>,
}

/// Metrics collected during testing
#[derive(Debug, Clone)]
pub struct TestMetrics {
    pub operations_per_second: f64,
    pub average_latency_ms: f64,
    pub memory_usage_mb: f64,
    pub success_rate: f64,
}

/// Test suite runner for coordinating all tests
pub struct TestSuiteRunner {
    pub results: Vec<TestResult>,
    pub start_time: Instant,
}

impl TestSuiteRunner {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Run a single test and record the result
    pub async fn run_test<F, Fut>(&mut self, test_name: &str, test_fn: F) -> Result<(), ArbError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<TestMetrics, ArbError>>,
    {
        info!("🧪 Running test: {}", test_name);
        let start_time = Instant::now();

        match test_fn().await {
            Ok(metrics) => {
                let duration = start_time.elapsed();
                info!(
                    "✅ Test '{}' passed in {:.2}ms",
                    test_name,
                    duration.as_secs_f64() * 1000.0
                );

                self.results.push(TestResult {
                    test_name: test_name.to_string(),
                    success: true,
                    duration,
                    error_message: None,
                    metrics: Some(metrics),
                });
                Ok(())
            }
            Err(e) => {
                let duration = start_time.elapsed();
                info!(
                    "❌ Test '{}' failed in {:.2}ms: {}",
                    test_name,
                    duration.as_secs_f64() * 1000.0,
                    e
                );

                self.results.push(TestResult {
                    test_name: test_name.to_string(),
                    success: false,
                    duration,
                    error_message: Some(e.to_string()),
                    metrics: None,
                });
                Err(e)
            }
        }
    }

    /// Generate comprehensive test report
    pub fn generate_report(&self) -> TestReport {
        let total_tests = self.results.len();
        let passed_tests = self.results.iter().filter(|r| r.success).count();
        let failed_tests = total_tests - passed_tests;

        let total_duration = self.start_time.elapsed();
        let average_test_duration = if total_tests > 0 {
            self.results.iter().map(|r| r.duration).sum::<Duration>() / total_tests as u32
        } else {
            Duration::from_secs(0)
        };

        TestReport {
            total_tests,
            passed_tests,
            failed_tests,
            success_rate: if total_tests > 0 {
                passed_tests as f64 / total_tests as f64
            } else {
                0.0
            },
            total_duration,
            average_test_duration,
            results: self.results.clone(),
        }
    }
}

/// Comprehensive test report
#[derive(Debug, Clone)]
pub struct TestReport {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub success_rate: f64,
    pub total_duration: Duration,
    pub average_test_duration: Duration,
    pub results: Vec<TestResult>,
}

impl TestReport {
    /// Print formatted test report
    pub fn print_summary(&self) {
        info!("📊 TEST SUITE SUMMARY");
        info!("==================");
        info!("Total Tests: {}", self.total_tests);
        info!("Passed: {} (✅)", self.passed_tests);
        info!("Failed: {} (❌)", self.failed_tests);
        info!("Success Rate: {:.1}%", self.success_rate * 100.0);
        info!("Total Duration: {:.2}s", self.total_duration.as_secs_f64());
        info!(
            "Average Test Duration: {:.2}ms",
            self.average_test_duration.as_secs_f64() * 1000.0
        );

        if self.failed_tests > 0 {
            info!("❌ FAILED TESTS:");
            for result in &self.results {
                if !result.success {
                    info!(
                        "  - {}: {}",
                        result.test_name,
                        result
                            .error_message
                            .as_ref()
                            .unwrap_or(&"Unknown error".to_string())
                    );
                }
            }
        }
    }
}
