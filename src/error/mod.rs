use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArbError {
    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("DEX error: {0}")]
    DexError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Insufficient liquidity: {0}")]
    InsufficientLiquidity(String),

    #[error("Slippage too high: expected {expected:.4}%, actual {actual:.4}%")]
    SlippageTooHigh { expected: f64, actual: f64 },

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Timeout error: {0}")]
    TimeoutError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

// Implement From traits for common error types
impl From<solana_client::client_error::ClientError> for ArbError {
    fn from(error: solana_client::client_error::ClientError) -> Self {
        ArbError::RpcError(error.to_string())
    }
}

// Add more From implementations for other error types
