//! Meteora DEX integration supporting both Dynamic AMM and DLMM pool types.
//
//! This module provides the necessary traits and structs to interact with the Meteora
//! decentralized exchange, allowing for the execution of trades, retrieval of market data,
//! and management of user positions and orders.
//
//! # Overview
//!
//! The Meteora DEX integration is designed to support two types of pool mechanisms:
//! Dynamic AMM (Automated Market Maker) and DLMM (Dynamic Liquidity Market Maker).
//!
//! - **Dynamic AMM**: A type of AMM where the liquidity pools' parameters can change
//!   dynamically based on market conditions, allowing for more flexible and responsive
//!   trading strategies.
//! - **DLMM**: An advanced version of AMM which not only allows dynamic parameters but
//!   also incorporates order book features, combining the benefits of AMMs and traditional
//!   order book exchanges.
//!
//! # Features
//!
//! - **Trade Execution**: Execute trades on the Meteora DEX with optimal pricing.
//! - **Market Data**: Retrieve real-time and historical market data for analysis and
//!   strategy development.
//! - **Order Management**: Place, modify, and cancel orders on the DEX.
//! - **Position Management**: Manage user positions, including margin and leverage settings.
//! - **Risk Management**: Tools to help manage and mitigate trading risks.
//!
//! # Usage
//!
//! To use the Meteora DEX integration, you need to create an instance of the `Meteora` struct,
//! configure it with your trading parameters, and then use its methods to interact with the DEX.
//!
//! ```rust
//! use crate::dex::protocols::meteora::Meteora;
//!
//! let meteora = Meteora::new(/* parameters */);
//! meteora.execute_trade(/* trade details */);
//! ```
//!
//! # Conclusion
//!
//! The Meteora DEX integration provides a comprehensive suite of tools for trading on the
//! Meteora decentralized exchange. Its support for both Dynamic AMM and DLMM pool types
//! offers traders enhanced flexibility and control over their trading strategies.
// --- Meteora Math Implementation ---
//! Meteora-specific mathematical calculations for Dynamic AMM and DLMM pools.
//!
//! This module provides the mathematical foundations and calculations required by the
//! Meteora DEX integration, ensuring accurate and efficient trade execution, price
//! determination, and liquidity management.
//!
//! # Overview
//!
//! The Meteora math implementation includes:
//! - Price calculations
//! - Liquidity provisioning and removal math
//! - Trade execution math
//! - Risk management calculations
//!
//! # Usage
//!
//! The math functions are used internally by the Meteora DEX integration to perform
//! necessary calculations. However, some utility functions may be exposed for external
//! use, such as price or liquidity calculations.
//!
//! ```rust
//! use crate::dex::protocols::meteora::math;
//!
//! let price = math::calculate_price(/* parameters */);
//! ```
//!
//! # Conclusion
//!
//! The Meteora math implementation is a crucial part of the DEX integration, providing
//! the necessary mathematical tools and functions to ensure accurate and efficient
//! trading on the Meteora decentralized exchange.
