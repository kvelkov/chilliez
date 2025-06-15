// src/websocket/mod.rs

pub mod market_data; // Defines the CryptoDataProvider trait
pub mod price_feeds; // Real-time WebSocket price feed infrastructure

// Re-export relevant items
pub use market_data::CryptoDataProvider;
pub use price_feeds::{
    PriceFeedManager, PriceUpdate, WebSocketFeed, WebSocketConfig, 
    WebSocketMetrics, ConnectionStatus
};

// DEX-specific WebSocket feeds
pub mod feeds;

// Re-export all WebSocket feed implementations
pub use feeds::{
    OrcaWebSocketFeed,
    MeteoraWebSocketFeed,
    RaydiumWebSocketFeed,
    PhoenixWebSocketFeed,
};
