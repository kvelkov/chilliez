// src/arbitrage/routing/mod.rs
//! Advanced Multi-Hop Routing and Smart Order Routing System
//!
//! Provides sophisticated routing capabilities for optimal trade execution:
//! - Cross-DEX multi-hop routing optimization
//! - Path finding algorithms for best prices across multiple pools
//! - Route splitting for large trades
//! - Dynamic routing based on liquidity and fees
//! - MEV-aware routing to minimize sandwich attacks
//! - Failover routing when primary paths fail

pub mod failover;
pub mod graph;
pub mod mev_protection;
pub mod optimizer;
pub mod pathfinder;
pub mod smart_router;
pub mod splitter;

pub use graph::{LiquidityPool, PoolHealth, PoolMetrics, PoolNode, RouteEdge, RoutingGraph};

pub use pathfinder::{
    PathConstraints, PathFinder, PathfinderAlgorithm, PathfinderConfig, RoutePath, RouteStep,
};

pub use splitter::{OptimalSplit, RouteSplitter, SplitRoute, SplitStrategy};

pub use optimizer::{OptimizationConstraints, OptimizationGoal, RouteOptimizer, RouteScore};

pub use mev_protection::{
    MevProtectedRouter, MevProtectionConfig, MevProtectionStrategy, MevRisk, MevThreatAnalysis,
    ProtectedRoute,
};

pub use failover::{
    DexHealthStatus, ExecutionAttempt, ExecutionResult, ExecutionStatus, FailoverConfig,
    FailoverPlan, FailoverRouter, FailoverStrategy,
};

pub use smart_router::{
    ExecutionRecommendation, RecommendedAction, RiskAssessment, RiskLevel, RouteConstraints,
    RouteQualityMetrics, RouteRequest, RoutingPriority, SmartRouter, SmartRouterConfig,
    SmartRoutingResult,
};

// Re-export common types for convenience
pub use serde::{Deserialize, Serialize};
pub use std::time::Duration;
