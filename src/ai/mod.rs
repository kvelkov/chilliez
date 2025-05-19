use crate::arbitrage::opportunity::ArbOpportunity;
use crate::dex::pool::Pool;
use crate::error::ArbError;

pub trait CryptoDataProvider {
    fn predict_price_movement(
        &self,
        token_mint: &str,
        timeframe_seconds: u64,
    ) -> Result<PriceMovementPrediction, ArbError>;
    fn predict_volatility(&self, token_mint: &str, timeframe_seconds: u64)
        -> Result<f64, ArbError>;
    fn predict_liquidity_change(
        &self,
        pool: &Pool,
        timeframe_seconds: u64,
    ) -> Result<f64, ArbError>;
}

pub trait OpportunityFilter {
    fn filter(&self, opportunities: Vec<ArbOpportunity>) -> Vec<ArbOpportunity>;
    fn rank(&self, opportunities: Vec<ArbOpportunity>) -> Vec<ArbOpportunity>;
}

pub struct PriceMovementPrediction {
    pub direction: PriceDirection,
    pub magnitude_percent: f64,
    pub confidence: f64,
}

pub enum PriceDirection {
    Up,
    Down,
    Sideways,
}

// Simple implementation for testing
pub struct MockDataProvider;

impl CryptoDataProvider for MockDataProvider {
    fn predict_price_movement(
        &self,
        token_mint: &str,
        timeframe_seconds: u64,
    ) -> Result<PriceMovementPrediction, ArbError> {
        // Mock implementation
        Ok(PriceMovementPrediction {
            direction: PriceDirection::Up,
            magnitude_percent: 0.5,
            confidence: 0.7,
        })
    }

    fn predict_volatility(
        &self,
        token_mint: &str,
        timeframe_seconds: u64,
    ) -> Result<f64, ArbError> {
        // Mock implementation
        Ok(0.02)
    }

    fn predict_liquidity_change(
        &self,
        pool: &Pool,
        timeframe_seconds: u64,
    ) -> Result<f64, ArbError> {
        // Mock implementation
        Ok(0.0)
    }
}
