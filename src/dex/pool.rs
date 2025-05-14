use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAmount {
    pub amount: u64,
    pub decimals: u8,
}

impl TokenAmount {
    pub fn new(amount: u64, decimals: u8) -> Self {
        Self { amount, decimals }
    }

    pub fn to_float(&self) -> f64 {
        let divisor = 10u64.pow(self.decimals as u32) as f64;
        self.amount as f64 / divisor
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub reserve: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DexType {
    Raydium,
    Orca,
    Jupiter,
    Whirlpool,
    Lifinity,
    Phoenix,
    Meteora,
    Unknown,
}

impl fmt::Display for DexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DexType::Raydium => write!(f, "Raydium"),
            DexType::Orca => write!(f, "Orca"),
            DexType::Jupiter => write!(f, "Jupiter"),
            DexType::Whirlpool => write!(f, "Whirlpool"),
            DexType::Lifinity => write!(f, "Lifinity"),
            DexType::Phoenix => write!(f, "Phoenix"),
            DexType::Meteora => write!(f, "Meteora"),
            DexType::Unknown => write!(f, "Unknown"),
        }
    }
}

// Trait for parsing different DEX pool formats
pub trait PoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo>;
    fn get_program_id() -> Pubkey;
    fn get_dex_type() -> DexType;
}

/// Returns spot price for a PoolInfo (A/B token price).
///
/// This function calculates the simple spot price ratio between token A and token B
/// in a liquidity pool. It's kept public for several important use cases:
///
/// - Testing: Verifying pool state and price calculations in unit/integration tests
/// - Analytics: Used in opportunity analysis and market monitoring modules
/// - UI/Reporting: Will be used in dashboard and reporting features
/// - Debugging: Helpful for diagnostics when troubleshooting arbitrage paths
///
/// Note: For actual swap execution, use calculate_output_amount() which accounts
/// for fees and slippage.
#[allow(dead_code)]
pub fn calculate_price(pool: &PoolInfo) -> f64 {
    let token_a_amount = pool.token_a.reserve as f64 / 10f64.powi(pool.token_a.decimals as i32);
    let token_b_amount = pool.token_b.reserve as f64 / 10f64.powi(pool.token_b.decimals as i32);

    token_a_amount / token_b_amount
}

// Calculate how much token_b you get for a given amount of token_a
pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    let (input_reserve, _input_decimals, output_reserve, output_decimals) = if is_a_to_b {
        (
            pool.token_a.reserve,
            pool.token_a.decimals,
            pool.token_b.reserve,
            pool.token_b.decimals,
        )
    } else {
        (
            pool.token_b.reserve,
            pool.token_b.decimals,
            pool.token_a.reserve,
            pool.token_a.decimals,
        )
    };

    let adjusted_input = input_amount.amount;

    // Apply fees
    let fee = adjusted_input * pool.fee_numerator / pool.fee_denominator;
    let input_with_fee = adjusted_input - fee;

    // Calculate output using constant product formula: x * y = k
    let numerator = input_with_fee * output_reserve;
    let denominator = input_reserve + input_with_fee;
    let output_amount = numerator / denominator;

    TokenAmount::new(output_amount, output_decimals)
}
