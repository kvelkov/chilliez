// src/websocket/mod.rs

// pub mod handlers; // To be removed if PoolParser is sufficient
// pub mod manager; // Removed as manager.rs is deleted
pub mod market_data; // Defines the CryptoDataProvider trait
// pub mod types;   // To be removed if PoolParser is sufficient (AccountUpdate struct)

// Re-export relevant items if necessary, for example:
pub use market_data::CryptoDataProvider;

// If handlers.rs and types.rs are removed, this file might only contain market_data.
// If they are kept for some specific reason, their declarations would remain.
