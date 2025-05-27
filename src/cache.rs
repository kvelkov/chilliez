// src/cache.rs
//! Provides a Redis-based caching layer, primarily for DEX API responses.

use crate::error::ArbError;
use anyhow::{anyhow, Result as AnyhowResult};
use log::{debug, error, info, warn};
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt; // For manual Debug impl

/// A shared Redis cache client.
/// Uses a `ConnectionManager` for automatic reconnection and resilience.
#[derive(Clone)] // Removed Debug derive initially
pub struct Cache {
    conn_manager: ConnectionManager, // This type might not be Debug
    default_ttl_secs: u64,
    redis_url: String, // Store for debug purposes
}

// Manual Debug implementation for Cache
impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache")
            .field("redis_url", &self.redis_url) // Use stored redis_url
            .field("default_ttl_secs", &self.default_ttl_secs)
            .field("conn_manager", &"<ConnectionManager instance>") // Placeholder for non-Debug field
            .finish()
    }
}

impl Cache {
    pub async fn new(redis_url: &str, default_ttl_secs: u64) -> AnyhowResult<Self> {
        info!(
            "Initializing Redis connection manager for URL: {}",
            redis_url
        );
        let client = redis::Client::open(redis_url)?;
        let conn_manager = ConnectionManager::new(client).await.map_err(|e| {
            error!("Failed to create Redis ConnectionManager: {}", e);
            anyhow!("Failed to create Redis ConnectionManager: {}", e)
        })?;
        info!(
            "Redis ConnectionManager initialized successfully. Default TTL: {}s",
            default_ttl_secs
        );
        Ok(Self {
            conn_manager,
            default_ttl_secs,
            redis_url: redis_url.to_string(), // Store for debug
        })
    }

    fn generate_key(prefix: &str, params: &[&str]) -> String {
        let mut key = prefix.to_string();
        for param in params {
            key.push(':');
            key.push_str(param);
        }
        key
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        key_prefix: &str,
        key_params: &[&str],
    ) -> Result<Option<T>, ArbError> {
        let key = Self::generate_key(key_prefix, key_params);
        debug!("Attempting to GET cache for key: {}", key);

        let mut conn = self.conn_manager.clone();
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(value_str)) => {
                debug!("Cache HIT for key: {}. Deserializing...", key);
                match serde_json::from_str::<T>(&value_str) {
                    Ok(value) => Ok(Some(value)),
                    Err(e) => {
                        warn!(
                            "Failed to deserialize cached JSON for key {}: {}. Data: '{}'",
                            key, e, value_str
                        );
                        Err(ArbError::Unknown(format!(
                            "Cache deserialization error for key {}: {}",
                            key, e
                        )))
                    }
                }
            }
            Ok(None) => {
                debug!("Cache MISS for key: {}", key);
                Ok(None)
            }
            Err(e) => {
                error!("Redis GET error for key {}: {}", key, e);
                Err(ArbError::Unknown(format!(
                    "Redis GET error for key {}: {}",
                    key, e
                )))
            }
        }
    }

    pub async fn set_ex<T: Serialize>(
        &self,
        prefix: &str,
        params: &[&str],
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> AnyhowResult<()> {
        let key = Self::generate_key(prefix, params);
        let value_str = serde_json::to_string(value)?;
        let mut conn = self.conn_manager.clone();
        let ttl_to_use = ttl_seconds.unwrap_or(self.default_ttl_secs);

        match conn.set_ex::<_, _, ()>(&key, value_str, ttl_to_use).await {
            Ok(_) => {
                debug!(
                    "Cache SETEX success for key: {} with TTL: {}s",
                    key, ttl_to_use
                );
                Ok(())
            }
            Err(e) => {
                warn!("Failed to SETEX key '{}' in Redis: {}", key, e);
                Err(anyhow!(e))
            }
        }
    }

    /// Delete a value from the cache by key.
    // TODO: This method is currently unused. It's available for future cache invalidation strategies.
    #[allow(dead_code)] // Add this attribute to explicitly acknowledge it's unused for now
    pub async fn _delete(&self, key: &str) -> Result<(), String> {
        let mut conn = self.conn_manager.clone();
        match conn.del::<_, ()>(key).await {
            Ok(_) => {
                debug!("Cache DELETE success for key: {}", key);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to DELETE key '{}' in Redis: {}", key, e);
                Err(format!("Redis DEL error for key {}: {}", key, e))
            }
        }
    }

    /// Updates the cache with new pool data, typically called from a WebSocket PoolUpdate handler.
    pub async fn update_pool_cache<T: Serialize>(
        &self,
        pool_pubkey: &str,
        pool_data: &T,
        ttl_seconds: Option<u64>,
    ) -> anyhow::Result<()> {
        // Use a consistent prefix for pool cache entries, e.g., "pool"
        self.set_ex("pool", &[pool_pubkey], pool_data, ttl_seconds)
            .await
    }
}
