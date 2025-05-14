use async_trait::async_trait;

/// A trait for anything that can provide crypto price data asynchronously.
/// Implementors can fetch prices for arbitrary symbols.
#[async_trait]
pub trait CryptoDataProvider: Send + Sync {
    /// Returns the price of the requested symbol, or None if unavailable.
    async fn get_price(&self, symbol: &str) -> Option<f64>;
}