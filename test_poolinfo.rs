// Test file to check PoolInfo structure
use solana_arb_bot::utils::PoolInfo;

fn main() {
    let pool = PoolInfo::default();
    
    // Try to access fields that should exist
    println!("token_a_vault: {:?}", pool.token_a_vault);
    println!("token_b_vault: {:?}", pool.token_b_vault);
    println!("fee_numerator: {:?}", pool.fee_numerator);
    println!("fee_denominator: {:?}", pool.fee_denominator);
    println!("fee_rate_bips: {:?}", pool.fee_rate_bips);
    println!("liquidity: {:?}", pool.liquidity);
    println!("sqrt_price: {:?}", pool.sqrt_price);
    println!("tick_current_index: {:?}", pool.tick_current_index);
    println!("tick_spacing: {:?}", pool.tick_spacing);
}
