use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum ArbError {
    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("Pool parsing error: {0}")]
    PoolParsingError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Other error: {0}")]
    Other(String),
}
