// examples/complete_integration_test.rs
//! Complete Integration Test for Sprint 3
//! 
//! Tests the entire high-throughput execution pipeline:
//! - Opportunity detection and batching
//! - Pre-transaction simulation
//! - MEV protection integration
//! - Jito bundle submission
//! - End-to-end metrics tracking

use solana_arb_bot::{
    arbitrage::{
        execution_engine::{BatchExecutionEngine, BatchExecutionConfig},
        jito_client::{JitoClient, JitoConfig},
        opportunity::MultiHopArbOpportunity,
        mev_protection::{AdvancedMevProtection, MevProtectionConfig},
        detector::ArbitrageDetector,
    },
    solana::rpc::SolanaRpcClient,
    utils::{DexType, PoolInfo, PoolToken},
    error::ArbError,
    metrics::Metrics,
};
use log::info;
use std::{sync::Arc, collections::HashMap};
use tokio::{sync::Mutex, time::{Duration, sleep}};
use solana_sdk::pubkey::Pubkey;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("🧪 Starting Complete Integration Test - Sprint 3");
    
    let test_suite = IntegrationTestSuite::new().await?;
    test_suite.run_all_tests().await?;

    info!("✅ All integration tests passed!");
    Ok(())
}

struct IntegrationTestSuite {
    batch_engine: Arc<BatchExecutionEngine>,
    detector: ArbitrageDetector,
    metrics: Arc<Mutex<Metrics>>,
    test_pools: HashMap<Pubkey, Arc<PoolInfo>>,
}

impl IntegrationTestSuite {
    async fn new() -> Result<Self, ArbError> {
        info!("🔧 Setting up integration test suite...");

        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![], // No fallback endpoints
            3,      // max_retries
            Duration::from_millis(1000), // retry_delay
        ));

        let batch_config = BatchExecutionConfig {
            max_opportunities_per_batch: 3,
            enable_parallel_simulation: true,
            simulation_timeout_ms: 1000,
            ..Default::default()
        };

        let batch_engine = Arc::new(BatchExecutionEngine::new(batch_config, rpc_client));

        let detector = ArbitrageDetector::new(0.5, 1.0, 150.0, 5000);
        let metrics = Arc::new(Mutex::new(Metrics::new(150.0, None)));
        let test_pools = Self::create_test_pools();

        Ok(Self {
            batch_engine,
            detector,
            metrics,
            test_pools,
        })
    }

    fn create_test_pools() -> HashMap<Pubkey, Arc<PoolInfo>> {
        let mut pools = HashMap::new();

        // SOL/USDC Orca pool
        let sol_usdc_orca = Arc::new(PoolInfo {
            address: Pubkey::new_unique(),
            name: "SOL/USDC Orca".to_string(),
            dex_type: DexType::Orca,
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1_000_000_000_000, // 1M SOL
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 150_000_000_000_000, // 150M USDC
            },
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            ..Default::default()
        });

        // SOL/USDC Raydium pool
        let sol_usdc_raydium = Arc::new(PoolInfo {
            address: Pubkey::new_unique(),
            name: "SOL/USDC Raydium".to_string(),
            dex_type: DexType::Raydium,
            token_a: PoolToken {
                mint: sol_usdc_orca.token_a.mint, // Same SOL mint
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 800_000_000_000, // 800K SOL
            },
            token_b: PoolToken {
                mint: sol_usdc_orca.token_b.mint, // Same USDC mint
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 125_000_000_000_000, // 125M USDC (slightly different price)
            },
            fee_numerator: Some(30),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(30),
            ..Default::default()
        });

        pools.insert(sol_usdc_orca.address, sol_usdc_orca);
        pools.insert(sol_usdc_raydium.address, sol_usdc_raydium);

        pools
    }

    async fn run_all_tests(&self) -> Result<(), ArbError> {
        info!("🧪 Running integration test suite...");

        self.test_opportunity_detection().await?;
        self.test_batch_creation().await?;
        self.test_simulation_pipeline().await?;
        self.test_mev_protection().await?;
        self.test_jito_integration().await?;
        self.test_end_to_end_flow().await?;

        Ok(())
    }

    async fn test_opportunity_detection(&self) -> Result<(), ArbError> {
        info!("🔍 Testing opportunity detection...");

        let mut metrics = self.metrics.lock().await;
        let opportunities = self.detector.find_all_opportunities(&self.test_pools, &mut metrics).await?;

        assert!(!opportunities.is_empty(), "Should detect arbitrage opportunities");
        info!("   ✅ Detected {} opportunities", opportunities.len());

        for opp in &opportunities {
            assert!(opp.profit_pct > 0.0, "Opportunity should be profitable");
            assert!(!opp.hops.is_empty(), "Opportunity should have hops");
            info!("      📊 {}: {:.2}% profit", opp.id, opp.profit_pct);
        }

        Ok(())
    }

    async fn test_batch_creation(&self) -> Result<(), ArbError> {
        info!("📦 Testing batch creation...");

        // Create test opportunities
        let opportunities = vec![
            self.create_test_opportunity("test1", 5.0, 100.0).await?,
            self.create_test_opportunity("test2", 3.0, 50.0).await?,
            self.create_test_opportunity("test3", 8.0, 200.0).await?,
        ];

        // Submit opportunities
        for opp in opportunities {
            self.batch_engine.submit_opportunity(opp).await?;
        }

        // Wait for batch processing
        sleep(Duration::from_millis(500)).await;

        let status = self.batch_engine.get_status().await;
        info!("   ✅ Batch engine processed opportunities");
        info!("      📊 Total processed: {}", status.total_processed);

        Ok(())
    }

    async fn test_simulation_pipeline(&self) -> Result<(), ArbError> {
        info!("🧪 Testing simulation pipeline...");

        // Create a high-value opportunity for simulation
        let opportunity = self.create_test_opportunity("sim_test", 10.0, 500.0).await?;
        
        // Submit for processing (includes simulation)
        self.batch_engine.submit_opportunity(opportunity).await?;

        // Wait for processing
        sleep(Duration::from_millis(300)).await;

        let metrics = self.batch_engine.get_metrics().await;
        info!("   ✅ Simulation pipeline executed");
        info!("      📊 Simulation success rate: {:.1}%", metrics.simulation_success_rate * 100.0);
        info!("      ⏱️ Average simulation time: {:.1}ms", metrics.average_simulation_time_ms);

        Ok(())
    }

    async fn test_mev_protection(&self) -> Result<(), ArbError> {
        info!("🛡️ Testing MEV protection...");

        let mev_protection = AdvancedMevProtection::new(MevProtectionConfig::default());
        let opportunity = self.create_test_opportunity("mev_test", 15.0, 1000.0).await?;

        // Test MEV risk analysis
        let risk_score = mev_protection.analyze_mev_risk(&opportunity).await?;
        assert!(risk_score >= 0.0 && risk_score <= 1.0, "Risk score should be between 0 and 1");
        info!("   ✅ MEV risk analysis: {:.2}", risk_score);

        // Test protection strategy
        let strategy = mev_protection.recommend_protection_strategy(&opportunity).await?;
        info!("   ✅ Protection strategy generated");
        info!("      🔒 Private mempool: {}", strategy.use_private_mempool);
        info!("      💰 Fee multiplier: {:.1}x", strategy.priority_fee_multiplier);

        // Test optimal fee calculation
        let optimal_fee = mev_protection.calculate_optimal_priority_fee(&opportunity, 10_000).await?;
        assert!(optimal_fee >= 10_000, "Optimal fee should be at least base fee");
        info!("   ✅ Optimal fee calculated: {} lamports", optimal_fee);

        Ok(())
    }

    async fn test_jito_integration(&self) -> Result<(), ArbError> {
        info!("🚀 Testing Jito integration...");

        let jito_client = JitoClient::new(JitoConfig::default());

        // Test tip calculation
        let optimal_tip = jito_client.get_optimal_tip(10_000).await?;
        assert!(optimal_tip > 0, "Optimal tip should be positive");
        info!("   ✅ Optimal tip calculated: {} lamports", optimal_tip);

        // Test tip accounts
        let tip_accounts = jito_client.get_tip_accounts();
        assert!(!tip_accounts.is_empty(), "Should have tip accounts");
        info!("   ✅ Tip accounts available: {}", tip_accounts.len());

        Ok(())
    }

    async fn test_end_to_end_flow(&self) -> Result<(), ArbError> {
        info!("🔄 Testing end-to-end flow...");

        let start_time = std::time::Instant::now();

        // Create multiple opportunities
        let opportunities = vec![
            self.create_test_opportunity("e2e_1", 6.0, 150.0).await?,
            self.create_test_opportunity("e2e_2", 4.0, 80.0).await?,
            self.create_test_opportunity("e2e_3", 9.0, 300.0).await?,
            self.create_test_opportunity("e2e_4", 2.5, 40.0).await?,
        ];

        // Submit all opportunities
        for opp in opportunities {
            self.batch_engine.submit_opportunity(opp).await?;
        }

        // Force execute to complete the flow
        self.batch_engine.force_execute_pending().await?;

        let execution_time = start_time.elapsed();
        let final_metrics = self.batch_engine.get_metrics().await;

        info!("   ✅ End-to-end flow completed in {:.2}ms", execution_time.as_secs_f64() * 1000.0);
        info!("      📊 Total opportunities: {}", final_metrics.total_opportunities_processed);
        info!("      📦 Total batches: {}", final_metrics.total_batches_created);
        info!("      🚀 Total bundles: {}", final_metrics.total_bundles_submitted);
        info!("      💰 Total profit: ${:.2}", final_metrics.total_profit_usd);
        info!("      🎯 Success rate: {:.1}%", 
              if final_metrics.total_bundles_submitted > 0 {
                  final_metrics.successful_bundles as f64 / final_metrics.total_bundles_submitted as f64 * 100.0
              } else {
                  0.0
              });

        // Verify metrics
        assert!(final_metrics.total_opportunities_processed > 0, "Should process opportunities");
        assert!(final_metrics.total_batches_created > 0, "Should create batches");

        Ok(())
    }

    async fn create_test_opportunity(
        &self,
        id: &str,
        profit_pct: f64,
        profit_usd: f64,
    ) -> Result<MultiHopArbOpportunity, ArbError> {
        use solana_arb_bot::arbitrage::opportunity::ArbHop;

        let hops = vec![
            ArbHop {
                dex: DexType::Orca,
                pool: Pubkey::new_unique(),
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 100.0,
                expected_output: 100.0 * (1.0 + profit_pct / 100.0),
            }
        ];

        Ok(MultiHopArbOpportunity {
            id: id.to_string(),
            hops,
            total_profit: profit_usd / 100.0,
            profit_pct,
            input_token: "SOL".to_string(),
            output_token: "USDC".to_string(),
            input_amount: 100.0,
            expected_output: 100.0 * (1.0 + profit_pct / 100.0),
            dex_path: vec![DexType::Orca],
            pool_path: vec![Pubkey::new_unique()],
            estimated_profit_usd: Some(profit_usd),
            input_amount_usd: Some(100.0),
            output_amount_usd: Some(100.0 + profit_usd),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn test_dex_client_creation() {
        let config = Config::test_default();
        let clients = create_dex_clients(&config).await;
        assert!(clients.is_ok());
        
        let clients = clients.unwrap();
        assert!(clients.len() >= 4); // At least Orca, Raydium, Meteora, Lifinity
    }

    #[tokio::test] 
    async fn test_integrated_service_creation() {
        let config = Arc::new(Config::test_default());
        let clients = create_dex_clients(&config).await.unwrap();
        let service = IntegratedPoolService::new(config, clients);
        assert!(service.is_ok());
    }
}