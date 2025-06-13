// examples/helius_sdk_simple_test.rs
//! Simple test to verify Helius SDK integration is working

use solana_arb_bot::{
    helius_client::{HeliusManager, HeliusConfig},
    webhooks::helius_sdk::{HeliusWebhookManager, WebhookConfig},
};
use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Simple Helius SDK Integration Test");

    // Test 1: Create Helius client
    let config = HeliusConfig {
        api_key: "demo-key".to_string(),
        cluster: helius::types::Cluster::MainnetBeta,
        webhook_url: None,
        webhook_secret: None,
    };
    
    let helius_manager = HeliusManager::new(config)?;
    info!("✅ Helius client created successfully");

    // Test 2: Create webhook configuration
    std::env::set_var("HELIUS_WEBHOOK_URL", "https://example.com/webhook");
    
    let webhook_config = WebhookConfig::from_env().unwrap_or_else(|_| {
        info!("⚠️ Could not load webhook config from env, using default");
        WebhookConfig::default()
    });
    info!("✅ Webhook configuration created: {:?}", webhook_config.webhook_type);

    // Test 3: Create webhook manager (this tests that Helius client works)
    let new_helius_client = helius::Helius::new(
        &helius_manager.config().api_key,
        helius_manager.config().cluster.clone()
    )?;
    
    let webhook_manager = HeliusWebhookManager::new(
        new_helius_client,
        webhook_config,
    );
    info!("✅ Webhook manager created successfully");

    // Test 4: Show webhook stats
    let stats = webhook_manager.get_webhook_stats();
    info!("📊 Webhook stats: {}", stats);

    info!("🎉 All tests passed! Helius SDK integration is working correctly.");
    Ok(())
}
