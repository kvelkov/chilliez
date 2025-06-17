//! Mock DEX Environment for Sprint 4 Testing
//!
//! Provides comprehensive mock implementations of all supported DEXs with:
//! - Realistic pool state simulation
//! - Configurable market conditions
//! - Transaction execution simulation
//! - Performance testing capabilities

use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    dex::api::{DexClient, PoolDiscoverable, Quote, SwapInfo},
    error::ArbError,
    utils::{DexType, PoolInfo, PoolToken},
};
use async_trait::async_trait;
use log::{debug, info, warn};
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Configuration for mock DEX behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockDexConfig {
    /// Base success rate for transactions (0.0 to 1.0)
    pub success_rate: f64,
    /// Simulated latency range in milliseconds
    pub latency_range: (u64, u64),
    /// Price volatility factor (0.0 = no volatility, 1.0 = high volatility)
    pub volatility: f64,
    /// Slippage simulation factor
    pub slippage_factor: f64,
    /// Whether to simulate network congestion
    pub simulate_congestion: bool,
    /// Pool count for this DEX
    pub pool_count: usize,
    /// Initial liquidity range (min, max) in USD
    pub liquidity_range: (f64, f64),
}

impl Default for MockDexConfig {
    fn default() -> Self {
        Self {
            success_rate: 0.95,
            latency_range: (50, 200),
            volatility: 0.02,
            slippage_factor: 0.001,
            simulate_congestion: false,
            pool_count: 100,
            liquidity_range: (10_000.0, 1_000_000.0),
        }
    }
}

/// Market condition presets for testing different scenarios
#[derive(Debug, Clone)]
pub enum MarketCondition {
    /// Normal market conditions
    Normal,
    /// High volatility market
    HighVolatility,
    /// Low liquidity conditions
    LowLiquidity,
    /// Network congestion
    Congested,
    /// Extreme conditions (stress test)
    Extreme,
}

impl MarketCondition {
    pub fn to_config(&self) -> MockDexConfig {
        match self {
            MarketCondition::Normal => MockDexConfig::default(),
            MarketCondition::HighVolatility => MockDexConfig {
                volatility: 0.05,
                slippage_factor: 0.003,
                success_rate: 0.90,
                ..Default::default()
            },
            MarketCondition::LowLiquidity => MockDexConfig {
                liquidity_range: (1_000.0, 50_000.0),
                slippage_factor: 0.01,
                success_rate: 0.85,
                ..Default::default()
            },
            MarketCondition::Congested => MockDexConfig {
                latency_range: (500, 2000),
                success_rate: 0.80,
                simulate_congestion: true,
                ..Default::default()
            },
            MarketCondition::Extreme => MockDexConfig {
                volatility: 0.10,
                slippage_factor: 0.02,
                latency_range: (1000, 5000),
                success_rate: 0.70,
                simulate_congestion: true,
                liquidity_range: (500.0, 10_000.0),
                ..Default::default()
            },
        }
    }
}

/// Mock transaction result
#[derive(Debug, Clone)]
pub struct MockTransactionResult {
    pub success: bool,
    pub signature: Option<String>,
    pub actual_output: u64,
    pub gas_used: u64,
    pub execution_time: Duration,
    pub error_message: Option<String>,
}

/// Mock DEX implementation
#[derive(Debug)]
pub struct MockDex {
    pub name: String,
    pub dex_type: DexType,
    pub config: MockDexConfig,
    pub pools: Arc<Mutex<HashMap<Pubkey, PoolInfo>>>,
    pub transaction_history: Arc<Mutex<Vec<MockTransactionResult>>>,
    pub start_time: Instant,
}

impl MockDex {
    pub fn new(name: String, dex_type: DexType, config: MockDexConfig) -> Self {
        let pools = Self::generate_mock_pools(&dex_type, &config);

        info!("Created mock DEX '{}' with {} pools", name, pools.len());

        Self {
            name,
            dex_type,
            config,
            pools: Arc::new(Mutex::new(pools)),
            transaction_history: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
        }
    }

    /// Generate realistic mock pools for the DEX
    fn generate_mock_pools(
        dex_type: &DexType,
        config: &MockDexConfig,
    ) -> HashMap<Pubkey, PoolInfo> {
        let mut pools = HashMap::new();
        let mut rng = thread_rng();

        // Common token mints for realistic pools
        let token_mints = vec![
            ("SOL", Pubkey::new_unique(), 9),
            ("USDC", Pubkey::new_unique(), 6),
            ("USDT", Pubkey::new_unique(), 6),
            ("RAY", Pubkey::new_unique(), 6),
            ("SRM", Pubkey::new_unique(), 6),
            ("ORCA", Pubkey::new_unique(), 6),
            ("MNGO", Pubkey::new_unique(), 6),
            ("COPE", Pubkey::new_unique(), 6),
        ];

        for _i in 0..config.pool_count {
            // Select random token pair
            let token_a_idx = rng.gen_range(0..token_mints.len());
            let mut token_b_idx = rng.gen_range(0..token_mints.len());
            while token_b_idx == token_a_idx {
                token_b_idx = rng.gen_range(0..token_mints.len());
            }

            let (token_a_symbol, token_a_mint, token_a_decimals) = &token_mints[token_a_idx];
            let (token_b_symbol, token_b_mint, token_b_decimals) = &token_mints[token_b_idx];

            // Generate realistic reserves based on liquidity range
            let total_liquidity_usd =
                rng.gen_range(config.liquidity_range.0..config.liquidity_range.1);
            let token_a_price_usd = Self::get_mock_token_price(token_a_symbol);
            let token_b_price_usd = Self::get_mock_token_price(token_b_symbol);

            let token_a_reserve_usd = total_liquidity_usd / 2.0;
            let token_b_reserve_usd = total_liquidity_usd / 2.0;

            let token_a_reserve = ((token_a_reserve_usd / token_a_price_usd)
                * 10f64.powi(*token_a_decimals as i32)) as u64;
            let token_b_reserve = ((token_b_reserve_usd / token_b_price_usd)
                * 10f64.powi(*token_b_decimals as i32)) as u64;

            let pool_address = Pubkey::new_unique();
            let pool = PoolInfo {
                address: pool_address,
                name: format!(
                    "{}/{} {}",
                    token_a_symbol,
                    token_b_symbol,
                    dex_type_name(dex_type)
                ),
                token_a: PoolToken {
                    mint: *token_a_mint,
                    symbol: token_a_symbol.to_string(),
                    decimals: *token_a_decimals,
                    reserve: token_a_reserve,
                },
                token_b: PoolToken {
                    mint: *token_b_mint,
                    symbol: token_b_symbol.to_string(),
                    decimals: *token_b_decimals,
                    reserve: token_b_reserve,
                },
                token_a_vault: Pubkey::new_unique(),
                token_b_vault: Pubkey::new_unique(),
                fee_numerator: Some(25),
                fee_denominator: Some(10000),
                fee_rate_bips: Some(25),
                last_update_timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                dex_type: dex_type.clone(),
                liquidity: Some((total_liquidity_usd * 1000.0) as u128),
                sqrt_price: Some(Self::calculate_sqrt_price(token_a_reserve, token_b_reserve)),
                tick_current_index: Some(rng.gen_range(-100000..100000)),
                tick_spacing: Some(64),
                // Orca-specific fields (not applicable to mock pools)
                tick_array_0: None,
                tick_array_1: None,
                tick_array_2: None,
                oracle: None,
            };

            pools.insert(pool_address, pool);
        }

        pools
    }

    fn get_mock_token_price(symbol: &str) -> f64 {
        match symbol {
            "SOL" => 150.0,
            "USDC" | "USDT" => 1.0,
            "RAY" => 2.5,
            "SRM" => 0.8,
            "ORCA" => 3.2,
            "MNGO" => 0.15,
            "COPE" => 0.05,
            _ => 1.0,
        }
    }

    fn calculate_sqrt_price(reserve_a: u64, reserve_b: u64) -> u128 {
        if reserve_a == 0 || reserve_b == 0 {
            return 0;
        }
        let price_ratio = reserve_b as f64 / reserve_a as f64;
        (price_ratio.sqrt() * (2f64.powi(64))) as u128
    }

    /// Simulate market volatility by updating pool reserves
    pub fn simulate_market_movement(&self) {
        let mut pools = self.pools.lock().unwrap();
        let mut rng = thread_rng();

        for pool in pools.values_mut() {
            if rng.gen::<f64>() < 0.1 {
                // 10% chance to update each pool
                let volatility_factor = 1.0 + (rng.gen::<f64>() - 0.5) * self.config.volatility;

                pool.token_a.reserve = ((pool.token_a.reserve as f64) * volatility_factor) as u64;
                pool.token_b.reserve = ((pool.token_b.reserve as f64) / volatility_factor) as u64;

                pool.last_update_timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                // Update sqrt_price for CLMM pools
                if pool.sqrt_price.is_some() {
                    pool.sqrt_price = Some(Self::calculate_sqrt_price(
                        pool.token_a.reserve,
                        pool.token_b.reserve,
                    ));
                }
            }
        }
    }

    /// Simulate transaction execution
    pub async fn simulate_transaction(&self, quote: &Quote) -> MockTransactionResult {
        let start_time = Instant::now();

        // Simulate network latency
        let latency =
            thread_rng().gen_range(self.config.latency_range.0..=self.config.latency_range.1);
        tokio::time::sleep(Duration::from_millis(latency)).await;

        let mut rng = thread_rng();
        let success = rng.gen::<f64>() < self.config.success_rate;

        let result = if success {
            // Simulate slippage
            let slippage = rng.gen::<f64>() * self.config.slippage_factor;
            let actual_output = ((quote.output_amount as f64) * (1.0 - slippage)) as u64;

            MockTransactionResult {
                success: true,
                signature: Some(format!("mock_tx_{}", rng.gen::<u64>())),
                actual_output,
                gas_used: rng.gen_range(5000..50000),
                execution_time: start_time.elapsed(),
                error_message: None,
            }
        } else {
            // Simulate various failure modes
            let error_messages = vec![
                "Insufficient liquidity",
                "Slippage tolerance exceeded",
                "Network congestion",
                "Pool state changed",
                "Transaction timeout",
            ];

            MockTransactionResult {
                success: false,
                signature: None,
                actual_output: 0,
                gas_used: rng.gen_range(1000..10000), // Failed transactions still consume some gas
                execution_time: start_time.elapsed(),
                error_message: Some(
                    error_messages[rng.gen_range(0..error_messages.len())].to_string(),
                ),
            }
        };

        // Record transaction
        self.transaction_history
            .lock()
            .unwrap()
            .push(result.clone());

        result
    }

    /// Get transaction statistics
    pub fn get_transaction_stats(&self) -> TransactionStats {
        let history = self.transaction_history.lock().unwrap();
        let total_transactions = history.len();

        if total_transactions == 0 {
            return TransactionStats::default();
        }

        let successful_transactions = history.iter().filter(|tx| tx.success).count();
        let total_gas_used: u64 = history.iter().map(|tx| tx.gas_used).sum();
        let total_execution_time: Duration = history.iter().map(|tx| tx.execution_time).sum();

        TransactionStats {
            total_transactions,
            successful_transactions,
            success_rate: successful_transactions as f64 / total_transactions as f64,
            average_gas_used: total_gas_used / total_transactions as u64,
            average_execution_time: total_execution_time / total_transactions as u32,
            uptime: self.start_time.elapsed(),
        }
    }

    /// Reset transaction history and pools
    pub fn reset(&self) {
        self.transaction_history.lock().unwrap().clear();
        let new_pools = Self::generate_mock_pools(&self.dex_type, &self.config);
        *self.pools.lock().unwrap() = new_pools;
        info!("Reset mock DEX '{}' with fresh state", self.name);
    }

    /// Update configuration and regenerate pools if needed
    pub fn update_config(&mut self, new_config: MockDexConfig) {
        let pool_count_changed = self.config.pool_count != new_config.pool_count;
        self.config = new_config;

        if pool_count_changed {
            let new_pools = Self::generate_mock_pools(&self.dex_type, &self.config);
            *self.pools.lock().unwrap() = new_pools;
            info!(
                "Updated mock DEX '{}' configuration and regenerated pools",
                self.name
            );
        }
    }
}

#[async_trait]
impl DexClient for MockDex {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> anyhow::Result<Quote> {
        // Determine if this is A->B or B->A swap based on input amount context
        // For simplicity, assume A->B swap
        let is_a_to_b = true;

        let (input_reserve, output_reserve, output_token) = if is_a_to_b {
            (
                pool.token_a.reserve,
                pool.token_b.reserve,
                &pool.token_b.symbol,
            )
        } else {
            (
                pool.token_b.reserve,
                pool.token_a.reserve,
                &pool.token_a.symbol,
            )
        };

        if input_reserve == 0 || output_reserve == 0 {
            return Err(anyhow::anyhow!("Pool has zero reserves"));
        }

        // Simple AMM calculation with fees
        let fee_rate =
            pool.fee_numerator.unwrap_or(25) as f64 / pool.fee_denominator.unwrap_or(10000) as f64;
        let input_amount_after_fee = (input_amount as f64) * (1.0 - fee_rate);

        let output_amount = (output_reserve as f64 * input_amount_after_fee)
            / (input_reserve as f64 + input_amount_after_fee);

        // Add some randomness for slippage simulation
        let mut rng = thread_rng();
        let slippage = rng.gen::<f64>() * self.config.slippage_factor;
        let final_output = (output_amount * (1.0 - slippage)) as u64;

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: output_token.clone(),
            input_amount,
            output_amount: final_output,
            dex: self.name.clone(),
            route: vec![pool.address],
            slippage_estimate: Some(slippage * 100.0),
        })
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> anyhow::Result<Instruction> {
        // Return a mock instruction
        Ok(Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![],
            data: vec![0u8; 32], // Mock instruction data
        })
    }

    async fn discover_pools(&self) -> anyhow::Result<Vec<PoolInfo>> {
        let pools = self.pools.lock().unwrap();
        Ok(pools.values().cloned().collect())
    }

    async fn get_swap_instruction_enhanced(
        &self,
        _swap_info: &crate::dex::api::CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        // Stub implementation for testing
        warn!("MockDex::get_swap_instruction_enhanced is a test stub");
        Err(crate::error::ArbError::InstructionError(
            "MockDex enhanced swap instruction not implemented".to_string(),
        ))
    }

    async fn health_check(
        &self,
    ) -> Result<crate::dex::api::DexHealthStatus, crate::error::ArbError> {
        let start_time = std::time::Instant::now();
        let response_time = start_time.elapsed().as_millis() as u64;

        let health_result = crate::dex::api::DexHealthStatus {
            is_healthy: true,
            last_successful_request: Some(start_time),
            error_count: 0,
            response_time_ms: Some(response_time),
            pool_count: Some(self.pools.lock().unwrap().len()),
            status_message: format!(
                "MockDex '{}' health check passed - {} pools available",
                self.name,
                self.pools.lock().unwrap().len()
            ),
        };

        info!("MockDex health check: {}", health_result.status_message);
        Ok(health_result)
    }
}

#[async_trait]
impl PoolDiscoverable for MockDex {
    async fn discover_pools(&self) -> anyhow::Result<Vec<PoolInfo>> {
        DexClient::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, pool_address: Pubkey) -> anyhow::Result<PoolInfo> {
        let pools = self.pools.lock().unwrap();
        pools
            .get(&pool_address)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Pool not found: {}", pool_address))
    }

    fn dex_name(&self) -> &str {
        &self.name
    }
}

/// Transaction statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct TransactionStats {
    pub total_transactions: usize,
    pub successful_transactions: usize,
    pub success_rate: f64,
    pub average_gas_used: u64,
    pub average_execution_time: Duration,
    pub uptime: Duration,
}

/// Mock DEX environment manager
#[derive(Debug)]
pub struct MockDexEnvironment {
    pub dexes: HashMap<String, Arc<MockDex>>,
    pub global_config: MockDexConfig,
}

impl MockDexEnvironment {
    pub fn new(market_condition: MarketCondition) -> Self {
        let global_config = market_condition.to_config();
        let mut dexes = HashMap::new();

        // Create mock DEXes
        let dex_configs = vec![
            ("Orca", DexType::Orca),
            ("Raydium", DexType::Raydium),
            ("Meteora", DexType::Meteora),
            ("Lifinity", DexType::Lifinity),
            ("Whirlpool", DexType::Whirlpool),
        ];

        for (name, dex_type) in dex_configs {
            let mock_dex = Arc::new(MockDex::new(
                name.to_string(),
                dex_type,
                global_config.clone(),
            ));
            dexes.insert(name.to_string(), mock_dex);
        }

        info!(
            "Created mock DEX environment with {} DEXes under {:?} conditions",
            dexes.len(),
            market_condition
        );

        Self {
            dexes,
            global_config,
        }
    }

    /// Get all mock DEXes as DexClient trait objects
    pub fn get_dex_clients(&self) -> Vec<Arc<dyn DexClient>> {
        self.dexes
            .values()
            .map(|dex| dex.clone() as Arc<dyn DexClient>)
            .collect()
    }

    /// Get all mock DEXes as PoolDiscoverable trait objects
    pub fn get_discoverable_clients(&self) -> Vec<Arc<dyn PoolDiscoverable>> {
        self.dexes
            .values()
            .map(|dex| dex.clone() as Arc<dyn PoolDiscoverable>)
            .collect()
    }

    /// Simulate market movements across all DEXes
    pub fn simulate_market_movements(&self) {
        for dex in self.dexes.values() {
            dex.simulate_market_movement();
        }
        debug!("Simulated market movements across all DEXes");
    }

    /// Get comprehensive statistics across all DEXes
    pub fn get_environment_stats(&self) -> EnvironmentStats {
        let mut total_pools = 0;
        let mut total_transactions = 0;
        let mut total_successful = 0;
        let mut total_gas_used = 0;
        let mut total_execution_time = Duration::from_secs(0);

        for dex in self.dexes.values() {
            let pools = dex.pools.lock().unwrap();
            total_pools += pools.len();

            let stats = dex.get_transaction_stats();
            total_transactions += stats.total_transactions;
            total_successful += stats.successful_transactions;
            total_gas_used += stats.average_gas_used * stats.total_transactions as u64;
            total_execution_time += stats.average_execution_time * stats.total_transactions as u32;
        }

        EnvironmentStats {
            total_dexes: self.dexes.len(),
            total_pools,
            total_transactions,
            total_successful_transactions: total_successful,
            overall_success_rate: if total_transactions > 0 {
                total_successful as f64 / total_transactions as f64
            } else {
                0.0
            },
            total_gas_used,
            average_execution_time: if total_transactions > 0 {
                total_execution_time / total_transactions as u32
            } else {
                Duration::from_secs(0)
            },
        }
    }

    /// Reset all DEXes to fresh state
    pub fn reset_all(&self) {
        for dex in self.dexes.values() {
            dex.reset();
        }
        info!("Reset all DEXes in mock environment");
    }

    /// Update market conditions for all DEXes
    pub fn update_market_condition(&mut self, condition: MarketCondition) {
        let new_config = condition.to_config();
        self.global_config = new_config.clone();

        for dex in self.dexes.values_mut() {
            Arc::get_mut(dex).unwrap().update_config(new_config.clone());
        }

        info!("Updated all DEXes to {:?} market conditions", condition);
    }
}

/// Environment-wide statistics
#[derive(Debug, Clone)]
pub struct EnvironmentStats {
    pub total_dexes: usize,
    pub total_pools: usize,
    pub total_transactions: usize,
    pub total_successful_transactions: usize,
    pub overall_success_rate: f64,
    pub total_gas_used: u64,
    pub average_execution_time: Duration,
}

// Helper function to get DEX type name
fn dex_type_name(dex_type: &DexType) -> &str {
    match dex_type {
        DexType::Orca => "Orca",
        DexType::Raydium => "Raydium",
        DexType::Meteora => "Meteora",
        DexType::Lifinity => "Lifinity",
        DexType::Phoenix => "Phoenix",
        DexType::Jupiter => "Jupiter",
        DexType::Whirlpool => "Whirlpool",
        DexType::Unknown(name) => name,
    }
}

/// Mock opportunity generator for testing
pub struct MockOpportunityGenerator {
    pub dex_environment: Arc<MockDexEnvironment>,
}

impl MockOpportunityGenerator {
    pub fn new(dex_environment: Arc<MockDexEnvironment>) -> Self {
        Self { dex_environment }
    }

    /// Generate realistic arbitrage opportunities for testing
    pub async fn generate_opportunities(
        &self,
        count: usize,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut opportunities = Vec::new();
        let mut rng = thread_rng();

        let dex_clients = self.dex_environment.get_dex_clients();

        for i in 0..count {
            // Select random DEXes for the opportunity
            let source_dex = &dex_clients[rng.gen_range(0..dex_clients.len())];
            let target_dex = &dex_clients[rng.gen_range(0..dex_clients.len())];

            // Get pools from both DEXes
            let source_pools = source_dex.discover_pools().await.map_err(|e| {
                ArbError::DexError(format!("Failed to discover source pools: {}", e))
            })?;
            let target_pools = target_dex.discover_pools().await.map_err(|e| {
                ArbError::DexError(format!("Failed to discover target pools: {}", e))
            })?;

            if source_pools.is_empty() || target_pools.is_empty() {
                continue;
            }

            let source_pool = &source_pools[rng.gen_range(0..source_pools.len())];
            let target_pool = &target_pools[rng.gen_range(0..target_pools.len())];

            // Generate opportunity with realistic profit
            let profit_pct = rng.gen_range(0.5..5.0); // 0.5% to 5% profit
            let input_amount = rng.gen_range(100.0..10000.0); // $100 to $10k

            let opportunity = MultiHopArbOpportunity {
                id: format!("mock_opp_{}", i),
                hops: vec![], // Simplified for mock
                total_profit: input_amount * profit_pct / 100.0,
                profit_pct,
                input_token: source_pool.token_a.symbol.clone(),
                output_token: target_pool.token_b.symbol.clone(),
                input_amount,
                expected_output: input_amount * (1.0 + profit_pct / 100.0),
                dex_path: vec![source_pool.dex_type.clone(), target_pool.dex_type.clone()],
                pool_path: vec![source_pool.address, target_pool.address],
                estimated_profit_usd: Some(input_amount * profit_pct / 100.0),
                input_amount_usd: Some(input_amount),
                output_amount_usd: Some(input_amount * (1.0 + profit_pct / 100.0)),
                ..Default::default()
            };

            opportunities.push(opportunity);
        }

        info!(
            "Generated {} mock arbitrage opportunities",
            opportunities.len()
        );
        Ok(opportunities)
    }
}
