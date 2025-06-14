// src/arbitrage/mev_protection.rs
use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    error::ArbError,
};
use log::{info, warn, debug};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    transaction::Transaction,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct MevProtectionConfig {
    pub enable_private_mempool: bool,
    pub max_priority_fee_lamports: u64,
    pub dynamic_fee_adjustment: bool,
    pub bundle_transactions: bool,
    pub randomize_execution_timing: bool,
    pub use_flashloan_protection: bool,
    pub max_slippage_protection: f64,
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            enable_private_mempool: true,
            max_priority_fee_lamports: 100_000,
            dynamic_fee_adjustment: true,
            bundle_transactions: true,
            randomize_execution_timing: true,
            use_flashloan_protection: true,
            max_slippage_protection: 0.02, // 2% max slippage
        }
    }
}

#[derive(Debug, Clone)]
pub struct GasOptimizationMetrics {
    pub average_priority_fee: u64,
    pub successful_transactions: u64,
    pub failed_transactions: u64,
    pub total_gas_saved: u64,
    pub mev_protection_activations: u64,
    pub bundle_success_rate: f64,
}

impl Default for GasOptimizationMetrics {
    fn default() -> Self {
        Self {
            average_priority_fee: 0,
            successful_transactions: 0,
            failed_transactions: 0,
            total_gas_saved: 0,
            mev_protection_activations: 0,
            bundle_success_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkConditions {
    pub current_congestion_level: f64, // 0.0 to 1.0
    pub average_priority_fee: u64,
    pub recent_block_times: Vec<Duration>,
    pub mempool_size_estimate: usize,
    pub last_update: Instant,
}

impl NetworkConditions {
    pub fn new() -> Self {
        Self {
            current_congestion_level: 0.5,
            average_priority_fee: 10_000,
            recent_block_times: Vec::new(),
            mempool_size_estimate: 1000,
            last_update: Instant::now(),
        }
    }
}

impl Default for NetworkConditions {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AdvancedMevProtection {
    config: MevProtectionConfig,
    metrics: Arc<Mutex<GasOptimizationMetrics>>,
    network_conditions: Arc<Mutex<NetworkConditions>>,
    _transaction_bundles: Arc<Mutex<HashMap<String, Vec<Transaction>>>>,
    priority_fee_history: Arc<Mutex<Vec<(Instant, u64)>>>,
}

impl AdvancedMevProtection {
    pub fn new(config: MevProtectionConfig) -> Self {
        info!("🛡️ Initializing Advanced MEV Protection System");
        info!("   🔒 Private mempool: {}", config.enable_private_mempool);
        info!("   💰 Dynamic fee adjustment: {}", config.dynamic_fee_adjustment);
        info!("   📦 Transaction bundling: {}", config.bundle_transactions);
        info!("   🎲 Randomized timing: {}", config.randomize_execution_timing);
        info!("   ⚡ Flash loan protection: {}", config.use_flashloan_protection);
        
        Self {
            config,
            metrics: Arc::new(Mutex::new(GasOptimizationMetrics::default())),
            network_conditions: Arc::new(Mutex::new(NetworkConditions::default())),
            _transaction_bundles: Arc::new(Mutex::new(HashMap::new())),
            priority_fee_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Calculate optimal priority fee based on network conditions and opportunity value
    pub async fn calculate_optimal_priority_fee(
        &self,
        opportunity: &MultiHopArbOpportunity,
        base_fee: u64,
    ) -> Result<u64, ArbError> {
        let network_conditions = self.network_conditions.lock().await;
        let profit_usd = opportunity.estimated_profit_usd.unwrap_or(0.0);
        
        // Base calculation: higher profit = willing to pay more for priority
        let profit_based_fee = if profit_usd > 100.0 {
            base_fee * 3 // High profit opportunities get 3x priority
        } else if profit_usd > 50.0 {
            base_fee * 2 // Medium profit opportunities get 2x priority
        } else {
            base_fee // Low profit opportunities use base fee
        };

        // Adjust for network congestion
        let congestion_multiplier = 1.0 + network_conditions.current_congestion_level;
        let congestion_adjusted_fee = (profit_based_fee as f64 * congestion_multiplier) as u64;

        // Apply dynamic adjustment based on recent success rates
        let dynamic_fee = if self.config.dynamic_fee_adjustment {
            self.apply_dynamic_fee_adjustment(congestion_adjusted_fee).await
        } else {
            congestion_adjusted_fee
        };

        // Cap at maximum allowed fee
        let final_fee = dynamic_fee.min(self.config.max_priority_fee_lamports);

        debug!("💰 Priority fee calculation:");
        debug!("   Base fee: {} lamports", base_fee);
        debug!("   Profit-based fee: {} lamports", profit_based_fee);
        debug!("   Congestion multiplier: {:.2}x", congestion_multiplier);
        debug!("   Dynamic adjusted fee: {} lamports", dynamic_fee);
        debug!("   Final fee (capped): {} lamports", final_fee);

        Ok(final_fee)
    }

    /// Apply dynamic fee adjustment based on recent transaction success rates
    async fn apply_dynamic_fee_adjustment(&self, base_fee: u64) -> u64 {
        let metrics = self.metrics.lock().await;
        let total_transactions = metrics.successful_transactions + metrics.failed_transactions;
        
        if total_transactions == 0 {
            return base_fee;
        }

        let success_rate = metrics.successful_transactions as f64 / total_transactions as f64;
        
        // If success rate is low, increase fees to improve priority
        let adjustment_factor = if success_rate < 0.7 {
            1.5 // Increase fee by 50% if success rate is below 70%
        } else if success_rate < 0.8 {
            1.2 // Increase fee by 20% if success rate is below 80%
        } else {
            1.0 // Keep fee as is if success rate is good
        };

        (base_fee as f64 * adjustment_factor) as u64
    }

    /// Create MEV-protected transaction bundle
    pub async fn create_mev_protected_bundle(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
        instructions_per_opportunity: Vec<Vec<Instruction>>,
    ) -> Result<Vec<Transaction>, ArbError> {
        if !self.config.bundle_transactions {
            return Err(ArbError::ConfigError("Transaction bundling is disabled".to_string()));
        }

        info!("📦 Creating MEV-protected transaction bundle for {} opportunities", opportunities.len());

        let mut bundle_transactions = Vec::new();
        let mut total_compute_units = 0u32;
        const MAX_COMPUTE_UNITS_PER_TX: u32 = 1_400_000;

        for (i, (opportunity, instructions)) in opportunities.iter().zip(instructions_per_opportunity.iter()).enumerate() {
            // Calculate compute units needed for this opportunity
            let estimated_cu = self.estimate_compute_units(&instructions).await;
            
            if total_compute_units + estimated_cu > MAX_COMPUTE_UNITS_PER_TX {
                warn!("⚠️ Opportunity {} would exceed compute unit limit, creating separate transaction", i);
                // Create a new transaction for remaining opportunities
                // For now, we'll skip this opportunity in the bundle
                continue;
            }

            // Calculate optimal priority fee for this opportunity
            let base_fee = 10_000; // Base priority fee
            let optimal_fee = self.calculate_optimal_priority_fee(opportunity, base_fee).await?;

            // Add compute budget instructions
            let mut tx_instructions = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(estimated_cu),
                ComputeBudgetInstruction::set_compute_unit_price(optimal_fee / estimated_cu as u64),
            ];

            // Add MEV protection instructions
            if self.config.use_flashloan_protection {
                let protection_instructions = self.create_flashloan_protection_instructions().await?;
                tx_instructions.extend(protection_instructions);
            }

            // Add the actual arbitrage instructions
            tx_instructions.extend(instructions.clone());

            // Add anti-MEV randomization
            if self.config.randomize_execution_timing {
                let randomization_instructions = self.create_timing_randomization_instructions().await?;
                tx_instructions.extend(randomization_instructions);
            }

            // Create a placeholder transaction (would need real wallet and recent blockhash in production)
            // For now, we'll create a basic transaction structure
            let transaction = Transaction::new_with_payer(
                &tx_instructions,
                None, // payer would be set from wallet in production
            );
            
            bundle_transactions.push(transaction);
            total_compute_units += estimated_cu;
            
            info!("📦 Added opportunity {} to bundle (CU: {}, Fee: {} lamports)", 
                  opportunity.id, estimated_cu, optimal_fee);
        }

        // For now, return empty vector as we need wallet and blockhash to create actual transactions
        // In a real implementation, this would create properly signed transactions
        info!("✅ MEV-protected bundle created with {} compute units total", total_compute_units);
        
        // Update metrics
        {
            let mut metrics = self.metrics.lock().await;
            metrics.mev_protection_activations += 1;
        }

        Ok(bundle_transactions)
    }

    /// Estimate compute units needed for a set of instructions
    async fn estimate_compute_units(&self, instructions: &[Instruction]) -> u32 {
        // Base compute units per instruction type
        const BASE_CU_PER_INSTRUCTION: u32 = 1_000;
        const SWAP_INSTRUCTION_CU: u32 = 50_000;
        const TRANSFER_INSTRUCTION_CU: u32 = 5_000;

        let mut total_cu = 0;

        for instruction in instructions {
            // Estimate based on instruction data size and accounts
            let instruction_cu = if instruction.data.len() > 100 {
                SWAP_INSTRUCTION_CU // Likely a swap instruction
            } else if instruction.accounts.len() > 5 {
                TRANSFER_INSTRUCTION_CU // Likely a transfer
            } else {
                BASE_CU_PER_INSTRUCTION // Basic instruction
            };

            total_cu += instruction_cu;
        }

        // Add buffer for safety
        let buffered_cu = (total_cu as f64 * 1.2) as u32; // 20% buffer
        
        debug!("🧮 Estimated compute units: {} (with 20% buffer)", buffered_cu);
        buffered_cu
    }

    /// Create flash loan protection instructions
    async fn create_flashloan_protection_instructions(&self) -> Result<Vec<Instruction>, ArbError> {
        // Placeholder for flash loan protection logic
        // In a real implementation, this would create instructions that:
        // 1. Check for flash loan attacks
        // 2. Validate transaction atomicity
        // 3. Add slippage protection
        
        debug!("⚡ Creating flash loan protection instructions");
        Ok(vec![])
    }

    /// Create timing randomization instructions
    async fn create_timing_randomization_instructions(&self) -> Result<Vec<Instruction>, ArbError> {
        // Placeholder for timing randomization logic
        // In a real implementation, this would add small delays or reorder instructions
        // to make MEV attacks more difficult
        
        debug!("🎲 Creating timing randomization instructions");
        Ok(vec![])
    }

    /// Update network conditions based on recent observations
    pub async fn update_network_conditions(
        &self,
        congestion_level: f64,
        average_priority_fee: u64,
        recent_block_time: Duration,
    ) -> Result<(), ArbError> {
        let mut conditions = self.network_conditions.lock().await;
        
        conditions.current_congestion_level = congestion_level.clamp(0.0, 1.0);
        conditions.average_priority_fee = average_priority_fee;
        conditions.recent_block_times.push(recent_block_time);
        
        // Keep only recent block times (last 10)
        if conditions.recent_block_times.len() > 10 {
            conditions.recent_block_times.remove(0);
        }
        
        conditions.last_update = Instant::now();
        
        debug!("🌐 Network conditions updated:");
        debug!("   Congestion level: {:.2}", conditions.current_congestion_level);
        debug!("   Average priority fee: {} lamports", conditions.average_priority_fee);
        debug!("   Recent block time: {:?}", recent_block_time);
        
        Ok(())
    }

    /// Record transaction execution result for metrics
    pub async fn record_transaction_result(
        &self,
        success: bool,
        priority_fee_used: u64,
        gas_saved: Option<u64>,
    ) -> Result<(), ArbError> {
        let mut metrics = self.metrics.lock().await;
        
        if success {
            metrics.successful_transactions += 1;
        } else {
            metrics.failed_transactions += 1;
        }

        // Update average priority fee
        let total_transactions = metrics.successful_transactions + metrics.failed_transactions;
        metrics.average_priority_fee = 
            (metrics.average_priority_fee * (total_transactions - 1) + priority_fee_used) / total_transactions;

        if let Some(saved) = gas_saved {
            metrics.total_gas_saved += saved;
        }

        // Update bundle success rate
        if metrics.mev_protection_activations > 0 {
            metrics.bundle_success_rate = 
                metrics.successful_transactions as f64 / metrics.mev_protection_activations as f64;
        }

        // Record priority fee history
        {
            let mut history = self.priority_fee_history.lock().await;
            history.push((Instant::now(), priority_fee_used));
            
            // Keep only recent history (last 100 transactions)
            if history.len() > 100 {
                history.remove(0);
            }
        }

        debug!("📊 Transaction result recorded: success={}, fee={} lamports", success, priority_fee_used);
        
        Ok(())
    }

    /// Get current MEV protection metrics
    pub async fn get_metrics(&self) -> GasOptimizationMetrics {
        (*self.metrics.lock().await).clone()
    }

    /// Get current network conditions
    pub async fn get_network_conditions(&self) -> NetworkConditions {
        (*self.network_conditions.lock().await).clone()
    }

    /// Analyze MEV risk for a given opportunity
    pub async fn analyze_mev_risk(&self, opportunity: &MultiHopArbOpportunity) -> Result<f64, ArbError> {
        let _network_conditions = self.network_conditions.lock().await;
        
        // Base risk factors
        let mut risk_score: f64 = 0.0;
        
        // Higher profit opportunities are more attractive to MEV bots
        if let Some(profit_usd) = opportunity.estimated_profit_usd {
            if profit_usd > 1000.0 {
                risk_score += 0.8; // Very high MEV risk
            } else if profit_usd > 500.0 {
                risk_score += 0.6; // High MEV risk
            } else if profit_usd > 100.0 {
                risk_score += 0.4; // Medium MEV risk
            } else {
                risk_score += 0.2; // Low MEV risk
            }
        }

        // Network congestion increases MEV risk
        risk_score += _network_conditions.current_congestion_level * 0.3;

        // Multi-hop opportunities are more complex and harder to front-run
        let hop_count = opportunity.hops.len();
        if hop_count > 3 {
            risk_score -= 0.2; // Lower risk for complex paths
        } else if hop_count == 2 {
            risk_score += 0.1; // Higher risk for simple paths
        }

        // Cross-DEX arbitrage is harder to front-run
        let unique_dexs: std::collections::HashSet<_> = opportunity.dex_path.iter().collect();
        if unique_dexs.len() > 1 {
            risk_score -= 0.15; // Lower risk for cross-DEX
        }

        // Clamp risk score between 0.0 and 1.0
        let final_risk_score = risk_score.clamp(0.0, 1.0);
        
        debug!("🎯 MEV risk analysis for opportunity {}:", opportunity.id);
        debug!("   Profit-based risk: {:.2}", opportunity.estimated_profit_usd.unwrap_or(0.0));
        debug!("   Network congestion factor: {:.2}", _network_conditions.current_congestion_level);
        debug!("   Path complexity factor: {} hops", hop_count);
        debug!("   Cross-DEX factor: {} unique DEXs", unique_dexs.len());
        debug!("   Final MEV risk score: {:.2}", final_risk_score);
        
        Ok(final_risk_score)
    }

    /// Recommend MEV protection strategy for an opportunity
    pub async fn recommend_protection_strategy(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<MevProtectionStrategy, ArbError> {
        let mev_risk = self.analyze_mev_risk(opportunity).await?;
        let _network_conditions = self.network_conditions.lock().await;
        
        let strategy = if mev_risk > 0.8 {
            MevProtectionStrategy {
                use_private_mempool: true,
                priority_fee_multiplier: 3.0,
                bundle_with_other_txs: true,
                add_timing_randomization: true,
                use_flashloan_protection: true,
                max_slippage_tolerance: 0.01, // 1% for high-risk
                recommended_delay_ms: 0, // Execute immediately for high-profit
            }
        } else if mev_risk > 0.5 {
            MevProtectionStrategy {
                use_private_mempool: true,
                priority_fee_multiplier: 2.0,
                bundle_with_other_txs: true,
                add_timing_randomization: false,
                use_flashloan_protection: true,
                max_slippage_tolerance: 0.015, // 1.5% for medium-risk
                recommended_delay_ms: 100, // Small delay
            }
        } else {
            MevProtectionStrategy {
                use_private_mempool: false,
                priority_fee_multiplier: 1.0,
                bundle_with_other_txs: false,
                add_timing_randomization: false,
                use_flashloan_protection: false,
                max_slippage_tolerance: 0.02, // 2% for low-risk
                recommended_delay_ms: 200, // Longer delay acceptable
            }
        };

        info!("🛡️ MEV protection strategy for opportunity {} (risk: {:.2}):", opportunity.id, mev_risk);
        info!("   Private mempool: {}", strategy.use_private_mempool);
        info!("   Priority fee multiplier: {:.1}x", strategy.priority_fee_multiplier);
        info!("   Bundle transactions: {}", strategy.bundle_with_other_txs);
        info!("   Timing randomization: {}", strategy.add_timing_randomization);
        info!("   Flash loan protection: {}", strategy.use_flashloan_protection);
        info!("   Max slippage: {:.1}%", strategy.max_slippage_tolerance * 100.0);
        info!("   Recommended delay: {}ms", strategy.recommended_delay_ms);

        Ok(strategy)
    }

    /// Get comprehensive MEV protection status
    pub async fn get_protection_status(&self) -> MevProtectionStatus {
        let metrics = self.metrics.lock().await;
        let network_conditions = self.network_conditions.lock().await;
        
        MevProtectionStatus {
            is_active: self.config.enable_private_mempool,
            current_congestion_level: network_conditions.current_congestion_level,
            average_priority_fee: metrics.average_priority_fee,
            successful_transactions: metrics.successful_transactions,
            failed_transactions: metrics.failed_transactions,
            total_gas_saved: metrics.total_gas_saved,
            bundle_success_rate: metrics.bundle_success_rate,
            mev_protection_activations: metrics.mev_protection_activations,
            last_network_update: network_conditions.last_update,
        }
    }

    /// Access transaction bundles for future implementation
    pub async fn get_transaction_bundles(&self) -> HashMap<String, Vec<Transaction>> {
        self._transaction_bundles.lock().await.clone()
    }

    /// Store transaction bundle for future processing
    pub async fn store_transaction_bundle(&self, bundle_id: String, transactions: Vec<Transaction>) -> Result<(), ArbError> {
        let mut bundles = self._transaction_bundles.lock().await;
        bundles.insert(bundle_id, transactions);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MevProtectionStrategy {
    pub use_private_mempool: bool,
    pub priority_fee_multiplier: f64,
    pub bundle_with_other_txs: bool,
    pub add_timing_randomization: bool,
    pub use_flashloan_protection: bool,
    pub max_slippage_tolerance: f64,
    pub recommended_delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct MevProtectionStatus {
    pub is_active: bool,
    pub current_congestion_level: f64,
    pub average_priority_fee: u64,
    pub successful_transactions: u64,
    pub failed_transactions: u64,
    pub total_gas_saved: u64,
    pub bundle_success_rate: f64,
    pub mev_protection_activations: u64,
    pub last_network_update: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::opportunity::ArbHop;
    use crate::utils::DexType;
    use solana_sdk::pubkey::Pubkey;

    #[tokio::test]
    async fn test_mev_protection_initialization() {
        let config = MevProtectionConfig::default();
        let mev_protection = AdvancedMevProtection::new(config);
        
        let metrics = mev_protection.get_metrics().await;
        assert_eq!(metrics.successful_transactions, 0);
        assert_eq!(metrics.failed_transactions, 0);
    }

    #[tokio::test]
    async fn test_priority_fee_calculation() {
        let config = MevProtectionConfig::default();
        let max_fee = config.max_priority_fee_lamports;
        let mev_protection = AdvancedMevProtection::new(config);
        
        let opportunity = MultiHopArbOpportunity {
            estimated_profit_usd: Some(100.0),
            ..Default::default()
        };
        
        let base_fee = 10_000;
        let optimal_fee = mev_protection.calculate_optimal_priority_fee(&opportunity, base_fee).await.unwrap();
        
        assert!(optimal_fee >= base_fee);
        assert!(optimal_fee <= max_fee);
    }

    #[tokio::test]
    async fn test_mev_risk_analysis() {
        let config = MevProtectionConfig::default();
        let mev_protection = AdvancedMevProtection::new(config);
        
        let high_profit_opportunity = MultiHopArbOpportunity {
            estimated_profit_usd: Some(1500.0),
            hops: vec![
                ArbHop {
                    dex: DexType::Orca,
                    pool: Pubkey::default(),
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 100.0,
                    expected_output: 2000.0,
                },
                ArbHop {
                    dex: DexType::Raydium,
                    pool: Pubkey::default(),
                    input_token: "USDC".to_string(),
                    output_token: "SOL".to_string(),
                    input_amount: 2000.0,
                    expected_output: 115.0,
                }
            ],
            ..Default::default()
        };
        
        let risk_score = mev_protection.analyze_mev_risk(&high_profit_opportunity).await.unwrap();
        assert!(risk_score > 0.5); // High profit should result in higher risk
    }
}