// src/helius_client.rs
//! Enhanced Helius SDK Client Manager with Production Rate Limiting
//! 
//! Provides a centralized interface for interacting with the Helius SDK
//! with advanced rate limiting (3000 req/h from 6.7M available) and 
//! production-grade error handling.

use anyhow::{Result, Context};
use log::{warn, info, error, debug};
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;

// Import our new API management infrastructure
use crate::api::{
    AdvancedRateLimiter, 
    RequestPriority,
};

// Temporary stub types to replace Helius SDK while dependency conflicts are resolved
#[derive(Debug, Clone)]
pub enum Cluster {
    MainnetBeta,
    Devnet,
}

pub struct Helius {
    rate_limiter: Arc<AdvancedRateLimiter>,
    #[allow(dead_code)]
    cluster: Cluster,
}

pub struct HeliusFactory {
    #[allow(dead_code)]
    api_key: String,
    rate_limiter: Arc<AdvancedRateLimiter>,
}

impl Helius {
    pub fn new(_api_key: &str, cluster: Cluster) -> Result<Self> {
        warn!("⚠️ Using enhanced stub Helius client with production rate limiting");
        
        // Initialize with production rate limiting (3000 req/h from 6.7M available)
        let rate_limiter = Arc::new(AdvancedRateLimiter::new_helius());
        
        Ok(Helius {
            rate_limiter,
            cluster,
        })
    }

    pub fn rpc(&self) -> StubRpcClient {
        StubRpcClient::new(self.rate_limiter.clone())
    }
    
    /// Get rate limiter for external usage
    pub fn rate_limiter(&self) -> Arc<AdvancedRateLimiter> {
        self.rate_limiter.clone()
    }
    
    /// Execute a rate-limited request
    #[instrument(skip(self, operation), fields(endpoint = %endpoint))]
    pub async fn execute_rate_limited<T, F, Fut>(&self, endpoint: &str, priority: RequestPriority, operation: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let start_time = Instant::now();
        
        // Acquire rate limit permit
        let permit = self.rate_limiter.acquire_permit(priority, endpoint).await
            .context("Failed to acquire rate limit permit")?;
        
        // Execute the operation
        match operation().await {
            Ok(result) => {
                permit.mark_success(priority).await;
                let duration_ms = start_time.elapsed().as_millis() as u64;
                debug!("✅ Helius request succeeded: {} ({}ms)", endpoint, duration_ms);
                Ok(result)
            }
            Err(e) => {
                // Check if this is a rate limit error
                if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                    permit.mark_rate_limited().await;
                    error!("🚫 Helius rate limit hit on endpoint: {}", endpoint);
                } else {
                    permit.mark_success(priority).await; // Don't penalize for non-rate-limit errors
                }
                Err(e)
            }
        }
    }
    
    /// Get current rate limiting statistics
    pub async fn get_rate_stats(&self) -> crate::api::RateLimitStats {
        self.rate_limiter.get_usage_stats().await
    }
}

impl HeliusFactory {
    pub fn new(api_key: &str) -> Self {
        warn!("⚠️ Using enhanced stub HeliusFactory with production rate limiting");
        
        // Initialize factory with production rate limiting
        let rate_limiter = Arc::new(AdvancedRateLimiter::new_helius());
        
        HeliusFactory {
            api_key: api_key.to_string(),
            rate_limiter,
        }
    }

    pub fn create(&self, cluster: Cluster) -> Result<Helius> {
        Ok(Helius {
            rate_limiter: self.rate_limiter.clone(),
            cluster,
        })
    }
    
    /// Get factory rate limiter
    pub fn rate_limiter(&self) -> Arc<AdvancedRateLimiter> {
        self.rate_limiter.clone()
    }
}

pub struct StubRpcClient {
    rate_limiter: Arc<AdvancedRateLimiter>,
}

impl StubRpcClient {
    pub fn new(rate_limiter: Arc<AdvancedRateLimiter>) -> Self {
        Self { rate_limiter }
    }

    pub async fn get_epoch_info(&self) -> Result<StubEpochInfo> {
        self.execute_rpc_call("get_epoch_info", RequestPriority::Medium, || async {
            warn!("⚠️ Using stub RPC client - returning dummy epoch info");
            Ok(StubEpochInfo)
        }).await
    }
    
    pub async fn get_slot(&self) -> Result<u64> {
        self.execute_rpc_call("get_slot", RequestPriority::High, || async {
            warn!("⚠️ Using stub RPC client - returning dummy slot");
            Ok(123456789u64)
        }).await
    }
    
    pub async fn get_account_info(&self, _pubkey: &str) -> Result<StubAccountInfo> {
        self.execute_rpc_call("get_account_info", RequestPriority::High, || async {
            warn!("⚠️ Using stub RPC client - returning dummy account info");
            Ok(StubAccountInfo)
        }).await
    }
    
    /// Execute an RPC call with rate limiting
    async fn execute_rpc_call<T, F, Fut>(&self, method: &str, priority: RequestPriority, operation: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let start_time = Instant::now();
        
        // Acquire rate limit permit
        let permit = self.rate_limiter.acquire_permit(priority, method).await
            .context("Failed to acquire rate limit permit for RPC call")?;
        
        // Execute the operation
        match operation().await {
            Ok(result) => {
                permit.mark_success(priority).await;
                let duration_ms = start_time.elapsed().as_millis() as u64;
                debug!("✅ Helius RPC call succeeded: {} ({}ms)", method, duration_ms);
                Ok(result)
            }
            Err(e) => {
                // Check if this is a rate limit error
                if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                    permit.mark_rate_limited().await;
                    error!("🚫 Helius RPC rate limit hit on method: {}", method);
                } else {
                    permit.mark_success(priority).await; // Don't penalize for non-rate-limit errors
                }
                Err(e)
            }
        }
    }
}

pub struct StubEpochInfo;
pub struct StubAccountInfo;

/// Configuration for Helius client
#[derive(Debug, Clone)]
pub struct HeliusConfig {
    pub api_key: String,
    pub cluster: Cluster,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
}

impl HeliusConfig {
    /// Create a new Helius configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("HELIUS_API_KEY")
            .context("HELIUS_API_KEY environment variable is required")?;
            
        let cluster_str = std::env::var("HELIUS_CLUSTER")
            .unwrap_or_else(|_| "mainnet-beta".to_string());
            
        let cluster = match cluster_str.as_str() {
            "mainnet-beta" | "mainnet" => Cluster::MainnetBeta,
            "devnet" => Cluster::Devnet,
            _ => {
                warn!("Unknown cluster '{}', defaulting to mainnet-beta", cluster_str);
                Cluster::MainnetBeta
            }
        };
        
        let webhook_url = std::env::var("HELIUS_WEBHOOK_URL").ok();
        let webhook_secret = std::env::var("HELIUS_WEBHOOK_SECRET").ok();
        
        Ok(HeliusConfig {
            api_key,
            cluster,
            webhook_url,
            webhook_secret,
        })
    }
}

/// Main Helius client manager for the arbitrage bot
pub struct HeliusManager {
    client: Helius,
    config: HeliusConfig,
}

impl HeliusManager {
    /// Create a new Helius manager with the given configuration
    pub fn new(config: HeliusConfig) -> Result<Self> {
        info!("Initializing Helius client for cluster: {:?}", config.cluster);
        
        let client = Helius::new(&config.api_key, config.cluster.clone())
            .context("Failed to create Helius client")?;
            
        debug!("Helius client created successfully");
        
        Ok(HeliusManager {
            client,
            config,
        })
    }
    
    /// Create a new Helius manager from environment variables
    pub fn from_env() -> Result<Self> {
        let config = HeliusConfig::from_env()?;
        Self::new(config)
    }
    
    /// Create a Helius manager with factory support for multiple clients
    pub fn with_factory(config: HeliusConfig) -> Result<Self> {
        info!("Initializing Helius client with factory support");
        
        let factory = HeliusFactory::new(&config.api_key);
        let client = factory.create(config.cluster.clone())?;
        
        debug!("Helius client with factory created successfully");
        
        Ok(HeliusManager {
            client,
            config,
        })
    }
    
    /// Get a reference to the Helius client
    pub fn client(&self) -> &Helius {
        &self.client
    }
    
    /// Get the configuration
    pub fn config(&self) -> &HeliusConfig {
        &self.config
    }
    
    /// Test the connection to Helius services
    pub async fn test_connection(&self) -> Result<bool> {
        info!("Testing Helius connection...");
        
        // Use stub implementation during dependency conflicts
        match self.client.rpc().get_epoch_info().await {
            Ok(_) => {
                info!("✅ Helius connection test successful (stub implementation)");
                Ok(true)
            }
            Err(e) => {
                error!("❌ Helius connection test failed: {}", e);
                Ok(false)
            }
        }
    }
    
    /// Get client information for monitoring
    pub fn get_client_info(&self) -> ClientInfo {
        ClientInfo {
            api_key_prefix: format!("{}...", &self.config.api_key[..8]),
            cluster: format!("{:?}", self.config.cluster),
            webhook_url: self.config.webhook_url.clone(),
            has_factory: false,
        }
    }
    
    /// Create additional clients if using factory
    pub fn create_additional_client(&self, _cluster: Cluster) -> Result<Helius> {
        error!("Cannot create additional client without factory");
        Err(anyhow::anyhow!("HeliusManager was not created with factory support"))
    }
}

/// Information about the Helius client for monitoring and debugging
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub api_key_prefix: String,
    pub cluster: String,
    pub webhook_url: Option<String>,
    pub has_factory: bool,
}

impl std::fmt::Display for ClientInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HeliusClient(key={}...cluster={}, webhook={}, factory={})",
            &self.api_key_prefix[..8],
            self.cluster,
            self.webhook_url.as_deref().unwrap_or("none"),
            self.has_factory
        )
    }
}

/// Thread-safe wrapper for HeliusManager
pub type SharedHeliusManager = Arc<HeliusManager>;

/// Convenience function to create a shared Helius manager
pub fn create_shared_helius_manager() -> Result<SharedHeliusManager> {
    let manager = HeliusManager::from_env()?;
    Ok(Arc::new(manager))
}

/// Convenience function to create a shared Helius manager with factory
pub fn create_shared_helius_manager_with_factory() -> Result<SharedHeliusManager> {
    let config = HeliusConfig::from_env()?;
    let manager = HeliusManager::with_factory(config)?;
    Ok(Arc::new(manager))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_helius_config_creation() {
        // Set up test environment
        env::set_var("HELIUS_API_KEY", "test-key");
        env::set_var("HELIUS_CLUSTER", "devnet");
        
        let config = HeliusConfig::from_env().unwrap();
        assert_eq!(config.api_key, "test-key");
        assert!(matches!(config.cluster, Cluster::Devnet));
    }
    
    #[tokio::test]
    async fn test_helius_client_creation() {
        let config = HeliusConfig {
            api_key: "test-key".to_string(),
            cluster: Cluster::Devnet,
            webhook_url: None,
            webhook_secret: None,
        };
        
        // This will fail without a real API key, but tests the creation logic
        let result = HeliusManager::new(config);
        // We expect this to work even with invalid key for creation
        assert!(result.is_ok());
    }
}
