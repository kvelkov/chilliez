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
    pub latency_ms: Option<u64>,       
    pub execution_score: Option<f64>,  
    pub route_path: Option<Vec<String>>, 
    pub slippage_estimate: Option<f64>,
}

#[async_trait::async_trait]
pub trait DexClient: Send + Sync {
    // These methods are fundamental to the trait and used by integration tests.
    // Their "unused" warning in lib code means the main arbitrage logic isn't using API quotes yet.
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<Quote>; // Changed to anyhow::Result

    fn get_supported_pairs(&self) -> Vec<(String, String)>;

    fn get_name(&self) -> &str; 
}

impl Quote {
    pub fn _profit(&self) -> i64 { // Prefixed
        self.output_amount as i64 - self.input_amount as i64
    }
    pub fn _profit_pct(&self) -> f64 { // Prefixed
        if self.input_amount == 0 {
            0.0
        } else {
            (self.output_amount as f64 - self.input_amount as f64) / self.input_amount as f64
                * 100.0
        }
    }
    pub fn _output_as_float(&self, decimals: u8) -> f64 { // Prefixed
        self.output_amount as f64 / 10f64.powi(decimals as i32)
    }
    pub fn _input_as_float(&self, decimals: u8) -> f64 { // Prefixed
        self.input_amount as f64 / 10f64.powi(decimals as i32)
    }
}

impl dyn DexClient {
    // Trait methods are defined above.
}

// These structs were already Clone.
#[derive(Debug, Serialize, Deserialize, Clone)] 
pub struct QuoteRequest {
    pub input_token: String,
    pub output_token: String,
    pub amount: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)] 
pub struct QuoteResponse {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
    pub route: Vec<String>,
    pub latency_ms: Option<u64>,       
    pub execution_score: Option<f64>,  
    pub route_path: Option<Vec<String>>,
    pub slippage_estimate: Option<f64>,
}