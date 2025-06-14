// src/paper_trading/mod.rs
//! Paper trading module for simulating arbitrage trades without real funds.
//! 
//! This module provides:
//! - Virtual portfolio management
//! - Simulated trade execution
//! - Performance analytics and reporting
//! - Risk-free strategy testing

pub mod config;
pub mod engine;
pub mod portfolio;
pub mod analytics;
pub mod reporter;

pub use config::*;
pub use engine::*;
pub use portfolio::*;
pub use analytics::*;
pub use reporter::*;
