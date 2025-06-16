// examples/event_driven_balance_demo.rs
//! Demo of event-driven balance monitoring integrated with webhook system

use anyhow::Result;
use solana_arb_bot::{
    solana::{
        EventDrivenBalanceMonitor, 
        EventDrivenBalanceConfig, 
        IntegratedBalanceConfigBuilder,
        BalanceMonitorConfig,
    },
    webhooks::{
        processor::PoolUpdateProcessor,
    },
};
use log::{info};
use solana_sdk::pubkey::Pubkey;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("🚀 Starting Event-Driven Balance Monitoring Demo");

    // Demo 1: Basic event-driven balance monitoring
    demo_basic_event_driven_balance().await?;

    // Demo 2: Integrated balance monitoring with webhooks
    demo_integrated_balance_monitoring().await?;

    // Demo 3: Real-time balance tracking simulation
    demo_real_time_balance_tracking().await?;

    info!("✅ Event-driven balance monitoring demos completed successfully");
    Ok(())
}

/// Demo 1: Basic event-driven balance monitoring
async fn demo_basic_event_driven_balance() -> Result<()> {
    info!("\n=== Demo 1: Basic Event-Driven Balance Monitoring ===");

    // Sample wallet addresses to monitor
    let sample_accounts = vec![
        "11111111111111111111111111111112".parse::<Pubkey>()?, // System program
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".parse::<Pubkey>()?, // Token program
    ];

    // Configure event-driven balance monitoring
    let config = EventDrivenBalanceConfig {
        base_config: BalanceMonitorConfig {
            safety_mode_threshold_pct: 10.0,
            balance_sync_timeout_ms: 30_000,
            max_balance_age_ms: 60_000,
            enable_emergency_pause: true,
            balance_check_interval_ms: 10_000, // Reduced polling frequency
        },
        monitored_accounts: sample_accounts.iter().cloned().collect(),
        track_token_accounts: true,
        balance_change_threshold: 1000, // 0.000001 SOL
        enable_webhook_integration: true,
    };

    // Create event-driven balance monitor
    let mut balance_monitor = EventDrivenBalanceMonitor::new(config);

    // Start monitoring
    balance_monitor.start().await?;
    info!("✅ Event-driven balance monitor started");

    // Add additional accounts dynamically
    let additional_accounts = vec![
        "So11111111111111111111111111111111111111112".parse::<Pubkey>()?, // WSOL mint
    ];
    balance_monitor.add_monitored_accounts(additional_accounts).await?;

    // Simulate some time passing
    sleep(Duration::from_secs(2)).await;

    // Get statistics
    let stats = balance_monitor.get_stats().await;
    info!("📊 Balance monitor stats: {:?}", stats);

    // Get balance for an account
    if let Some(balance) = balance_monitor.get_account_balance(sample_accounts[0]).await {
        info!("💰 Balance for {}: {} lamports", sample_accounts[0], balance);
    }

    // Force refresh accounts
    balance_monitor.force_refresh_accounts(sample_accounts.clone()).await?;

    info!("✅ Basic event-driven balance monitoring demo completed");
    Ok(())
}

/// Demo 2: Integrated balance monitoring with webhooks
async fn demo_integrated_balance_monitoring() -> Result<()> {
    info!("\n=== Demo 2: Integrated Balance Monitoring with Webhooks ===");

    // Sample accounts for comprehensive monitoring
    let trading_accounts = vec![
        "11111111111111111111111111111112".parse::<Pubkey>()?, // System program
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".parse::<Pubkey>()?, // Token program
        "So11111111111111111111111111111111111111112".parse::<Pubkey>()?, // WSOL mint
    ];

    // Build configuration using the builder pattern
    let config = IntegratedBalanceConfigBuilder::new()
        .with_monitored_accounts(trading_accounts.clone())
        .with_balance_threshold(5000) // 0.000005 SOL threshold
        .with_token_tracking(true)
        .with_webhook_integration(true)
        .build();

    // Create webhook components (simplified for demo)
    let webhook_processor = Arc::new(PoolUpdateProcessor::new());
    
    // Create a simplified event-driven monitor for demo purposes
    let mut event_monitor = EventDrivenBalanceMonitor::new(config);
    event_monitor.start().await?;
    
    // Register with webhook processor
    event_monitor.register_with_webhook_processor(&webhook_processor).await?;
    
    info!("✅ Event-driven balance monitor with webhook integration started");

    // Add more accounts dynamically
    let additional_accounts = vec![
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse::<Pubkey>()?, // USDC mint
    ];
    event_monitor.add_monitored_accounts(additional_accounts).await?;

    // Simulate balance events
    simulate_balance_events(&event_monitor).await?;

    // Get statistics
    let stats = event_monitor.get_stats().await;
    info!("📊 Event-driven monitoring stats:");
    info!("   - Total webhook events: {}", stats.total_webhook_events);
    info!("   - Balance events: {}", stats.balance_triggering_events);
    info!("   - Native transfer events: {}", stats.native_transfer_events);

    // Force refresh all monitored accounts
    event_monitor.force_refresh_accounts(trading_accounts.clone()).await?;

    // Check individual account balances
    for account in &trading_accounts {
        if let Some(balance) = event_monitor.get_account_balance(*account).await {
            info!("💰 Balance for {}: {} lamports", account, balance);
        }
    }

    info!("✅ Integrated balance monitoring demo completed");
    Ok(())
}

/// Demo 3: Real-time balance tracking simulation
async fn demo_real_time_balance_tracking() -> Result<()> {
    info!("\n=== Demo 3: Real-time Balance Tracking Simulation ===");

    // Create a high-frequency monitoring setup
    let config = EventDrivenBalanceConfig {
        base_config: BalanceMonitorConfig {
            balance_check_interval_ms: 30_000, // Event-driven polling
            ..Default::default()
        },
        monitored_accounts: vec![
            "11111111111111111111111111111112".parse::<Pubkey>()?,
            "So11111111111111111111111111111111111111112".parse::<Pubkey>()?,
        ].into_iter().collect(),
        balance_change_threshold: 100, // Very low threshold for demo
        track_token_accounts: true,
        enable_webhook_integration: true,
    };

    let webhook_processor = Arc::new(PoolUpdateProcessor::new());
    
    let mut system = EventDrivenBalanceMonitor::new(config);
    system.start().await?;
    system.register_with_webhook_processor(&webhook_processor).await?;
    
    info!("✅ Real-time balance tracking system started");

    // Simulate rapid balance changes
    info!("🔄 Simulating rapid balance changes...");
    let test_accounts = vec![
        "11111111111111111111111111111112".parse::<Pubkey>()?,
        "So11111111111111111111111111111111111111112".parse::<Pubkey>()?,
    ];
    
    for i in 0..5 {
        // Force refresh to simulate balance queries
        system.force_refresh_accounts(test_accounts.clone()).await?;
        
        let stats = system.get_stats().await;
        info!("📊 Iteration {}: {} balance events processed", 
            i + 1, stats.balance_triggering_events);
        
        sleep(Duration::from_millis(500)).await;
    }

    // Demonstrate emergency scenarios
    info!("🚨 Simulating emergency balance monitoring...");
    let emergency_accounts = vec![
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse::<Pubkey>()?, // USDC
    ];
    system.force_refresh_accounts(emergency_accounts).await?;

    // Final statistics
    let final_stats = system.get_stats().await;
    info!("📈 Final monitoring statistics:");
    info!("   - Total webhook events: {}", final_stats.total_webhook_events);
    info!("   - Balance triggering events: {}", final_stats.balance_triggering_events);
    info!("   - Native transfer events: {}", final_stats.native_transfer_events);
    info!("   - Token transfer events: {}", final_stats.token_transfer_events);

    info!("✅ Real-time balance tracking simulation completed");
    Ok(())
}

/// Simulate balance events for testing
async fn simulate_balance_events(monitor: &EventDrivenBalanceMonitor) -> Result<()> {
    info!("🎭 Simulating balance events...");

    // Simulate balance changes through force refresh
    let test_accounts = vec![
        "11111111111111111111111111111112".parse::<Pubkey>()?,
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".parse::<Pubkey>()?,
    ];

    for (i, account) in test_accounts.iter().enumerate() {
        info!("📨 Simulating balance event {} for account {}", i + 1, account);
        monitor.force_refresh_accounts(vec![*account]).await?;
        sleep(Duration::from_millis(200)).await;
    }

    info!("✅ Balance event simulation completed");
    Ok(())
}

/// Helper function to demonstrate configuration options
fn print_configuration_options() {
    info!("\n=== Event-Driven Balance Monitoring Configuration Options ===");
    info!("1. Base Configuration:");
    info!("   - safety_mode_threshold_pct: Percentage threshold for emergency pause");
    info!("   - balance_sync_timeout_ms: Timeout for balance synchronization");
    info!("   - max_balance_age_ms: Maximum age for balance data");
    info!("   - enable_emergency_pause: Enable automatic emergency pause");
    info!("   - balance_check_interval_ms: Reduced polling interval (event-driven)");
    
    info!("\n2. Event-Driven Configuration:");
    info!("   - monitored_accounts: Set of accounts to monitor for changes");
    info!("   - track_token_accounts: Enable token account balance tracking");
    info!("   - balance_change_threshold: Minimum change to trigger update");
    info!("   - enable_webhook_integration: Enable webhook event processing");
    
    info!("\n3. Integration Benefits:");
    info!("   - Real-time event-driven updates instead of polling");
    info!("   - Webhook integration for external event sources");
    info!("   - Reduced resource usage with targeted monitoring");
    info!("   - Enhanced accuracy with multiple data sources");
}
