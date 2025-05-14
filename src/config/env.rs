use crate::config::settings::Config;
use log::error;
use std::env;

pub const REQUIRED_ENV_VARS: &[&str] = &[
    "RPC_URL",
    "WS_URL",
    "ORCA_API_KEY",
    "LIFINITY_API_KEY",
    "METEORA_API_KEY",
    "PHOENIX_API_KEY",
    "WHIRLPOOL_API_KEY",
    "TRADER_WALLET_ADDRESS",
];

pub fn check_and_print_env_vars() {
    let mut missing = Vec::new();

    for &key in REQUIRED_ENV_VARS {
        if env::var(key).is_err() {
            error!("ERROR: {key} is not set!");
            missing.push(key);
        }
    }

    if !missing.is_empty() {
        error!("Missing required environment variables: {:?}", missing);
        std::process::exit(1);
    }

    // Load and validate the full configuration
    let config = Config::from_env();
    config.validate_and_log();
}

pub fn load_config() -> Config {
    // First check that all required variables are set
    check_and_print_env_vars();

    // Then load the full configuration
    Config::from_env()
}
