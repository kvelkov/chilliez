use crate::solana::rpc::SolanaRpcClient; // Changed to use our HA client
use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cache for token metadata to reduce RPC calls.
/// Primed for future UI/metrics/analytics integrations.
#[allow(dead_code)] // Struct and its methods are not yet used
pub struct TokenMetadataCache {
    cache: Arc<RwLock<HashMap<Pubkey, TokenMetadata>>>,
}

/// Metadata for a token, public for orchestrator/metrics/analytics future expansion.
#[derive(Debug, Clone)]
pub struct TokenMetadata {
    pub mint: Pubkey,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub logo_uri: Option<String>,
}

impl TokenMetadata {
    #[allow(dead_code)]
    fn _suppress_dead_code_warnings(&self) {
        let _ = &self.mint;
        let _ = &self.symbol;
        let _ = &self.name;
        let _ = self.decimals;
        let _ = &self.logo_uri;
    }
}

#[allow(dead_code)] // Impl block and its methods are not yet used
impl TokenMetadataCache {
    /// Create a new token metadata cache
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get token metadata, fetching it if not in cache
    pub async fn get_metadata(
        &self,
        mint: &Pubkey,
        rpc_client: &SolanaRpcClient, // Changed parameter type
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
    async fn fetch_token_metadata(
        mint: &Pubkey,
        rpc_client: &SolanaRpcClient,
    ) -> Result<TokenMetadata> {
        let _ = rpc_client; // Suppress unused variable warning
                            // Illustrative: If metadata were in an account, you'd fetch and parse it.
                            // match rpc_client.get_account_data(mint).await {
                            //     Ok(account_data) => {
                            //         // TODO: Parse account_data to extract actual metadata
                            //         // For now, we continue with the placeholder logic.
                            //         log::debug!("Successfully fetched account data for mint {}, len: {}. Placeholder parsing follows.", mint, account_data.len());
                            //     }
                            //     Err(e) => {
                            //         log::warn!("Failed to fetch account data for mint {}: {}. Using placeholder.", mint, e);
                            //     }
                            // }

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
    pub async fn update_metadata(&self, metadata: TokenMetadata) {
        self.cache.write().await.insert(metadata.mint, metadata);
    }
}

impl Default for TokenMetadataCache {
    fn default() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[allow(dead_code)]
fn _suppress_dead_code_warnings_for_account_utils() {
    let _ = parse_account_data("token", &[]);
    let _ = parse_token_account(&[]);
    let _ = parse_mint_account(&[]);
}

/// Parse account data based on its structure.
/// Utility for future integration with program-specific account parsing.
pub fn parse_account_data(account_type: &str, data: &[u8]) -> Result<serde_json::Value> {
    match account_type {
        "token" => parse_token_account(data),
        "mint" => parse_mint_account(data),
        _ => Err(anyhow!("Unsupported account type: {}", account_type)),
    }
}

/// Parse a token account. Will be expanded with proper SPL token account parsing.
fn parse_token_account(data: &[u8]) -> Result<serde_json::Value> {
    // In a real implementation, this would use the correct token account structure
    // For now, just return a placeholder
    Ok(serde_json::json!({
        "account_type": "token",
        "data_length": data.len(),
    }))
}

/// Parse a mint account. Will be expanded with proper SPL mint account parsing.
fn parse_mint_account(data: &[u8]) -> Result<serde_json::Value> {
    // In a real implementation, this would use the correct mint account structure
    // For now, just return a placeholder
    Ok(serde_json::json!({
        "account_type": "mint",
        "data_length": data.len(),
    }))
}
