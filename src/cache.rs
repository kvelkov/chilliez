use redis::{aio::Connection, Client, AsyncCommands};
use std::sync::Arc;
use tokio::sync::Mutex;

// Create a Redis connection pool for efficiency
pub struct Cache {
    client: Arc<Client>,
    connection: Arc<Mutex<Connection>>,
}

impl Cache {
    // Initialize Redis client and connection pool
    pub async fn new(redis_url: &str) -> Self {
        let client = Client::open(redis_url).expect("Failed to connect to Redis");
        let connection = Arc::new(Mutex::new(client.get_async_connection().await.unwrap()));

        Cache {
            client: Arc::new(client),
            connection,
        }
    }

    // Retrieves cached data if available
    pub async fn get_cached_data(&self, key: &str) -> Option<String> {
        let mut conn = self.connection.lock().await;
        match conn.get::<_, String>(key).await {
            Ok(value) => Some(value),
            Err(_) => {
                eprintln!("🔴 Cache miss or Redis error for key: {}", key);
                None
            }
        }
    }

    // Stores API responses in Redis with expiration (converted to `usize`)
    pub async fn store_in_cache(&self, key: &str, value: &str, expiration_secs: u64) {
        let expiration: usize = expiration_secs.try_into().unwrap_or_else(|_| {
            eprintln!("Warning: Expiration time exceeds usize range. Defaulting to 3600s.");
            3600 // Default to 1 hour if conversion fails
        });

        let mut conn = self.connection.lock().await;
        if let Err(err) = conn.set_ex::<_, _, ()>(key, value, expiration).await {
            eprintln!("❌ Failed to store data in Redis: {}", err);
        }
    }
}
