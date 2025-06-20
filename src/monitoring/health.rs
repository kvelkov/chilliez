// src/monitoring/health.rs
//! Health Monitoring and System Health Checks
//!
//! This module provides comprehensive health monitoring including:
//! - System health checks
//! - Component status monitoring
//! - Heartbeat tracking
//! - Health metrics aggregation

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, warn, error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;

/// Overall system health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

impl HealthStatus {
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s >= 0.8 => HealthStatus::Healthy,
            s if s >= 0.6 => HealthStatus::Warning,
            s if s >= 0.3 => HealthStatus::Critical,
            _ => HealthStatus::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "HEALTHY",
            HealthStatus::Warning => "WARNING",
            HealthStatus::Critical => "CRITICAL",
            HealthStatus::Unknown => "UNKNOWN",
        }
    }
}

/// Health check result for individual components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub component: String,
    pub status: HealthStatus,
    pub score: f64,
    pub message: String,
    pub last_check: SystemTime,
    pub response_time: Duration,
    pub details: HashMap<String, String>,
}

/// Overall system health report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall_status: HealthStatus,
    pub overall_score: f64,
    pub timestamp: SystemTime,
    pub uptime: Duration,
    pub component_results: HashMap<String, HealthCheckResult>,
    pub summary: String,
}

/// Configuration for health monitoring
#[derive(Debug, Clone)]
pub struct HealthConfig {
    pub check_interval: Duration,
    pub component_timeout: Duration,
    pub unhealthy_threshold: Duration,
    pub critical_threshold: Duration,
    pub max_check_history: usize,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            component_timeout: Duration::from_secs(10),
            unhealthy_threshold: Duration::from_secs(60),
            critical_threshold: Duration::from_secs(300),
            max_check_history: 100,
        }
    }
}

/// Health monitor for system components
pub struct HealthMonitor {
    config: HealthConfig,
    component_checks: HashMap<String, Box<dyn HealthChecker + Send + Sync>>,
    check_history: RwLock<Vec<HealthReport>>,
    start_time: Instant,
    last_check: RwLock<Option<SystemTime>>,
}

/// Trait for component health checks
#[async_trait::async_trait]
pub trait HealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult>;
    fn component_name(&self) -> &str;
}

/// Database connectivity health checker
pub struct DatabaseHealthChecker {
    component_name: String,
}

/// Cache system health checker
pub struct CacheHealthChecker {
    component_name: String,
}

/// Network connectivity health checker
pub struct NetworkHealthChecker {
    component_name: String,
    endpoints: Vec<String>,
}

/// System resources health checker
pub struct SystemResourcesHealthChecker {
    component_name: String,
}

/// DEX connection health checker
pub struct DexHealthChecker {
    component_name: String,
    dex_name: String,
}

impl HealthMonitor {
    pub fn new(config: HealthConfig) -> Self {
        Self {
            config,
            component_checks: HashMap::new(),
            check_history: RwLock::new(Vec::new()),
            start_time: Instant::now(),
            last_check: RwLock::new(None),
        }
    }

    /// Register a health checker for a component
    pub fn register_checker(&mut self, checker: Box<dyn HealthChecker + Send + Sync>) {
        let component_name = checker.component_name().to_string();
        self.component_checks.insert(component_name, checker);
    }

    /// Run health checks for all registered components
    pub async fn run_health_checks(&self) -> Result<HealthReport> {
        let check_start = Instant::now();
        let mut component_results = HashMap::new();
        let mut total_score = 0.0;
        let mut component_count = 0;

        debug!("Starting health checks for {} components", self.component_checks.len());

        // Run health checks for all components
        for (component_name, checker) in &self.component_checks {
            let result = match tokio::time::timeout(
                self.config.component_timeout,
                checker.check_health()
            ).await {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => {
                    warn!("Health check failed for {}: {}", component_name, e);
                    HealthCheckResult {
                        component: component_name.clone(),
                        status: HealthStatus::Critical,
                        score: 0.0,
                        message: format!("Health check failed: {}", e),
                        last_check: SystemTime::now(),
                        response_time: check_start.elapsed(),
                        details: HashMap::new(),
                    }
                },
                Err(_) => {
                    warn!("Health check timeout for {}", component_name);
                    HealthCheckResult {
                        component: component_name.clone(),
                        status: HealthStatus::Critical,
                        score: 0.0,
                        message: "Health check timeout".to_string(),
                        last_check: SystemTime::now(),
                        response_time: self.config.component_timeout,
                        details: HashMap::new(),
                    }
                }
            };

            total_score += result.score;
            component_count += 1;
            component_results.insert(component_name.clone(), result);
        }

        // Calculate overall health
        let overall_score = if component_count > 0 {
            total_score / component_count as f64
        } else {
            0.0
        };

        let overall_status = HealthStatus::from_score(overall_score);
        let uptime = self.start_time.elapsed();
        let timestamp = SystemTime::now();

        // Generate summary
        let summary = self.generate_summary(&component_results, overall_status, overall_score);

        let report = HealthReport {
            overall_status,
            overall_score,
            timestamp,
            uptime,
            component_results,
            summary,
        };

        // Update last check time
        *self.last_check.write().await = Some(timestamp);

        // Store in history
        let mut history = self.check_history.write().await;
        history.push(report.clone());
        if history.len() > self.config.max_check_history {
            history.remove(0);
        }

        debug!("Health checks completed in {:?}. Overall status: {}", 
               check_start.elapsed(), overall_status.as_str());

        Ok(report)
    }

    /// Get the latest health report
    pub async fn get_latest_health(&self) -> Option<HealthReport> {
        self.check_history.read().await.last().cloned()
    }

    /// Get health history
    pub async fn get_health_history(&self) -> Vec<HealthReport> {
        self.check_history.read().await.clone()
    }

    /// Check if system is healthy
    pub async fn is_healthy(&self) -> bool {
        if let Some(report) = self.get_latest_health().await {
            matches!(report.overall_status, HealthStatus::Healthy)
        } else {
            false
        }
    }

    /// Start continuous health monitoring
    pub async fn start_monitoring(&self) -> Result<()> {
        let mut interval = tokio::time::interval(self.config.check_interval);
        
        loop {
            interval.tick().await;
            
            match self.run_health_checks().await {
                Ok(report) => {
                    match report.overall_status {
                        HealthStatus::Critical => {
                            error!("CRITICAL HEALTH STATUS: {}", report.summary);
                        },
                        HealthStatus::Warning => {
                            warn!("System health warning: {}", report.summary);
                        },
                        HealthStatus::Healthy => {
                            debug!("System health check passed");
                        },
                        HealthStatus::Unknown => {
                            warn!("Unknown system health status");
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to run health checks: {}", e);
                }
            }
        }
    }

    /// Generate a human-readable summary
    fn generate_summary(
        &self,
        results: &HashMap<String, HealthCheckResult>,
        overall_status: HealthStatus,
        overall_score: f64,
    ) -> String {
        let healthy_count = results.values()
            .filter(|r| matches!(r.status, HealthStatus::Healthy))
            .count();
        let warning_count = results.values()
            .filter(|r| matches!(r.status, HealthStatus::Warning))
            .count();
        let critical_count = results.values()
            .filter(|r| matches!(r.status, HealthStatus::Critical))
            .count();

        let total_components = results.len();

        let mut summary = format!(
            "Overall: {} ({:.1}% health score) - {}/{} components healthy",
            overall_status.as_str(),
            overall_score * 100.0,
            healthy_count,
            total_components
        );

        if warning_count > 0 {
            summary.push_str(&format!(", {} warnings", warning_count));
        }
        if critical_count > 0 {
            summary.push_str(&format!(", {} critical issues", critical_count));
        }

        // Add details for non-healthy components
        if warning_count > 0 || critical_count > 0 {
            summary.push_str(". Issues: ");
            let issues: Vec<String> = results.values()
                .filter(|r| !matches!(r.status, HealthStatus::Healthy))
                .map(|r| format!("{}: {}", r.component, r.message))
                .collect();
            summary.push_str(&issues.join(", "));
        }

        summary
    }
}

impl DatabaseHealthChecker {
    pub fn new(component_name: String) -> Self {
        Self { component_name }
    }
}

#[async_trait::async_trait]
impl HealthChecker for DatabaseHealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        
        // TODO: Implement actual database connectivity check
        // This is a placeholder implementation
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let response_time = start_time.elapsed();
        let is_healthy = response_time < Duration::from_secs(1);
        
        let (status, score, message) = if is_healthy {
            (HealthStatus::Healthy, 1.0, "Database connection healthy".to_string())
        } else {
            (HealthStatus::Warning, 0.5, "Database response slow".to_string())
        };

        let mut details = HashMap::new();
        details.insert("response_time_ms".to_string(), response_time.as_millis().to_string());
        details.insert("connection_pool_size".to_string(), "10".to_string()); // TODO: Get actual value

        Ok(HealthCheckResult {
            component: self.component_name.clone(),
            status,
            score,
            message,
            last_check: SystemTime::now(),
            response_time,
            details,
        })
    }

    fn component_name(&self) -> &str {
        &self.component_name
    }
}

impl CacheHealthChecker {
    pub fn new(component_name: String) -> Self {
        Self { component_name }
    }
}

#[async_trait::async_trait]
impl HealthChecker for CacheHealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        
        // TODO: Implement actual cache health check
        // This is a placeholder implementation
        tokio::time::sleep(Duration::from_millis(5)).await;
        
        let response_time = start_time.elapsed();
        let cache_hit_rate = 0.85; // TODO: Get actual cache hit rate
        
        let (status, score, message) = if cache_hit_rate > 0.8 {
            (HealthStatus::Healthy, 1.0, "Cache performing well".to_string())
        } else if cache_hit_rate > 0.6 {
            (HealthStatus::Warning, 0.7, "Cache hit rate below optimal".to_string())
        } else {
            (HealthStatus::Critical, 0.3, "Poor cache performance".to_string())
        };

        let mut details = HashMap::new();
        details.insert("hit_rate".to_string(), format!("{:.2}", cache_hit_rate));
        details.insert("response_time_ms".to_string(), response_time.as_millis().to_string());
        details.insert("cache_size".to_string(), "1024".to_string()); // TODO: Get actual value

        Ok(HealthCheckResult {
            component: self.component_name.clone(),
            status,
            score,
            message,
            last_check: SystemTime::now(),
            response_time,
            details,
        })
    }

    fn component_name(&self) -> &str {
        &self.component_name
    }
}

impl NetworkHealthChecker {
    pub fn new(component_name: String, endpoints: Vec<String>) -> Self {
        Self { component_name, endpoints }
    }
}

#[async_trait::async_trait]
impl HealthChecker for NetworkHealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        let mut successful_checks = 0;
        let total_checks = self.endpoints.len();

        // TODO: Implement actual network connectivity checks
        // This is a placeholder implementation
        for _endpoint in &self.endpoints {
            tokio::time::sleep(Duration::from_millis(20)).await;
            // Simulate 90% success rate
            if rand::random::<f64>() < 0.9 {
                successful_checks += 1;
            }
        }

        let response_time = start_time.elapsed();
        let success_rate = if total_checks > 0 {
            successful_checks as f64 / total_checks as f64
        } else {
            0.0
        };

        let (status, score, message) = if success_rate >= 0.9 {
            (HealthStatus::Healthy, 1.0, "All network endpoints healthy".to_string())
        } else if success_rate >= 0.7 {
            (HealthStatus::Warning, 0.7, "Some network endpoints experiencing issues".to_string())
        } else {
            (HealthStatus::Critical, 0.3, "Multiple network endpoints failing".to_string())
        };

        let mut details = HashMap::new();
        details.insert("success_rate".to_string(), format!("{:.2}", success_rate));
        details.insert("successful_endpoints".to_string(), successful_checks.to_string());
        details.insert("total_endpoints".to_string(), total_checks.to_string());
        details.insert("response_time_ms".to_string(), response_time.as_millis().to_string());

        Ok(HealthCheckResult {
            component: self.component_name.clone(),
            status,
            score,
            message,
            last_check: SystemTime::now(),
            response_time,
            details,
        })
    }

    fn component_name(&self) -> &str {
        &self.component_name
    }
}

impl SystemResourcesHealthChecker {
    pub fn new(component_name: String) -> Self {
        Self { component_name }
    }
}

#[async_trait::async_trait]
impl HealthChecker for SystemResourcesHealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        
        // TODO: Implement actual system resource monitoring
        // This is a placeholder implementation using random values
        let cpu_usage = rand::random::<f64>() * 100.0;
        let memory_usage = rand::random::<f64>() * 100.0;
        let disk_usage = rand::random::<f64>() * 100.0;
        
        let response_time = start_time.elapsed();

        // Calculate health score based on resource usage
        let cpu_score = if cpu_usage < 80.0 { 1.0 } else if cpu_usage < 95.0 { 0.5 } else { 0.0 };
        let memory_score = if memory_usage < 85.0 { 1.0 } else if memory_usage < 95.0 { 0.5 } else { 0.0 };
        let disk_score = if disk_usage < 90.0 { 1.0 } else if disk_usage < 98.0 { 0.5 } else { 0.0 };
        
        let score = (cpu_score + memory_score + disk_score) / 3.0;
        let status = HealthStatus::from_score(score);

        let message = match status {
            HealthStatus::Healthy => "System resources healthy".to_string(),
            HealthStatus::Warning => "System resources under pressure".to_string(),
            HealthStatus::Critical => "System resources critically low".to_string(),
            HealthStatus::Unknown => "Unable to determine resource status".to_string(),
        };

        let mut details = HashMap::new();
        details.insert("cpu_usage".to_string(), format!("{:.1}%", cpu_usage));
        details.insert("memory_usage".to_string(), format!("{:.1}%", memory_usage));
        details.insert("disk_usage".to_string(), format!("{:.1}%", disk_usage));
        details.insert("response_time_ms".to_string(), response_time.as_millis().to_string());

        Ok(HealthCheckResult {
            component: self.component_name.clone(),
            status,
            score,
            message,
            last_check: SystemTime::now(),
            response_time,
            details,
        })
    }

    fn component_name(&self) -> &str {
        &self.component_name
    }
}

impl DexHealthChecker {
    pub fn new(component_name: String, dex_name: String) -> Self {
        Self { component_name, dex_name }
    }
}

#[async_trait::async_trait]
impl HealthChecker for DexHealthChecker {
    async fn check_health(&self) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        
        // TODO: Implement actual DEX connectivity check
        // This is a placeholder implementation
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let response_time = start_time.elapsed();
        let is_responsive = response_time < Duration::from_secs(2);
        let api_available = rand::random::<f64>() < 0.95; // 95% uptime simulation
        
        let (status, score, message) = if is_responsive && api_available {
            (HealthStatus::Healthy, 1.0, format!("{} DEX healthy", self.dex_name))
        } else if api_available {
            (HealthStatus::Warning, 0.6, format!("{} DEX slow response", self.dex_name))
        } else {
            (HealthStatus::Critical, 0.0, format!("{} DEX unavailable", self.dex_name))
        };

        let mut details = HashMap::new();
        details.insert("dex_name".to_string(), self.dex_name.clone());
        details.insert("response_time_ms".to_string(), response_time.as_millis().to_string());
        details.insert("api_available".to_string(), api_available.to_string());
        details.insert("pool_count".to_string(), "150".to_string()); // TODO: Get actual pool count

        Ok(HealthCheckResult {
            component: self.component_name.clone(),
            status,
            score,
            message,
            last_check: SystemTime::now(),
            response_time,
            details,
        })
    }

    fn component_name(&self) -> &str {
        &self.component_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_from_score() {
        assert_eq!(HealthStatus::from_score(0.9), HealthStatus::Healthy);
        assert_eq!(HealthStatus::from_score(0.7), HealthStatus::Warning);
        assert_eq!(HealthStatus::from_score(0.4), HealthStatus::Critical);
        assert_eq!(HealthStatus::from_score(0.1), HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_health_monitor() {
        let config = HealthConfig::default();
        let mut monitor = HealthMonitor::new(config);

        // Register test checkers
        monitor.register_checker(Box::new(DatabaseHealthChecker::new("database".to_string())));
        monitor.register_checker(Box::new(CacheHealthChecker::new("cache".to_string())));

        let report = monitor.run_health_checks().await.unwrap();

        assert_eq!(report.component_results.len(), 2);
        assert!(report.component_results.contains_key("database"));
        assert!(report.component_results.contains_key("cache"));
    }

    #[tokio::test]
    async fn test_database_health_checker() {
        let checker = DatabaseHealthChecker::new("test_db".to_string());
        let result = checker.check_health().await.unwrap();

        assert_eq!(result.component, "test_db");
        assert!(result.response_time < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn test_network_health_checker() {
        let endpoints = vec![
            "https://api.example.com".to_string(),
            "https://backup.example.com".to_string(),
        ];
        let checker = NetworkHealthChecker::new("network".to_string(), endpoints);
        let result = checker.check_health().await.unwrap();

        assert_eq!(result.component, "network");
        assert!(result.details.contains_key("success_rate"));
        assert!(result.details.contains_key("total_endpoints"));
    }
}
