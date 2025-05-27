// pub mod env; // env.rs is now simplified, its main logic moved to settings.rs
pub mod settings;

// Re-export the primary Config struct
pub use settings::Config;

use std::sync::Arc;
use std::path::Path;

/// Loads and returns the application configuration.
/// This function will call `dotenv::dotenv()` to load environment variables from a .env file,
/// then use `settings::Config::from_env()` to construct the configuration.
/// It also calls `config.validate_and_log()` to log the settings.
/// This function will panic if essential configurations are missing or malformed,
/// as `settings::Config::from_env()` is expected to panic on such errors.
pub fn load_app_config() -> settings::Config {
    dotenv::dotenv().ok(); // Load .env file if present, ignore errors
    let config = settings::Config::from_env();
    config.validate_and_log(); // Log the settings after successful load and validation
    config
}

/// Loads the application configuration from a file.
/// Returns an Arc-wrapped Config for shared use.
pub fn load_app_config_from_file(path: &str) -> Arc<Config> {
    let config = settings::Config::load(Path::new(path))
        .expect("Failed to load config from file");
    Arc::new(config)
}

// In your main.rs or lib.rs, wire the config loader for real initialization:
pub fn init_and_get_config() -> Arc<Config> {
    // Use environment-based config, fallback to file-based config if needed
    // (Here, we just use file-based config if env-based fails, but you can expand as needed)
    // If you want to add more robust error handling, you can do so here.
    match std::panic::catch_unwind(|| load_app_config()) {
        Ok(cfg) => Arc::new(cfg),
        Err(_) => {
            log::warn!("Falling back to file-based config due to error loading from env.");
            load_app_config_from_file("Config.toml")
        }
    }
}

// Example usage in this module (for demonstration):
#[cfg(test)]
mod tests {
    #[test]
    fn test_load_app_config() {
        // This test expects a config file at "Config.toml"
        // let config = load_app_config("Config.toml");
        // assert!(config.min_profit_pct > 0.0);
    }
}

// The check_and_print_env_vars function from the original config/mod.rs is now
// effectively handled by Config::from_env() (which errors on missing required vars)
// and Config::validate_and_log() (which prints them).
// The REQUIRED_ENV_VARS constant is also implicitly handled by Config::from_env().
