// src/dex/quote.rs
use serde::{Deserialize, Serialize};

/// Represents a raw quote retrieved from a DEX API for a token swap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    /// The route or steps involved (if multi-step swap occurs).
    pub route: Vec<String>,
    /// Optional latency in milliseconds for the quote retrieval.
    pub latency_ms: Option<u64>,
    /// Optional score indicating execution viability.
    pub execution_score: Option<f64>,
    /// Optional route path as a series of routes.
    pub route_path: Option<Vec<String>>,
    /// Optional estimate of slippage percentage.
    pub slippage_estimate: Option<f64>,
}

/// A more standardized response version of a Quote.
/// This may be used after internal processing to ensure all data is present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteResponse {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    pub latency_ms: u64,
    pub execution_score: f64,
    pub route_path: Vec<String>,
    pub slippage_estimate: f64,
}

/// The DexClient trait defines the interface available to all DEX API clients.
/// Implementing clients must provide methods to retrieve the best swap quote,
/// report supported token pairings, and supply their client name.
#[async_trait::async_trait]
pub trait DexClient: Send + Sync {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<Quote>;

    fn get_supported_pairs(&self) -> Vec<(String, String)>;

    fn get_name(&self) -> &str;
}

// ---- Helper Methods for Quote ----

impl Quote {
    /// Calculates the profit (output minus input) as a signed integer.
    pub fn profit(&self) -> i64 {
        self.output_amount as i64 - self.input_amount as i64
    }

    /// Calculates the profit percentage, returning 0.0 if input_amount is zero.
    pub fn profit_pct(&self) -> f64 {
        if self.input_amount == 0 {
            0.0
        } else {
            (self.output_amount as f64 - self.input_amount as f64) / self.input_amount as f64 * 100.0
        }
    }

    /// Returns the output amount converted to a float, given the token's decimal precision.
    pub fn output_as_float(&self, decimals: u8) -> f64 {
        self.output_amount as f64 / 10f64.powi(decimals as i32)
    }

    /// Returns the input amount converted to a float, given the token's decimal precision.
    pub fn input_as_float(&self, decimals: u8) -> f64 {
        self.input_amount as f64 / 10f64.powi(decimals as i32)
    }
}

/// Represents a request for a quote from the DEX API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QuoteRequest {
    pub input_token: String,
    pub output_token: String,
    pub amount: u64,
}
