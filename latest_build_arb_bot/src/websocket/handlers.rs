use crate::websocket::types::AccountUpdate;
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_response::RpcKeyedAccount;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Parses a Solana account update and returns an `AccountUpdate` if valid.
pub fn parse_account_update(account: &RpcKeyedAccount) -> Option<AccountUpdate> {
    let pubkey = Pubkey::from_str(&account.pubkey).ok()?;
    let data = &account.account.data;
    let binary_data = match data {
        solana_account_decoder::UiAccountData::Binary(encoded, UiAccountEncoding::Base64) => {
            base64::decode(encoded).ok()?
        }
        _ => return None,
    };
    if binary_data.len() < 16 {
        return None;
    }
    let reserve_a = u64::from_le_bytes(binary_data[0..8].try_into().ok()?);
    let reserve_b = u64::from_le_bytes(binary_data[8..16].try_into().ok()?);
    Some(AccountUpdate {
        pubkey,
        reserve_a,
        reserve_b,
    })
}
