pub mod arbitrage;
pub mod local_metrics;
pub mod config;
pub mod dex;
pub mod solana;
pub mod utils;
pub mod error;
pub mod websocket;
pub mod webhooks;
pub mod cache;
pub mod helius_client;  // NEW: Helius SDK client management
pub mod testing; // Testing infrastructure
pub mod paper_trading; // Paper trading simulation system
pub mod monitoring; // Enhanced monitoring and alerting

// Re-export key testing components for easy access
pub use testing::{
    MockDexEnvironment, MarketCondition, TestSuiteRunner,
};