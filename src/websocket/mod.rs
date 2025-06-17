// src/websocket/mod.rs

pub mod market_data; // Defines the CryptoDataProvider trait
pub mod price_feeds; // Real-time WebSocket price feed infrastructure

// Re-export relevant items
pub use market_data::CryptoDataProvider;
pub use price_feeds::{
    ConnectionStatus, PriceFeedManager, PriceUpdate, WebSocketConfig, WebSocketFeed,
    WebSocketMetrics,
};

// DEX-specific WebSocket feeds
pub mod feeds;

// Re-export all WebSocket feed implementations
pub use feeds::{
    MeteoraWebSocketFeed, OrcaWebSocketFeed, PhoenixWebSocketFeed, RaydiumWebSocketFeed,
};
