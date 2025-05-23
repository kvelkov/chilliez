// src/cache.rs
//! Provides a Redis-based caching layer, primarily for DEX API responses.

use crate::error::ArbError;
use anyhow::{anyhow, Result as AnyhowResult};
use log::{debug, error, info, warn};
use redis::{aio::ConnectionManager, AsyncCommands}; // Removed RedisError as it's not directly used
use serde::{de::DeserializeOwned, Serialize};
// Removed: use std::sync::Arc; // Not used here
// Removed: use std::time::Duration; // Not used here

/// A shared Redis cache client.
/// Uses a `ConnectionManager` for automatic reconnection and resilience.
#[derive(Clone, Debug)] // Added Debug
pub struct Cache {
    conn_manager: ConnectionManager,
    default_ttl_secs: u64,
}

impl Cache {
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

    // get_connection is not strictly needed if conn_manager.clone() is used directly.
    // async fn get_connection(&self) -> AnyhowResult<ConnectionManager> {
    //     Ok(self.conn_manager.clone())
    // }

    fn generate_key(prefix: &str, params: &[&str]) -> String {
        let mut key = prefix.to_string();
        for param in params {
            key.push(':');
            key.push_str(param);
        }
        key
    }

    pub async fn get_json<T: DeserializeOwned>(&self, key_prefix: &str, key_params: &[&str]) -> Result<Option<T>, ArbError> {
        let key = Self::generate_key(key_prefix, key_params);
        debug!("Attempting to GET cache for key: {}", key);

        let mut conn = self.conn_manager.clone();
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(value_str)) => {
                debug!("Cache HIT for key: {}. Deserializing...", key);
                match serde_json::from_str::<T>(&value_str) {
                    Ok(value) => Ok(Some(value)),
                    Err(e) => {
                        warn!("Failed to deserialize cached JSON for key {}: {}. Data: '{}'", key, e, value_str);
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

    // Renamed from set_ex to set_json_with_ttl for clarity if preferred, or keep set_ex
    pub async fn set_ex<T: Serialize>(
        &self,
        prefix: &str,
        params: &[&str],
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> AnyhowResult<()> {
        let key = Self::generate_key(prefix, params);
        let value_str = serde_json::to_string(value)?;
        let mut conn = self.conn_manager.clone(); // Get a connection from the manager
        let ttl_to_use = ttl_seconds.unwrap_or(self.default_ttl_secs);

        match conn.set_ex::<_, _, ()>(&key, value_str, ttl_to_use).await { // Ensure value_str is passed by ref if needed by redis crate version
            Ok(_) => {
                debug!("Cache SETEX success for key: {} with TTL: {}s", key, ttl_to_use);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to SETEX key '{}' in Redis: {}", key, e);
                Err(anyhow!(e))
            }
        }
    }

    #[allow(dead_code)]
    pub async fn delete(&self, key_prefix: &str, key_params: &[&str]) -> Result<bool, ArbError> {
        let key = Cache::generate_key(key_prefix, key_params); // Corrected call
        debug!("Attempting to DEL cache for key: {}", key);
        let mut conn = self.conn_manager.clone();
        match conn.del::<_, i32>(&key).await {
            Ok(count) => Ok(count > 0),
            Err(e) => {
                error!("Redis DEL error for key {}: {}", key, e);
                Err(ArbError::Unknown(format!("Redis DEL error: {}", e)))
            }
        }
    }
}