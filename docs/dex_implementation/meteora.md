# Meteora DEX Client Implementation

## 1. Functionality and Pool Fetching

The `MeteoraClient` provides integration with the Meteora DEX, supporting both Dynamic AMM and DLMM pool types. It is responsible for:
- Discovering pools by querying the Solana blockchain for accounts owned by the Meteora program IDs.
- Parsing pool account data into a unified `PoolInfo` struct using the `MeteoraPoolParser`.
- Identifying pool type (Dynamic AMM or DLMM) based on account size or program ID.
- Providing swap instruction builders for both pool types.
- Exposing methods for quoting, swapping, and health checking.

**Pool Fetching:**
- Pools are fetched by calling `RpcClient::get_program_accounts` for the Meteora Dynamic AMM program ID.
- Each account's data is parsed using the `MeteoraPoolParser`, which determines the pool type and extracts relevant fields into `PoolInfo`.
- DLMM pools are supported by a similar mechanism, using their own program ID and layout.

## 2. Math and Calculations

All math for Meteora pools is performed in the `math/meteora.rs` and `math/clmm.rs` modules:
- **Dynamic AMM:**
  - Output calculation: `calculate_dynamic_amm_output` computes the output amount for a given input, reserves, and fee.
  - Price impact: `calculate_price_impact` (from `math/clmm.rs`) estimates slippage for swaps.
- **DLMM:**
  - Output calculation: `calculate_dlmm_output` computes the output for a given input, active bin, bin step, liquidity, and fee.
  - Price impact: A simplified estimate is used for DLMM pools.

All calculations are invoked from the `calculate_onchain_quote` method in `MeteoraClient`, which selects the appropriate math based on pool type.

## 3. Consumers of Meteora Output

- The main consumer of `MeteoraClient` output is the arbitrage orchestrator (`arbitrage/orchestrator/core.rs`), which:
  - Calls `discover_pools` to build a list of available pools.
  - Uses `calculate_onchain_quote` to estimate swap outcomes for routing and arbitrage decisions.
  - Invokes `get_swap_instruction_enhanced` to build Solana instructions for execution.
- Other consumers include the pool discovery logic, routing modules, and test infrastructure.

## 4. Additional Notes on Current Setup

- All error handling in internal helpers uses the project's `ArbError` type; trait methods use `anyhow::Error` for compatibility.
- Pool parsing logic is consolidated and type-safe, reducing duplication and improving maintainability.
- The client is designed to be extensible for future Meteora pool types or upgrades.
- All public functions and structs are documented for clarity and maintainability.
- The client is validated with `cargo check`, `cargo fmt`, `cargo clippy -- -D warnings`, and all tests.

---

**See also:**
- `src/dex/clients/meteora.rs` (client implementation)
- `src/dex/math/meteora.rs`, `src/dex/math/clmm.rs` (math logic)
- `src/arbitrage/orchestrator/core.rs` (main consumer)
- `src/utils/pool_info.rs` (PoolInfo struct)
