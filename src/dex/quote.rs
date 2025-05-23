// src/dex/quote.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    pub route: Vec<String>,

    // New recommended fields
    pub latency_ms: Option<u64>,         // Time it took to fetch quote
    pub execution_score: Option<f64>,    // AI/ML score for quality
    pub route_path: Option<Vec<String>>, // Explicit path (if multi-hop)
    pub slippage_estimate: Option<f64>,  // % slippage expected
}

#[async_trait]
pub trait DexClient: Send + Sync {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> Result<Quote>;

    fn get_supported_pairs(&self) -> Vec<(String, String)>;

    fn get_name(&self) -> &str; // Returns the name of the DEX implementation
}

impl Quote {
    /// Returns the profit in base units (output - input)
    pub fn profit(&self) -> i64 {
        self.output_amount as i64 - self.input_amount as i64
    }
    /// Returns the profit as a percentage of input
    pub fn profit_pct(&self) -> f64 {
        if self.input_amount == 0 {
            0.0
        } else {
            (self.output_amount as f64 - self.input_amount as f64) / self.input_amount as f64
                * 100.0
        }
    }
    /// Returns the output amount as a float, given the token decimals
    pub fn output_as_float(&self, decimals: u8) -> f64 {
        self.output_amount as f64 / 10f64.powi(decimals as i32)
    }
    /// Returns the input amount as a float, given the token decimals
    pub fn input_as_float(&self, decimals: u8) -> f64 {
        self.input_amount as f64 / 10f64.powi(decimals as i32)
    }
}

#[allow(dead_code)]
impl dyn DexClient {
    // These trait methods are required for DEX pool integration and dynamic dispatch.
    // They may be called via trait objects or in downstream modules.
}

use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize, Clone)] // Added Clone
pub struct QuoteRequest {
    pub input_token: String,
    pub output_token: String,
    pub amount: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)] // Added Clone
pub struct QuoteResponse {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    pub route: Vec<String>,

    // New recommended fields
    pub latency_ms: Option<u64>,         // Time it took to fetch quote
    pub execution_score: Option<f64>,    // AI/ML score for quality
    pub route_path: Option<Vec<String>>, // Explicit path (if multi-hop)
    pub slippage_estimate: Option<f64>,  // % slippage expected
}
