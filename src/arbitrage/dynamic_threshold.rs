// src/arbitrage/dynamic_threshold.rs
// ArbitrageDetector is not used here after removing DynamicThresholdUpdater
// use crate::arbitrage::detector::ArbitrageDetector; 
use crate::config::settings::Config; // Keep if used by other items, or remove if not.
// Metrics is not used here after removing DynamicThresholdUpdater
// use crate::metrics::Metrics; 
use log::{info}; // Removed warn as it\'s unused after removing DynamicThresholdUpdater
use std::collections::VecDeque;
// Arc and Mutex are not used here after removing DynamicThresholdUpdater
// use std::sync::Arc; 
// use std::time::Duration; // Keep if used by other items, or remove if not.
// use tokio::sync::Mutex; 

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