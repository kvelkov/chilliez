// src/utils/pool_validation.rs
//! Pool validation utilities extracted from legacy pool_discovery module

use crate::utils::PoolInfo;
use crate::solana::rpc::SolanaRpcClient;
use anyhow::Result;
use log::warn;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for pool validation
#[derive(Debug, Clone)]
pub struct PoolValidationConfig {
    /// Maximum age of pool data before it's considered stale (in seconds)
    pub max_pool_age_secs: u64,
    /// Whether to skip pools with zero liquidity
    pub skip_empty_pools: bool,
    /// Minimum reserve amount required for a pool to be considered active
    pub min_reserve_threshold: u64,
    /// Whether to verify pools exist on-chain (expensive)
    pub verify_on_chain: bool,
}

impl Default for PoolValidationConfig {
    fn default() -> Self {
        Self {
            max_pool_age_secs: 300, // 5 minutes
            skip_empty_pools: true,
            min_reserve_threshold: 1000, // Minimum 1000 tokens in reserve
            verify_on_chain: false, // Disabled by default as it's expensive
        }
    }
}

/// Filters and validates discovered pools based on configuration criteria
/// 
/// This function validates pools for:
/// - Minimum reserve requirements (respects skip_empty_pools and min_reserve_threshold)
/// - Data freshness (uses max_pool_age_secs for timestamp checking)
/// - Required field validation
/// - Optional on-chain verification (respects verify_on_chain setting)
pub async fn validate_pools(
    pools: &[PoolInfo], 
    config: &PoolValidationConfig,
    rpc_client: Option<&Arc<SolanaRpcClient>>,
) -> Result<Vec<PoolInfo>> {
    let mut valid_pools = Vec::with_capacity(pools.len());
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for pool in pools {
        // Check if pool meets minimum reserve requirements (using config)
        if config.skip_empty_pools {
            if pool.token_a.reserve < config.min_reserve_threshold ||
               pool.token_b.reserve < config.min_reserve_threshold {
                warn!("Pool {} validation failed: reserves below threshold (A: {}, B: {}, min: {})", 
                      pool.address, pool.token_a.reserve, pool.token_b.reserve, config.min_reserve_threshold);
                continue;
            }
        }
        
        // Check if pool data is fresh enough (using config)
        if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
            warn!("Pool {} validation failed: data too old ({} seconds old, max: {})", 
                  pool.address, current_time.saturating_sub(pool.last_update_timestamp), config.max_pool_age_secs);
            continue;
        }
        
        // Additional validation: ensure required fields are present
        if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
            warn!("Pool {} validation failed: invalid token mints", pool.address);
            continue;
        }
        
        // Optional: Use RPC client to verify the pool exists on-chain (using config)
        if config.verify_on_chain {
            if let Some(rpc) = rpc_client {
                match rpc.primary_client.get_account(&pool.address).await {
                    Ok(_) => {
                        log::debug!("Pool {} on-chain verification passed", pool.address);
                    }
                    Err(e) => {
                        warn!("Pool {} validation failed: on-chain verification failed: {}", pool.address, e);
                        continue;
                    }
                }
            } else {
                warn!("Pool {} validation skipped: on-chain verification requested but no RPC client provided", pool.address);
                // Continue anyway since RPC client wasn't provided
            }
        }
        
        valid_pools.push(pool.clone());
    }
    
    log::info!("Pool validation completed: {}/{} pools passed validation", valid_pools.len(), pools.len());
    Ok(valid_pools)
}

/// Quick validation for pools without RPC calls
/// Useful for basic filtering before expensive operations
pub fn validate_pools_basic(pools: &[PoolInfo], config: &PoolValidationConfig) -> Vec<PoolInfo> {
    let mut valid_pools = Vec::with_capacity(pools.len());
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for pool in pools {
        // Check if pool meets minimum reserve requirements
        if config.skip_empty_pools {
            if pool.token_a.reserve < config.min_reserve_threshold ||
               pool.token_b.reserve < config.min_reserve_threshold {
                warn!("Pool {} skipped: reserves below threshold (A: {}, B: {}, min: {})", 
                      pool.address, pool.token_a.reserve, pool.token_b.reserve, config.min_reserve_threshold);
                continue;
            }
        }
        
        // Check if pool data is fresh enough
        if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
            warn!("Pool {} skipped: data too old ({} seconds old, max: {})", 
                  pool.address, current_time.saturating_sub(pool.last_update_timestamp), config.max_pool_age_secs);
            continue;
        }
        
        // Basic field validation
        if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
            warn!("Pool {} skipped: invalid token mints", pool.address);
            continue;
        }
        
        valid_pools.push(pool.clone());
    }
    
    valid_pools
}

/// Validates a single pool for real-time webhook processing
/// Returns true if pool passes all validation criteria
pub fn validate_single_pool(pool: &PoolInfo, config: &PoolValidationConfig) -> bool {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    // Check reserve requirements
    if config.skip_empty_pools {
        if pool.token_a.reserve < config.min_reserve_threshold ||
           pool.token_b.reserve < config.min_reserve_threshold {
            return false;
        }
    }
    
    // Check data freshness
    if current_time.saturating_sub(pool.last_update_timestamp) > config.max_pool_age_secs {
        return false;
    }
    
    // Check basic field validity
    if pool.token_a.mint == Pubkey::default() || pool.token_b.mint == Pubkey::default() {
        return false;
    }
    
    true
}

/// Validates pools for webhook integration with detailed logging
/// This function is designed to be called from the webhook processing pipeline
pub fn validate_pools_for_webhook(pools: &[PoolInfo], config: &PoolValidationConfig) -> (Vec<PoolInfo>, usize) {
    let total_input = pools.len();
    let valid_pools = validate_pools_basic(pools, config);
    let filtered_count = total_input - valid_pools.len();
    
    if filtered_count > 0 {
        warn!("Pool validation filtered out {} of {} pools ({:.1}% rejection rate)", 
              filtered_count, total_input, (filtered_count as f64 / total_input as f64) * 100.0);
    } else {
        log::debug!("All {} pools passed validation", total_input);
    }
    
    (valid_pools, filtered_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{PoolToken, DexType};

    fn create_test_pool(reserve_a: u64, reserve_b: u64) -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: reserve_b,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            dex_type: DexType::Raydium,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
        }
    }

    #[test]
    fn test_basic_validation() {
        let config = PoolValidationConfig::default();
        
        let pools = vec![
            create_test_pool(100000, 100000), // Valid - above threshold
            create_test_pool(100, 100000),    // Invalid - low reserve A
            create_test_pool(100000, 100),    // Invalid - low reserve B
            create_test_pool(500, 500),       // Invalid - both reserves below threshold
        ];
        
        let valid_pools = validate_pools_basic(&pools, &config);
        assert_eq!(valid_pools.len(), 1, "Only one pool should pass validation");
        
        // Test with different config
        let lenient_config = PoolValidationConfig {
            max_pool_age_secs: 600,
            skip_empty_pools: false, // Don't skip empty pools
            min_reserve_threshold: 50, // Lower threshold
            verify_on_chain: false,
        };
        
        let valid_pools_lenient = validate_pools_basic(&pools, &lenient_config);
        assert_eq!(valid_pools_lenient.len(), 4, "All pools should pass with lenient config");
    }
    
    #[test]
    fn test_single_pool_validation() {
        let config = PoolValidationConfig::default();
        let valid_pool = create_test_pool(100000, 100000);
        let invalid_pool = create_test_pool(100, 100);
        
        assert!(validate_single_pool(&valid_pool, &config), "Valid pool should pass");
        assert!(!validate_single_pool(&invalid_pool, &config), "Invalid pool should fail");
    }
    
    #[test]
    fn test_webhook_validation_reporting() {
        let config = PoolValidationConfig::default();
        let pools = vec![
            create_test_pool(100000, 100000), // Valid
            create_test_pool(100, 100000),    // Invalid
            create_test_pool(100000, 100),    // Invalid
        ];
        
        let (valid_pools, filtered_count) = validate_pools_for_webhook(&pools, &config);
        assert_eq!(valid_pools.len(), 1, "One pool should be valid");
        assert_eq!(filtered_count, 2, "Two pools should be filtered out");
    }
}
