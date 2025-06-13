// src/helius_client.rs
//! Helius SDK Client Manager
//! 
//! Provides a centralized interface for interacting with the Helius SDK
//! for enhanced performance and native Helius service integration.

use helius::{Helius, HeliusFactory};
use helius::types::Cluster;
use anyhow::{Result, Context};
use std::sync::Arc;
use tracing::{info, warn, error, debug};

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
        
        // Use a simple RPC call to test connectivity
        match self.client.rpc().solana_client.get_epoch_info() {
            Ok(_) => {
                info!("✅ Helius connection test successful");
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
