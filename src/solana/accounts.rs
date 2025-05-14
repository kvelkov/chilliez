use anyhow::{anyhow, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cache for token metadata to reduce RPC calls.
/// Primed for future UI/metrics/analytics integrations.
#[allow(dead_code)]
pub struct TokenMetadataCache {
    cache: Arc<RwLock<HashMap<Pubkey, TokenMetadata>>>,
}

/// Metadata for a token, public for orchestrator/metrics/analytics future expansion.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TokenMetadata {
    pub mint: Pubkey,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub logo_uri: Option<String>,
}

impl TokenMetadataCache {
    /// Create a new token metadata cache
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get token metadata, fetching it if not in cache
    #[allow(dead_code)]
    pub async fn get_metadata(
        &self,
        mint: &Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<TokenMetadata> {
        // Check if we have it in cache
        if let Some(metadata) = self.cache.read().await.get(mint) {
            return Ok(metadata.clone());
        }

        // If not in cache, fetch it
        let metadata = Self::fetch_token_metadata(mint, rpc_client).await?;

        // Add to cache
        self.cache.write().await.insert(*mint, metadata.clone());

        Ok(metadata)
    }

    /// Fetch token metadata from RPC (or token list as fallback)
    #[allow(dead_code)]
    async fn fetch_token_metadata(mint: &Pubkey, _rpc_client: &RpcClient) -> Result<TokenMetadata> {
        // In a real implementation, we would fetch metadata from on-chain program
        // For now, we'll use a simplified approach with hardcoded values for common tokens
        // or derive a simple symbol from the mint address

        // Known tokens map (this would be much more extensive in production)
        let known_tokens: HashMap<&str, (&str, &str, u8)> = [
            (
                "So11111111111111111111111111111111111111112",
                ("SOL", "Solana", 9),
            ),
            (
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                ("USDC", "USD Coin", 6),
            ),
            (
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
                ("USDT", "Tether", 6),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let mint_str = mint.to_string();

        if let Some(&(symbol, name, decimals)) = known_tokens.get(mint_str.as_str()) {
            return Ok(TokenMetadata {
                mint: *mint,
                symbol: symbol.to_string(),
                name: name.to_string(),
                decimals,
                logo_uri: None,
            });
        }

        // For unknown tokens, generate a simple symbol from the mint address
        let symbol_str = format!("{}..", &mint_str[0..4]);

        Ok(TokenMetadata {
            mint: *mint,
            symbol: symbol_str.clone(), // Clone here to fix the borrow-after-move error
            name: format!("Unknown Token ({})", symbol_str), // Use symbol_str instead of symbol
            decimals: 6,                // Default to 6 decimals
            logo_uri: None,
        })
    }

    /// Add or update token metadata in the cache
    #[allow(dead_code)]
    pub async fn update_metadata(&self, metadata: TokenMetadata) {
        self.cache.write().await.insert(metadata.mint, metadata);
    }
}

/// Parse account data based on its structure.
/// Utility for future integration with program-specific account parsing.
#[allow(dead_code)]
pub fn parse_account_data(account_type: &str, data: &[u8]) -> Result<serde_json::Value> {
    match account_type {
        "token" => parse_token_account(data),
        "mint" => parse_mint_account(data),
        _ => Err(anyhow!("Unsupported account type: {}", account_type)),
    }
}

/// Parse a token account. Will be expanded with proper SPL token account parsing.
#[allow(dead_code)]
fn parse_token_account(data: &[u8]) -> Result<serde_json::Value> {
    // In a real implementation, this would use the correct token account structure
    // For now, just return a placeholder
    Ok(serde_json::json!({
        "account_type": "token",
        "data_length": data.len(),
    }))
}

/// Parse a mint account. Will be expanded with proper SPL mint account parsing.
#[allow(dead_code)]
fn parse_mint_account(data: &[u8]) -> Result<serde_json::Value> {
    // In a real implementation, this would use the correct mint account structure
    // For now, just return a placeholder
    Ok(serde_json::json!({
        "account_type": "mint",
        "data_length": data.len(),
    }))
}
