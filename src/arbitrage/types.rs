// src/arbitrage/types.rs
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use rust_decimal::prelude::*;
use std::collections::HashMap;

/// Strategy for executing arbitrage opportunities
#[derive(Debug, Clone)]
pub enum ExecutionStrategy {
    /// Execute all opportunities immediately using single executor
    SingleExecution(Vec<MultiHopArbOpportunity>),
    /// Execute using batch engine (batchable opportunities, immediate opportunities)
    BatchExecution(Vec<MultiHopArbOpportunity>, Vec<MultiHopArbOpportunity>),
    /// Hybrid approach: immediate execution + batching
    HybridExecution {
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>,
    },
}

/// Analysis of opportunity competitiveness for execution decision
#[derive(Debug, Clone)]
pub struct CompetitivenessAnalysis {
    #[allow(dead_code)]
    pub competitive_score: Decimal,
    /// Factors that affect competitiveness (latency, gas, etc.)
    #[allow(dead_code)]
    pub risk_factors: Vec<String>,
    pub execution_recommendation: ExecutionRecommendation,
    pub reason: String,
}

/// Recommendation for execution method based on competitiveness
#[derive(Debug, Clone)]
pub enum ExecutionRecommendation {
    /// Execute immediately with single executor for speed
    ImmediateSingle,
    /// Safe to include in batch execution
    SafeToBatch,
}

/// Performance tracking metrics for arbitrage detection
#[derive(Debug, Default)]
pub struct DetectionMetrics {
    pub total_detection_cycles: u64,
    pub total_opportunities_found: u64,
    pub average_detection_time_ms: f64,
    pub hot_cache_hits: u64,
    pub hot_cache_misses: u64,
    pub last_detection_timestamp: u64,
}

/// Health status for various components
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub is_healthy: bool,
    pub last_check: std::time::Instant,
    pub error_count: u64,
    pub message: String,
}

/// Pool validation configuration
#[derive(Debug, Clone)]
pub struct PoolValidationMetrics {
    pub total_pools_validated: u64,
    pub pools_passed: u64,
    pub pools_failed: u64,
    pub validation_errors: HashMap<String, u64>,
}
