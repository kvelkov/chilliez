// src/cache.rs
//! Provides a Redis-based caching layer, primarily for DEX API responses.

use crate::error::ArbError; // For error handling
use anyhow::{anyhow, Result as AnyhowResult}; // Using anyhow::Result for errors
use log::{debug, error, info, warn}; // Added debug
use redis::{aio::ConnectionManager, AsyncCommands, RedisError}; // Using ConnectionManager for resilience
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration; // For specifying TTL

/// A shared Redis cache client.
/// Uses a `ConnectionManager` for automatic reconnection and resilience.
#[derive(Clone)] // Removed Debug
pub struct Cache {
    // ConnectionManager is designed to be cloned and is thread-safe.
    conn_manager: ConnectionManager,
    default_ttl_secs: u64,
}

impl Cache {
    /// Initializes a new Redis cache client with a connection manager.
    ///
    /// # Arguments
    /// * `redis_url` - The connection string for the Redis server (e.g., "redis://127.0.0.1/").
    /// * `default_ttl_secs` - Default time-to-live for cached items in seconds.
    pub async fn new(redis_url: &str, default_ttl_secs: u64) -> AnyhowResult<Self> {
        info!("Initializing Redis connection manager for URL: {}", redis_url);
        let client = redis::Client::open(redis_url)?;
        let conn_manager = ConnectionManager::new(client).await.map_err(|e| {
            error!("Failed to create Redis ConnectionManager: {}", e);
            anyhow!("Failed to create Redis ConnectionManager: {}", e)
        })?;
        info!("Redis ConnectionManager initialized successfully. Default TTL: {}s", default_ttl_secs);
        Ok(Self {
            conn_manager,
            default_ttl_secs,
        })
    }

    async fn get_connection(&self) -> AnyhowResult<ConnectionManager> {
        // In this setup, conn_manager is already the connection pool/manager.
        // For a single-use connection, you might clone it or get a connection from it
        // depending on how ConnectionManager is designed to be used.
        // Here, we assume it can be cloned for use.
        Ok(self.conn_manager.clone())
    }

    /// Generates a standardized cache key.
    /// Example: "quote:SOL:USDC:1000000" for a quote request.
    fn generate_key(prefix: &str, params: &[&str]) -> String {
        let mut key = prefix.to_string();
        for param in params {
            key.push(':');
            key.push_str(param);
        }
        key
    }

    /// Retrieves and deserializes cached data if available.
    ///
    /// # Arguments
    /// * `key_prefix` - A prefix for the key (e.g., "quote", "pool_data").
    /// * `key_params` - A slice of strings that form the unique parts of the key.
    ///
    /// # Type Parameters
    /// * `T` - The type of the data to be deserialized from JSON. Must implement `DeserializeOwned`.
    pub async fn get_json<T: DeserializeOwned>(&self, key_prefix: &str, key_params: &[&str]) -> Result<Option<T>, ArbError> {
        let key = Self::generate_key(key_prefix, key_params);
        debug!("Attempting to GET cache for key: {}", key);

        let mut conn = self.conn_manager.clone(); // Clone for use, CM handles connection state
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(value_str)) => {
                debug!("Cache HIT for key: {}. Deserializing...", key);
                match serde_json::from_str::<T>(&value_str) {
                    Ok(value) => Ok(Some(value)),
                    Err(e) => {
                        warn!("Failed to deserialize cached JSON for key {}: {}. Data: '{}'", key, e, value_str);
                        // Optionally, delete the malformed cache entry
                        // let _: Result<(), RedisError> = conn.del(&key).await;
                        Err(ArbError::Unknown(format!("Cache deserialization error for key {}: {}", key, e)))
                    }
                }
            }
            Ok(None) => {
                debug!("Cache MISS for key: {}", key);
                Ok(None)
            }
            Err(e) => {
                error!("Redis GET error for key {}: {}", key, e);
                Err(ArbError::Unknown(format!("Redis GET error for key {}: {}", key, e)))
            }
        }
    }

    /// Serializes and stores data in Redis with an optional TTL or default TTL.
    ///
    /// # Arguments
    /// * `key_prefix` - A prefix for the key.
    /// * `key_params` - A slice of strings that form the unique parts of the key.
    /// * `value` - The data to be serialized to JSON and cached. Must implement `Serialize`.
    /// * `custom_ttl_secs` - Optional custom TTL in seconds. If None, `default_ttl_secs` is used.
    ///
    /// # Type Parameters
    /// * `T` - The type of the data to be cached. Must implement `Serialize`.
    pub async fn set_ex<T: Serialize>(
        &self,
        prefix: &str,
        params: &[&str],
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> AnyhowResult<()> {
        let key = self.generate_key(prefix, params);
        let value_str = serde_json::to_string(value)?;
        let mut conn = self.get_connection().await?;
        let ttl_to_use = ttl_seconds.unwrap_or(self.default_ttl_secs);

        match conn.set_ex::<_, _, ()>(&key, &value_str, ttl_to_use).await {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("Failed to SETEX key \'{}\' in Redis: {}", key, e);
                Err(anyhow!(e))
            }
        }
    }

    /// Deletes an item from the cache.
    #[allow(dead_code)]
    pub async fn delete(&self, key_prefix: &str, key_params: &[&str]) -> Result<bool, ArbError> {
        let key = Self::generate_key(key_prefix, key_params);
        debug!("Attempting to DEL cache for key: {}", key);
        let mut conn = self.conn_manager.clone();
        match conn.del::<_, i32>(&key).await {
            Ok(count) => Ok(count > 0), // Returns true if one or more keys were deleted
            Err(e) => {
                error!("Redis DEL error for key {}: {}", key, e);
                Err(ArbError::Unknown(format!("Redis DEL error: {}", e)))
            }
        }
    }
}
