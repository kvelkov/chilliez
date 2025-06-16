//! Test for Wallet-Jito Integration
//! 
//! This test verifies that the wallet pool and Jito client integration works correctly.

#[cfg(test)]
mod tests {
    use crate::{
        wallet::{WalletJitoIntegration, WalletJitoConfig, WalletPoolConfig},
        arbitrage::JitoClientConfig,
    };
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_sdk::{signature::Keypair, signer::Signer};
    use std::{sync::Arc, time::Duration};

    #[tokio::test]
    async fn test_wallet_jito_integration_creation() {
        // Setup test configuration
        let config = WalletJitoConfig {
            wallet_config: WalletPoolConfig {
                wallet_ttl_secs: 300,
                sweep_threshold_lamports: 10_000,
                fee_reserve_lamports: 5_000,
                max_pool_size: 10,
                auto_cleanup: true,
            },
            jito_config: JitoClientConfig {
                default_tip_lamports: 10_000,
                max_bundle_size: 5,
                submission_timeout: Duration::from_secs(30),
                max_retries: 3,
                dynamic_tips: true,
                ..Default::default()
            },
            auto_sweep_profits: true,
            min_profit_threshold: 50_000,
            optimize_bundles: true,
        };

        // Create test collector wallet
        let collector_keypair = Keypair::new();
        let collector_pubkey = collector_keypair.pubkey();

        // Create test RPC client
        let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));

        // Create integration system
        let integration_system = WalletJitoIntegration::new(
            config,
            collector_pubkey,
            rpc_client,
        );

        // Test that the system was created successfully
        assert_eq!(integration_system.get_success_rate(), 0.0); // No trades yet
        
        // Get initial stats
        let stats = integration_system.get_comprehensive_stats().await;
        assert_eq!(stats.wallet_stats.total_wallets, 0);
        assert_eq!(stats.jito_stats.total_bundles_submitted, 0);
        assert_eq!(stats.integration_stats.total_trades_executed, 0);

        println!("✅ Wallet-Jito integration system created successfully!");
        println!("📊 Initial stats: {:?}", stats.integration_stats);
    }

    #[test]
    fn test_wallet_jito_config_defaults() {
        let config = WalletJitoConfig::default();
        
        // Verify default configuration values
        assert_eq!(config.wallet_config.wallet_ttl_secs, 300);
        assert_eq!(config.wallet_config.sweep_threshold_lamports, 10_000);
        assert_eq!(config.wallet_config.max_pool_size, 100);
        assert!(config.auto_sweep_profits);
        assert_eq!(config.min_profit_threshold, 50_000);
        assert!(config.optimize_bundles);
        
        println!("✅ Default configuration test passed!");
    }

    #[test]
    fn test_error_handling() {
        // Test that insufficient profit is handled correctly
        use crate::error::ArbError;
        
        let error = ArbError::InsufficientProfit("Test error".to_string());
        assert!(!error.is_recoverable());
        assert!(!error.should_retry());
        
        let error = ArbError::TransactionFailed("Network timeout".to_string());
        assert!(error.is_recoverable());
        assert!(error.should_retry());
        
        println!("✅ Error handling test passed!");
    }
}
