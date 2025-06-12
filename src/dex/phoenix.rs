// src/dex/phoenix.rs
//! Phoenix client and parser for on-chain order book data and instruction building.
//! This implementation leverages the official phoenix-sdk.

// PHOENIX CLIENT DISABLED DUE TO INCOMPATIBLE DEPENDENCY
//
// use crate::dex::quote::{DexClient, Quote, SwapInfo};
// use crate::solana::rpc::SolanaRpcClient;
// use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
// use anyhow::{anyhow, Result as AnyhowResult};
// use log::info;
// use phoenix_sdk::sdk_client::SDKClient;
// use phoenix_sdk::types::{Side, Token};
// use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
// use std::sync::Arc;
//
// // --- On-Chain Data Parser ---
//
// pub struct PhoenixMarketParser;
//
// #[async_trait::async_trait]
// impl UtilsPoolParser for PhoenixMarketParser {
//     async fn parse_pool_data(
//         &self,
//         address: Pubkey,
//         data: &[u8],
//         _rpc_client: &Arc<SolanaRpcClient>,
//     ) -> AnyhowResult<PoolInfo> {
//         let (header, market_bytes) = data.split_at(std::mem::size_of::<phoenix_sdk::types::Header>());
//         let market = phoenix_sdk::types::Market::load_with_header(header, market_bytes)
//             .map_err(|e| anyhow!("Failed to parse Phoenix market state for {}: {:?}", address, e))?;
//
//         info!("Parsing Phoenix market data for address: {}", address);
//
//         // An order book doesn't have "reserves" in the AMM sense.
//         // We populate PoolInfo pragmatically. The core logic will use the raw market data.
//         Ok(PoolInfo {
//             address,
//             name: format!("Phoenix Market/{}", address),
//             token_a: PoolToken {
//                 mint: market.get_base_mint(),
//                 symbol: "BASE".to_string(), // Placeholder
//                 decimals: market.get_base_decimals() as u8,
//                 reserve: 1, // Not applicable for an order book
//             },
//             token_b: PoolToken {
//                 mint: market.get_quote_mint(),
//                 symbol: "QUOTE".to_string(), // Placeholder
//                 decimals: market.get_quote_decimals() as u8,
//                 reserve: 1, // Not applicable for an order book
//             },
//             fee_numerator: market.get_fee_bps() as u64,
//             fee_denominator: 10000,
//             last_update_timestamp: market.get_creation_timestamp(),
//             dex_type: DexType::Phoenix,
//         })
//     }
//
//     fn get_program_id(&self) -> Pubkey {
//         phoenix_sdk::ID
//     }
// }
//
// // --- DEX Client Implementation ---
//
// #[derive(Debug, Clone, Default)]
// pub struct PhoenixClient;
//
// impl PhoenixClient {
//     pub fn new() -> Self {
//         Self::default()
//     }
// }
//
// impl DexClient for PhoenixClient {
//     fn get_name(&self) -> &str {
//         "Phoenix"
//     }
//
//     /// Calculates a quote by walking the on-chain order book.
//     fn calculate_onchain_quote(
//         &self,
//         pool: &PoolInfo,
//         input_amount: u64,
//     ) -> AnyhowResult<Quote> {
//         // To calculate a quote, we need the raw market data, not just the PoolInfo.
//         // This function would need to be adapted to accept the raw `data: &[u8]` as well.
//         // For now, we return an error indicating the need for this change.
//         return Err(anyhow!(
//             "Phoenix quote calculation requires the raw market account data."
//         ));
//
//         // TODO: A full implementation would look like this:
//         // let (header, market_bytes) = raw_market_data.split_at(..);
//         // let market = Market::load_with_header(header, market_bytes)?;
//         //
//         // // Assuming we are selling token A (base) for token B (quote)
//         // let quote = market.get_quote_for_base_amount(input_amount, Side::Ask)?;
//         //
//         // Ok(Quote {
//         //     input_token: pool.token_a.symbol.clone(),
//         //     output_token: pool.token_b.symbol.clone(),
//         //     input_amount,
//         //     output_amount: quote.total_quote_amount,
//         //     dex: self.get_name().to_string(),
//         //     route: vec![pool.address],
//         //     slippage_estimate: None,
//         // })
//     }
//
//     /// Builds a swap instruction, which for Phoenix is an immediate-or-cancel limit order.
//     fn get_swap_instruction(
//         &self,
//         swap_info: &SwapInfo,
//     ) -> AnyhowResult<Instruction> {
//         // This is a simplified swap. A more advanced implementation might place limit orders.
//         // We will need to create a dummy SDKClient to use the instruction builder.
//         // A real implementation would manage this client more elegantly.
//         let dummy_signer = Keypair::new();
//         let client = SDKClient::new(&dummy_signer);
//
//         // We determine the worst acceptable price for the IOC order to act like a swap.
//         // This is a complex calculation; for now, we use a placeholder.
//         let worst_case_price_in_ticks = 0; // Placeholder - this must be calculated properly.
//
//         let instruction = client.new_swap_instruction(
//             &swap_info.pool.address,
//             &swap_info.user_source_token_account,
//             &swap_info.user_destination_token_account,
//             Side::Ask, // Assuming we are selling token A for token B
//             swap_info.amount_in,
//             worst_case_price_in_ticks,
//         );
//
//         Ok(instruction)
//     }
// }