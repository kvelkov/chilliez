# Lifinity DEX Client Implementation

## Overview
The Lifinity client integrates with the Lifinity DEX on Solana, supporting proactive market making and concentrated liquidity pools. It is responsible for discovering pools, parsing on-chain data, constructing swap instructions, and providing on-chain price/quote calculations for arbitrage and routing logic.

---

## 1. Functionality and Pool Fetching

### Pool Discovery
- **On-chain Fetching:**
  - The client fetches all pool accounts using the Lifinity program ID via Solana RPC (`get_program_accounts`).
  - Each pool account's data is parsed using the `LifinityPoolParser`, which unpacks the binary layout into a `PoolInfo` struct.
  - Token vaults and mint accounts are fetched concurrently for each pool to populate reserve and metadata fields.
  - The current implementation logs discovered pools and returns an empty vector (parsing logic is stubbed for future completion).

### Pool Data Parsing
- **Parser:**
  - `LifinityPoolParser` implements the `PoolParser` trait.
  - It unpacks the pool state, token vaults, and mint data, and constructs a `PoolInfo` struct with all relevant fields (reserves, mints, fee structure, liquidity, sqrt_price, tick index, oracle, etc).

### Swap Instruction Construction
- **Instruction Building:**
  - The client builds swap instructions using the `build_swap_instruction` method, which encodes the swap direction, input/output amounts, and all required accounts for the Lifinity program.
  - The instruction is returned for downstream transaction construction and submission.

### API/Traits
- Implements the `DexClient` and `PoolDiscoverable` traits, exposing methods for:
  - `discover_pools` (async, fetches and parses all pools)
  - `fetch_pool_data` (async, fetches and parses a single pool)
  - `calculate_onchain_quote` (performs math for a swap quote)
  - `get_swap_instruction` and `get_swap_instruction_enhanced` (builds swap instructions)
  - `health_check` (returns client health status)

---

## 2. Math and Calculations

### Math Location
- **All Lifinity math is performed in `src/dex/math/lifinity.rs`.**
- The client calls `calculate_lifinity_output` for all on-chain quote calculations.
- Math is never duplicated in the client; all production-grade math is centralized in the math module.

### Types of Calculations
- **Proactive Market Making:**
  - Calculates output amount for a given input, using reserves, fee bps, and (optionally) an oracle price.
  - Adjusts for concentration and inventory, supporting Lifinity's unique AMM design.
- **Oracle Price Calculation:**
  - The client can calculate a price from reserves as a fallback if the oracle is unavailable.
- **Slippage Estimate:**
  - The client estimates slippage based on input size and pool reserves.

---

## 3. Consumption of Lifinity Client Output

### Downstream Consumers
- **Arbitrage Engine:**
  - The `ArbitrageOrchestrator` (in `src/arbitrage/orchestrator/core.rs`) is the primary consumer.
  - It calls `DexClient` trait methods to discover pools, fetch pool data, and request swap quotes for routing and opportunity detection.
- **Routing/Simulation:**
  - The output of the Lifinity client (quotes, instructions, pool info) is used by the arbitrage engine and routing logic to simulate, validate, and execute trades.
- **Test Infrastructure:**
  - Unit and integration tests in `src/arbitrage/tests.rs` and related files use the client for mock and real pool discovery and quoting.

---

## Summary Table
| Component                | Role/Functionality                                      |
|--------------------------|--------------------------------------------------------|
| LifinityClient           | Main entry point, implements DexClient/PoolDiscoverable |
| LifinityPoolParser       | Parses on-chain pool data into PoolInfo                 |
| math/lifinity.rs         | All swap math and calculations                          |
| ArbitrageOrchestrator    | Consumes client for routing/arbitrage                   |
| PoolInfo                 | Standardized pool data struct for all DEXes             |

---

## References
- [`src/dex/clients/lifinity.rs`](../../src/dex/clients/lifinity.rs)
- [`src/dex/math/lifinity.rs`](../../src/dex/math/lifinity.rs)
- [`src/arbitrage/orchestrator/core.rs`](../../src/arbitrage/orchestrator/core.rs)
- [`src/utils/pool_info.rs`](../../src/utils/pool_info.rs)
