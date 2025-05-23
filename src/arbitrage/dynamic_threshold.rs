/// Simple rolling volatility tracker and dynamic threshold calculator
use std::collections::VecDeque;
use crate::arbitrage::detector::ArbitrageDetector;
use crate::config::settings::Config;
use crate::metrics::Metrics;
use std::sync::{Arc, Mutex as StdMutex}; // Aliased Mutex to StdMutex
use std::time::Duration;
use tokio::sync::Mutex; // Added tokio Mutex

/// Tracks recent prices and computes rolling volatility (std deviation).
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

    /// Returns sample std deviation (volatility).
    pub fn volatility(&self) -> f64 {
        let n = self.window.len();
        if n < 2 {
            return 0.0;
        }
        let mean = self.window.iter().copied().sum::<f64>() / n as f64;
        let var = self.window.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
        var.sqrt()
    }
}

/// Calculate min_profit_threshold based on measured volatility
pub fn recommend_min_profit_threshold(volatility: f64, base_threshold: f64, volatility_factor: f64) -> f64 {
    // Ensure volatility_factor is positive to avoid issues.
    let factor = volatility_factor.max(0.0); // Ensure factor is not negative
    let dynamic_adjustment = volatility * factor;
    base_threshold + dynamic_adjustment
}

pub struct DynamicThresholdUpdater {
    min_profit_threshold: f64,
    volatility_factor: f64,
    update_interval: Duration,
    detector: Arc<Mutex<ArbitrageDetector>>, // Changed to tokio::sync::Mutex
    metrics: Arc<Mutex<Metrics>>,            // Changed to tokio::sync::Mutex
    config: Arc<Config>,
}

impl DynamicThresholdUpdater {
    pub fn new(
        config: Arc<Config>,
        detector: Arc<Mutex<ArbitrageDetector>>, // Changed to tokio::sync::Mutex
        metrics: Arc<Mutex<Metrics>>,            // Changed to tokio::sync::Mutex
    ) -> Self {
        Self {
            min_profit_threshold: config.min_profit_pct, // Initialize from config
            volatility_factor: config.volatility_threshold_factor.unwrap_or(0.5), // Default if not in config
            update_interval: Duration::from_secs(config.dynamic_threshold_update_interval_secs.unwrap_or(300)),
            detector,
            metrics,
            config,
        }
    }

    pub async fn run(&self) {
        loop {
            tokio::time::sleep(self.update_interval).await;
            info!("Updating dynamic profit threshold based on volatility...");

            let detector_guard = self.detector.lock().await; // Use tokio::sync::Mutex lock
            // let historical_volatility = detector_guard.calculate_historical_volatility(); // Assuming this method exists
            let historical_volatility = 0.05; // Placeholder value
            drop(detector_guard);

            let base_threshold = self.config.min_profit_pct;
            let volatility_factor = self.config.volatility_threshold_factor.unwrap_or(0.1);

            let new_threshold = recommend_min_profit_threshold(historical_volatility, base_threshold, volatility_factor);
            
            // Update the threshold in ArbitrageEngine or wherever it's stored
            // This might involve sending the new_threshold through a channel or calling a method on an Arc<RwLock<f64>>
            info!("Recommended new min profit threshold: {:.4}%", new_threshold * 100.0);
            
            let metrics_guard = self.metrics.lock().await; // Use tokio::sync::Mutex lock
            // metrics_guard.log_dynamic_threshold_update(new_threshold).await; // Assuming this method exists
            drop(metrics_guard);
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
        let threshold = recommend_min_profit_threshold(volatility, vol, 0.5);
        assert!(vol > 0.0);
        assert!(threshold >= 0.01);
    }
}
