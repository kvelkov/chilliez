// src/dex/quoting_engine.rs

use crate::dex::quote::{DexClient, Quote};
use crate::utils::PoolInfo;
use anyhow::Result;
use dashmap::DashMap;
use log::{debug, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait defining the operations for a quoting engine.
/// This allows for mocking `AdvancedQuotingEngine` in tests.
#[async_trait::async_trait]
pub trait QuotingEngineOperations: Send + Sync {
    async fn calculate_best_quote(
        &self,
        input_mint: Pubkey,
        output_mint: Pubkey,
        amount_in: u64,
    ) -> Result<Option<Quote>>;
}

/// `AdvancedQuotingEngine` is responsible for finding the best possible quote
/// for a token swap across all known DEX pools.
#[derive(Clone)]
pub struct AdvancedQuotingEngine {
    pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    #[allow(dead_code)] // This field is kept for potential future use, but not directly read after client_map is built.
    dex_clients: Vec<Arc<dyn DexClient>>,
    client_map: HashMap<String, Arc<dyn DexClient>>,
}

impl AdvancedQuotingEngine {
    /// Creates a new `AdvancedQuotingEngine`.
    ///
    /// # Arguments
    /// * `pool_cache` - A shared, concurrent-safe cache of `PoolInfo` objects.
    /// * `dex_clients` - A vector of `DexClient` implementations for various DEXs.
    ///
    /// # Returns
    /// A new instance of `AdvancedQuotingEngine`.
    pub fn new(
        pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        dex_clients: Vec<Arc<dyn DexClient>>,
    ) -> Self {
        let mut client_map = HashMap::new();
        for client in &dex_clients {
            client_map.insert(client.get_name().to_string(), Arc::clone(client));
        }
        info!(
            "AdvancedQuotingEngine initialized with {} DEX clients: {:?}",
            dex_clients.len(),
            dex_clients.iter().map(|c| c.get_name()).collect::<Vec<&str>>()
        );
        Self {
            pool_cache,
            dex_clients, // Keep the original Vec for other potential uses
            client_map,
        }
    }
}

#[async_trait::async_trait]
impl QuotingEngineOperations for AdvancedQuotingEngine {
    async fn calculate_best_quote(
        &self,
        input_mint: Pubkey,
        output_mint: Pubkey,
        amount_in: u64,
    ) -> Result<Option<Quote>> {
        if amount_in == 0 {
            return Ok(None); // No amount to swap
        }

        let mut best_quote: Option<Quote> = None;
        debug!(
            "Calculating best quote for {} -> {} with amount {}",
            input_mint, output_mint, amount_in
        );

        for entry in self.pool_cache.iter() {
            let pool_info_arc = entry.value();
            let pool_info = &**pool_info_arc; // Dereference Arc<PoolInfo> to &PoolInfo

            // Determine the DEX client for this pool
            // Using format!("{:?}") for DexType to get its string representation.
            // This assumes DexClient names match the Debug output of DexType variants (e.g., "Orca", "Raydium").
            let dex_type_str = format!("{:?}", pool_info.dex_type);
            let dex_client = match self.client_map.get(&dex_type_str) {
                Some(client) => client,
                None => {
                    warn!(
                        "No DexClient found for DEX type {:?} (pool {}). Skipping.",
                        pool_info.dex_type, pool_info.address
                    );
                    continue;
                }
            };

            // Try to get a quote from this pool
            match dex_client.calculate_onchain_quote(pool_info, amount_in) {
                Ok(current_quote) => {
                    // Verify if the quote matches the desired input/output mints and direction
                    let mut quote_is_valid_for_request = false;

                    if pool_info.token_a.mint == input_mint
                        && pool_info.token_b.mint == output_mint
                    {
                        // Expected A -> B
                        if current_quote.input_token == pool_info.token_a.symbol
                            && current_quote.output_token == pool_info.token_b.symbol
                        {
                            quote_is_valid_for_request = true;
                        }
                    } else if pool_info.token_b.mint == input_mint
                        && pool_info.token_a.mint == output_mint
                    {
                        // Expected B -> A
                        if current_quote.input_token == pool_info.token_b.symbol
                            && current_quote.output_token == pool_info.token_a.symbol
                        {
                            quote_is_valid_for_request = true;
                        }
                    }

                    if quote_is_valid_for_request {
                        debug!(
                            "Valid quote from pool {}: In: {} {}, Out: {} {}",
                            pool_info.address,
                            current_quote.input_amount,
                            current_quote.input_token,
                            current_quote.output_amount,
                            current_quote.output_token
                        );
                        if best_quote.is_none()
                            || current_quote.output_amount
                                > best_quote.as_ref().unwrap().output_amount
                        {
                            best_quote = Some(current_quote);
                        }
                    } else {
                        debug!(
                            "Quote from pool {} ({} -> {}) does not match desired path {} -> {}. Quote: {:?}",
                            pool_info.address, current_quote.input_token, current_quote.output_token,
                            input_mint, output_mint, current_quote
                        );
                    }
                }
                Err(e) => {
                    debug!(
                        "Failed to get quote from pool {} (DEX: {}): {}",
                        pool_info.address,
                        dex_client.get_name(),
                        e
                    );
                }
            }
        }

        if let Some(ref bq) = best_quote {
            info!(
                "Best quote found for {} -> {}: Output {} {} via DEX {} (Pools: {:?})",
                input_mint,
                output_mint,
                bq.output_amount,
                bq.output_token,
                bq.dex,
                bq.route
            );
        } else {
            info!(
                "No quote found for {} -> {} with amount_in {}",
                input_mint, output_mint, amount_in
            );
        }

        Ok(best_quote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::quote::Quote;
    use crate::utils::{PoolToken, DexType};
    use async_trait::async_trait;
    use solana_sdk::instruction::Instruction;

    // Mock DexClient
    #[derive(Clone)]
    struct MockDexClient {
        name: String,
        pools_data: HashMap<Pubkey, (String, String, u64)>, // pool_addr -> (in_sym, out_sym, out_amt)
    }

    #[async_trait]
    impl DexClient for MockDexClient {
        fn get_name(&self) -> &str { &self.name }

        fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> Result<Quote> {
            if let Some((in_sym, out_sym, out_amt)) = self.pools_data.get(&pool.address) {
                // Simulate quote based on whether input_amount is for token_a or token_b
                // This mock assumes the engine correctly identifies the input token for the pool
                Ok(Quote {
                    input_token: in_sym.clone(),
                    output_token: out_sym.clone(),
                    input_amount,
                    output_amount: *out_amt, // Fixed output for simplicity
                    dex: self.name.clone(),
                    route: vec![pool.address],
                    slippage_estimate: Some(0.01),
                })
            } else {
                Err(anyhow::anyhow!("Mock pool not found"))
            }
        }

        fn get_swap_instruction(&self, _swap_info: &crate::dex::quote::SwapInfo) -> Result<Instruction> {
            Err(anyhow::anyhow!("Not implemented for mock"))
        }

        async fn discover_pools(&self) -> Result<Vec<PoolInfo>> {
            Ok(vec![])
        }
    }

    fn create_mock_pool(
        address: Pubkey,
        dex_type: DexType,
        token_a_mint: Pubkey,
        token_a_sym: &str,
        token_b_mint: Pubkey,
        token_b_sym: &str,
    ) -> Arc<PoolInfo> {
        Arc::new(PoolInfo {
            address,
            name: format!("{:?}_{}", dex_type, address.to_string().chars().take(4).collect::<String>()),
            token_a: PoolToken { mint: token_a_mint, symbol: token_a_sym.to_string(), decimals: 6, reserve: 1000 },
            token_b: PoolToken { mint: token_b_mint, symbol: token_b_sym.to_string(), decimals: 6, reserve: 1000 },
            dex_type,
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn test_calculate_best_quote_simple() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        let pool_cache = Arc::new(DashMap::new());
        let pool1_addr = Pubkey::new_unique();
        let pool1 = create_mock_pool(pool1_addr, DexType::Orca, mint_a, "MINTA", mint_b, "MINTB");
        pool_cache.insert(pool1_addr, Arc::clone(&pool1));

        let mut orca_pools_data = HashMap::new();
        orca_pools_data.insert(pool1_addr, ("MINTA".to_string(), "MINTB".to_string(), 105)); // Quote A->B

        let orca_client = Arc::new(MockDexClient { name: "Orca".to_string(), pools_data: orca_pools_data }) as Arc<dyn DexClient>;
        
        let engine = AdvancedQuotingEngine::new(pool_cache, vec![orca_client]);

        let best_quote = engine.calculate_best_quote(mint_a, mint_b, 100).await.unwrap();

        assert!(best_quote.is_some());
        let quote = best_quote.unwrap();
        assert_eq!(quote.output_amount, 105);
        assert_eq!(quote.input_token, "MINTA");
        assert_eq!(quote.output_token, "MINTB");
        assert_eq!(quote.dex, "Orca");
    }

    #[tokio::test]
    async fn test_calculate_best_quote_multiple_pools() {
        let mint_x = Pubkey::new_unique();
        let mint_y = Pubkey::new_unique();

        let pool_cache = Arc::new(DashMap::new());

        let pool_orca_addr = Pubkey::new_unique();
        let pool_orca = create_mock_pool(pool_orca_addr, DexType::Orca, mint_x, "MINTX", mint_y, "MINTY");
        pool_cache.insert(pool_orca_addr, Arc::clone(&pool_orca));

        let pool_raydium_addr = Pubkey::new_unique();
        let pool_raydium = create_mock_pool(pool_raydium_addr, DexType::Raydium, mint_x, "MINTX", mint_y, "MINTY");
        pool_cache.insert(pool_raydium_addr, Arc::clone(&pool_raydium));

        let mut orca_data = HashMap::new();
        orca_data.insert(pool_orca_addr, ("MINTX".to_string(), "MINTY".to_string(), 200));
        let orca_client = Arc::new(MockDexClient { name: "Orca".to_string(), pools_data: orca_data }) as Arc<dyn DexClient>;

        let mut raydium_data = HashMap::new();
        raydium_data.insert(pool_raydium_addr, ("MINTX".to_string(), "MINTY".to_string(), 210)); // Raydium gives better quote
        let raydium_client = Arc::new(MockDexClient { name: "Raydium".to_string(), pools_data: raydium_data }) as Arc<dyn DexClient>;

        let engine = AdvancedQuotingEngine::new(pool_cache, vec![orca_client, raydium_client]);
        let best_quote = engine.calculate_best_quote(mint_x, mint_y, 100).await.unwrap();

        assert!(best_quote.is_some());
        let quote = best_quote.unwrap();
        assert_eq!(quote.output_amount, 210);
        assert_eq!(quote.dex, "Raydium");
    }

     #[tokio::test]
    async fn test_no_quote_if_pool_wrong_direction() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        let pool_cache = Arc::new(DashMap::new());
        let pool1_addr = Pubkey::new_unique();
        // Pool is A/B
        let pool1 = create_mock_pool(pool1_addr, DexType::Orca, mint_a, "MINTA", mint_b, "MINTB");
        pool_cache.insert(pool1_addr, Arc::clone(&pool1));

        let mut orca_pools_data = HashMap::new();
         // Mock DexClient returns a quote for B -> A, but we want A -> B
        orca_pools_data.insert(pool1_addr, ("MINTB".to_string(), "MINTA".to_string(), 105));
        let orca_client = Arc::new(MockDexClient { name: "Orca".to_string(), pools_data: orca_pools_data }) as Arc<dyn DexClient>;
        
        let engine = AdvancedQuotingEngine::new(pool_cache, vec![orca_client]);

        // Request A -> B
        let best_quote = engine.calculate_best_quote(mint_a, mint_b, 100).await.unwrap();
        assert!(best_quote.is_none(), "Should not find a quote if the mock client returns wrong direction");
    }

    #[tokio::test]
    async fn test_no_pools_found() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let pool_cache = Arc::new(DashMap::new()); // Empty cache
        let engine = AdvancedQuotingEngine::new(pool_cache, vec![]);
        let best_quote = engine.calculate_best_quote(mint_a, mint_b, 100).await.unwrap();
        assert!(best_quote.is_none());
    }

    #[tokio::test]
    async fn test_quote_for_reverse_pair_in_pool() {
        let mint_a = Pubkey::new_unique(); // Treat as USDC
        let mint_b = Pubkey::new_unique(); // Treat as SOL

        let pool_cache = Arc::new(DashMap::new());
        let pool1_addr = Pubkey::new_unique();
        // PoolInfo stores SOL (mint_b) as token_a, and USDC (mint_a) as token_b
        let pool1 = create_mock_pool(pool1_addr, DexType::Raydium, mint_b, "SOL", mint_a, "USDC");
        pool_cache.insert(pool1_addr, Arc::clone(&pool1));

        let mut raydium_pools_data = HashMap::new();
        // Mock DexClient is configured to provide a quote for USDC -> SOL
        // Input token symbol "USDC", output "SOL"
        raydium_pools_data.insert(pool1_addr, ("USDC".to_string(), "SOL".to_string(), 150)); 

        let raydium_client = Arc::new(MockDexClient { name: "Raydium".to_string(), pools_data: raydium_pools_data }) as Arc<dyn DexClient>;
        
        let engine = AdvancedQuotingEngine::new(pool_cache, vec![raydium_client]);

        // We want to quote USDC (mint_a) for SOL (mint_b)
        let best_quote = engine.calculate_best_quote(mint_a, mint_b, 10000).await.unwrap();

        assert!(best_quote.is_some());
        let quote = best_quote.unwrap();
        assert_eq!(quote.output_amount, 150);
        assert_eq!(quote.input_token, "USDC");
        assert_eq!(quote.output_token, "SOL");
        assert_eq!(quote.dex, "Raydium");
    }
}