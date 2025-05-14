/// Simple rolling volatility tracker and dynamic threshold calculator
use std::collections::VecDeque;

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
pub fn recommended_min_profit_threshold(volatility: f64) -> f64 {
    // Example scaling: up to 2x base threshold as volatility rises
    let base = 0.01; // 1% base
    let boost = (volatility * 10.0).min(0.02);
    base + boost
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
        let threshold = recommended_min_profit_threshold(vol);
        assert!(vol > 0.0);
        assert!(threshold >= 0.01);
    }
}
