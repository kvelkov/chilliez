// src/arbitrage/dynamic_threshold.rs
use crate::arbitrage::detector::ArbitrageDetector; // For engine_detector type
use crate::config::settings::Config;
use crate::metrics::Metrics;
use log::{debug, info, warn};
use std::collections::VecDeque;
use crate::websocket::CryptoDataProvider; // Added for price_provider
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

const DEFAULT_VOLATILITY_WINDOW: usize = 20;
const MIN_THRESHOLD_PCT: f64 = 0.01; // Minimum 0.01%
const MAX_THRESHOLD_PCT: f64 = 5.0; // Maximum 5%
const BASE_THRESHOLD_ADJUSTMENT_FACTOR: f64 = 0.5; // How much volatility impacts threshold

#[derive(Debug, Clone)]
pub struct VolatilityTracker {
    price_history: VecDeque<f64>,
    window_size: usize,
    current_volatility: f64,
}

impl VolatilityTracker {
    pub fn new(window_size: usize) -> Self {
        Self {
            price_history: VecDeque::with_capacity(window_size),
            window_size,
            current_volatility: 0.0,
        }
    }

    pub fn add_price(&mut self, price: f64) {
        if price <= 0.0 {
            warn!("Attempted to add non-positive price to VolatilityTracker: {}. Ignoring.", price);
            return;
        }
        if self.price_history.len() == self.window_size {
            self.price_history.pop_front();
        }
        self.price_history.push_back(price);
        self.calculate_volatility();
        debug!("VolatilityTracker: Added price {}, new volatility: {:.6}", price, self.current_volatility);
    }

    fn calculate_volatility(&mut self) {
        if self.price_history.len() < 2 {
            self.current_volatility = 0.0;
            return;
        }
        let prices: Vec<f64> = self.price_history.iter().cloned().collect();
        let mean = prices.iter().sum::<f64>() / prices.len() as f64;
        let variance = prices.iter().map(|price| (price - mean).powi(2)).sum::<f64>() / prices.len() as f64;
        self.current_volatility = variance.sqrt() / mean; // Relative volatility (coefficient of variation)
    }

    pub fn volatility(&self) -> f64 {
        self.current_volatility
    }

    // TODO: This method is currently unused in the main program flow.
    // It's kept for potential future use in debugging, detailed logging, or advanced volatility analysis.
    pub fn _get_price_history(&self) -> &VecDeque<f64> {
        &self.price_history
    }
}

pub struct DynamicThresholdUpdater {
    volatility_tracker: VolatilityTracker,
    config: Arc<Config>,
}

impl DynamicThresholdUpdater {
    pub fn new(config: Arc<Config>) -> Self {
        let window_size = config.volatility_tracker_window.unwrap_or(DEFAULT_VOLATILITY_WINDOW);
        Self {
            volatility_tracker: VolatilityTracker::new(window_size),
            config: Arc::clone(&config),
        }
    }

    // This method is now synchronous
    pub fn add_price_observation(&mut self, price: f64) {
        if price <= 0.0 {
            warn!("Attempted to add non-positive price observation: {}. Ignoring.", price);
            return;
        }
        // Directly access volatility_tracker's method
        self.volatility_tracker.add_price(price);
        debug!(
            "Added price observation: {}. History size: {}. Volatility: {:.6}",
            price,
            self.volatility_tracker.price_history.len(),
            self.volatility_tracker.volatility()
        );
    }

    // This method is now synchronous
    pub fn get_current_threshold(&self) -> f64 {
        let volatility = self.volatility_tracker.volatility(); // Directly access
        let base_threshold = self.config.min_profit_pct * 100.0;

        let adjustment_factor = 1.0 + (volatility * BASE_THRESHOLD_ADJUSTMENT_FACTOR * 10.0);
        let mut new_threshold = base_threshold * adjustment_factor;

        new_threshold = new_threshold.max(MIN_THRESHOLD_PCT).min(MAX_THRESHOLD_PCT);

        info!(
            "Calculated new dynamic threshold: {:.4}% (Base: {:.4}%, Volatility: {:.6}, AdjustmentFactor: {:.4})",
            new_threshold, base_threshold, volatility, adjustment_factor
        );
        new_threshold
    }

    pub async fn start_monitoring_task(
        updater_arc: Arc<Mutex<DynamicThresholdUpdater>>,
        metrics: Arc<Mutex<Metrics>>,
        engine_detector: Arc<Mutex<ArbitrageDetector>>,
        price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>, // Added price_provider
    ) {
        info!("DynamicThresholdUpdater monitoring task started.");
        let (update_interval_secs, health_check_symbol) = {
            let updater_guard = updater_arc.lock().await;
            (
                updater_guard.config.dynamic_threshold_update_interval_secs.unwrap_or(300),
                updater_guard.config.health_check_token_symbol.clone().unwrap_or_default(),
            )
        };
        let mut interval = tokio::time::interval(Duration::from_secs(update_interval_secs));

        loop {
            interval.tick().await;
            debug!("DynamicThresholdUpdater task tick.");

            // Fetch price and update volatility tracker
            if let Some(provider) = &price_provider {
                if !health_check_symbol.is_empty() {
                    // Assuming provider.get_price now returns Option<f64> directly
                    let price_option = provider.get_price(&health_check_symbol).await;

                    match price_option {
                        Some(price) => { // Successfully got a price
                            let mut updater_guard = updater_arc.lock().await;
                            updater_guard.add_price_observation(price); // This makes add_price_observation live
                        }
                        None => { // Original Result was an Err, or get_price itself returned None if it were Option-based
                            warn!("[DynamicThresholdUpdater] Failed to get price for {} or price was not available.", health_check_symbol);
                        }
                    }
                }
            }

            let new_threshold_pct = {
                let updater = updater_arc.lock().await;
                updater.get_current_threshold() // Call synchronous method
            };

            {
                let mut detector_guard = engine_detector.lock().await;
                detector_guard.set_min_profit_threshold(new_threshold_pct);
            }

            {
                let mut metrics_guard = metrics.lock().await;
                metrics_guard.log_dynamic_threshold_update(new_threshold_pct / 100.0);
            }

            info!(
                "DynamicThresholdUpdater Task: Applied new min profit threshold: {:.4}% to detector.",
                new_threshold_pct
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::Config;
    use tokio;
    use std::collections::HashMap;

    fn create_test_config(vol_window: Option<usize>, base_profit_pct: f64) -> Arc<Config> {
        Arc::new(Config {
            min_profit_pct: base_profit_pct / 100.0,
            dynamic_threshold_update_interval_secs: Some(60),
            rpc_url: "test_rpc_url".to_string(),
            rpc_url_secondary: None,
            ws_url: "test_ws_url".to_string(),
            wallet_path: "test_wallet_path".to_string(),
            trader_wallet_keypair_path: Some("test_keypair_path".to_string()),
            default_priority_fee_lamports: 5000,
            health_check_interval_secs: Some(60),
            health_check_token_symbol: Some("SOL/USDC".to_string()),
            degradation_profit_factor: Some(1.5),
            max_ws_reconnect_attempts: Some(5),
            enable_fixed_input_arb_detection: false,
            fixed_input_arb_amount: None,
            sol_price_usd: Some(150.0),
            min_profit_usd_threshold: Some(0.05),
            max_slippage_pct: 0.005,
            max_tx_fee_lamports_for_acceptance: Some(100000),
            
            transaction_priority_fee_lamports: Some(10000).unwrap_or(5000),
            pool_refresh_interval_secs: Some(10).unwrap_or(30),
            redis_url: None::<String>.unwrap_or_else(|| "".to_string()),
            redis_default_ttl_secs: Some(3600).unwrap_or(300),
            dex_quote_cache_ttl_secs: Some(HashMap::new()), // Wrapped in Some()
            volatility_tracker_window: vol_window,
            volatility_threshold_factor: Some(0.5),
            max_risk_score_for_acceptance: Some(0.75),
            max_hops: Some(3),
            max_pools_per_hop: Some(5),
            max_concurrent_executions: Some(10),
            execution_timeout_secs: Some(30),
            transaction_cu_limit: Some(400_000), // Add missing field
            simulation_mode: Some(false).unwrap_or(true),
            paper_trading: Some(false).unwrap_or(false),
            metrics_log_path: None,
            rpc_url_backup: None,
            rpc_max_retries: Some(3),
            rpc_retry_delay_ms: Some(1000),
            max_transaction_timeout_seconds: Some(120),
            ws_update_channel_size: Some(1024),
            congestion_update_interval_secs: Some(15),
            cycle_interval_seconds: Some(5),
            pool_read_timeout_ms: Some(1000),
            log_level: Some("info".to_string()),
        })
    }

    #[tokio::test]
    async fn test_volatility_tracker_basic() {
        let mut tracker = VolatilityTracker::new(5);
        let prices = [1.0, 1.05, 0.95, 1.1, 1.0];
        for p in prices {
            tracker.add_price(p);
        }
        let vol = tracker.volatility();
        assert!(vol > 0.0, "Volatility should be positive after adding varied prices.");
        println!("Calculated Volatility: {}", vol);

        let mut tracker_stable = VolatilityTracker::new(5);
        let stable_prices = [1.0, 1.0, 1.0, 1.0, 1.0];
        for p in stable_prices {
            tracker_stable.add_price(p);
        }
        let vol_stable = tracker_stable.volatility();
        assert!((vol_stable - 0.0).abs() < 1e-9, "Volatility should be zero for stable prices.");
        println!("Calculated Stable Volatility: {}", vol_stable);
    }

    #[tokio::test]
    async fn test_dynamic_threshold_adjustment() {
        let config = create_test_config(Some(5), 0.1); // 0.1% base profit
        let mut updater = DynamicThresholdUpdater::new(Arc::clone(&config));

        // Initial threshold should be based on config's min_profit_pct
        let initial_threshold = updater.get_current_threshold();
        assert!((initial_threshold - 0.1).abs() < 1e-9, "Initial threshold should be base profit percentage");

        // Add some stable prices
        updater.add_price_observation(100.0);
        updater.add_price_observation(100.1);
        updater.add_price_observation(100.0);
        updater.add_price_observation(99.9);
        updater.add_price_observation(100.0);

        let threshold_low_vol = updater.get_current_threshold();
        info!("Threshold with low volatility: {:.4}%", threshold_low_vol);
        // With low volatility, threshold should be close to base
        assert!(threshold_low_vol >= 0.1 && threshold_low_vol < 0.15, "Threshold with low volatility ({:.4}%) should be slightly above base (0.1%) but not too high", threshold_low_vol);

        // Add volatile prices
        updater.add_price_observation(120.0); // Large jump
        updater.add_price_observation(80.0);  // Large drop
        updater.add_price_observation(110.0);
        updater.add_price_observation(90.0);
        updater.add_price_observation(100.0);

        let threshold_high_vol = updater.get_current_threshold();
        info!("Threshold with high volatility: {:.4}%", threshold_high_vol);
        // With high volatility, threshold should increase significantly
        assert!(threshold_high_vol > threshold_low_vol, "Threshold with high volatility ({:.4}%) should be greater than with low volatility ({:.4}%)", threshold_high_vol, threshold_low_vol);
        assert!(threshold_high_vol > 0.15, "Threshold with high volatility ({:.4}%) should be significantly above base (0.1%)", threshold_high_vol);

        // Test clamping
        // Simulate extreme volatility to push threshold towards max
        let mut extreme_vol_updater = DynamicThresholdUpdater::new(create_test_config(Some(3), 0.01)); // Very low base
        extreme_vol_updater.add_price_observation(1.0);
        extreme_vol_updater.add_price_observation(1000.0);
        extreme_vol_updater.add_price_observation(1.0);
        let threshold_extreme_vol = extreme_vol_updater.get_current_threshold();
        assert!((threshold_extreme_vol - MAX_THRESHOLD_PCT).abs() < 1e-9 || threshold_extreme_vol < MAX_THRESHOLD_PCT, "Threshold with extreme volatility should be clamped at MAX_THRESHOLD_PCT or lower");

        // Simulate zero volatility (all same prices) to push towards min
        let mut zero_vol_updater = DynamicThresholdUpdater::new(create_test_config(Some(5), 10.0)); // High base, should be pulled down by clamping
        for _ in 0..5 {
            zero_vol_updater.add_price_observation(100.0);
        }
        let threshold_zero_vol = zero_vol_updater.get_current_threshold();
         // If base is high, it might be clamped by MAX_THRESHOLD_PCT even with zero vol.
         // If base is low, it should be clamped by MIN_THRESHOLD_PCT.
         // The logic is base * (1 + vol_factor), then clamped. If vol is 0, it's base, then clamped.
        let expected_clamped_base = 10.0f64.max(MIN_THRESHOLD_PCT).min(MAX_THRESHOLD_PCT);
        assert!((threshold_zero_vol - expected_clamped_base).abs() < 1e-9, "Threshold with zero volatility ({:.4}%) should be the base threshold clamped ({:.4}%)", threshold_zero_vol, expected_clamped_base);
    }

    #[tokio::test]
    async fn test_add_price_observation_updates_volatility() {
        let config = create_test_config(Some(5), 0.1);
        let mut updater = DynamicThresholdUpdater::new(Arc::clone(&config));

        updater.add_price_observation(100.0);
        updater.add_price_observation(105.0);
        updater.add_price_observation(95.0);

        let volatility_initial = updater.volatility_tracker.volatility(); // Direct access
        assert!(volatility_initial > 0.0, "Initial volatility should be greater than 0 after adding prices");

        updater.add_price_observation(110.0);
        updater.add_price_observation(90.0);

        let volatility_updated = updater.volatility_tracker.volatility(); // Direct access
        assert!(volatility_updated > volatility_initial, "Volatility should change with more price observations");

        info!("Initial volatility: {:.6}, Updated volatility: {:.6}", volatility_initial, volatility_updated);
    }
}
