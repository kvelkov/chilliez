use solana_arb_bot::dex::clients::JupiterClient;
use solana_arb_bot::dex::DexClient;

#[test]
fn test_jupiter_client_creation() {
    let client = JupiterClient::new();
    assert_eq!(client.get_name(), "Jupiter");
}

#[tokio::test]
async fn test_token_support_check() {
    let client = JupiterClient::new();
    // Test with well-known tokens (these should be supported)
    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    // Note: This test requires internet connection
    // In a real test environment, you might want to mock the HTTP client
    let _sol_supported = client.is_token_supported(sol_mint).await;
    let _usdc_supported = client.is_token_supported(usdc_mint).await;
}

#[tokio::test]
async fn test_jupiter_quote_error_handling() {
    let client = JupiterClient::new();
    // Use obviously invalid mints to force an error
    let result = client.get_quote("INVALID", "INVALID", 1, 100).await;
    assert!(result.is_err(), "Expected error for invalid mints");
}

#[tokio::test]
async fn test_jupiter_price_error_handling() {
    let client = JupiterClient::new();
    // Use an invalid mint to force an error
    let result = client.get_prices(vec!["INVALID".to_string()]).await;
    assert!(result.is_err(), "Expected error for invalid price request");
}

#[tokio::test]
async fn test_jupiter_token_list_error_handling() {
    let client = JupiterClient::new();
    // Simulate a forced refresh (should not error, but will test network failure if offline)
    let _ = client.refresh_token_list().await;
}

#[tokio::test]
async fn test_jupiter_end_to_end_simulation() {
    let client = JupiterClient::new();
    // Use real, well-known token mints
    let sol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let amount = 1_000_000u64; // 1 SOL (in lamports)
    let slippage_bps = 50; // 0.5%

    // Request a quote from Jupiter
    let quote_result = client.get_quote(sol_mint, usdc_mint, amount, slippage_bps).await;
    assert!(quote_result.is_ok(), "Jupiter quote request failed: {:?}", quote_result);
    let quote = quote_result.unwrap();
    println!("Simulated Jupiter quote: {} {} -> {} {} (out: {})", amount, sol_mint, quote.output_mint, usdc_mint, quote.out_amount);
    // Basic checks
    assert_eq!(quote.input_mint, sol_mint);
    assert_eq!(quote.output_mint, usdc_mint);
    assert!(quote.out_amount.parse::<u64>().unwrap_or(0) > 0, "Output amount should be positive");
}
