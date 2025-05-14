pub mod env; // Handles environment variable management
pub mod settings; // Handles trading parameters and app settings

use log::{error, info};
use std::env as sys_env; // Rename to avoid conflict with `config::env`

pub const REQUIRED_ENV_VARS: &[&str] = &[
    "RPC_URL",
    "WS_URL",
    "PAPER_TRADING",
    "MIN_PROFIT",
    "MAX_SLIPPAGE",
    "CYCLE_INTERVAL",
    "TRADER_WALLET_KEYPAIR_PATH",
];

pub fn check_and_print_env_vars() {
    let mut missing = Vec::new();
    info!("--- Environment Variable Check ---");

    for &key in REQUIRED_ENV_VARS {
        match sys_env::var(key) {
            Ok(val) => {
                if key.contains("KEYPAIR_PATH") || key.contains("SECRET") || key.contains("KEY") {
                    info!("{key}: ******** (path hidden)");
                } else {
                    info!("{key}: {val}");
                }
            }
            Err(_) => {
                error!("ERROR: {key} is not set!");
                missing.push(key);
            }
        }
    }

    // Optional: Check TRADER_WALLET_ADDRESS
    match sys_env::var("TRADER_WALLET_ADDRESS") {
        Ok(wallet) => info!("TRADER_WALLET_ADDRESS: {wallet}"),
        Err(_) => info!("TRADER_WALLET_ADDRESS: Not set (optional for verification)"),
    }

    info!("----------------------------------");

    if !missing.is_empty() {
        error!(
            "FATAL: The following environment variables are missing: {:?}\n\
            Please set them in your .env file before running the bot.",
            missing
        );
        std::process::exit(1);
    }
}
