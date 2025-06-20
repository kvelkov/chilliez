# Orca DEX Client Implementation (Chilliez Arbitrage Bot)

## Overview
The Orca DEX client in Chilliez is responsible for integrating with Orca Whirlpools (CLMM) on Solana, fetching pool data, and performing precise, production-grade swap math. This document details:
- How pools are fetched and parsed
- Where and how math is performed
- How the results are consumed by other agents in the system

---

## 1. Pool Fetching and Parsing

- **On-chain Data Fetching:**
  - Pool addresses are loaded from a static JSON file (`config/orca_whirlpool_pools.json`).
  - For each address, the client fetches account data from Solana using the `SolanaRpcClient`.
  - The raw account data is parsed into a `WhirlpoolState` struct, which contains all relevant on-chain state for a Whirlpool pool (liquidity, sqrt_price, tick index, vaults, etc).
  - The parsed data is then mapped into a `PoolInfo` struct, which is the unified pool representation used throughout the bot.

- **API Data Fetching:**
  - The client can also deserialize external API responses into `OrcaApiResponse` and `OrcaApiPool` structs for off-chain analytics or monitoring.

---

## 2. Math: Where and How Calculations Are Performed

- **Precise Math Location:**
  - All production-grade math for Orca Whirlpools (CLMM) is implemented in `src/dex/math/orca.rs`.
  - The main entry point is `calculate_whirlpool_swap_output`, which computes the output amount, new sqrt price, new tick, fee, and price impact for a swap.
  - This function is called directly by the Orca client when a quote is needed for a Whirlpool pool.
  - The math module also provides helpers for tick/price conversion, slippage, and pool state validation.

- **Fallback Math:**
  - For classic (non-Whirlpool) Orca pools, the client falls back to a generic constant product formula from `src/utils/mod.rs`.

---

## 3. Consumption: How Orca Output Is Used

- **Quote Calculation:**
  - The main consumer of Orca math is the `calculate_onchain_quote` method in the Orca client (`src/dex/clients/orca.rs`).
  - This method is called by the arbitrage orchestrator and routing logic to determine expected output for a given input amount and pool.

- **Downstream Consumers:**
  - The output of `calculate_onchain_quote` (a `Quote` struct) is used by:
    - The arbitrage orchestrator (`src/arbitrage/orchestrator/core.rs`) to evaluate and select profitable routes.
    - The routing engine and smart router modules to compare swap opportunities across DEXes.
    - The simulation and paper trading modules for backtesting and analytics.

- **Instruction Building:**
  - The Orca client is also responsible for building swap instructions, but this is currently a stub and not implemented in the provided code.

---

## 4. Implementation Details and Refactor Summary (2025-06-20)

### Refactor & Cleanup (2025-06-20)
- All redundant pool construction logic is now consolidated in `build_orca_pool_info`, used by all parsing and discovery code paths.
- Error handling is unified: all trait methods use `anyhow::Error` for compatibility, with internal helpers using idiomatic error handling.
- All public functions, trait methods, and the `OrcaClient` struct are fully documented with `///` doc comments.
- The client is fully validated with `cargo check`, `cargo fmt`, `cargo clippy -- -D warnings`, and all tests (Orca logic passes; unrelated test failures remain).
- The code is now more maintainable, with less duplication, consistent error handling, and improved documentation.

### Current Limitations
- Swap instruction building is stubbed and not yet implemented.
- Token metadata (symbol, decimals) is not resolved in the pool parser and is set to placeholder values.
- Classic (non-Whirlpool) pool support is fallback only; all production math for Whirlpools is in `math/orca.rs`.

---
*Last updated: 2025-06-20 by GitHub Copilot*
