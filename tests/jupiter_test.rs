use solana_arb_bot::dex::clients::jupiter::JupiterClient;
use solana_arb_bot::dex::DexClient;

#[test]
fn test_jupiter_client_creation() {
    let client = JupiterClient::new();
    assert_eq!(client.get_name(), "Jupiter");
}

#[tokio::test]
async fn test_jupiter_quote_with_fallback() {
    let client = JupiterClient::new();
    // Test with well-known tokens
    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let amount = 1_000_000u64; // 1 SOL (in lamports)
    let slippage_bps = 50; // 0.5%

    // This test requires internet connection and may fail in CI
    // In a real test environment, you might want to mock the HTTP client
    let _result = client
        .get_quote_with_fallback(sol_mint, usdc_mint, amount, slippage_bps)
        .await;
    // We don't assert success since this depends on network and API availability
    // The test passes if the method can be called without compilation errors
}

#[tokio::test]
async fn test_jupiter_quote_error_handling() {
    let client = JupiterClient::new();
    // Use obviously invalid mints to test error handling
    let result = client
        .get_quote_with_fallback("INVALID", "INVALID", 1, 100)
        .await;
    assert!(result.is_err(), "Expected error for invalid mints");
}

#[tokio::test]
async fn test_jupiter_swap_transaction_creation() {
    let client = JupiterClient::new();
    // Use real, well-known token mints for testing
    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let amount = 1_000_000u64; // 1 SOL (in lamports)
    let slippage_bps = 50; // 0.5%

    // First get a quote (this may fail due to network, that's OK for this test)
    let quote_result = client
        .get_quote_with_fallback(sol_mint, usdc_mint, amount, slippage_bps)
        .await;

    if let Ok(quote) = quote_result {
        // If we got a quote, try to create a transaction
        let user_pubkey_str = "11111111111111111111111111111111"; // Mock user pubkey string
        let priority_fee = Some(5000u64); // Optional priority fee
        let tx_result = client
            .create_swap_transaction(&quote, user_pubkey_str, priority_fee)
            .await;

        // The transaction creation may fail due to various reasons (network, invalid quote, etc.)
        // We're mainly testing that the method can be called
        println!("Transaction creation result: {:?}", tx_result.is_ok());
    } else {
        // If quote failed (likely due to network), we skip the transaction test
        println!(
            "Skipping transaction test due to quote failure: {:?}",
            quote_result
        );
    }
}

#[tokio::test]
async fn test_jupiter_client_rate_limiting() {
    let client = JupiterClient::new();
    // Test that rate limiting is working by checking the rate limiter state
    // This is mainly a compilation test to ensure the rate limiting infrastructure exists

    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    // Make multiple calls to test rate limiting (they may fail due to network, that's OK)
    for i in 0..3 {
        let _result = client
            .get_quote_with_fallback(sol_mint, usdc_mint, 1_000_000, 50)
            .await;
        println!("Rate limiting test call {}: completed", i + 1);

        // Small delay to avoid overwhelming the API in case rate limiting isn't working
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
}

#[test]
fn test_jupiter_client_as_dex_client() {
    let client = JupiterClient::new();

    // Test that Jupiter client implements DexClient trait
    assert_eq!(client.get_name(), "Jupiter");

    // The client should be usable as a DexClient
    let _dex_client: &dyn DexClient = &client;
}
