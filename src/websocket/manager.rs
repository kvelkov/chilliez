// Add to your imports
use crate::error::{ArbError, ArbResult, CircuitBreaker, RetryPolicy};

impl SolanaWebsocketManager {
    /// Enhanced connection with circuit breaker
    pub async fn connect_with_error_handling(&mut self) -> ArbResult<()> {
        let mut circuit_breaker = CircuitBreaker::new(3, Duration::from_secs(30));
        
        circuit_breaker.execute(async {
            self.connect().await
                .map_err(|e| ArbError::WebSocketError(e.to_string()))
        }).await
    }

    /// Enhanced subscription with retry logic
    pub async fn subscribe_to_pools_with_retry(&mut self, pool_addresses: Vec<Pubkey>) -> ArbResult<()> {
        let retry_policy = RetryPolicy::new(3, Duration::from_millis(500), Duration::from_secs(5));
        
        retry_policy.execute(|| async {
            self.subscribe_to_pools(pool_addresses.clone()).await
                .map_err(|e| ArbError::WebSocketError(e.to_string()))
        }).await
    }
}