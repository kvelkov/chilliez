// src/arbitrage/dynamic_threshold.rs
use crate::arbitrage::detector::ArbitrageDetector; // No ArbitrageEngine needed here directly
use crate::config::settings::Config;
use crate::metrics::Metrics;
use log::{info, warn}; // Added warn and info
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex; // Using tokio's Mutex

pub struct VolatilityTracker {
    window: VecDeque<f64>,
    max_samples: usize,
}

impl VolatilityTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn add_price(&mut self, price: f64) {
        if self.window.len() == self.max_samples {
            self.window.pop_front();
        }
        self.window.push_back(price);
    }

    pub fn volatility(&self) -> f64 {
        let n = self.window.len();
        if n < 2 {
            return 0.0;
        }
        let mean = self.window.iter().copied().sum::<f64>() / n as f64;
        let var_sum = self.window.iter().map(|p| (p - mean).powi(2)).sum::<f64>();
        if n - 1 == 0 { return 0.0; } // Avoid division by zero if n=1 somehow (though guarded by n<2)
        let var = var_sum / (n as f64 - 1.0);
        var.sqrt()
    }
}

pub fn recommend_min_profit_threshold(volatility: f64, base_threshold: f64, volatility_factor: f64) -> f64 {
    let factor = volatility_factor.max(0.0);
    let dynamic_adjustment = volatility * factor;
    // Ensure the threshold doesn't become excessively high or negative
    (base_threshold + dynamic_adjustment).max(0.0001) // Minimum 0.01% e.g.
}

pub struct DynamicThresholdUpdater {
    // min_profit_threshold: f64, // This state is now primarily managed by ArbitrageEngine/Detector
    volatility_factor: f64,
    update_interval: Duration,
    // Instead of holding detector and metrics directly, this task could communicate updates
    // or receive necessary components if it were part of a larger actor system.
    // For simplicity, if this is spawned by ArbitrageEngine, it can update engine's threshold.
    // However, the original code had it directly.
    detector: Arc<Mutex<ArbitrageDetector>>,
    metrics: Arc<Mutex<Metrics>>,
    config: Arc<Config>, // To access base_threshold and volatility_factor
    // price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>, // If fetching price directly
}

impl DynamicThresholdUpdater {
    pub fn new(
        config: Arc<Config>,
        detector: Arc<Mutex<ArbitrageDetector>>,
        metrics: Arc<Mutex<Metrics>>,
        // price_provider: Option<Arc<dyn CryptoDataProvider + Send + Sync>>,
    ) -> Self {
        // The unwrap_or calls are correct if config fields are Options.
        // The compiler errors suggest they are not Options in the user's build.
        // Assuming the provided Config struct where these are Options is the correct one.
        let volatility_factor_val = config.volatility_threshold_factor.unwrap_or(0.5);
        let update_interval_secs = config.dynamic_threshold_update_interval_secs.unwrap_or(300);

        Self {
            volatility_factor: volatility_factor_val,
            update_interval: Duration::from_secs(update_interval_secs),
            detector,
            metrics,
            config,
            // price_provider,
        }
    }

    // This run method is designed to be spawned as a task by ArbitrageEngine or main.
    // It would need a way to update the engine's shared min_profit_threshold.
    // The version in ArbitrageEngine::run_dynamic_threshold_updates is more integrated.
    // This standalone struct might be for a different architectural approach or testing.
    #[allow(dead_code)]
    pub async fn run_standalone_updater(&self) { // Renamed to avoid confusion with engine's method
        let mut vol_tracker = VolatilityTracker::new(self.config.volatility_tracker_window.unwrap_or(20));
        loop {
            tokio::time::sleep(self.update_interval).await;
            info!("DynamicThresholdUpdater: Updating dynamic profit threshold...");

            // In a standalone updater, fetching price would require its own provider.
            // For now, using a placeholder.
            let current_price_of_major_asset = 1.0; // Placeholder
            vol_tracker.add_price(current_price_of_major_asset);
            let historical_volatility = vol_tracker.volatility();

            let base_threshold = self.config.min_profit_pct; // This is fractional
            // This uses the factor from the struct, which was initialized from config or default
            // let volatility_factor = self.volatility_factor;
            // The line from original error:
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.1); // This is correct if config field is Option.

            let new_threshold_fractional = recommend_min_profit_threshold(historical_volatility, base_threshold, volatility_factor);
            let new_threshold_pct = new_threshold_fractional * 100.0;

            // Update the detector's threshold directly
            self.detector.lock().await.set_min_profit_threshold(new_threshold_pct);

            info!("DynamicThresholdUpdater: Recommended new min profit threshold: {:.4}% (Volatility: {:.6})", new_threshold_pct, historical_volatility);
            
            self.metrics.lock().await.log_dynamic_threshold_update(new_threshold_fractional);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_volatility_and_threshold() {
        let mut tracker = VolatilityTracker::new(5);
        let prices = [1.0, 1.05, 0.95, 1.1, 1.0];
        for p in prices {
            tracker.add_price(p);
        }
        let vol = tracker.volatility();
        // recommend_min_profit_threshold expects base_threshold as a fraction (e.g., 0.01 for 1%)
        let base_threshold_fractional = 0.01; // 1%
        let volatility_factor = 0.5;
        let threshold = recommend_min_profit_threshold(vol, base_threshold_fractional, volatility_factor);
        assert!(vol > 0.0);
        assert!(threshold >= base_threshold_fractional); // Threshold should be at least the base
        println!("Volatility: {}, Base Threshold (frac): {}, Recommended Threshold (frac): {}", vol, base_threshold_fractional, threshold);
    }
}