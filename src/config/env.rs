// src/dex/whirlpool_parser.rs
//! Parser for Orca Whirlpool account data.

use crate::utils::{DexType, PoolInfo, PoolParser, PoolToken}; // Assuming these are in crate::utils
use anyhow::{anyhow, Result as AnyhowResult};
use solana_sdk::pubkey::Pubkey;
use std::convert::TryInto;
use std::str::FromStr;
use log::{error, info, warn}; // For logging

/// The program ID for Orca Whirlpools on Solana Mainnet.
pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbmvGdJ8kT34DbDZpeMZQRAu8da5nq7WaRDRtyQ";

/// Represents the state of an Orca Whirlpool.
/// The layout of this struct must exactly match the on-chain account data.
/// Refer to the official Orca Whirlpools SDK or on-chain program for the canonical layout.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct WhirlpoolState {
    // Discriminator: typically 8 bytes for Anchor programs, but Whirlpools might be different.
    // If not using Anchor, there might not be a global discriminator.
    // Often, the first few bytes identify the account type if not using a global one.
    // For this example, we'll assume the fields start directly as per the previous utils.rs version.

    pub whirlpools_config: Pubkey, // Field from Orca SDK
    pub whirlpool_bump: [u8; 1],   // Field from Orca SDK (typically u8, not array, but matching previous)
    
    pub fee_rate: u16,          // Fee rate in hundredths of a basis point (1/1,000,000)
    pub protocol_fee_rate: u16, // Protocol fee rate
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub protocol_fee_owed_a: u64,
    pub protocol_fee_owed_b: u64,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,      // The ATA that holds token A for the pool
    pub fee_growth_global_a: u128,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,      // The ATA that holds token B for the pool
    pub fee_growth_global_b: u128,
    
    // Reward Infos - these are part of the Whirlpool struct in the Orca SDK
    pub reward_last_updated_timestamp: u64,
    pub reward_infos: [RewardInfo; 3], // NUM_REWARDS is 3 in Orca SDK

    // Additional fields that might be present or were in the previous version
    pub tick_spacing: u16, // This is usually near the top
    // pub token_a_reserves: u64, // Reserves are not directly stored like this in Whirlpools; they are derived or fetched from vaults.
    // pub token_b_reserves: u64, // The `token_a` and `token_b` fields from the original struct were likely misinterpretations.
                               // Actual reserves need to be fetched from token_vault_a and token_vault_b.
    // pub open_time: u64, // This is not a standard field in the WhirlpoolState account.
}

/// Represents reward information for a Whirlpool.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RewardInfo {
    pub mint: Pubkey,           // Mint of the reward token
    pub vault: Pubkey,          // Vault ATA for the reward token
    pub authority: Pubkey,      // Authority that can modify emissions
    pub emissions_per_second_x64: u128, // Q64.64 fixed point number representing emissions per second
    pub growth_global_x64: u128,       // Q64.64 fixed point number
}

impl RewardInfo {
    /// Parses `RewardInfo` from a byte slice.
    /// Expected size is 32 (mint) + 32 (vault) + 32 (authority) + 16 (emissions) + 16 (growth) = 128 bytes.
    pub fn parse(buf: &[u8]) -> AnyhowResult<Self> {
        const EXPECTED_LEN: usize = 32 + 32 + 32 + 16 + 16;
        if buf.len() < EXPECTED_LEN {
            return Err(anyhow!(
                "RewardInfo buffer too short: expected {}, got {}",
                EXPECTED_LEN,
                buf.len()
            ));
        }
        let mut offset = 0;
        let mint = Pubkey::new_from_array(buf[offset..offset + 32].try_into()?); offset += 32;
        let vault = Pubkey::new_from_array(buf[offset..offset + 32].try_into()?); offset += 32;
        let authority = Pubkey::new_from_array(buf[offset..offset + 32].try_into()?); offset += 32;
        let emissions_per_second_x64 = u128::from_le_bytes(buf[offset..offset + 16].try_into()?); offset += 16;
        let growth_global_x64 = u128::from_le_bytes(buf[offset..offset + 16].try_into()?);
        // offset += 16; // No need, last field

        Ok(RewardInfo {
            mint,
            vault,
            authority,
            emissions_per_second_x64,
            growth_global_x64,
        })
    }

    /// Size of the `RewardInfo` struct in bytes.
    const fn size_in_bytes() -> usize {
        32 + 32 + 32 + 16 + 16 // mint + vault + authority + emissions + growth
    }
}


impl WhirlpoolState {
    /// Parses `WhirlpoolState` from account data.
    /// The exact layout and size should be verified against the Orca Whirlpools program.
    /// Anchor often prepends an 8-byte discriminator.
    pub fn parse(data: &[u8]) -> AnyhowResult<Self> {
        const ANCHOR_DISCRIMINATOR_SIZE: usize = 8;
        // Expected size based on Orca SDK:
        // whirlpools_config (32) + bump (1) + tick_spacing (2) + padding (5) to align next field
        // + fee_rate (2) + protocol_fee_rate (2) + liquidity (16) + sqrt_price (16) + tick_current_index (4)
        // + protocol_fee_owed_a (8) + protocol_fee_owed_b (8)
        // + token_mint_a (32) + token_vault_a (32) + fee_growth_global_a (16)
        // + token_mint_b (32) + token_vault_b (32) + fee_growth_global_b (16)
        // + reward_last_updated_timestamp (8) + reward_infos (3 * 128 = 384)
        // Total = 32+1+2+5 + 2+2+16+16+4+8+8 + 32+32+16 + 32+32+16 + 8+384 = 640 bytes approx (without discriminator)
        // With discriminator: 640 + 8 = 648 bytes.
        // This is a common size for Whirlpool accounts.

        // The struct provided by the user in utils.rs was slightly different.
        // I will try to match the Orca SDK structure more closely.
        // If the data doesn't have a discriminator, adjust `data_offset`.
        let data_offset = ANCHOR_DISCRIMINATOR_SIZE;
        const EXPECTED_DATA_LEN_AFTER_DISCRIMINATOR: usize = 32+1+2+5 + 2+2+16+16+4+8+8 + 32+32+16 + 32+32+16 + 8+384;


        if data.len() < data_offset + EXPECTED_DATA_LEN_AFTER_DISCRIMINATOR {
            return Err(anyhow!(
                "WhirlpoolState buffer too short: expected at least {} bytes after discriminator, got total {} bytes",
                EXPECTED_DATA_LEN_AFTER_DISCRIMINATOR,
                data.len()
            ));
        }

        let mut offset = data_offset;

        let whirlpools_config = Pubkey::new_from_array(data[offset..offset+32].try_into()?); offset += 32;
        let whirlpool_bump = data[offset..offset+1].try_into()?; offset += 1; // Assuming [u8;1]
        let tick_spacing = u16::from_le_bytes(data[offset..offset+2].try_into()?); offset += 2;
        offset += 5; // Skip 5 bytes of padding to align fee_rate

        let fee_rate = u16::from_le_bytes(data[offset..offset+2].try_into()?); offset += 2;
        let protocol_fee_rate = u16::from_le_bytes(data[offset..offset+2].try_into()?); offset += 2;
        let liquidity = u128::from_le_bytes(data[offset..offset+16].try_into()?); offset += 16;
        let sqrt_price = u128::from_le_bytes(data[offset..offset+16].try_into()?); offset += 16;
        let tick_current_index = i32::from_le_bytes(data[offset..offset+4].try_into()?); offset += 4;
        let protocol_fee_owed_a = u64::from_le_bytes(data[offset..offset+8].try_into()?); offset += 8;
        let protocol_fee_owed_b = u64::from_le_bytes(data[offset..offset+8].try_into()?); offset += 8;
        
        let token_mint_a = Pubkey::new_from_array(data[offset..offset+32].try_into()?); offset += 32;
        let token_vault_a = Pubkey::new_from_array(data[offset..offset+32].try_into()?); offset += 32;
        let fee_growth_global_a = u128::from_le_bytes(data[offset..offset+16].try_into()?); offset += 16;

        let token_mint_b = Pubkey::new_from_array(data[offset..offset+32].try_into()?); offset += 32;
        let token_vault_b = Pubkey::new_from_array(data[offset..offset+32].try_into()?); offset += 32;
        let fee_growth_global_b = u128::from_le_bytes(data[offset..offset+16].try_into()?); offset += 16;

        let reward_last_updated_timestamp = u64::from_le_bytes(data[offset..offset+8].try_into()?); offset += 8;

        let mut reward_infos = [RewardInfo { // Initialize with default
            mint: Pubkey::default(), vault: Pubkey::default(), authority: Pubkey::default(),
            emissions_per_second_x64: 0, growth_global_x64: 0,
        }; 3];
        for i in 0..3 {
            reward_infos[i] = RewardInfo::parse(&data[offset..offset + RewardInfo::size_in_bytes()])?;
            offset += RewardInfo::size_in_bytes();
        }

        Ok(Self {
            whirlpools_config, whirlpool_bump, fee_rate, protocol_fee_rate, liquidity, sqrt_price,
            tick_current_index, protocol_fee_owed_a, protocol_fee_owed_b, token_mint_a,
            token_vault_a, fee_growth_global_a, token_mint_b, token_vault_b, fee_growth_global_b,
            reward_last_updated_timestamp, reward_infos, tick_spacing
        })
    }
}

/// Parser implementation for Orca Whirlpools.
pub struct WhirlpoolPoolParser;

impl PoolParser for WhirlpoolPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo> {
        info!("Parsing Whirlpool pool data for address: {}", address);
        match WhirlpoolState::parse(data) {
            Ok(wp_state) => {
                // For Whirlpools, actual token reserves are in the `token_vault_a` and `token_vault_b` accounts.
                // Parsing WhirlpoolState gives you the mints and vault addresses.
                // To get reserves, you would typically need to make additional RPC calls to get_token_account_balance
                // on wp_state.token_vault_a and wp_state.token_vault_b.
                // This parser, as defined by the trait, expects to derive PoolInfo from *this single account's data*.
                // This is a limitation if reserves aren't directly in WhirlpoolState.
                // The original struct in utils.rs had `token_a` and `token_b` fields which might have been
                // intended for reserves, but they are not standard in WhirlpoolState.
                // For now, we'll set reserves to 0 and log a warning.
                // A more complete system would have an intermediate step or an enriched PoolInfo
                // that can be populated by multiple account fetches.

                warn!(
                    "Whirlpool ({}) reserves are not directly in WhirlpoolState. \
                    Actual reserves must be fetched from token vaults: Vault A ({}), Vault B ({}). \
                    Setting reserves to 0 in PoolInfo for now.",
                    address, wp_state.token_vault_a, wp_state.token_vault_b
                );

                // TODO: Implement robust fetching of token metadata (symbol, decimals)
                let token_a_metadata = fetch_token_metadata_blocking(&wp_state.token_mint_a);
                let token_b_metadata = fetch_token_metadata_blocking(&wp_state.token_mint_b);

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                Ok(PoolInfo {
                    address,
                    name: format!("WP/{}-{}", token_a_metadata.symbol, token_b_metadata.symbol),
                    token_a: PoolToken {
                        mint: wp_state.token_mint_a,
                        symbol: token_a_metadata.symbol,
                        decimals: token_a_metadata.decimals,
                        reserve: 0, // Placeholder - see warning above
                    },
                    token_b: PoolToken {
                        mint: wp_state.token_mint_b,
                        symbol: token_b_metadata.symbol,
                        decimals: token_b_metadata.decimals,
                        reserve: 0, // Placeholder - see warning above
                    },
                    fee_numerator: wp_state.fee_rate as u64, // fee_rate is e.g., 500 for 0.05%
                    fee_denominator: 1_000_000, // Standard denominator for this fee_rate scale
                    last_update_timestamp: timestamp, // Or use wp_state.reward_last_updated_timestamp
                    dex_type: DexType::Whirlpool,
                })
            }
            Err(e) => {
                error!("Failed to parse Whirlpool state for address {}: {:?}", address, e);
                Err(e)
            }
        }
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID)
            .expect("Failed to parse ORCA_WHIRLPOOL_PROGRAM_ID")
    }

    fn get_dex_type() -> DexType {
        DexType::Whirlpool
    }
}

// Placeholder for fetching token metadata.
// In a real application, this would query a token registry service or on-chain metadata.
#[derive(Default, Clone)]
struct MinimalTokenMetadata {
    symbol: String,
    decimals: u8,
}

fn fetch_token_metadata_blocking(mint: &Pubkey) -> MinimalTokenMetadata {
    warn!("Fetching placeholder metadata for mint {}. Implement actual metadata fetching.", mint);
    // Simulate fetching based on known tokens or default
    if mint.to_string() == "So11111111111111111111111111111111111111112" { // SOL
        MinimalTokenMetadata { symbol: "SOL".to_string(), decimals: 9 }
    } else if mint.to_string() == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" { // USDC
        MinimalTokenMetadata { symbol: "USDC".to_string(), decimals: 6 }
    } else {
        MinimalTokenMetadata {
            symbol: format!("{}..", &mint.to_string()[..4]),
            decimals: 6, // Defaulting to 6 is a GUESS and often incorrect.
        }
    }
}
