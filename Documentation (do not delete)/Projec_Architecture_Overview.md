Project Architecture Overview


This document provides a high-level overview of the Solana Arbitrage Bot's architecture. The system is designed with a clear separation of concerns, ensuring that each component is modular, independently testable, and maintainable. This design allows for scalability and makes it easier to add new features, DEX integrations, or strategies in the future.

Functional Architecture Diagram
The following diagram illustrates the primary functional components of the application and the flow of information between them.

    
    B --- B1; C --- C1; D --- D1; E --- E1;

Core Components & Responsibilities
The application is divided into several distinct service layers, each with a specific responsibility.

1. The Core Logic (arbitrage/)
This is the brain of the bot. It is responsible for the central task of finding and acting on arbitrage opportunities.

engine.rs: The main orchestrator that coordinates the entire process, from receiving market data to initiating a trade.

pathfinder.rs: Contains the complex algorithms for analyzing liquidity pools and finding the most profitable trading routes, potentially across multiple DEXs (multi-hop).

execution.rs: The "trigger-puller." This module takes a confirmed profitable route and builds the final transaction. It integrates with Jito for MEV protection and prepares the transaction for signing. This is where the Executor logic resides.

safety.rs: Performs final pre-flight checks on a transaction before it's sent to the blockchain to ensure its validity and safety.

opportunity.rs: Defines the data structure for a potential arbitrage opportunity.

2. The Data Layer (data/)
This layer is the bot's eyes and ears, responsible for ingesting all external information and caching it for high-speed access.

websockets.rs: Connects to DEX-specific WebSocket feeds to receive real-time price and liquidity updates.

webhooks.rs: Runs a web server (using Axum) to listen for incoming event notifications from services like QuickNode.

cache.rs: Implements a high-performance cache for frequently accessed data, such as liquidity pool states and token prices, to minimize external API calls.

3. The DEX Layer (dex/)
This is a crucial abstraction layer that acts as a universal translator for all supported Decentralized Exchanges.

api.rs: Defines a common DexClient trait. This trait ensures that the arbitrage engine can interact with any DEX using a standardized interface (e.g., get_quote(), build_swap_instructions()).

discovery.rs: Handles the discovery and validation of new liquidity pools.

protocols/: This directory contains the specific adapter implementation for each supported DEX (e.g., jupiter.rs, orca.rs, raydium.rs). Each adapter implements the DexClient trait.

4. The Blockchain Layer (blockchain/)
This is the lowest-level service, responsible for all direct interaction with the Solana network.

client.rs: Manages a high-availability pool of connections to Solana RPC nodes, complete with failover and rate-limiting logic.

wallet.rs: Manages all cryptographic key operations. It loads the main private key, signs transactions prepared by the execution.rs module, and manages the pool of ephemeral wallets used for trades. This is where the Wallet logic resides.

accounts.rs: Contains utilities for fetching and parsing token account data.

5. Supporting Services
These modules provide essential functionality that supports the core application.

monitoring/: Consolidates all application metrics, performance tracking, and health checks. It includes the balance_monitor.rs to track funds in real-time.

simulation/: Provides a complete paper trading environment for testing strategies against live or historical market data without risking real assets.

config.rs & /configs: Manages the loading and validation of all application settings from TOML and JSON files.

/tests & /examples: Contains integration tests and runnable examples for development, validation, and documentation.

Information Flow: From Signal to Trade
Signal: The data layer receives a real-time price update via websockets.rs or webhooks.rs.

Detection: This update is passed to the arbitrage/engine.rs, which triggers the arbitrage/pathfinder.rs to analyze the new market state for profitable routes.

Quoting: The pathfinder uses the dex/api.rs trait to request quotes from multiple DEX adapters in dex/protocols/.

Verification: Once a profitable route is confirmed, the arbitrage/engine.rs hands it off to the arbitrage/execution.rs.

Transaction Building: The execution module works with the relevant dex adapter to build the necessary swap instructions.

Signing: The complete, unsigned transaction is passed to the blockchain/wallet.rs for signing.

Submission: The now-signed transaction is sent to the Solana network via the RPC client in blockchain/client.rs.