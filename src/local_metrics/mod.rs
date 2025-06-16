#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TradingPair(pub String, pub String);

// #[derive(Debug, Clone)]
// pub struct OpportunityDetail {
//     input_token: String,
//     output_token: String,
//     profit_percentage: f64,
//     input_amount: f64,
// }

#[derive(Debug)]
#[allow(dead_code)]
pub struct ExecutionRecord {
    opportunity_id: String,
    success: bool,
    execution_time_ms: u64,
    actual_profit_usd: Option<f64>,
    transaction_signature: Option<String>,
    error_message: Option<String>,
    timestamp: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum TradeOutcome {
    Attempted,
    Success,
    Failure,
    Skipped,
}

// Removed unused imports after Metrics struct removal
mod metrics;
pub use metrics::Metrics;
