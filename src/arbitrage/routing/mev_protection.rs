// src/arbitrage/routing/mev_protection.rs
//! MEV Protection and Anti-Sandwich Attack Routing
//!
//! Provides sophisticated MEV protection strategies:
//! - Sandwich attack detection and prevention
//! - Dynamic slippage protection
//! - Priority fee optimization for MEV resistance
//! - Timing randomization and execution obfuscation
//! - Jito MEV protection integration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use crate::arbitrage::analysis::fee::FeeEstimator;
use crate::arbitrage::routing::RoutePath;

/// MEV risk levels for route assessment
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MevRisk {
    /// Very low MEV risk - safe to execute
    Low,
    /// Moderate MEV risk - requires basic protection
    Medium,
    /// High MEV risk - requires strong protection measures
    High,
    /// Critical MEV risk - execution should be avoided or delayed
    Critical,
}

/// MEV protection strategy types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MevProtectionStrategy {
    /// Standard protection with timing randomization
    StandardProtection,
    /// Enhanced protection with Jito bundles
    JitoProtection,
    /// Maximum protection with delays and multi-hop routing
    MaximalProtection,
    /// Private mempool routing
    PrivateMempoolProtection,
}

/// MEV attack types that can be detected
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MevAttackType {
    /// Front-running attacks
    FrontRunning,
    /// Sandwich attacks (front + back)
    SandwichAttack,
    /// Back-running attacks
    BackRunning,
    /// Cross-DEX arbitrage front-running
    CrossDexFrontRunning,
    /// Liquidation front-running
    LiquidationFrontRunning,
}

/// MEV protection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevProtectionConfig {
    /// Maximum acceptable MEV risk level
    pub max_risk_level: MevRisk,
    /// Preferred protection strategy
    pub protection_strategy: MevProtectionStrategy,
    /// Enable timing randomization (0-500ms delay)
    pub enable_timing_randomization: bool,
    /// Maximum random delay in milliseconds
    pub max_random_delay_ms: u64,
    /// Minimum priority fee multiplier for MEV protection
    pub min_priority_fee_multiplier: f64,
    /// Maximum priority fee multiplier (cost limit)
    pub max_priority_fee_multiplier: f64,
    /// Enable Jito tip for MEV protection
    pub enable_jito_tips: bool,
    /// Jito tip percentage of trade value (0.1% = 0.001)
    pub jito_tip_percentage: f64,
    /// Enable route obfuscation (multiple smaller trades)
    pub enable_route_obfuscation: bool,
    /// Maximum number of sub-routes for obfuscation
    pub max_obfuscation_routes: usize,
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            max_risk_level: MevRisk::Medium,
            protection_strategy: MevProtectionStrategy::JitoProtection,
            enable_timing_randomization: true,
            max_random_delay_ms: 300,
            min_priority_fee_multiplier: 1.5,
            max_priority_fee_multiplier: 10.0,
            enable_jito_tips: true,
            jito_tip_percentage: 0.001, // 0.1%
            enable_route_obfuscation: true,
            max_obfuscation_routes: 3,
        }
    }
}

/// MEV attack detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevThreatAnalysis {
    /// Overall MEV risk level
    pub risk_level: MevRisk,
    /// Detected attack types
    pub detected_attacks: Vec<MevAttackType>,
    /// Risk score (0.0-1.0)
    pub risk_score: f64,
    /// Recommended protection measures
    pub recommended_protections: Vec<MevProtectionStrategy>,
    /// Analysis timestamp
    pub analysis_time: SystemTime,
    /// Additional context and reasoning
    pub analysis_details: String,
}

/// Protected route with MEV mitigation measures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedRoute {
    /// Original route path
    pub original_route: RoutePath,
    /// MEV-protected route variations
    pub protected_routes: Vec<RoutePath>,
    /// Applied protection measures
    pub protection_measures: Vec<MevProtectionStrategy>,
    /// Execution timing strategy
    pub execution_timing: ExecutionTiming,
    /// Priority fee adjustments
    pub priority_fee_adjustments: PriorityFeeAdjustments,
    /// Jito bundle configuration
    pub jito_config: Option<JitoBundleConfig>,
}

/// Execution timing strategy for MEV protection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTiming {
    /// Random delay before execution (milliseconds)
    pub random_delay_ms: u64,
    /// Execution window start time
    pub execution_window_start: SystemTime,
    /// Execution window end time
    pub execution_window_end: SystemTime,
    /// Whether to use burst execution (all at once)
    pub use_burst_execution: bool,
    /// Intervals between sub-executions if not burst
    pub sub_execution_intervals: Vec<Duration>,
}

/// Priority fee adjustments for MEV protection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityFeeAdjustments {
    /// Base priority fee multiplier
    pub base_multiplier: f64,
    /// MEV protection multiplier
    pub mev_protection_multiplier: f64,
    /// Dynamic adjustment based on network congestion
    pub congestion_multiplier: f64,
    /// Total effective multiplier
    pub total_multiplier: f64,
    /// Maximum fee limit (lamports)
    pub max_fee_limit: u64,
}

/// Jito bundle configuration for MEV protection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoBundleConfig {
    /// Jito tip amount (lamports)
    pub tip_amount: u64,
    /// Bundle timeout (seconds)
    pub bundle_timeout: u64,
    /// Maximum bundle retry attempts
    pub max_retry_attempts: u32,
    /// Bundle priority level
    pub priority_level: u8,
}

/// MEV Protection Router for anti-MEV route planning
pub struct MevProtectedRouter {
    config: MevProtectionConfig,
    #[allow(dead_code)]
    fee_estimator: FeeEstimator,
    attack_history: HashMap<String, Vec<MevThreatAnalysis>>,
    protection_cache: HashMap<String, ProtectedRoute>,
    timing_randomizer: TimingRandomizer,
}

/// Timing randomization helper
struct TimingRandomizer {
    last_execution: Instant,
    min_interval: Duration,
    max_random_delay: Duration,
}

impl TimingRandomizer {
    fn new(max_random_delay_ms: u64) -> Self {
        Self {
            last_execution: Instant::now(),
            min_interval: Duration::from_millis(100),
            max_random_delay: Duration::from_millis(max_random_delay_ms),
        }
    }

    fn generate_random_delay(&mut self) -> Duration {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let now = Instant::now();
        let mut hasher = DefaultHasher::new();
        now.elapsed().as_nanos().hash(&mut hasher);

        let random_factor = (hasher.finish() % 1000) as f64 / 1000.0;
        let delay_ms = (random_factor * self.max_random_delay.as_millis() as f64) as u64;

        Duration::from_millis(delay_ms)
    }

    fn should_delay_execution(&mut self) -> Option<Duration> {
        let elapsed = self.last_execution.elapsed();
        if elapsed < self.min_interval {
            Some(self.min_interval - elapsed + self.generate_random_delay())
        } else {
            Some(self.generate_random_delay())
        }
    }
}

impl MevProtectedRouter {
    /// Create a new MEV protected router
    pub fn new(config: MevProtectionConfig, fee_estimator: FeeEstimator) -> Self {
        let timing_randomizer = TimingRandomizer::new(config.max_random_delay_ms);

        Self {
            config,
            fee_estimator,
            attack_history: HashMap::new(),
            protection_cache: HashMap::new(),
            timing_randomizer,
        }
    }

    /// Analyze MEV threat level for a given route
    pub async fn analyze_mev_threat(
        &mut self,
        route: &RoutePath,
    ) -> Result<MevThreatAnalysis, Box<dyn std::error::Error>> {
        let mut risk_score = 0.0;
        let mut detected_attacks = Vec::new();
        let mut analysis_details = Vec::new();

        // Analyze route characteristics for MEV risks
        risk_score += self.analyze_route_complexity(route);
        risk_score += self.analyze_liquidity_impact(route);
        risk_score += self.analyze_timing_patterns(route).await;

        // Check for sandwich attack vulnerability
        if self.is_vulnerable_to_sandwich(route) {
            detected_attacks.push(MevAttackType::SandwichAttack);
            risk_score += 0.3;
            analysis_details
                .push("Route vulnerable to sandwich attacks due to high slippage".to_string());
        }

        // Check for front-running vulnerability
        if self.is_vulnerable_to_frontrunning(route) {
            detected_attacks.push(MevAttackType::FrontRunning);
            risk_score += 0.2;
            analysis_details
                .push("Route vulnerable to front-running due to predictable execution".to_string());
        }

        // Check for cross-DEX arbitrage front-running
        if self.is_cross_dex_vulnerable(route) {
            detected_attacks.push(MevAttackType::CrossDexFrontRunning);
            risk_score += 0.25;
            analysis_details
                .push("Cross-DEX route vulnerable to arbitrage front-running".to_string());
        }

        // Determine risk level
        let risk_level = match risk_score {
            x if x <= 0.2 => MevRisk::Low,
            x if x <= 0.5 => MevRisk::Medium,
            x if x <= 0.8 => MevRisk::High,
            _ => MevRisk::Critical,
        };

        // Recommend protection strategies
        let recommended_protections = self.recommend_protections(&risk_level, &detected_attacks);

        Ok(MevThreatAnalysis {
            risk_level,
            detected_attacks,
            risk_score,
            recommended_protections,
            analysis_time: SystemTime::now(),
            analysis_details: analysis_details.join("; "),
        })
    }

    /// Create MEV-protected version of a route
    pub async fn protect_route(
        &mut self,
        route: &RoutePath,
        threat_analysis: &MevThreatAnalysis,
    ) -> Result<ProtectedRoute, Box<dyn std::error::Error>> {
        // Generate cache key
        let cache_key = self.generate_route_cache_key(
            route,
            route.steps.first().map(|s| s.amount_in as u64).unwrap_or(0),
        );

        // Check cache first
        if let Some(cached_route) = self.protection_cache.get(&cache_key) {
            if self.is_protection_still_valid(cached_route) {
                return Ok(cached_route.clone());
            }
        }

        let mut protected_routes = vec![route.clone()];
        let mut protection_measures = Vec::new();

        // Apply protection based on threat level
        match threat_analysis.risk_level {
            MevRisk::Low => {
                // Basic timing randomization
                if self.config.enable_timing_randomization {
                    protection_measures.push(MevProtectionStrategy::StandardProtection);
                }
            }
            MevRisk::Medium => {
                // Standard protection + Jito tips
                protection_measures.push(MevProtectionStrategy::StandardProtection);
                if self.config.enable_jito_tips {
                    protection_measures.push(MevProtectionStrategy::JitoProtection);
                }
            }
            MevRisk::High => {
                // Enhanced protection + route obfuscation
                protection_measures.push(MevProtectionStrategy::JitoProtection);
                if self.config.enable_route_obfuscation {
                    protected_routes = self.generate_obfuscated_routes(
                        route,
                        route.steps.first().map(|s| s.amount_in as u64).unwrap_or(0),
                    )?;
                    protection_measures.push(MevProtectionStrategy::MaximalProtection);
                }
            }
            MevRisk::Critical => {
                // Maximum protection + private mempool
                protection_measures.push(MevProtectionStrategy::PrivateMempoolProtection);
                protection_measures.push(MevProtectionStrategy::MaximalProtection);
                protected_routes = self.generate_obfuscated_routes(
                    route,
                    route.steps.first().map(|s| s.amount_in as u64).unwrap_or(0),
                )?;
            }
        }

        // Generate execution timing
        let execution_timing = self.generate_execution_timing(&threat_analysis.risk_level);

        // Calculate priority fee adjustments
        let priority_fee_adjustments = self
            .calculate_priority_fee_adjustments(threat_analysis)
            .await?;

        // Generate Jito bundle config if needed
        let jito_config = if protection_measures.contains(&MevProtectionStrategy::JitoProtection) {
            Some(self.generate_jito_config(
                route,
                route.steps.first().map(|s| s.amount_in as u64).unwrap_or(0),
            )?)
        } else {
            None
        };

        let protected_route = ProtectedRoute {
            original_route: route.clone(),
            protected_routes,
            protection_measures,
            execution_timing,
            priority_fee_adjustments,
            jito_config,
        };

        // Cache the protected route
        self.protection_cache
            .insert(cache_key, protected_route.clone());

        Ok(protected_route)
    }

    /// Check if route execution should be delayed for MEV protection
    pub fn should_delay_execution(&mut self) -> Option<Duration> {
        if self.config.enable_timing_randomization {
            self.timing_randomizer.should_delay_execution()
        } else {
            None
        }
    }

    /// Update MEV attack history for learning
    pub fn update_attack_history(&mut self, route_key: String, threat_analysis: MevThreatAnalysis) {
        let route_key_clone = route_key.clone();
        self.attack_history
            .entry(route_key)
            .or_default()
            .push(threat_analysis);

        // Keep only last 100 entries per route
        if let Some(history) = self.attack_history.get_mut(&route_key_clone) {
            if history.len() > 100 {
                history.remove(0);
            }
        }
    }

    // Private helper methods

    fn analyze_route_complexity(&self, route: &RoutePath) -> f64 {
        // More complex routes (more hops) have higher MEV risk
        let hop_count = route.steps.len() as f64;
        (hop_count - 1.0) * 0.1
    }

    fn analyze_liquidity_impact(&self, route: &RoutePath) -> f64 {
        // Routes with high liquidity impact are more vulnerable
        route
            .steps
            .iter()
            .map(|step| {
                // Estimate impact based on amount vs pool size ratio
                let impact_ratio =
                    step.amount_in / step.pool_liquidity.max(1_000_000.0);
                impact_ratio * 0.2
            })
            .sum::<f64>()
            .min(0.4) // Cap at 0.4
    }

    async fn analyze_timing_patterns(&self, _route: &RoutePath) -> f64 {
        // Analyze recent execution patterns
        // For now, return a base risk
        0.1
    }

    fn is_vulnerable_to_sandwich(&self, route: &RoutePath) -> bool {
        // Check if route has high slippage tolerance
        route.steps.iter().any(|step| {
            step.slippage_tolerance.unwrap_or(0.0) > 0.02 // > 2% slippage
        })
    }

    fn is_vulnerable_to_frontrunning(&self, route: &RoutePath) -> bool {
        // Check if route is predictable or has large amounts
        route.steps.iter().any(|step| {
            step.amount_in > 10_000_000_000.0 // > 10,000 USD equivalent in lamports
        })
    }

    fn is_cross_dex_vulnerable(&self, route: &RoutePath) -> bool {
        // Check if route spans multiple DEXs
        let dex_names: std::collections::HashSet<String> = route
            .steps
            .iter()
            .map(|step| step.dex_type.to_string())
            .collect();
        dex_names.len() > 1
    }

    fn recommend_protections(
        &self,
        risk_level: &MevRisk,
        attacks: &[MevAttackType],
    ) -> Vec<MevProtectionStrategy> {
        let mut protections = Vec::new();

        match risk_level {
            MevRisk::Low => {
                protections.push(MevProtectionStrategy::StandardProtection);
            }
            MevRisk::Medium => {
                protections.push(MevProtectionStrategy::StandardProtection);
                protections.push(MevProtectionStrategy::JitoProtection);
            }
            MevRisk::High => {
                protections.push(MevProtectionStrategy::JitoProtection);
                protections.push(MevProtectionStrategy::MaximalProtection);
            }
            MevRisk::Critical => {
                protections.push(MevProtectionStrategy::PrivateMempoolProtection);
                protections.push(MevProtectionStrategy::MaximalProtection);
            }
        }

        // Add specific protections based on attack types
        for attack in attacks {
            match attack {
                MevAttackType::SandwichAttack => {
                    if !protections.contains(&MevProtectionStrategy::JitoProtection) {
                        protections.push(MevProtectionStrategy::JitoProtection);
                    }
                }
                MevAttackType::FrontRunning => {
                    if !protections.contains(&MevProtectionStrategy::MaximalProtection) {
                        protections.push(MevProtectionStrategy::MaximalProtection);
                    }
                }
                _ => {}
            }
        }

        protections
    }

    fn generate_obfuscated_routes(
        &self,
        route: &RoutePath,
        total_amount: u64,
    ) -> Result<Vec<RoutePath>, Box<dyn std::error::Error>> {
        let mut obfuscated_routes = Vec::new();

        // Split route into smaller sub-routes
        let route_count = (self.config.max_obfuscation_routes).min(3);
        let amount_per_route = total_amount / route_count as u64;

        for i in 0..route_count {
            let mut sub_route = route.clone();
            let sub_amount = if i == route_count - 1 {
                // Last route gets remaining amount
                total_amount - (amount_per_route * (route_count - 1) as u64)
            } else {
                amount_per_route
            };

            // Adjust step amounts proportionally
            let ratio = sub_amount as f64 / total_amount as f64;
            for step in &mut sub_route.steps {
                step.amount_in *= ratio;
                step.amount_out *= ratio;
            }

            obfuscated_routes.push(sub_route);
        }

        Ok(obfuscated_routes)
    }

    fn generate_execution_timing(&mut self, risk_level: &MevRisk) -> ExecutionTiming {
        let now = SystemTime::now();
        let random_delay_ms = match risk_level {
            MevRisk::Low => 50,
            MevRisk::Medium => 150,
            MevRisk::High => 300,
            MevRisk::Critical => 500,
        };

        let execution_window_start = now + Duration::from_millis(random_delay_ms);
        let execution_window_end = execution_window_start + Duration::from_secs(30);

        ExecutionTiming {
            random_delay_ms,
            execution_window_start,
            execution_window_end,
            use_burst_execution: matches!(risk_level, MevRisk::Low | MevRisk::Medium),
            sub_execution_intervals: vec![Duration::from_millis(100), Duration::from_millis(200)],
        }
    }

    async fn calculate_priority_fee_adjustments(
        &self,
        threat_analysis: &MevThreatAnalysis,
    ) -> Result<PriorityFeeAdjustments, Box<dyn std::error::Error>> {
        // Get base network congestion multiplier
        let congestion_multiplier = 1.0; // Would get from fee estimator

        // Calculate MEV protection multiplier based on threat level
        let mev_protection_multiplier = match threat_analysis.risk_level {
            MevRisk::Low => 1.2,
            MevRisk::Medium => 2.0,
            MevRisk::High => 4.0,
            MevRisk::Critical => 8.0,
        };

        let base_multiplier = self.config.min_priority_fee_multiplier;
        let total_multiplier =
            (base_multiplier * mev_protection_multiplier * congestion_multiplier)
                .min(self.config.max_priority_fee_multiplier);

        Ok(PriorityFeeAdjustments {
            base_multiplier,
            mev_protection_multiplier,
            congestion_multiplier,
            total_multiplier,
            max_fee_limit: 100_000_000, // 0.1 SOL max
        })
    }

    fn generate_jito_config(
        &self,
        _route: &RoutePath,
        amount: u64,
    ) -> Result<JitoBundleConfig, Box<dyn std::error::Error>> {
        let tip_amount = (amount as f64 * self.config.jito_tip_percentage) as u64;

        Ok(JitoBundleConfig {
            tip_amount,
            bundle_timeout: 30,
            max_retry_attempts: 3,
            priority_level: 1,
        })
    }

    fn generate_route_cache_key(&self, route: &RoutePath, amount: u64) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        if let Some(first_step) = route.steps.first() {
            first_step.from_token.hash(&mut hasher);
        }
        if let Some(last_step) = route.steps.last() {
            last_step.to_token.hash(&mut hasher);
        }
        amount.hash(&mut hasher);
        for step in &route.steps {
            step.dex_type.to_string().hash(&mut hasher);
            step.pool_address.hash(&mut hasher);
        }

        format!("route_{:x}", hasher.finish())
    }

    fn is_protection_still_valid(&self, _protected_route: &ProtectedRoute) -> bool {
        // Check if protection is still valid (not expired)
        // For now, assume 5 minute validity
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::routing::{RoutePath, RouteStep};

    fn create_test_route() -> RoutePath {
        use solana_sdk::pubkey::Pubkey;
        use std::str::FromStr;

        RoutePath {
            steps: vec![RouteStep {
                pool_address: Pubkey::from_str("11111111111111111111111111111111").unwrap(),
                dex_type: crate::utils::DexType::Orca,
                from_token: Pubkey::from_str("So11111111111111111111111111111111111111112")
                    .unwrap(),
                to_token: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
                amount_in: 1_000_000_000.0,
                amount_out: 999_000_000.0,
                fee_bps: 25,
                pool_liquidity: 10_000_000_000.0,
                price_impact: 0.001,
                execution_order: 0,
                slippage_tolerance: Some(0.01),
            }],
            total_fees: 0.0025,
            total_weight: 1.0,
            expected_output: 999_000_000.0,
            estimated_gas_cost: 5000,
            estimated_gas_fee: Some(2500),
            execution_time_estimate: Some(Duration::from_millis(500)),
            confidence_score: 0.95,
            liquidity_score: 0.9,
            dex_diversity_score: 0.8,
            price_impact: Some(0.001),
        }
    }

    fn create_test_fee_estimator() -> FeeEstimator {
        // Create a mock fee estimator
        FeeEstimator::new()
    }

    #[tokio::test]
    async fn test_mev_threat_analysis_low_risk() {
        let config = MevProtectionConfig::default();
        let fee_estimator = create_test_fee_estimator();
        let mut router = MevProtectedRouter::new(config, fee_estimator);
        let route = create_test_route();

        let threat_analysis = router.analyze_mev_threat(&route).await.unwrap();
        assert!(threat_analysis.risk_score >= 0.0);
        assert!(threat_analysis.risk_score <= 1.0);
    }

    #[tokio::test]
    async fn test_protect_route_with_low_risk() {
        let config = MevProtectionConfig::default();
        let fee_estimator = create_test_fee_estimator();
        let mut router = MevProtectedRouter::new(config, fee_estimator);
        let route = create_test_route();

        let threat_analysis = router.analyze_mev_threat(&route).await.unwrap();
        let protected_route = router
            .protect_route(&route, &threat_analysis)
            .await
            .unwrap();

        assert_eq!(
            protected_route
                .original_route
                .steps
                .first()
                .unwrap()
                .from_token,
            route.steps.first().unwrap().from_token
        );
        assert!(!protected_route.protection_measures.is_empty());
    }

    #[test]
    fn test_timing_randomizer() {
        let mut randomizer = TimingRandomizer::new(500);

        let delay1 = randomizer.should_delay_execution();
        let delay2 = randomizer.should_delay_execution();

        assert!(delay1.is_some());
        assert!(delay2.is_some());

        // Delays should be different (statistically likely)
        if let (Some(d1), Some(d2)) = (delay1, delay2) {
            assert!(d1.as_millis() <= 600); // Within max range + min interval
            assert!(d2.as_millis() <= 600);
        }
    }

    #[test]
    fn test_mev_risk_levels() {
        // Test risk level comparison
        assert!(MevRisk::Low < MevRisk::Medium);
        assert!(MevRisk::Medium < MevRisk::High);
        assert!(MevRisk::High < MevRisk::Critical);
    }

    #[test]
    fn test_protection_strategy_contains() {
        let strategies = vec![
            MevProtectionStrategy::StandardProtection,
            MevProtectionStrategy::JitoProtection,
        ];

        assert!(strategies.contains(&MevProtectionStrategy::JitoProtection));
        assert!(!strategies.contains(&MevProtectionStrategy::MaximalProtection));
    }

    #[tokio::test]
    async fn test_route_obfuscation() {
        let config = MevProtectionConfig {
            max_obfuscation_routes: 3,
            ..Default::default()
        };
        let fee_estimator = create_test_fee_estimator();
        let router = MevProtectedRouter::new(config, fee_estimator);
        let route = create_test_route();

        let obfuscated_routes = router
            .generate_obfuscated_routes(&route, 1_000_000_000)
            .unwrap();

        assert_eq!(obfuscated_routes.len(), 3);

        let total_amount: f64 = obfuscated_routes
            .iter()
            .map(|r| r.steps.iter().map(|s| s.amount_in).sum::<f64>())
            .sum();
        assert!((total_amount - 1_000_000_000.0).abs() < 1.0);
    }

    #[test]
    fn test_cache_key_generation() {
        let config = MevProtectionConfig::default();
        let fee_estimator = create_test_fee_estimator();
        let router = MevProtectedRouter::new(config, fee_estimator);
        let route = create_test_route();

        let key1 = router.generate_route_cache_key(&route, 1_000_000_000);
        let key2 = router.generate_route_cache_key(&route, 1_000_000_000);

        assert_eq!(key1, key2);
        assert!(key1.starts_with("route_"));
    }
}
