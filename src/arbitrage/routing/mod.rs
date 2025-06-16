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

pub mod graph;
pub mod pathfinder;
pub mod splitter;
pub mod optimizer;
pub mod mev_protection;
pub mod failover;
pub mod smart_router;

pub use graph::{
    RoutingGraph,
    PoolNode,
    RouteEdge,
    LiquidityPool,
    PoolMetrics,
    PoolHealth,
};

pub use pathfinder::{
    PathFinder,
    RoutePath,
    RouteStep,
    PathfinderConfig,
    PathfinderAlgorithm,
    PathConstraints,
};

pub use splitter::{
    RouteSplitter,
    SplitRoute,
    SplitStrategy,
    OptimalSplit,
};

pub use optimizer::{
    RouteOptimizer,
    OptimizationGoal,
    RouteScore,
    OptimizationConstraints,
};

pub use mev_protection::{
    MevProtectedRouter,
    MevRisk,
    MevProtectionStrategy,
    MevThreatAnalysis,
    ProtectedRoute,
    MevProtectionConfig,
};

pub use failover::{
    FailoverRouter,
    FailoverStrategy,
    ExecutionStatus,
    DexHealthStatus,
    FailoverConfig,
    FailoverPlan,
    ExecutionAttempt,
    ExecutionResult,
};

pub use smart_router::{
    SmartRouter,
    SmartRouterConfig,
    RouteRequest,
    SmartRoutingResult,
    RoutingPriority,
    RouteQualityMetrics,
    ExecutionRecommendation,
    RecommendedAction,
    RiskAssessment,
    RiskLevel,
    RouteConstraints,
};

// Re-export common types for convenience
pub use std::time::Duration;
pub use serde::{Serialize, Deserialize};
