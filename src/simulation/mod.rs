pub mod analytics;
pub mod config;
pub mod engine;
pub mod portfolio;
pub mod reporter;

pub use analytics::SimulationAnalytics;
pub use config::SimulationConfig;
pub use engine::{SimulatedExecutionEngine, SimulatedTradeResult};
pub use portfolio::{DexPerformance, PortfolioSummary, SafeVirtualPortfolio};
pub use reporter::{PerformanceSummary, SimulationReporter, TradeLogEntry};
