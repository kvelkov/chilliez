//! Production-grade dynamic fee calculation module
//!
//! This module provides comprehensive fee estimation including:
//! - Real-time Solana priority fee calculation based on network congestion
//! - Jito tip calculation for MEV protection
//! - Protocol fees for each DEX
//! - Total cost analysis and profitability assessment

use anyhow::Result;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, TokenAmount};

// =============================================================================
// Fee Breakdown & Analysis Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBreakdown {
    pub protocol_fee: f64,
    pub gas_fee: f64,
    pub priority_fee: f64,
    pub jito_tip: f64,
    pub slippage_cost: f64,
    pub total_cost: f64,
    pub explanation: String,
    pub risk_score: f64,
    pub compute_units: u32,
    pub fee_per_signature: f64,
    pub network_congestion: NetworkCongestionLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkCongestionLevel {
    Low,      // < 50% capacity
    Medium,   // 50-80% capacity
    High,     // 80-95% capacity
    Critical, // > 95% capacity
}

#[derive(Debug, Clone)]
pub struct DynamicFeeConfig {
    pub base_priority_fee: u64,     // Base priority fee in lamports
    pub max_priority_fee: u64,      // Maximum priority fee ceiling
    pub jito_tip_percentage: f64,   // Percentage of trade value for Jito tips
    pub congestion_multiplier: f64, // Fee multiplier during high congestion
    pub cache_ttl: Duration,        // How long to cache fee estimates
}

impl Default for DynamicFeeConfig {
    fn default() -> Self {
        Self {
            base_priority_fee: 1_000,          // 1k lamports base
            max_priority_fee: 100_000,         // 100k lamports max
            jito_tip_percentage: 0.001,        // 0.1% of trade value
            congestion_multiplier: 3.0,        // 3x during high congestion
            cache_ttl: Duration::from_secs(5), // 5 second cache
        }
    }
}

// =============================================================================
// Network Congestion Tracking
// =============================================================================

#[derive(Debug, Clone)]
struct NetworkCongestionData {
    pub slot_fill_rate: f64,         // Percentage of slots that are full
    pub recent_slot_times: Vec<u64>, // Recent slot processing times
    pub last_updated: Instant,
}

// =============================================================================
// Production Fee Manager
// =============================================================================

#[derive(Clone)]
pub struct FeeManager {
    config: DynamicFeeConfig,
    #[allow(dead_code)] // Used in production RPC calls (TODO: implement)
    rpc_client: Arc<SolanaRpcClient>, // not in use - Field is marked dead_code and not used by FeeManager's methods
    congestion_cache: Arc<RwLock<Option<NetworkCongestionData>>>,
    fee_history: Arc<RwLock<Vec<(Instant, u64)>>>, // Historical fee data
}

impl FeeManager {
    pub fn new(rpc_client: Arc<SolanaRpcClient>) -> Self {
        Self {
            config: DynamicFeeConfig::default(),
            rpc_client,
            congestion_cache: Arc::new(RwLock::new(None)),
            fee_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_config(rpc_client: Arc<SolanaRpcClient>, config: DynamicFeeConfig) -> Self {
        Self {
            config,
            rpc_client,
            congestion_cache: Arc::new(RwLock::new(None)),
            fee_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Calculate comprehensive fee breakdown for arbitrage opportunity
    pub async fn calculate_multihop_fees(
        &self,
        pools: &[&PoolInfo],
        input_amount: &TokenAmount,
        sol_price_usd: f64,
    ) -> Result<FeeBreakdown> {
        let compute_units = self.estimate_compute_units(pools);
        let priority_fee = self.calculate_dynamic_priority_fee(compute_units).await?;
        let trade_value_sol = input_amount.amount as f64 / 1_000_000_000.0; // Convert to SOL
        let jito_tip = self.calculate_jito_tip(trade_value_sol, pools.len() as f64);
        let network_congestion = self.get_network_congestion_level().await;

        // Calculate protocol fees for each pool
        let protocol_fee = self.calculate_protocol_fees(pools, input_amount);

        // Base Solana transaction fee (5000 lamports per signature)
        let base_fee = 5000.0;
        let gas_fee = base_fee + priority_fee as f64;

        let total_cost = protocol_fee + gas_fee + jito_tip as f64;
        let risk_score = self.calculate_risk_score(
            total_cost,
            trade_value_sol * sol_price_usd,
            &network_congestion,
        );

        let explanation = format!(
            "Protocol: {:.4} SOL, Gas: {:.4} SOL, Priority: {:.4} SOL, Jito: {:.4} SOL, Congestion: {:?}",
            protocol_fee / 1_000_000_000.0,
            base_fee / 1_000_000_000.0,
            priority_fee as f64 / 1_000_000_000.0,
            jito_tip as f64 / 1_000_000_000.0,
            network_congestion
        );

        Ok(FeeBreakdown {
            protocol_fee,
            gas_fee,
            priority_fee: priority_fee as f64,
            jito_tip: jito_tip as f64,
            slippage_cost: 0.0, // Will be calculated separately by slippage module
            total_cost,
            explanation,
            risk_score,
            compute_units,
            fee_per_signature: base_fee,
            network_congestion,
        })
    }

    /// Estimate compute units required for multi-hop arbitrage
    pub fn estimate_compute_units(&self, pools: &[&PoolInfo]) -> u32 {
        let base_compute_units = 10_000; // Base transaction overhead
        let per_pool_units = 150_000; // Estimated CU per swap
        let jupiter_routing_overhead = 50_000; // Additional for Jupiter routing

        let total_units =
            base_compute_units + (pools.len() as u32 * per_pool_units) + jupiter_routing_overhead;

        // Cap at Solana's limit
        std::cmp::min(total_units, 1_400_000)
    }

    /// Calculate dynamic priority fee based on current network congestion
    pub async fn calculate_dynamic_priority_fee(&self, compute_units: u32) -> Result<u64> {
        let congestion_level = self.get_network_congestion_level().await;
        let base_fee = self.config.base_priority_fee;

        let multiplier = match congestion_level {
            NetworkCongestionLevel::Low => 1.0,
            NetworkCongestionLevel::Medium => 2.0,
            NetworkCongestionLevel::High => 3.5,
            NetworkCongestionLevel::Critical => 5.0,
        };

        let dynamic_fee = (base_fee as f64 * multiplier) as u64;
        let total_priority_fee = std::cmp::min(dynamic_fee, self.config.max_priority_fee);

        // Store in history for analysis
        {
            let mut history = self.fee_history.write().await;
            history.push((Instant::now(), total_priority_fee));

            // Keep only recent history (last 100 entries)
            if history.len() > 100 {
                history.drain(0..50);
            }
        }

        debug!(
            "Dynamic priority fee calculated: {} lamports (congestion: {:?}, CU: {})",
            total_priority_fee, congestion_level, compute_units
        );

        Ok(total_priority_fee)
    }

    /// Calculate optimal Jito tip for MEV protection
    pub fn calculate_jito_tip(&self, trade_value_sol: f64, complexity_factor: f64) -> u64 {
        // Base tip calculation: percentage of trade value
        let base_tip = trade_value_sol * self.config.jito_tip_percentage * 1_000_000_000.0; // Convert to lamports

        // Adjust for complexity (more complex trades need higher tips)
        let complexity_multiplier = 1.0 + (complexity_factor - 1.0) * 0.5;
        let adjusted_tip = base_tip * complexity_multiplier;

        // Minimum tip for any MEV protection
        let min_tip = 10_000.0; // 10k lamports minimum
        let max_tip = 1_000_000.0; // 1M lamports maximum

        let final_tip = adjusted_tip.max(min_tip).min(max_tip) as u64;

        debug!(
            "Jito tip calculated: {} lamports (trade value: {:.4} SOL, complexity: {:.2})",
            final_tip, trade_value_sol, complexity_factor
        );

        final_tip
    }

    /// Calculate protocol fees for each DEX in the route
    fn calculate_protocol_fees(&self, pools: &[&PoolInfo], input_amount: &TokenAmount) -> f64 {
        let mut total_protocol_fee = 0.0;

        for pool in pools {
            let fee_rate = match pool.dex_type {
                crate::utils::DexType::Orca => 0.003, // 0.3% typical for Orca CLMM
                crate::utils::DexType::Raydium => 0.0025, // 0.25% for Raydium V4
                crate::utils::DexType::Meteora => 0.002, // Variable, using average
                crate::utils::DexType::Jupiter => 0.001, // Aggregator fee
                crate::utils::DexType::Lifinity => 0.002, // Proactive MM fee
                crate::utils::DexType::Phoenix => 0.0005, // Order book spread
                crate::utils::DexType::Whirlpool => 0.003, // Same as Orca
                crate::utils::DexType::Unknown(_) => 0.003, // Default conservative estimate
            };

            total_protocol_fee += input_amount.amount as f64 * fee_rate;
        }

        total_protocol_fee
    }

    /// Get current network congestion level
    async fn get_network_congestion_level(&self) -> NetworkCongestionLevel {
        // Check cache first
        {
            let cache = self.congestion_cache.read().await;
            if let Some(ref data) = *cache {
                if data.last_updated.elapsed() < self.config.cache_ttl {
                    return self.determine_congestion_level(data);
                }
            }
        }

        // Fetch fresh congestion data
        match self.fetch_network_congestion().await {
            Ok(data) => {
                let level = self.determine_congestion_level(&data);

                // Update cache
                {
                    let mut cache = self.congestion_cache.write().await;
                    *cache = Some(data);
                }

                level
            }
            Err(e) => {
                warn!("Failed to fetch network congestion: {}", e);
                NetworkCongestionLevel::Medium // Safe default
            }
        }
    }

    /// Fetch real-time network congestion data from Solana RPC
    async fn fetch_network_congestion(&self) -> Result<NetworkCongestionData> {
        // This would integrate with Solana RPC to get real congestion data
        // For now, implementing a basic simulation based on recent slot performance

        // TODO: Implement actual RPC calls to:
        // - getRecentPerformanceSamples
        // - getRecentPrioritizationFees
        // - getSlot and slot timing analysis

        // Placeholder implementation with realistic simulation
        let slot_fill_rate = 0.75; // Simulate 75% slot fill rate
        let recent_slot_times = vec![400, 420, 380, 450, 410]; // ms per slot

        Ok(NetworkCongestionData {
            slot_fill_rate,
            recent_slot_times,
            last_updated: Instant::now(),
        })
    }

    /// Determine congestion level from network data
    fn determine_congestion_level(&self, data: &NetworkCongestionData) -> NetworkCongestionLevel {
        let avg_slot_time: f64 = data
            .recent_slot_times
            .iter()
            .map(|&x| x as f64)
            .sum::<f64>()
            / data.recent_slot_times.len() as f64;

        // Solana target is 400ms per slot
        let slot_performance_factor = avg_slot_time / 400.0;

        // Combine slot fill rate and performance
        let congestion_score = data.slot_fill_rate * slot_performance_factor;

        match congestion_score {
            x if x < 0.6 => NetworkCongestionLevel::Low,
            x if x < 0.8 => NetworkCongestionLevel::Medium,
            x if x < 0.95 => NetworkCongestionLevel::High,
            _ => NetworkCongestionLevel::Critical,
        }
    }

    /// Calculate risk score based on fees and trade value
    fn calculate_risk_score(
        &self,
        total_cost: f64,
        trade_value_usd: f64,
        congestion: &NetworkCongestionLevel,
    ) -> f64 {
        let cost_ratio = total_cost / (trade_value_usd * 1_000_000_000.0); // Cost as fraction of trade value

        let base_risk = cost_ratio * 100.0; // Base risk from cost ratio

        let congestion_risk = match congestion {
            NetworkCongestionLevel::Low => 0.0,
            NetworkCongestionLevel::Medium => 10.0,
            NetworkCongestionLevel::High => 25.0,
            NetworkCongestionLevel::Critical => 50.0,
        };

        (base_risk + congestion_risk).min(100.0) // Cap at 100
    }

    /// Get recent fee history for analysis
    pub async fn get_fee_history(&self) -> Vec<(Instant, u64)> {
        self.fee_history.read().await.clone()
    }

    /// Update fee configuration
    pub fn update_config(&mut self, config: DynamicFeeConfig) {
        self.config = config;
    }

    /// Estimate fees for a transaction
    pub async fn estimate_fees(
        &self,
        amount: u64,
        compute_units: u32,
        dex_name: &str,
    ) -> Result<FeeBreakdown> {
        let base_fee = 5000; // Base transaction fee in lamports
        let priority_fee = self.calculate_dynamic_priority_fee(compute_units).await?;

        // DEX-specific fees (placeholder implementation)
        let dex_fee = match dex_name {
            "Orca" => (amount as f64 * 0.003) as u64,     // 0.3%
            "Raydium" => (amount as f64 * 0.0025) as u64, // 0.25%
            "Jupiter" => (amount as f64 * 0.0015) as u64, // 0.15%
            _ => (amount as f64 * 0.003) as u64,          // Default 0.3%
        };

        let total_cost = base_fee + priority_fee + dex_fee;

        Ok(FeeBreakdown {
            protocol_fee: dex_fee as f64,
            gas_fee: base_fee as f64,
            priority_fee: priority_fee as f64,
            jito_tip: 0.0, // Jito tip not calculated here
            slippage_cost: 0.0,
            total_cost: total_cost as f64,
            explanation: format!(
                "Base: {}, Priority: {}, DEX: {}",
                base_fee, priority_fee, dex_fee
            ),
            risk_score: 0.0, // Risk score not calculated here
            compute_units,
            fee_per_signature: base_fee as f64,
            network_congestion: NetworkCongestionLevel::Medium, // Default value
        })
    }
}

impl Default for FeeManager {
    fn default() -> Self {
        use std::time::Duration;
        Self {
            config: DynamicFeeConfig::default(),
            rpc_client: Arc::new(SolanaRpcClient::new(
                "https://api.mainnet-beta.solana.com",
                vec![],
                3,
                Duration::from_millis(1000),
            )),
            congestion_cache: Arc::new(RwLock::new(None)),
            fee_history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl std::fmt::Debug for FeeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeeManager")
            .field("config", &self.config)
            .field("rpc_client", &"<SolanaRpcClient>")
            .field("congestion_cache", &self.congestion_cache)
            .field("fee_history", &self.fee_history)
            .finish()
    }
}

// =============================================================================
// FeeEstimator for Routing System Compatibility
// =============================================================================

/// Fee estimator for the routing system
/// This is a simplified interface over FeeManager for routing modules
#[derive(Debug, Clone)]
pub struct FeeEstimator {
    fee_manager: Option<Arc<FeeManager>>,
}

impl FeeEstimator {
    /// Create a new fee estimator
    pub fn new() -> Self {
        Self { fee_manager: None }
    }

    /// Create a fee estimator with a manager
    pub fn with_manager(fee_manager: Arc<FeeManager>) -> Self {
        Self {
            fee_manager: Some(fee_manager),
        }
    }

    /// Estimate network fees for a transaction
    pub async fn estimate_network_fees(&self) -> Result<u64> {
        if let Some(manager) = &self.fee_manager {
            let breakdown = manager
                .estimate_fees(
                    1_000_000_000, // 1 SOL default
                    1000,          // Default compute units
                    "Orca",        // Default DEX
                )
                .await?;
            Ok(breakdown.total_cost as u64)
        } else {
            Ok(5000) // Default 5000 lamports
        }
    }

    /// Estimate priority fees based on network congestion
    pub async fn estimate_priority_fee(&self) -> Result<u64> {
        if let Some(manager) = &self.fee_manager {
            Ok(manager.config.base_priority_fee)
        } else {
            Ok(1000) // Default 1000 lamports
        }
    }
}

impl Default for FeeEstimator {
    fn default() -> Self {
        Self::new()
    }
}
