// pub mod env; // env.rs is now simplified, its main logic moved to settings.rs
pub mod settings;

// Re-export the primary Config struct and the load_config function
pub use settings::Config;
// The load_config function is now directly available from the env module,
// but we can re-export it here for a cleaner import path if desired, e.g. `config::load_config()`.
// Or, users can call config::env::load_config().
// For simplicity, let's assume users will call settings::Config::from_env() or a new load_config here.

use crate::error::ArbError; // Ensure ArbError is in scope if used here
use std::sync::Arc;

// /// Loads and returns the application configuration.
// /// This function will panic if essential configurations are missing or malformed.
// /// It centralizes the configuration loading process.
// pub fn load_app_config() -> settings::Config {
//     match settings::Config::from_env() {
//         Ok(config) => {
//             config.log_settings(); // Log the settings after successful load and validation
//             config
//         }
//         Err(e) => {
//             log::error!("Failed to load application configuration: {}", e);
//             panic!("Application configuration error: {}", e);
//         }
//     }
// }

/// Loads and returns the application configuration as an `Arc<Config>`.
/// This function will panic if essential configurations are missing or malformed.
/// It centralizes the configuration loading process.
pub fn load_config() -> Result<Arc<settings::Config>, ArbError> {
    dotenv::dotenv().ok(); // Load .env file if present, ignore errors

    // Directly use from_env which now returns Config, not Result<Config, _>
    let config = settings::Config::from_env();

    // Perform any additional validation if necessary
    // For example, check if critical URLs are reachable, etc.
    // This was previously in validate_and_log, but can be expanded here.
    if config.rpc_url.is_empty() {
        return Err(ArbError::ConfigError("RPC_URL cannot be empty".to_string()));
    }
    if config.redis_url.is_empty() {
        return Err(ArbError::ConfigError("REDIS_URL cannot be empty".to_string()));
    }
    // Add more checks as needed...

    config.validate_and_log(); // Log the (now Debug-printable) config

    Ok(Arc::new(config))
}

// The check_and_print_env_vars function from the original config/mod.rs is now
// effectively handled by Config::from_env() (which errors on missing required vars)
// and Config::log_settings() (which prints them).
// The REQUIRED_ENV_VARS constant is also implicitly handled by Config::from_env().
// Removing the old check_and_print_env_vars and REQUIRED_ENV_VARS from here.
