use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct AccountUpdate {
    pub pubkey: Pubkey,
    pub reserve_a: u64,
    pub reserve_b: u64,
}
