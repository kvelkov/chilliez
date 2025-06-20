# Chilliez Project Functional Map (Top-Level)

- `/Cargo.toml`
    - **Purpose:** Rust project manifest file. Specifies project metadata, dependencies, build scripts, and features.
- `/Cargo.lock`
    - **Purpose:** Lock file for Cargo dependencies. Ensures reproducible builds by recording exact versions of dependencies.
- `/cleanup_todo_done.md`
    - **Purpose:** Markdown log of completed cleanup tasks. Used for project maintenance tracking.
- `/PROJECT_CLEANUP_LOG.md`
    - **Purpose:** Ongoing log of project cleanup activities, issues, and resolutions.
- `/refactoring_proj.md`
    - **Purpose:** (Currently empty) Intended for documenting the refactoring project plan, progress, or notes.
- `/src/`
    - **Purpose:** Main source code directory for the Chilliez project. Contains all Rust modules and application logic.
    - `arbitrage_monitor.rs`
        - **Purpose:** Rust source file for monitoring arbitrage opportunities and market conditions.
    - `banned_pairs_log.csv`
        - **Purpose:** CSV log of banned trading pairs, used to prevent trades on problematic or blacklisted pairs.
    - `cache.rs`
        - **Purpose:** Implements caching logic for quotes, routes, or other frequently accessed data.
    - `execution.rs`
        - **Purpose:** Handles trade execution logic, including order placement and transaction management.
    - `ffi.rs`
        - **Purpose:** Foreign Function Interface bindings for interoperability with other languages or systems.
    - `jito_bundle.rs`
        - **Purpose:** Manages Jito bundle creation and submission for MEV-protected transactions.
    - `lib.rs`
        - **Purpose:** Main library entry point for the Chilliez crate, exposing core modules and APIs.
    - `main.rs`
        - **Purpose:** Main binary entry point for the Chilliez application.

    - `ai/`
        - **Purpose:** (Folder) Contains AI-related modules or logic.
    - `api/`
        - **Purpose:** (Folder) Contains API definitions and integration logic.
    - `arbitrage/`
        - **Purpose:** (Folder) Contains arbitrage logic, strategies, and supporting modules.
    - `config/`
        - **Purpose:** (Folder) Contains configuration management code and settings.
    - `dashboard/`
        - **Purpose:** (Folder) Contains dashboard or UI-related code for monitoring and analytics.
    - `dex/`
        - **Purpose:** (Folder) Contains DEX (Decentralized Exchange) client implementations and related logic.
    - `discovery/`
        - **Purpose:** (Folder) Contains modules for discovering pools, tokens, or opportunities.
    - `error/`
        - **Purpose:** (Folder) Contains error handling and custom error types.
    - `functions/`
        - **Purpose:** (Folder) Contains utility or helper functions used across the project.
    - `local_metrics/`
        - **Purpose:** (Folder) Contains local metrics collection and reporting logic.
    - `metrics/`
        - **Purpose:** (Folder) Contains metrics collection, aggregation, and reporting modules.
    - `monitoring/`
        - **Purpose:** (Folder) Contains monitoring logic for system health, performance, and trading activity.
    - `optimization/`
        - **Purpose:** (Folder) Contains optimization algorithms and related modules.
    - `paper_trading/`
        - **Purpose:** (Folder) Contains paper trading logic, analytics, and simulation modules.
    - `performance/`
        - **Purpose:** (Folder) Contains performance measurement and optimization code.
    - `quicknode/`
        - **Purpose:** (Folder) Contains QuickNode integration and streaming logic.
    - `solana/`
        - **Purpose:** (Folder) Contains Solana-specific utilities and integration code.
    - `streams/`
        - **Purpose:** (Folder) Contains streaming data processing and event handling modules.
    - `testing/`
        - **Purpose:** (Folder) Contains testing utilities and support code.
    - `tests/`
        - **Purpose:** (Folder) Contains additional or legacy test modules.
    - `utils/`
        - **Purpose:** (Folder) Contains general utility functions and helpers.
    - `wallet/`
        - **Purpose:** (Folder) Contains wallet management and integration logic.
    - `webhooks/`
        - **Purpose:** (Folder) Contains webhook integration and event handling code.
    - `websocket/`
        - **Purpose:** (Folder) Contains WebSocket client and server logic for real-time data feeds.
- `/config/`
    - **Purpose:** Configuration files for the project, including DEX programs, pool data, and trading settings.
    - `dex_programs.json`
        - **Purpose:** Lists supported DEX (Decentralized Exchange) programs for the project. Contains arrays for each DEX (Orca, Raydium, Jupiter, Meteora, Lifinity) to specify program IDs or related data. Used to configure which DEXs are available for trading and monitoring.
    - `orca_whirlpool_pools.json`
        - **Purpose:** Contains a list of Solana addresses for Orca Whirlpool liquidity pools. Used to identify and interact with specific pools for trading, monitoring, or analytics.
    - `paper-trading.toml`
        - **Purpose:** Main configuration for paper trading mode. Defines trading parameters (initial balance, slippage, network latency simulation), risk management settings, monitoring options, enabled DEX protocols, and market data feed intervals.
    - `quicknode_streams.toml`
        - **Purpose:** Configuration for QuickNode real-time data streams. Sets up subscriptions for DEX account changes and program logs, specifying which addresses and programs to monitor for arbitrage detection and analytics.
- `/demo_paper_logs/`
    - **Purpose:** Stores logs from demo paper trading runs for analytics and debugging.
    - `paper_analytics_20250618_145547.json`
        - **Purpose:** Stores analytics summary for a demo paper trading session, including session times, trade statistics, profit/loss, Sharpe ratio, drawdown, and breakdowns by DEX and token pair. Used for performance analysis and reporting.
    - `paper_trades_20250618_145547.jsonl`
        - **Purpose:** Contains line-delimited JSON records of individual simulated trades from a demo session. Each entry logs trade details, execution results, slippage, fees, and failure reasons for granular analysis and debugging.
- `/docs/`
    - **Purpose:** Project documentation, guides, architecture notes, and cleanup guides.
- `/examples/`
    - **Purpose:** Example Rust programs demonstrating usage, integration, and performance of Chilliez components.
    - `advanced_routing_demo.rs`
        - **Purpose:** Demonstrates advanced multi-hop and smart order routing, including cross-DEX optimization, path finding, route splitting, MEV protection, failover, and analytics.
    - `api_management_demo.rs`
        - **Purpose:** Shows advanced API management, including rate limiting, RPC connection pooling, priority queuing, and health checks for robust API usage.
    - `comprehensive_performance_demo.rs`
        - **Purpose:** Showcases the complete performance optimization system: parallel DEX quote calculations, concurrent route optimization, advanced caching, and real-time monitoring.
    - `enhanced_long_term_paper_trading_validation.rs`
        - **Purpose:** Runs a 48+ hour paper trading validation with performance monitoring, security auditing, and accuracy validation.
    - `jupiter_cache_demo.rs`
        - **Purpose:** Demonstrates integration with Jupiter's cache system for efficient quote retrieval and cache configuration.
    - `jupiter_route_optimization_demo.rs`
        - **Purpose:** Demonstrates Jupiter's multi-route optimization and fallback management for best route selection.
    - `long_term_paper_trading_validation.rs`
        - **Purpose:** Runs long-term (48+ hour) paper trading with validation, security audit logging, and performance monitoring.
    - `mainnet_paper_trading_demo.rs`
        - **Purpose:** Runs paper trading against real mainnet data with simulated trades, error handling, and real-time reporting.
    - `microsecond_trading_example.cpp`
        - **Purpose:** Educational C++ example of ultra-low latency trading setup, including lock-free ring buffer and CPU optimizations for microsecond-level trading.
    - `paper_trading_demo.rs`
        - **Purpose:** Simple demonstration of paper trading functionality, including analytics, reporting, and simulated execution.
    - `performance_optimization_demo.rs`
        - **Purpose:** Demonstrates performance optimization features: parallel processing, advanced caching, monitoring, and benchmarking.
    - `performance_validation_demo.rs`
        - **Purpose:** Validates performance optimizations: parallel processing, caching, monitoring, stress testing, and stability simulation.
    - `quicknode_stream_demo.rs`
        - **Purpose:** Demonstrates use of Solana stream filters with QuickNode streams to detect arbitrage opportunities in real time.
    - `real_environment_paper_trading.rs`
        - **Purpose:** Demonstrates paper trading with real market data, realistic slippage, and comprehensive analytics using Jupiter API and Solana RPC.
    - `setup_paper_funds.rs`
        - **Purpose:** Example of setting up paper trading with virtual funds and initial token balances for simulation.
    - `simple_advanced_routing_demo.rs`
        - **Purpose:** Simplified demo of advanced multi-hop and smart order routing, focusing on core routing infrastructure.
    - `simple_routing_demo.rs`
        - **Purpose:** Simple demo of advanced routing capabilities without complex setup, for educational and testing purposes.
    - `wallet_jito_integration_demo.rs`
        - **Purpose:** Demonstrates integration of ephemeral wallets with Jito bundle submission for MEV-protected arbitrage, including error handling and workflow demonstration.
- `/paper_trading_logs/`
    - **Purpose:** Logs and status files from paper trading sessions, including arbitrage opportunities and trading results.
    - `arbitrage_opportunities.json`, `arbitrage_opportunities_Jupiter.json`, `arbitrage_opportunities_Lifinity.json`, `arbitrage_opportunities_Meteora.json`, `arbitrage_opportunities_Orca.json`, `arbitrage_opportunities_Raydium.json`
        - **Purpose:** Store lists of detected arbitrage opportunities for each DEX or in aggregate. Used for tracking, analytics, and debugging of arbitrage detection logic.
    - `enhanced_paper_trading_2025-06-17T21-06-31-542Z.jsonl` (and similar)
        - **Purpose:** Line-delimited JSON logs of enhanced paper trading sessions, recording each trade's details, status, P&L, and session metadata for long-term validation and analysis.
    - `status_Jupiter.json`, `status_Lifinity.json`, `status_Meteora.json`, `status_Orca.json`, `status_Raydium.json`
        - **Purpose:** Status and runtime metrics for each DEX during paper trading, including connection status, runtime, opportunities found, and performance statistics.
- `/paper_trading_reports/`
    - **Purpose:** Reports generated from paper trading activities.
    - *(Currently empty)*
        - **Purpose:** Intended for storing generated reports from paper trading activities, such as summaries, analytics, or exportable data for further analysis.
- `/scripts/`
    - **Purpose:** Utility scripts for project automation, setup, or data processing.
- `/streams/`
    - **Purpose:** Likely contains code or data related to streaming market data or events.
- `/target/`
    - **Purpose:** Cargo build output directory (auto-generated, not source).
- `/tests/`
    - **Purpose:** Integration and system tests for the Chilliez project.
    - `engine_integration.rs`
        - **Purpose:** Integration tests for the arbitrage engine and orchestrator, including dependency setup and banned pairs management.
    - `jito_bundle_integration.rs`
        - **Purpose:** Tests for Jito bundle integration, including ephemeral wallet creation, bundle submission, and error handling.
    - `jupiter_fallback.rs`
        - **Purpose:** Integration tests for Jupiter fallback logic, ensuring correct failover and price aggregation.
    - `jupiter_test.rs`
        - **Purpose:** Unit and async tests for Jupiter DEX client creation and quote retrieval, including fallback scenarios.
    - `lifinity_integration.rs`
        - **Purpose:** Comprehensive integration tests for Lifinity DEX client, including market making and oracle integration.
    - `meteora_integration.rs`
        - **Purpose:** Comprehensive integration tests for Meteora DEX client, covering both Dynamic AMM and DLMM pool types.
    - `orca_clmm_integration.rs`
        - **Purpose:** Integration tests for Orca CLMM (Whirlpool) implementation, focusing on quote calculations and swap instruction building.
    - `phoenix_integration.rs`
        - **Purpose:** Comprehensive test suite for Phoenix DEX integration, including order book math, client functionality, and WebSocket feeds.
    - `raydium_integration.rs`
        - **Purpose:** Integration tests for Raydium V4 AMM, validating math, swap instructions, pool discovery, and error handling.
    - `token_decimal_safety_test.rs`
        - **Purpose:** (Empty or placeholder) Intended for tests ensuring token decimal safety and correctness in calculations.

### /src/ai/
- `mod.rs`
    - **Purpose:** Aggregates all DEX-specific clients and services for AI-related modules. Provides a unified interface for interacting with various decentralized exchanges on Solana, including banned pairs, pool discovery, quoting, and individual DEX clients (Lifinity, Meteora, Orca, Raydium).

### /src/api/
- `connection_pool.rs`
    - **Purpose:** Implements an advanced RPC connection pool with automatic failover, load balancing, health monitoring, and circuit breaker patterns for Solana RPC endpoints.
- `enhanced_error_handling.rs`
    - **Purpose:** Provides enhanced API error handling, including ban detection, error classification, and jitter/backoff strategies for robust API usage.
- `manager.rs`
    - **Purpose:** Central API manager coordinating rate limiting, connection pooling, and request distribution across all API providers for optimal reliability and performance.
- `mod.rs`
    - **Purpose:** API management module root. Exposes advanced rate limiting, connection pooling, request distribution, and health monitoring for production-scale operations.
- `rate_limiter.rs`
    - **Purpose:** Implements advanced rate limiting with priority queuing, exponential backoff, per-DEX limits, and integration with connection pooling for API management.

### /src/config/
- `env.rs`
    - **Purpose:** Handles environment variable parsing and provides utilities for loading and validating environment-specific configuration, including on-chain program IDs and state layouts.
- `mod.rs`
    - **Purpose:** Root module for configuration management. Re-exports settings and provides functions to load and validate application configuration from environment and files.
- `settings.rs`
    - **Purpose:** Defines the main `Config` struct and related configuration logic, including deserialization, validation, and environment variable integration for all application settings.

### /src/arbitrage/
- `README.txt`
    - **Purpose:** Documentation for the arbitrage module, including structure, Jupiter DEX aggregator support, and file/folder map.
- `analysis/`
    - **Purpose:** (Folder) Contains mathematical analysis, fee logic, thresholds, and simulation logic for arbitrage.
    - `fee.rs`
        - **Purpose:** Production-grade dynamic fee calculation, including Solana priority fees, Jito tips, DEX protocol fees, and profitability assessment.
    - `math.rs`
        - **Purpose:** Advanced arbitrage mathematics and intelligent slippage models, including pool depth analysis, trade impact, and volatility adjustments.
    - `mod.rs`
        - **Purpose:** Main module for analysis, re-exporting fee and math logic, and orchestrating analysis components (e.g., ArbitrageAnalyzer).
    - `safety.rs`
        - **Purpose:** (Not in use) Placeholder or legacy file; main safety logic is in `arbitrage/safety.rs`.
- `calculator_tests.rs`
    - **Purpose:** Unit tests for arbitrage opportunity calculation logic, including profitability checks and simulation outcomes.
- `jito_client.rs`
    - **Purpose:** Provides functionality to submit transaction bundles via Jito for MEV protection and atomic execution of arbitrage trades.
- `jupiter/`
    - **Purpose:** (Folder) Contains Jupiter DEX aggregator integration and related logic.
    - `cache.rs`
        - **Purpose:** Jupiter quote caching system, implementing TTL, bucketing, volatility-based invalidation, and LRU eviction for API responses.
    - `integration.rs`
        - **Purpose:** Jupiter integration manager, handling fallback logic, intelligent caching, and high-level Jupiter operations.
    - `mod.rs`
        - **Purpose:** Root module for Jupiter-specific arbitrage components, exposing caching, integration, and multi-route optimization features.
    - `routes.rs`
        - **Purpose:** Implements advanced multi-route optimization, parallel route evaluation, intelligent scoring, and route caching for Jupiter trades.
- `orchestrator/`
    - **Purpose:** (Folder) Contains the central orchestrator logic for arbitrage detection, execution, and concurrency management.
    - `concurrency_manager.rs`
        - **Purpose:** Manages thread-safe execution, deadlock prevention, and concurrent operation management for the arbitrage orchestrator.
    - `core.rs`
        - **Purpose:** Core arbitrage orchestrator structure, defining main orchestrator and core functionality.
    - `detection_engine.rs`
        - **Purpose:** Handles arbitrage opportunity detection, hot cache management, pool validation, and opportunity analysis.
    - `execution_manager.rs`
        - **Purpose:** Manages execution logic, including strategy selection, coordination, and monitoring.
    - `mod.rs`
        - **Purpose:** Root module for orchestrator, exposing all orchestrator components and traits for price data providers.
- `routing/`
    - **Purpose:** (Folder) Contains routing algorithms and logic for finding optimal arbitrage paths across DEXs.
    - `failover.rs`
        - **Purpose:** Provides robust failover routing and recovery, including fallback, DEX health monitoring, and emergency execution modes.
    - `graph.rs`
        - **Purpose:** Routing graph module for multi-hop DEX routing, representing liquidity pools and enabling efficient pathfinding.
    - `mev_protection.rs`
        - **Purpose:** Implements MEV protection and anti-sandwich attack routing, including risk assessment and dynamic slippage protection.
    - `mod.rs`
        - **Purpose:** Root module for advanced multi-hop and smart order routing, exposing all routing components and algorithms.
    - `optimizer.rs`
        - **Purpose:** Route optimizer for multi-objective optimization, evaluating and improving routing paths based on profit, speed, and reliability.
    - `pathfinder.rs`
        - **Purpose:** PathFinder module for multi-hop route discovery, implementing various pathfinding algorithms for optimal trading routes.
    - `smart_router.rs`
        - **Purpose:** Smart router integrating all routing components, providing unified cross-DEX multi-hop routing, MEV protection, and analytics.
    - `splitter.rs`
        - **Purpose:** Route splitter for optimal trade distribution, dividing large trades across pools/DEXs to minimize price impact and maximize efficiency.
- `safety.rs`
    - **Purpose:** Implements transaction safety checks, retry logic, and recovery mechanisms for arbitrage execution.
- `strategy.rs`
    - **Purpose:** Responsible for finding arbitrage paths, integrating with the orchestrator, and providing path-finding services.
- `strategy_manager.rs`
    - **Purpose:** Manages arbitrage strategies, including opportunity selection, pool validation, and detection metrics.
- `tests.rs`
    - **Purpose:** Integration and unit tests for arbitrage logic, including orchestrator, opportunity, and DEX client tests.
- `types.rs`
    - **Purpose:** Defines shared types and enums for arbitrage execution strategies, detection metrics, and opportunity analysis.

### /src/dex/
- `api.rs`
    - **Purpose:** DEX API infrastructure, defining the `DexClient` trait, quote structures, and enhanced swap interfaces for all DEX clients.
- `clients/`
    - **Purpose:** (Folder) Contains client implementations for each supported DEX on Solana.
    - `jupiter.rs`
        - **Purpose:** Jupiter aggregator client, providing integration, quote retrieval, and liquidity aggregation.
    - `jupiter_api.rs`
        - **Purpose:** Jupiter API V6 data structures for request/response, used for fallback price aggregation.
    - `lifinity.rs`
        - **Purpose:** Lifinity DEX client with proactive market making support and pool parsing.
    - `meteora.rs`
        - **Purpose:** Meteora DEX client supporting Dynamic AMM and DLMM pool types.
    - `mod.rs`
        - **Purpose:** Root module for DEX clients, re-exporting all client structs and traits for easier access.
    - `orca.rs`
        - **Purpose:** Orca Whirlpools client and parser for on-chain data and instruction building.
    - `phoenix.rs`
        - **Purpose:** Phoenix DEX client with order book support, market order execution, and swap instruction building.
    - `raydium.rs`
        - **Purpose:** Raydium client and parser for on-chain data, following the official V4 layout for accuracy.
- `dex_tests.rs`
    - **Purpose:** Comprehensive tests for all DEX clients and functionality, including Meteora, Lifinity, Phoenix, and enhanced features.
- `discovery.rs`
    - **Purpose:** Pool discovery, management, and banned pairs filtering, consolidating pool management and routing logic.
- `live_update_manager.rs`
    - **Purpose:** Service for real-time pool updates, connecting webhooks to the orchestrator's hot cache for sub-millisecond access.
- `math/`
    - **Purpose:** (Folder) Contains mathematical calculation modules for each DEX and generic math utilities.
    - `lifinity.rs`
        - **Purpose:** Lifinity-specific mathematical calculations for proactive market making and output estimation.
    - `meteora.rs`
        - **Purpose:** Meteora-specific math for Dynamic AMM and DLMM pools.
    - `orca.rs`
        - **Purpose:** Orca Whirlpools CLMM math for concentrated liquidity pools, used by the Orca client.
    - `phoenix.rs`
        - **Purpose:** Phoenix DEX order book math, including market order execution and price impact calculations.
    - `raydium.rs`
        - **Purpose:** Raydium V4 AMM math for accurate quote calculations and fee handling.
- `math.rs`
    - **Purpose:** Advanced DEX math module, providing generic and high-precision calculations for various DEX types.
- `mod.rs`
    - **Purpose:** Root module for the DEX system, providing a unified interface for all supported DEXs, core modules, and test framework.

### /src/discovery/
- `mod.rs`
    - **Purpose:** (Empty) Placeholder for pool/token/opportunity discovery logic. Intended to aggregate discovery-related modules.

### /src/error/
- `mod.rs`
    - **Purpose:** Defines custom error types for the project, including `ArbError` for DEX, network, and WebSocket errors, using the `thiserror` crate for ergonomic error handling and logging.

### /src/functions/
- *(Empty)*
    - **Purpose:** (Folder exists but contains no files.) Intended for utility or helper functions used across the project.

### /src/local_metrics/
- **Purpose:** Local metrics collection and reporting logic, likely for development or isolated environments.
    - `metrics.rs`: Defines the `Metrics` struct for tracking various counters (e.g., pools, opportunities, execution stats) using atomic types and mutexes for thread safety. Provides methods for incrementing and updating metrics.
    - `mod.rs`: Declares types for trading pairs, execution records, and trade outcomes. Re-exports the `Metrics` struct from `metrics.rs`.

### /src/metrics/
- **Purpose:** Main metrics collection, aggregation, and reporting modules for the project.
    - `metrics.rs`: Implements the primary `Metrics` struct, similar to `local_metrics`, but with integration for external metrics libraries (e.g., `metrics` crate). Tracks pool, opportunity, and execution statistics, and provides logging and reporting methods.
    - `mod.rs`: Declares types for trading pairs, opportunity details, execution records, and trade outcomes. Integrates with logging and time libraries for detailed metrics and reporting.

### /src/monitoring/
- **Purpose:** Monitoring logic for system health, performance, and trading activity.
    - `balance_monitor_enhanced.rs`: Provides a comprehensive, real-time balance monitoring module with WebSocket integration, thread-safe updates, and alerting for discrepancies. Includes configuration, alert types, and metrics for monitoring account balances.
    - `mod.rs`: Module entry point. Documents the monitoring module's capabilities and re-exports key types and structs from `balance_monitor_enhanced.rs` for use elsewhere in the project.

### /src/optimization/
- **Purpose:** (Currently empty) Intended for optimization algorithms and related modules.
### /src/paper_trading/
- **Purpose:** Contains the full paper trading simulation system, including analytics, configuration, execution engine, portfolio management, and reporting.
    - `analytics.rs`: Provides analytics and performance tracking for paper trading, including P&L, execution stats, slippage, and trade outcomes.
    - `config.rs`: Defines configuration structures for paper trading mode, such as initial balances, simulated fees, slippage, and logging options.
    - `engine.rs`: Implements the simulated execution engine for paper trading, handling trade simulation, slippage, fee calculation, and result reporting.
    - `mod.rs`: Module entry point. Re-exports all submodules and documents the paper trading system's features.
    - `portfolio.rs`: Manages the virtual portfolio for paper trading, including balance tracking, trade execution, and P&L calculation.
    - `reporter.rs`: Handles reporting and logging of simulated trades, analytics, and performance summaries for paper trading sessions.
    - `todo.md`: Markdown file outlining integration plans and TODOs for QuickNode and paper trading enhancements.

### /src/performance/
- **Purpose:** Provides performance optimization, benchmarking, caching, metrics, and parallel processing infrastructure.
    - `benchmark.rs`: Comprehensive benchmarking suite for route calculation, DEX latency, cache performance, and system stress testing.
    - `cache.rs`: High-performance caching system for pool states, route calculations, and quote results, with TTL and freshness validation.
    - `metrics.rs`: Collects and analyzes performance metrics, including system resource usage, latency, throughput, and error rates.
    - `mod.rs`: Module entry point. Re-exports all submodules and defines the main performance configuration struct.
    - `parallel.rs`: Implements parallel processing and concurrent execution for DEX quotes, simulations, and optimizations.

### /src/quicknode/
- **Purpose:** Integrates QuickNode for real-time data ingestion, function client, and webhook processing.
    - `README.md`: Documentation for integrating QuickNode with the bot, including data flow, struct definitions, and integration steps.
    - `function_client.rs`: Client for calling QuickNode functions, parsing responses, and converting them into internal opportunity structures.
    - `mod.rs`: Module entry point, re-exporting the function client.
    - `webhook.rs`: Handles incoming webhooks from QuickNode, processes payloads, and forwards opportunities to the arbitrage orchestrator.

### /src/solana/
- **Purpose:** Solana-specific utilities, account management, balance monitoring, RPC, and WebSocket integration.
    - `accounts.rs`: Manages token metadata and account information, including a cache for efficient lookups.
    - `balance_monitor.rs`: Real-time balance monitoring using WebSocket, with risk management and event tracking.
    - `event_driven_balance.rs`: Event-driven balance monitoring that reacts to webhook events for real-time updates.
    - `mod.rs`: Module entry point, re-exporting key components for balance monitoring and event-driven monitoring.
    - `rpc.rs`: Provides a high-availability Solana RPC client, health checks, and account/program data retrieval.
    - `websocket.rs`: Implements a WebSocket manager for Solana, handling account updates, metrics, and connection management.

### /src/streams/
- **Purpose:** Streaming data processing and event handling modules for Solana and QuickNode.
    - `mod.rs`: Module entry point. Re-exports types and modules for QuickNode and Solana stream filtering.
    - `quicknode.rs`: Defines types and logic for parsing and handling QuickNode event streams, including block, transaction, and event data.
    - `solana_stream_filter.rs`: Implements Solana stream filtering for QuickNode streams, filtering transactions and account changes for arbitrage opportunities.

### /src/testing/
- **Purpose:** Comprehensive testing infrastructure for the arbitrage bot, including mocks, integration, performance, and stress tests.
    - `integration_tests.rs`: (Empty) Placeholder for integration test implementations.
    - `mock_dex.rs`: Provides a mock DEX environment for testing, simulating pool states, market conditions, and transaction execution.
    - `mod.rs`: Module entry point. Re-exports mock DEX and defines test result/metrics types and the test suite runner.
    - `performance_tests.rs`: (Empty) Placeholder for performance test implementations.
    - `stress_tests.rs`: Implements stress testing utilities, including high-frequency operation, memory leak detection, and concurrent load testing.

### /src/tests/
- **Purpose:** Additional or legacy test modules for integration and component testing.
    - `detector_test.rs`: Unit and integration tests for the arbitrage detector, engine, executor, and related components.
    - `integration_test.rs`: Integration tests for DEX pool parsing, account info, and Solana RPC interactions.
    - `mod.rs`
        - **Purpose:** Module entry point. Re-exports all test modules.
    - `pool_discovery_test.rs`: Integration tests for pool discovery functionality, especially for Raydium.

### /src/utils/
- **Purpose:** General utility functions, types, and helpers used throughout the project.
    - `mod.rs`: Provides common utility structures (e.g., `PoolInfo`, `PoolToken`, `DexType`), traits (e.g., `PoolParser`), and functions for logging, keypair loading, token amount conversions, and swap calculations.
    - `parsers/`: (Empty) Intended for parser modules for various DEXs or data formats.
    - `timing.rs`: Timing utilities for performance monitoring, including operation timers and checkpointing.

### /src/wallet/
- **Purpose:** Wallet management and integration logic, including ephemeral wallet pools and Jito integration.
    - `helper.rs`: Utilities for getting or creating associated token accounts for wallets and token mints.
    - `integration.rs`: Implements the integrated workflow for ephemeral wallets and Jito bundle submission for MEV-protected arbitrage.
    - `mod.rs`: Module entry point. Re-exports main wallet management components and test modules.
    - `tests.rs`: Tests for wallet pool and Jito integration, verifying correct setup and operation.
    - `wallet_pool.rs`: Provides the wallet pool system for generating and managing ephemeral wallets for arbitrage operations.

### /src/webhooks/
- **Purpose:** Webhook integration for real-time DEX pool updates, QuickNode notifications, and event-driven pool monitoring.
    - `integration.rs`
        - **Purpose:** Implements the main Axum-based webhook integration service for QuickNode POST handlers. Manages pool cache, event processing, and service initialization for real-time DEX updates.
    - `integration_new.rs`
        - **Purpose:** (Empty) Placeholder for a new or refactored integration service.
    - `mod.rs`
        - **Purpose:** Module entry point. Documents the webhook system, re-exports core types and processors, and consolidates integration exports for pool monitoring and event handling.
    - `pool_monitor.rs`
        - **Purpose:** (Empty) Placeholder for pool monitoring logic, likely intended for tracking pool health or status.
    - `processor.rs`
        - **Purpose:** Implements the pool update processor, handling real-time webhook notifications, updating pool information, and managing callback pipelines for event-driven updates.
    - `quicknode_converter.rs`
        - **Purpose:** (Empty) Placeholder for logic to convert QuickNode webhook payloads or data formats.
    - `server.rs`
        - **Purpose:** Implements the Axum-based webhook server for receiving QuickNode notifications. Handles authentication, request routing, and forwards opportunities to the arbitrage orchestrator.
    - `types.rs`
        - **Purpose:** Defines all types and structures for webhook handling, including QuickNode webhook payloads, block data, arbitrage transactions, DEX swaps, token transfers, and event types.

### /src/websocket/
- **Purpose:** Real-time WebSocket infrastructure for price feeds, DEX-specific feeds, and market data aggregation.
    - `manager.rs`
        - **Purpose:** Provides enhanced connection and subscription management for Solana WebSocket feeds, including circuit breaker and retry logic for robust error handling.
    - `market_data.rs`
        - **Purpose:** Defines the `CryptoDataProvider` trait for asynchronous crypto price data retrieval, allowing for flexible integration of price sources.
    - `mod.rs`
        - **Purpose:** Module entry point. Re-exports core traits, price feed infrastructure, and DEX-specific WebSocket feeds. Organizes all WebSocket-related modules.
    - `price_feeds.rs`
        - **Purpose:** Implements the main real-time price feed infrastructure, including connection management, event types, metrics, and price update structures for all supported DEXs.
    - `feeds/`
        - **Purpose:** DEX-specific WebSocket feed implementations for real-time price and liquidity data.
        - `lifinity.rs`
            - **Purpose:** Lifinity WebSocket feed implementation, supporting pool concentration levels, message parsing, and real-time updates for Lifinity pools.
        - `meteora.rs`
            - **Purpose:** Meteora WebSocket feed implementation, supporting Dynamic AMM, DLMM, and legacy pool types, with real-time pool update handling.
        - `meteora_full.rs`
            - **Purpose:** (Empty) Placeholder for a full-featured Meteora WebSocket feed or extended implementation.
        - `mod.rs`
            - **Purpose:** Module entry point for DEX-specific feeds. Re-exports all feed implementations for unified access.
        - `orca.rs`
            - **Purpose:** Orca Whirlpools WebSocket feed, subscribing to Solana RPC WebSocket account change notifications for real-time price monitoring.
        - `phoenix.rs`
            - **Purpose:** Phoenix DEX WebSocket feed, production-ready implementation for real-time order book and price updates.
        - `raydium.rs`
            - **Purpose:** Raydium WebSocket feed, production-ready implementation for real-time account monitoring and price updates.

---

## Refactoring Opportunities and Recommendations

This section identifies concrete, categorized refactoring opportunities for the Chilliez codebase, based on the comprehensive functional map above. Each opportunity includes rationale and suggested next steps.

### 1. Code Organization & Structure
- **Consolidate Empty/Placeholder Modules:**
    - `/src/functions/`, `/src/optimization/`, `/src/testing/integration_tests.rs`, `/src/testing/performance_tests.rs`, `/src/discovery/mod.rs`, `/src/utils/parsers/`, `/paper_trading_reports/` are empty or placeholders. Remove or implement as needed to reduce clutter and clarify project structure.
    - **Next Steps:** Remove empty files/folders if not planned for near-term use, or add `TODO` comments with intended purpose.

- **Unify Metrics Modules:**
    - Both `/src/local_metrics/` and `/src/metrics/` define similar `Metrics` structs. Consider merging or clearly separating their purposes (e.g., local/dev vs. production/external metrics).
    - **Next Steps:** Audit usage, refactor into a single module with feature flags or clear separation.

### 2. Code Quality & Idiomatic Rust
- **Clippy Lint Compliance:**
    - Run `cargo clippy` and address all warnings, focusing on idiomatic Rust, performance, and safety improvements.
    - **Next Steps:** Systematically fix lints, document exceptions, and enforce clippy in CI.

- **Consistent Error Handling:**
    - Standardize error types and propagation using `thiserror` and `anyhow` throughout all modules (especially in DEX clients, orchestrator, and API layers).
    - **Next Steps:** Refactor custom error types for consistency and ergonomic usage.

- **Reduce Code Duplication:**
    - Several modules (e.g., DEX math, metrics, routing) have similar logic. Abstract common patterns into shared utilities or traits.
    - **Next Steps:** Identify duplicated code, extract into reusable components.

### 3. Performance & Maintainability
- **Optimize Caching Strategies:**
    - Multiple caching layers exist (e.g., `/src/cache.rs`, `/src/performance/cache.rs`, `/src/arbitrage/jupiter/cache.rs`). Unify caching logic or clearly document their distinct roles.
    - **Next Steps:** Audit cache usage, refactor for single-responsibility, and document cache hierarchy.

- **Parallelism & Concurrency:**
    - Ensure all parallel/concurrent code (e.g., orchestrator, performance, metrics) uses idiomatic Rust concurrency primitives and is free of race conditions.
    - **Next Steps:** Review thread safety, use `Arc`, `Mutex`, `RwLock` as appropriate, and add tests for concurrency bugs.

### 4. Testing & Documentation
- **Expand and Unify Test Coverage:**
    - Some test modules are empty or duplicated. Consolidate tests, remove legacy/unused files, and ensure all critical paths are covered.
    - **Next Steps:** Remove or implement empty test files, unify test structure, and add missing integration/unit tests.

- **Documentation Consistency:**
    - Ensure all modules, especially public APIs and complex logic (DEX clients, orchestrator, routing), have up-to-date doc comments and usage examples.
    - **Next Steps:** Add or update Rustdoc comments, and generate documentation regularly.

### 5. Configuration & Environment
- **Centralize Configuration Management:**
    - Multiple config files and modules exist. Consider centralizing configuration loading, validation, and environment management.
    - **Next Steps:** Refactor config logic into a single entry point, document all config options, and validate at startup.

---

## Refactoring Roadmap (Prioritized)

1. **Remove or Document Empty/Placeholder Files and Folders**
2. **Run and Fix All Clippy Lints**
3. **Unify Metrics and Caching Logic**
4. **Standardize Error Handling and Reduce Duplication**
5. **Expand and Consolidate Test Coverage**
6. **Centralize and Document Configuration**
7. **Review and Optimize Concurrency/Parallelism**
8. **Update and Enforce Documentation Standards**

Each step should be validated with code quality tools (`clippy`, `fmt`, and tests) and documented in the project log.
