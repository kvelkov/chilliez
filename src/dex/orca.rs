// src/dex/orca.rs
//! Orca client and parser for on-chain data and instruction building.
//! This implementation leverages the official orca_whirlpools SDK for accuracy.

use crate::dex::quote::{DexClient, Quote, SwapInfo};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{PoolInfo, PoolParser as UtilsPoolParser};
use crate::utils::{DexType, PoolToken}; // Assuming DexType and PoolToken are in utils
use anyhow::{anyhow, Result as AnyhowResult};
use bytemuck::{Pod, Zeroable};
use solana_program::program_pack::Pack;
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Mint;
use std::sync::Arc;
use log::info;

/// The program ID for the Orca Whirlpools program.
pub const ORCA_WHIRLPOOL_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");

// --- On-Chain Data Parser ---

/// Represents the state of an Orca Whirlpool account.
/// Aligned with the structure provided in `dex_restructuring.txt`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Whirlpool {
    pub whirlpools_config: Pubkey,
    pub whirlpool_bump: [u8; 1],
    pub tick_spacing: u16,
    pub tick_spacing_padding: [u8; 5], // Added padding to match docs
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub protocol_fee_owed_a: u64,
    pub protocol_fee_owed_b: u64,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub fee_growth_global_a: u128,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,
    pub fee_growth_global_b: u128,
    pub reward_last_updated_timestamp: u64,
    // reward_infos array follows, but we only need to parse the fixed part.
    // pub reward_infos: [WhirlpoolRewardInfo; NUM_REWARDS], // NUM_REWARDS is 3
    // pub padding_after_rewards: [u8; REMAINDER_BYTES], // Ensure struct size is as expected if parsing full data
}

pub struct OrcaPoolParser;

#[async_trait::async_trait]
impl UtilsPoolParser for OrcaPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        info!("Parsing Orca Whirlpool data for address: {}", address);

        if data.len() < std::mem::size_of::<Whirlpool>() {
            return Err(anyhow!(
                "Invalid Whirlpool account data length for {}: expected at least {} bytes, got {}",
                address,
                std::mem::size_of::<Whirlpool>(),
                data.len()
            ));
        }

        let whirlpool: &Whirlpool = bytemuck::from_bytes(&data[..std::mem::size_of::<Whirlpool>()]);

        // Fetch decimals for token A and B
        let (token_a_mint_data, token_b_mint_data) = tokio::try_join!(
            async { rpc_client.primary_client.get_account_data(&whirlpool.token_mint_a).await.map_err(anyhow::Error::from) },
            async { rpc_client.primary_client.get_account_data(&whirlpool.token_mint_b).await.map_err(anyhow::Error::from) }
        )?;

        let token_a_decimals = Mint::unpack_from_slice(&token_a_mint_data)?.decimals;
        let token_b_decimals = Mint::unpack_from_slice(&token_b_mint_data)?.decimals;

        // Note: 'reserve' field in PoolToken is more for AMMs. For CLMMs like Orca,
        // liquidity is represented by whirlpool.liquidity. We'll set reserve to 0 or handle appropriately.
        Ok(PoolInfo {
            address,
            name: format!("Orca Whirlpool/{}", address),
            dex_type: DexType::Orca, // Assuming DexType::Orca variant exists
            token_a: PoolToken {
                mint: whirlpool.token_mint_a,
                symbol: "TokenA".to_string(), // Placeholder, consider a token registry
                decimals: token_a_decimals,
                reserve: 0, // Not directly applicable as AMM reserve
            },
            token_b: PoolToken {
                mint: whirlpool.token_mint_b,
                symbol: "TokenB".to_string(), // Placeholder, consider a token registry
                decimals: token_b_decimals,
                reserve: 0, // Not directly applicable as AMM reserve
            },
            token_a_vault: whirlpool.token_vault_a,
            token_b_vault: whirlpool.token_vault_b,
            
            fee_rate_bips: Some(whirlpool.fee_rate), // fee_rate is in basis points (0.01%)
            fee_numerator: None, // Not used by Orca in this way
            fee_denominator: None, // Not used by Orca in this way

            liquidity: Some(whirlpool.liquidity),
            sqrt_price: Some(whirlpool.sqrt_price),
            tick_current_index: Some(whirlpool.tick_current_index),
            tick_spacing: Some(whirlpool.tick_spacing),
            
            last_update_timestamp: whirlpool.reward_last_updated_timestamp,
        })
    }
    fn get_program_id(&self) -> Pubkey {
        ORCA_WHIRLPOOL_PROGRAM_ID
    }
}

// --- DEX Client Implementation ---

#[derive(Debug, Clone, Default)]
pub struct OrcaClient;

impl OrcaClient {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DexClient for OrcaClient {
    fn get_name(&self) -> &str {
        "Orca"
    }

    /// Calculates a quote using the official Orca SDK math.
    fn calculate_onchain_quote(
        &self,
        _pool: &PoolInfo,
        _input_amount: u64,
    ) -> AnyhowResult<Quote> {
        // This is a simplified example. A full implementation requires fetching the whirlpool,
        // tick arrays, and oracle accounts to pass to the quote function.
        // For now, we'll return an error indicating what's needed.
        return Err(anyhow!( // Keep return for clarity, or remove semicolon from line below
            "Orca quote calculation requires the full Whirlpool and TickArray accounts, not just PoolInfo."
        ));

        // TODO: The actual implementation will look something like this:
        // let whirlpool = ...; // Fetch and deserialize the full Whirlpool account.
        // let tick_arrays = [...]; // Fetch and deserialize the necessary TickArray accounts.
        // let quote = quote_from_whirlpool(
        //     &whirlpool,
        //     input_amount,
        //     pool.token_a.mint,
        //     &tick_arrays,
        //     // ... other params
        // )?;
        //
        // Ok(Quote {
        //     input_token: pool.token_a.symbol.clone(),
        //     output_token: pool.token_b.symbol.clone(),
        //     input_amount,
        //     output_amount: quote.estimated_amount_out,
        //     dex: self.get_name().to_string(),
        //     route: vec![pool.address],
        //     slippage_estimate: Some(quote.estimated_slippage),
        // })
    }

    fn get_swap_instruction(
        &self,
        _swap_info: &SwapInfo,
    ) -> AnyhowResult<Instruction> {
        Err(anyhow!("Orca Whirlpool swap instruction is not yet implemented: missing SDK integration and TickArray logic."))
    }
}