pub mod calculator;
pub mod calculator_tests;
pub mod detector;
pub mod dynamic_threshold;
pub mod engine;
pub mod executor;
pub mod fee_manager;
pub mod opportunity;
pub mod pipeline;
pub mod tests;

// Re-export HTTP/logging utilities for DEX modules
pub use crate::dex::http_utils_shared::headers_with_api_key;
