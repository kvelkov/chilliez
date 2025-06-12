// src/dex/whirlpool_parser.rs
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser, PoolToken};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use log::{debug, warn};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;

pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

// Minimal relevant fields from a Whirlpool account.
// Offsets are based on common Whirlpool layouts.
// Ensure these are correct for the pools you are targeting.
const TOKEN_MINT_A_OFFSET: usize = 127;
const TOKEN_VAULT_A_OFFSET: usize = 159;
const TOKEN_MINT_B_OFFSET: usize = 207;
const TOKEN_VAULT_B_OFFSET: usize = 239;
const FEE_RATE_OFFSET: usize = 43; // tick_spacing (u16) is often related, but fee_rate (u16) is at offset 43 in some layouts.
                                  // Whirlpool fee is tick_spacing dependent, usually 1, 8, 64, etc.
                                  // The actual fee_rate (e.g., 30 for 0.3%) is part of the Whirlpool struct.
                                  // For simplicity, we'll use a common default or extract if offset is certain.
                                  // A u16 at offset 43 is often `fee_rate`.

const MIN_WHIRLPOOL_DATA_LEN: usize = TOKEN_VAULT_B_OFFSET + 32; // Ensure data is long enough to read up to token_vault_b

pub struct WhirlpoolPoolParser;

#[async_trait]
impl PoolParser for WhirlpoolPoolParser {
    /// Parses raw account data for a Whirlpool pool.
    /// This stub implementation checks for sufficient data length and returns placeholder values.
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>, // Rename from _rpc_client to rpc_client
    ) -> Result<PoolInfo> {
        if data.len() < MIN_WHIRLPOOL_DATA_LEN {
            warn!(
                "Whirlpool parsing failed for {} - Insufficient data length: {}. Expected at least {} bytes.",
                address,
                data.len(),
                MIN_WHIRLPOOL_DATA_LEN
            );
            return Err(anyhow!(
                "Data too short for Whirlpool pool {}: expected at least {} bytes, got {}",
                address, MIN_WHIRLPOOL_DATA_LEN, data.len()
            ));
        }

        debug!("Parsing Whirlpool account {} with data length {}", address, data.len());

        let token_mint_a_bytes: &[u8; 32] = data[TOKEN_MINT_A_OFFSET..TOKEN_MINT_A_OFFSET + 32].try_into().context("Slice token_mint_a")?;
        let token_mint_a = Pubkey::new_from_array(*token_mint_a_bytes);

        let token_vault_a_bytes: &[u8; 32] = data[TOKEN_VAULT_A_OFFSET..TOKEN_VAULT_A_OFFSET + 32].try_into().context("Slice token_vault_a")?;
        let token_vault_a = Pubkey::new_from_array(*token_vault_a_bytes);

        let token_mint_b_bytes: &[u8; 32] = data[TOKEN_MINT_B_OFFSET..TOKEN_MINT_B_OFFSET + 32].try_into().context("Slice token_mint_b")?;
        let token_mint_b = Pubkey::new_from_array(*token_mint_b_bytes);

        let token_vault_b_bytes: &[u8; 32] = data[TOKEN_VAULT_B_OFFSET..TOKEN_VAULT_B_OFFSET + 32].try_into().context("Slice token_vault_b")?;
        let token_vault_b = Pubkey::new_from_array(*token_vault_b_bytes);

        // Fee rate is often a u16 (basis points)
        // Example: if fee_rate is at offset 43
        let fee_rate_bytes: [u8; 2] = data[FEE_RATE_OFFSET..FEE_RATE_OFFSET+2].try_into().context("Slice fee_rate")?;
        let fee_rate = u16::from_le_bytes(fee_rate_bytes);

        // Fetch actual decimals and reserves
        let (decimals_a, decimals_b, reserve_a, reserve_b): (u8, u8, u64, u64) = tokio::try_join!(
            rpc_client.get_token_mint_decimals(&token_mint_a),
            rpc_client.get_token_mint_decimals(&token_mint_b),
            rpc_client.get_token_account_balance(&token_vault_a),
            rpc_client.get_token_account_balance(&token_vault_b)
        ).with_context(|| format!("Failed to fetch details for Whirlpool {}", address))?;

        Ok(PoolInfo {
            address,
            name: format!("Whirlpool/{}", address.to_string().chars().take(6).collect::<String>()),
            token_a: PoolToken {
                mint: token_mint_a,
                symbol: format!("TKA_WP-{}", token_mint_a.to_string().chars().take(4).collect::<String>()),
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: token_mint_b,
                symbol: format!("TKB_WP-{}", token_mint_b.to_string().chars().take(4).collect::<String>()),
                decimals: decimals_b,
                reserve: reserve_b,
            },
            token_a_vault: token_vault_a,
            token_b_vault: token_vault_b,
            fee_numerator: Some(fee_rate as u64), // Fee rate in basis points
            fee_denominator: Some(10000),      // Basis points denominator
            fee_rate_bips: Some(fee_rate),
            last_update_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Whirlpool,
            liquidity: None, // Would need to parse from bytes if needed
            sqrt_price: None, // Would need to parse from bytes if needed
            tick_current_index: None, // Would need to parse from bytes if needed
            tick_spacing: None, // Would need to parse from bytes if needed
        })
    }
    fn get_program_id(&self) -> Pubkey {
        Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).unwrap()
    }
}