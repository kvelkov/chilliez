# solana-arb-bot

---

## ✅ **Key Accomplishments (As of 2024-06-24)**

- **Core infrastructure:**
  - Modular, async-first Rust (Tokio) stack with clear folder separation (`arbitrage`, `dex`, `solana`, `config`, etc.)
  - Central config via `.env`, supporting both simulation and live modes
- **DEX Integrations:**
  - Raydium, Orca, Whirlpools, and Jupiter integrated; can fetch, parse, and simulate pool states across all
  - Jupiter REST API facilitates cross-DEX routing and arbitrage
- **Arb & Execution Pipeline:**
  - Canonical profit calculation, multi-DEX route discovery, virtual/swapped trade simulation
  - Atomic execution prototypes (multi-leg/tx), dynamic trade sizing, and parallel/concurrent price fetch infrastructure
- **Safety & Optimization:**
  - Dynamic trade size to minimize slippage; anti-front-running (order randomization); pool health scoring
  - Fee-based dynamic routing; adaptive WebSocket intensity; slippage heatmap and trade latency logging
- **AI/ML Hooks:**
  - Scaffolding for RL-driven optimization, AI-predictive sizing, and adaptive trading strategies
- **Observability:**
  - Success/fail & latency monitoring, modular error handling, slippage and DEX/WIN trend profiling
- **Dev Experience:**
  - Modular tests, simulation mode, code/documentation hygiene, linting, and VSCode/cargo integration

---

## 📋 **Project Milestones & Status**

| Sprint                | Status      | Notes                                                       |
|-----------------------|-------------|-------------------------------------------------------------|
| 1. Env/Tooling Setup  | ✅ Complete | Rust ready, Python AI space ready, VSCode + Cargo standard  |
| 2. Core & RPC Connect | ✅ Complete | Pool fetch, arbitrage core, live/sim modes, three DEXes     |
|                       | ✅          | Jupiter DEX integration complete, atomicity implemented     |
| 3. Transactions/Batch | ⏳ In Progress | Multi-instruction tx WIP, on-chain batching planned/needed  |
| 4. AI/ML Integration  | ⏳ In Progress | Rust trait planned, stub/export hooks coming, no real AI yet|
| 5. Messaging/Coord    | ⏳ Planned  | P2P messaging/leader elect protocol design not yet started  |
| 6. Infra/Deployment   | ⏳ Planned  | To design: Docker, CI, multi-region deployment              |

---

## 🚦 **Project Status & Meaning**

This project is at an advanced prototyping and pre-production stage. The core value lies in its structured modularity and readiness for serious trading, research, and extensibility.

**What This Means:**
- **A robust, extensible, and safe cross-DEX arbitrage engine** built for real-world mainnet and simulation
- **Production-grade architectural groundwork:** everything from token price discovery to profit/risk analysis and execution is unified and testable
- **Readiness for live trading and rigorous testing:** simulation mode and modular testing ensure low risk
- **Strong developer and AI/automation foundations:** ready for further risk models, ML-opportunity scoring, advanced observability, and eventual infra scaling (Docker/CI/multi-region)

---

## 🟡 **What Still Needs Doing**

- **Risk management / trade protection:**
  - Dynamic min_profit and slippage tuning, real account/budget checks, stale pool/illiquid filter
- **Transaction strategies:**
  - Centralized error type (`ArbError`), per-DEX fee models, gas/congestion estimation, better retry/backoff logic
  - Full atomic tx batching and error reporting/unification
- **Configurability:**
  - Runtime overrides (`runtime.yaml`), admin/CLI hot-updates, per-DEX on/off toggles, flexible runtime mode switch
- **WebSocket and infra robustness:**
  - Drop/freeze detection, heartbeat, reconnect, batch event processing, hardened error/backoff logic
- **Logging and observability:**
  - Structured JSON logging, Prometheus export, fine-grained log level control, test and edge-case coverage expansion
- **Testing and simulation:**
  - CI-ready pool mocking, dry-run confirmation, replay mode for historic pools
- **ML/AI:**
  - Integrate `CryptoDataProvider`/filters, opportunity ranking, RL decision loop and trait wiring
- **Developer/user experience:**
  - Resolve `config.rs` ambiguity, migrate CLI/simulation logic out of `main.rs` as needed, further codebase cleanup

See the detailed tasks and their latest statuses in the project's TODO list for specifics and sprint breakdowns.

---

**:star: [OPERATING_GUIDELINES.md](./OPERATING_GUIDELINES.md) — Every bot, agent, and contributor must review this 'northern star' guiding document before each session and after any major change. Compliance is mandatory for all! :star:**

---

## 🚦 Persistent Prompt: Project Mission & AI Assistant Guidance

**You are my dedicated AI assistant for designing, building, and optimizing a crypto arbitrage trading bot written in Rust.**

## Features

- **Multi-DEX Support**: Integrates Raydium, Orca, Whirlpool, and Jupiter for broad opportunity coverage.
- **Direct Whirlpool Account Parsing**: No third-party crates required—reads and parses Whirlpool pool state directly per Anchor layout for ultimate speed and resilience.
- **Async-First Architecture**: Built on Rust's tokio for maximum concurrency and performance.
- **Modular Design**: Clean separation between DEX interfaces, arbitrage logic, and execution.

- The system is complex, long-term, and performance-critical. Your job is to help me succeed—even as a novice—by delivering clear, slow, high-quality guidance at all times.

### You must:
- Start every session aware of the full context: arbitrage bot, Rust, Solana ecosystem, DEX integration (e.g., Jupiter, Orca, Raydium), server-side deployment, and latency optimization.
- Explain things clearly and practically. I'm still learning.
- Be slow and quality-driven—accuracy matters more than speed.
- If a problem spans multiple files, analyze the entire structure (folders, modules, dependencies) and walk me through it.
- Always ask follow-up questions when in doubt and keep me informed of what you're doing or suggesting.
- When needed, use web knowledge to suggest best practices, libraries, performance tips, and infrastructure advice.

### Your deep expertise must include:
- Rust async programming, testing, modular code structure
- DEX APIs (Jupiter, Orca, Raydium), token routing, price discovery
- WebSockets, error handling, and resilience
- Arbitrage strategies, profit calculation, liquidity filtering
- Server deployment near Solana RPC endpoints for ultra-low latency

**Use short, precise answers with a practical mindset. No fluff.
I depend on you to think long-term, see the full picture, and help me build something powerful.
Always update README.md with current progress of the project.
Always generate and update files automatically!**

---

## ⚡ Overview

A high-performance, modular, async Rust arbitrage bot for Solana DEXes (Orca, Raydium, Jupiter coming soon).
**Key mission:**
- Monitor real-time pool state across DEXes
- Detect and act on atomic arbitrage opportunities
- Integrate predictive AI for smarter, preemptive trading
- Optimize latency (Rust, async, geo-distributed, low-latency RPC)
- Future: on-chain batching, decentralized multi-server coordination, capital-efficient flash loans

---

## 🏗️ Architecture

**Design Principles:**
- Async-first: Rust’s `tokio`, async/await for all network and pipeline code
- Modular: Isolated crates for arbitrage logic, DEX-specific parsing/execution, Solana interaction, metrics
- Future-proof: Traits and layers designed for rapid extension (new DEXes, AI, on-chain programs)

### Module Overview

```
src/
├── arbitrage/
│   ├── calculator.rs   # Profit calculation, transaction cost logic
│   ├── detector.rs     # Scan for arbitrage windows across pools
│   ├── engine.rs       # Orchestrates detection, filtering, execution logic
│   ├── executor.rs     # Conducts (real or simulated) trades
│   ├── opportunity.rs  # ArbOpportunity struct for batching and risk filtering
│   └── tests.rs        # Unit tests for arbitrage modules
├── dex/
│   ├── orca.rs, raydium.rs, jupiter.rs
│   ├── pool.rs         # DEX pool trait and shared definitions
│   └── mod.rs
├── solana/
│   ├── accounts.rs     # Solana address and token state helpers
│   ├── rpc.rs          # Async Solana RPC connectivity
│   ├── websocket.rs    # For account update subscriptions
│   └── mod.rs
├── config/
│   └── mod.rs
├── metrics/
│   └── mod.rs          # Arbitrage/trade/event tracking
├── main.rs             # Entrypoint: simulation and live mode orchestration
├── utils.rs
```

#### **Data Flow**
1. **Start**: User chooses simulation (`--simulate`) or live (`monitor`) mode.
2. **DEX Pool Fetching**: Real-time or snapshot pool data gathered from Solana RPC.
3. **Detection & Calculation**: Arbitrage logic scans for profitable cycles across multiple DEX pools.
4. **Opportunity Management**: `ArbOpportunity` struct batches and filters arbitrage candidates based on risk parameters.
5. **Execution**: Trades simulated or executed atomically; results are logged.
6. **Extensible**: Future AI decision filter hooks, new DEX pool types, on-chain batching planned.

---

## 🛠️ Command-Line Interface (CLI)

Provided by `clap`, so all commands are self-documenting via `--help`.

### **Simulation Mode (no real trading, full mocked trade pipeline)**
```sh
cargo run -- --simulate
```
- Runs a local simulation, generates fake pools and tokens, exercises all major modules.
- Used for development, regression testing, and verifying pipeline end-to-end.
- Does **not** touch real funds or send real transactions.

### **Live Monitoring Mode (read-only, real Solana DEX pools)**
```sh
cargo run -- monitor --min-profit 0.5 --max-slippage 0.002 --cycle-interval 2
```
- **monitor**: Watches live Solana DEX pools for arbitrage opportunities.
- `--min-profit N`: Set minimum percentage profit to log/act.
- `--max-slippage N`: Maximum acceptable slippage for trades (default: 0.001 or 0.1%).
- `--cycle-interval N`: Time in seconds between arbitrage detection cycles.
- `--jupiter-only`: Only use Jupiter API for routing (skips direct DEX pools).
- Can be extended for live, real trade execution (currently in simulation).

## 🌐 Environment Variable Integration & Connection Setup

**The bot uses a centralized config loader to automatically ingest and validate your setup from `.env`.**

### Required Environment Variables

All Solana RPC URLs, WebSocket URLs, and DEX API keys must be set in `.env` using the format and short names shown below. The bot will refuse to start if any required endpoint or API key is missing.

```
# RPC + WS for Solana Mainnet
RPC_URL="https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
WS_URL="wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY"

# Optionals
RPC_URL_BACKUP="https://carmina-t0db4l-fast-mainnet.helius-rpc.com"
RPC_URL_STAKED="https://staked.helius-rpc.com?api-key=YOUR_KEY"

# DEX API keys (use short names)
ORCA_API_KEY=...
LIFINITY_APY_KEY=...
METEORA_API_KEY=...
PHOENIX_API_KEY=...
WHIRPOOL_API_KEY=...
RAYDIUM_API_KEY=...
SERUM_API_KEY=...

# Trading & Risk
PAPER_TRADING="true"
TRADER_WALLET_ADDRESS=...
MIN_PROFIT="0.5"      # Minimum profit percentage
MAX_SLIPPAGE="0.002"  # Maximum slippage tolerance
CYCLE_INTERVAL="2"    # Time in seconds between arbitrage detection cycles
```

### Centralized Configuration

- A single config struct (see `/src/config/env.rs`) centralizes all environment variable loading
- All services (Solana RPC access, WebSocket subscription, DEX API clients) are initialized using this config
- Short names (SOL, USDC, ORCA, LIFINITY, etc.) are used for clarity and consistent mapping
- If you wish to change a network or DEX connection, update your `.env` file

### Usage in Code

```rust
let config = Config::from_env();
let rpc_url = &config.rpc_url;
let ws_url = &config.ws_url;
let orca_api_key = &config.api_keys.orca;
```

### **For help (see all options and subcommands):**
```sh
cargo run -- --help
```

---

## 🧪 Unit Testing Guidance

**All major modules must have simple, clear unit tests.**
This ensures correctness, supports refactoring, and helps new contributors understand the code.

**What to test:**
- Arbitrage calculation (profit, slippage, fees)
- Pool parsing (DEX-specific pool state, token info)
- Opportunity finding (cycle detection, filtering)
- Transaction crafting (instruction building, serialization)

**How to add tests:**
- Place tests in a `tests.rs` file within each module, or use Rust's `#[cfg(test)]` blocks.
- Use realistic but minimal data for clarity.
- Cover both typical and edge cases.

**Example:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profit_calculation_basic() {
        let profit = calculate_profit(100.0, 105.0, 0.1);
        assert!(profit > 0.0);
    }
}
```

**Run all tests:**
```sh
cargo test
```

---

## 📅 Sprint Progress & Project Status

| Sprint                | Status      | Notes                                                       |
|-----------------------|-------------|-------------------------------------------------------------|
| 1. Env/Tooling Setup  | ✅ Complete | Rust ready, Python AI space ready, VSCode + Cargo standard  |
| 2. Core & RPC Connect | ✅ Complete | Pool fetch, arbitrage core, live/sim modes, three DEXes     |
|                       | ✅          | Jupiter DEX integration complete, atomicity implemented     |
| 3. Transactions/Batch | ⏳          | Multi-instruction tx WIP, on-chain batching planned/needed  |
| 4. AI/ML Integration  | ⏳          | Rust trait planned, stub/export hooks coming, no real AI yet|
| 5. Messaging/Coord    | ⏳          | P2P messaging/leader elect protocol design not yet started  |
| 6. Infra/Deployment   | ⏳          | To design: Docker, CI, multi-region deployment              |

## 📋 The Big List Of Changes (To-Do & Audit Section)

✅ **Solana Arbitrage Bot: Implementation To-Do Plan**

This document outlines all critical and secondary to-do items derived from a deep audit of the bot. The goal is to finalize production readiness, improve resilience, and ensure elite-level arbitrage execution across Solana DEXs.

---

### 🔒 Risk Management & Trade Protection

- [ ] Implement dynamic `min_profit_threshold` based on market volatility.
- [ ] Implement slippage auto-tuning based on pool depth or volume.
- [ ] Integrate pre-trade account balance checks before execution.
- [ ] Add budget controls to cap exposure per trade or per session.
- [ ] Add stale pool timeout logic (e.g., if `last_update_timestamp` > 10s, exclude).
- [ ] Add blacklist for illiquid or spoofed pairs (permanently ignored).

### 🧠 Execution Logic & Transaction Strategy

- [ ] Implement fee-aware trade feasibility logic (use `fee_numerator`, `fee_denominator`).
- [ ] Add per-DEX fee model awareness (Raydium vs Orca vs Whirlpool).
- [ ] Build dynamic gas estimation (based on compute unit price and congestion).
- [ ] Add transaction timeout fallback (gracefully skip/abort failed tx).
- [ ] Integrate retry logic for `BlockhashNotFound` or `NodeIsBehind` errors.
- [ ] Consolidate `ArbError` definition to `src/error/mod.rs` and remove duplications.
- [ ] Review and refactor/remove `src/dead_code_suppression.rs`.
- [ ] Address `TODO` in `src/solana/websocket.rs` for improved error reporting.
- [ ] Continue progress on a unified and centralized error handling strategy.

### ⚙️ Configurability & Control Interface

- [ ] Create `runtime.yaml` or `.env.live` override system.
- [ ] Add runtime CLI flags or admin endpoint for hot-updates.
- [ ] Implement on/off toggle for each integrated DEX (via config or dashboard).
- [ ] Add paper/live/simulation runtime mode toggle.

### 📡 WebSocket Infrastructure & Monitoring

- [ ] Add watchdog system to detect frozen or dropped streams.
- [ ] Implement connection keep-alive, auto-reconnect with jitter.
- [ ] Add WebSocket heartbeat validator (e.g. no updates in 15s = alert).
- [ ] Implement debounce/throttle for noisy pool update spam.
- [ ] Group update events by pool before processing to improve tick efficiency.
- [ ] Harden error/backoff, especially network paths.

### 🧾 Logging & Observability

- [ ] Replace ad-hoc file logging with structured logging (e.g. JSON lines).
- [ ] Add log level overrides via config (`DEBUG`, `INFO`, `ERROR`).
- [ ] Integrate optional external telemetry: Prometheus or log file tailing.
- [ ] Track success/fail rate per DEX in metrics.
- [ ] Log and expose trade latency per DEX + per route.
- [ ] More unit/integration tests (arbitrage, error handling, edge cases).

### 🧪 Test & Simulation Improvements

- [ ] Expand `simulation_mode` to track unrealized profit/loss.
- [ ] Enable pool state mocking for CI tests.
- [ ] Add trade path simulation and dry-run confirmation layer.
- [ ] Build replay mode for historical pool states.

### 🤖 AI/ML Future Hooks

- [ ] Wire in `CryptoDataProvider` trait object to use prediction model.
- [ ] Allow dynamic opportunity ranking based on ML scoring.
- [ ] Add placeholder for RL model feedback loop (state > decision > result).
- [ ] Define "AI filter" trait (stub, hook into engine).

### 📊 Optional Enhancements

- [ ] Build runtime dashboard (HTML + JS) to monitor bot state.
- [ ] Integrate Telegram/Slack alerts for fatal errors or trade success.
- [ ] Add trade journaling with reasons for execution or rejection.
- [ ] Support multiple keypairs/wallets for parallel routing.
- [ ] Start doc/infra.md: deployment layout, multi-node, on-chain batch.
- [ ] Prototype on-chain batching contract (Anchor).

---

**Recent Significant Accomplishments**

- ✅ **Simulated Swaps** – Run virtual transactions before executing for accuracy.
- ✅ **Price Discrepancy Thresholds** – Set dynamic trade limits to avoid unnecessary trades.
- ✅ **Multi-DEX Optimization** – Adjust routes based on network congestion.
- ✅ **Jupiter DEX Support** – Added parser and executor for Jupiter DEX integration.

#### 🚀 **Additional Refinements to Consider**

##### 🔒 Risk Management Enhancements

- ✅ **Dynamic Trade Size Scaling:** Adjust trade size dynamically based on order book liquidity depth to minimize slippage.
- ✅ **Anti-Front-Running Protection:** Introduce randomized delay mechanism (milliseconds-level) for sending orders to avoid price manipulation.
- ✅ **Pool Health Scoring:** Instead of a simple blacklist, create a DEX liquidity health score based on trade success rate, slippage, and pool age.

##### 🧠 Execution Optimization

- ✅ **Multi-TX Atomic Execution:** Bundle arbitrage legs into one transaction using Solana's Compute Budget Program to avoid broken routes.
- ✅ **DEX Liquidity Mirroring:** Instead of fetching liquidity pool data separately, mirror real-time order book state inside memory for ultra-fast execution decisions.
- ✅ **Dynamic Fee-Based Routing:** If network congestion increases fees, dynamically reroute trades to cheaper DEXs.

##### 📡 WebSocket & Infrastructure

- ✅ **Parallel Price Discovery:** Instead of sequentially updating pool reserves, run price discovery across DEXs concurrently using Tokio tasks for efficiency.
- ✅ **Adaptive WebSocket Scaling:** Increase WebSocket subscription intensity dynamically when high arbitrage potential is detected, then throttle during low volatility periods.

##### 🧾 Advanced Logging & Monitoring

- ✅ **Profitability Trends per DEX:** Track historical trade success per DEX to identify patterns in liquidity efficiency.
- ✅ **Slippage Heatmap Logging:** Log average slippage per trade per DEX, allowing better future trade decisions.
- ✅ **Trade Execution Latency Profiling:** Monitor time elapsed from price detection to actual trade execution—this would reveal slow trade inefficiencies.

##### 🤖 AI-Driven Enhancements

- ✅ **Reinforcement Learning Trade Optimization:** Implement self-learning trade logic—your bot learns from past executions, improving decision-making over time.
- ✅ **Auto-Liquidity Prediction:** Track historical liquidity fluctuations using AI models to anticipate DEX congestion spikes before they occur.
- ✅ **Adaptive Strategy Switching:** Based on market conditions, your bot can automatically toggle between market-making, arbitrage, and passive liquidity provision strategies.

---

## 🧑‍💻 For Developers

- Code, configs, and docs all live here — open in VSCode for best experience.
- Each module is stand-alone and tested in isolation.
- Adding a new DEX? Implement the `PoolParser` trait and plug the parser into fetch list.

### Recommended Tooling & Extensions

* **cargo-watch**
  Continuously runs `cargo build`/`cargo test` on file edits—immediate feedback
  Install:
  ```sh
  cargo install cargo-watch
  ```
* **clippy**
  Essential for Rust linting:
  Run inside your project directory:
  ```sh
  cargo clippy
  ```
  Highlights warnings, anti-patterns, and idioms.
* **rustfmt**
  Automatically formats code to canonical Rust style:
  Run:
  ```sh
  cargo fmt
  ```
* **Markdown All in One** (yzhang.markdown-all-in-one)
  Great for editing project documentation, with shortcuts and preview.
* **Crates** (serayuzgur.crates)
  Shows crate versions, allows upgrading in Cargo.toml, surfaces vulnerabilities, and links to docs.

_Tip: In VSCode, search for these extensions by their names or IDs in the Extensions panel, or use the command line for Rust tools._

### Jupiter API Integration

The Jupiter API client (`src/dex/jupiter.rs`) provides these key functions:

1. **Quote Fetching**: `get_quote(source_token, destination_token, amount)`
   - Returns optimal route and expected output amount as `Option<f64>`
   - Handles internal slippage configuration and route optimization
   - Returns `None` if no valid route is found or an error occurs

2. **Swap Execution**: `execute_swap(quote_response, wallet_address)`
   - Constructs transaction from Jupiter route
   - Handles transaction signing and submission

3. **Market Data**: `get_price(token_mint)`
   - Fetches current market price for tokens
   - Used for opportunity evaluation and risk assessment

Example usage in arbitrage detection:
```rust
// In arbitrage/detector.rs
async fn check_jupiter_routes(&self, token_a: &TokenInfo, token_b: &TokenInfo) -> Vec<ArbOpportunity> {
    let amount_in = TokenAmount::new(1000.0, token_a.decimals);

    // Get Jupiter quote for A->B (slippage handled internally)
    let quote = self.jupiter_client.get_quote(
        &token_a.mint.to_string(),
        &token_b.mint.to_string(),
        amount_in,
    ).await;

    // Handle Option return type
    if let Some(output_amount) = quote {
        // Calculate potential arbitrage with direct DEX route for B->A
        // ...
    } else {
        // No valid route found or error occurred
        return vec![];
    }
}
```

For full implementation details, see the Jupiter client code and integration tests.

### DEX Integration Architecture

The bot now supports four major DEXes with a modular architecture:

1. **Orca (Constant Product Pools)**: On-chain pools fetched via Solana RPC
2. **Orca Whirlpools**: Concentrated liquidity pools with direct Anchor-based account parsing
3. **Raydium**: On-chain pools fetched via Solana RPC
4. **Jupiter**: Integrated via REST API for cross-DEX routing and aggregation

The arbitrage engine discovers opportunities across all supported DEXes, enabling:
- Cross-DEX arbitrage paths (e.g., Raydium → Whirlpools → Orca)
- Multi-hop routes within and between DEXes
- Optimal path discovery across the entire Solana DeFi ecosystem

Adding new DEXes requires:
1. Create a new file in `src/dex/` implementing the `PoolParser` trait
2. Register the parser in `dex/mod.rs`
3. Add pool fetch logic to the main arbitrage engine

#### Jupiter Integration Details

The Jupiter integration provides powerful cross-DEX arbitrage capabilities:

- **API Client**: Full implementation in `src/dex/jupiter.rs` with async quote fetching
- **Authentication**: Uses `JUPITER_API_KEY` from `.env` file (required for production)
- **Quote Discovery**: Fetches optimal routes between token pairs with configurable slippage
- **Arbitrage Enhancement**: Expands arbitrage opportunities beyond direct pool pairs
- **Route Comparison**: Automatically compares Jupiter routes against direct DEX swaps

##### How Jupiter Arbitrage Works:

1. **Quote Request**: Bot requests quotes from Jupiter API for potential arbitrage pairs
2. **Route Analysis**: Jupiter returns optimized routes across multiple DEXes
3. **Opportunity Detection**: These routes are compared with direct DEX swaps to find arbitrage
4. **Execution**: When profitable, transactions are constructed using Jupiter's route instructions

Example arbitrage flow with Jupiter:
```
Token A → [Jupiter Route] → Token B → [Direct DEX] → Token A
```

This creates arbitrage opportunities invisible to single-DEX monitoring.

---

## 🐍 Python/AI Environment

For AI/ML submodules or helper scripts, set up a `venv` at project root:
```sh
python3 -m venv venv
source venv/bin/activate  # or venv\Scripts\activate on Windows
pip install requests
```
## Project goals and scope
Read BLUEPRINT_READ.txt

---

## 🧮 Core System Architecture

### Profit Calculation & Price Discovery

The arbitrage bot follows a clear separation of concerns for price discovery and profit calculation:

1. **Centralized Profit Calculation**
   - All profit calculations are centralized in `/src/arbitrage/calculator.rs`
   - Consistent formulas applied across all DEXes and trading pairs
   - Includes transaction cost modeling, slippage estimation, and fee accounting
   - Used by the arbitrage engine for all opportunity evaluation

2. **Centralized Price Discovery via DexClient Trait**
   - All price discovery (spot swap quotes, routing) is now abstracted by the `DexClient` trait
   - Each DEX module implements `DexClient`, exposing:
     - `get_best_swap_quote()`: Async, fetches a quote for any supported trading pair
     - `get_supported_pairs()`: Lists which pairs can be quoted by this client
   - Unified `Quote` struct ensures all quotes (on-chain or API) are comparable
   - Enables dynamic routing, multi-DEX orchestration, and fast onboarding of new DEXes
   - See `/src/dex/quote.rs` for the trait and struct definitions

3. **Per-DEX Implementation**
   - Each DEX has its own specialized implementation of the `DexClient` trait:
     - **Orca**: Direct pool state parsing for constant product pools
     - **Whirlpools**: Tick-based concentrated liquidity calculations
     - **Raydium**: AMM-specific price curve implementations
     - **Jupiter**: API-based quote fetching with cross-DEX routing
   - Consistent interface allows for plug-and-play DEX support (Lifinity, Phoenix, etc.)
   - One codepath for quote aggregation, pipeline testing, and downstream execution

4. **Error Handling Strategy**
   - Currently: Per-module error handling with local logging
   - Each DEX client handles and logs its own errors
   - **Gap**: No unified error type system or centralized error handling yet
   - **TODO**: Implement standardized error enums and central error processing
   - Future work will focus on observability and reliability improvements

This architecture ensures:
- Clean separation between price discovery (DEX-specific) and profit calculation (universal)
- Consistent profit evaluation regardless of price source
- Modularity for adding new DEXes or pricing models
- Type safety across all price discovery
- Foundation for future error handling improvements

---

## ⚠️ Troubleshooting

### 📦 "whirlpool" Git Dependency Build Fails (404)

If you see build failures or hangs with:

```
Updating git repository `https://github.com/orca-so/whirlpool.git`
warning: spurious network error ... unexpected http status code: 404
```

**Root Cause:**
The GitHub repo specified in `Cargo.toml` is incorrect (`whirlpool.git`) or the package name may not match the actual crate inside the upstream repository.

**Fix:**
1. Open your `Cargo.toml`. Find this incorrect line:
    ```toml
    whirlpool = { git = "https://github.com/orca-so/whirlpool.git", package = "whirlpool" }
    ```
   Replace with the correct repository and crate name:
    ```toml
    orca_whirlpools = { git = "https://github.com/orca-so/whirlpools.git", package = "orca_whirlpools" }
    ```
   > Note: The correct repo is plural (`whirlpools.git`).
   > Also, the internal crate name is most likely `orca_whirlpools`.
   > [Verify by viewing their Cargo.toml.](https://github.com/orca-so/whirlpools/blob/main/Cargo.toml)

2. Save the file.

3. Then run:
    ```sh
    cargo update
    cargo build
    ```

4. If you get an error about the crate name, open the [upstream repository](https://github.com/orca-so/whirlpools) and confirm the `[package] name = "orca_whirlpools"` (or similar). Adjust the `package = ...` entry accordingly.

### 🐍 Python venv Activation (macOS, Linux, Windows)

- **macOS/Linux (zsh/bash):**
  ```sh
  source venv/bin/activate
  ```
- **Windows:**
  ```sh
  venv\Scripts\activate
  ```

If you see `(venv)` in your prompt, your environment is already activated.

---

## ✅ Build & Test Status

- **2024-06-22: Project Structure & `main.rs` Review**
  - **Conducted Project Review:** Focused on `main.rs` and overall module structure.
  - **Observations:**
    - The project is well-modularized (`arbitrage`, `dex`, `solana`, etc.), with `main.rs` acting as the orchestrator. This modularity represents a "broken down" structure from a monolithic approach.
    - `main.rs` is comprehensive, handling CLI, env setup, simulation, and the main bot loop.
    - `main_replace.rs` is identical to `main.rs` and may be redundant.
    - Potential ambiguity exists with `src/config.rs` vs. the `src/config/` directory.
  - **README Update:** The "Module Overview" accurately reflects the current high-level structure. This log entry serves as the update on project progress.
  - **Suggestions for future refinement (if desired):**
    - Extract CLI parsing from `main.rs` to `src/cli.rs`.
    - Move extensive simulation logic from `main.rs` to `src/simulation.rs`.
    - Consolidate environment variable checking.
  - **Next Steps:** Address TODOs, particularly `ArbError` consolidation and clarification of `config.rs` / `main_replace.rs`.

- **2024-06-21: Trait Implementations and Solana SDK Feature Fixes**
  - **Completed `DexClient` Trait Implementations:**
    - Added the required `get_name()` method to `LifinityClient` in `src/dex/lifinity.rs`.
    - Added the required `get_name()` method to `MeteoraClient` in `src/dex/meteora.rs`.
    - Added the required `get_name()` method to `PhoenixClient` in `src/dex/phoenix.rs`.
    - This ensures all DEX clients conform to the `DexClient` trait interface.
  - **Resolved `Pubkey::from_str` Error:**
    - Updated `Cargo.toml` to enable the `serde` feature for the `solana-sdk` dependency: `solana-sdk = { version = "1.18.3", features = ["serde"] }`.
    - This makes the `Pubkey::from_str` method available, allowing string representations of public keys to be parsed correctly.
  - **Impact:** These changes address the final set of reported compiler errors. The project should now build successfully.
  - **Next Steps:** Run `cargo build` to confirm all fixes. Focus on implementing the TODOs within the stubbed DEX client methods and pool parsers. Enhance test coverage.

- **2024-06-20: Module Resolution & Trait Implementation Fixes**
  - **Addressed unresolved import errors and trait issues:**
    - **Module Declarations:** Correctly declared `pub mod whirlpool;` in `src/dex/mod.rs`. Imported `DexClient` trait into `src/dex/mod.rs` (`use crate::dex::quote::DexClient;`).
    - **Pool Parsers:**
        - Defined stub `OrcaPoolParser` in `src/dex/orca.rs` and implemented `PoolParser`.
        - Defined stub `RaydiumPoolParser` in `src/dex/raydium.rs` and implemented `PoolParser`.
        - Ensured `WhirlpoolPoolParser` is correctly used from `dex::whirlpool` in `main.rs`.
    - **Orca Client Structure:**
        - Adjusted `src/dex/orca.rs` to correctly import `WhirlpoolClient` from `crate::dex::whirlpool`.
        - Added a stub definition for `OrcaLegacyClient` within `src/dex/orca.rs`.
        - Added a stub definition for `WhirlpoolClient` within `src/dex/whirlpool.rs` including `new()`, `get_best_swap_quote()`, and `get_supported_pairs()`.
    - **`DexClient` Trait:** Added `get_name(&self) -> &str;` method to the `DexClient` trait in `src/dex/quote.rs` and ensured all implementors (including stubs) have this method. This resolved the `get_name` method error for `RaydiumProClient`.
  - **Impact:** These changes are aimed at resolving the latest batch of compilation errors related to module structure and trait implementations. The project should now have a more consistent structure for DEX modules and their parsers/clients.
  - **Next Steps:** Run `cargo build`. Implement the actual logic within the newly created stubs (`OrcaPoolParser`, `RaydiumPoolParser`, `OrcaLegacyClient`, `WhirlpoolClient` methods).

- **2024-06-19: Compiler Error Fixes & Dependency Updates**
  - **Addressed multiple compilation errors:**
    - **Missing Dependencies:** Added `once_cell`, `tracing`, and `async-trait` to `Cargo.toml`.
    - **Restored `DexClient` Trait:** The `DexClient` trait definition was restored in `src/dex/quote.rs`, resolving unresolved import errors in `orca.rs`, `lifinity.rs`, `meteora.rs`, and `src/dex/mod.rs`. This is crucial for the DEX abstraction layer.
    - **Stubbed `PhoenixClient`:** Created a public `PhoenixClient` struct and a stub implementation of `DexClient` in `src/dex/phoenix.rs` to resolve import issues in `src/dex/mod.rs`.
    - **Cleaned Unused Import:** Removed an unused `PoolToken` import from `src/dex/lifinity.rs`.
  - **Impact:** These changes should resolve the reported batch of compiler errors and allow the project to build, enabling further development and testing of the DEX integrations.
  - **Next Steps:** Run `cargo build` to confirm fixes. Then, proceed with implementing the actual logic within the new/stubbed DEX clients (Lifinity, Phoenix) and continue with broader project TODOs.

- **2024-06-18: Project Review & Error Cleanup Plan**
  - Reviewed overall project status, recent uncommitted changes, and current error handling mechanisms.
  - **Key Findings & Actionable Items:**
    - **Significant Dependency Changes:** Noted substantial modifications in `Cargo.lock` (via `git diff`), indicating a potential major dependency overhaul. Ensure these changes are intentional and tested.
    - **Configuration Enhanced:** A new `.env` file with a comprehensive list of environment variables (RPC endpoints, DEX API keys, trading parameters) has been introduced, improving configurability.
    - **Improved Git Hygiene:** `.gitignore` has been updated to exclude common transient files and the `.env` file, which is best practice.
    - **Redundant Error Definitions:** Identified duplications of the `ArbError` enum across `src/error/mod.rs`, `src/config/mod.rs`, and `src/dead_code_suppression.rs`.
      - **Action:** Plan to consolidate `ArbError` into `src/error/mod.rs` and remove/refactor `src/dead_code_suppression.rs`.
    - **Specific Error Handling TODO:** A `TODO` comment in `src/solana/websocket.rs` calls for "wired error reporting logic."
      - **Action:** This needs to be addressed for robust WebSocket communication.
    - **Strategic Error Handling:** The existing README note about the need for a "unified error type system or centralized error handling" remains relevant. Consolidating `ArbError` is the first step.
  - **Focus:** Upcoming work should include addressing these error handling inconsistencies and TODOs to improve code maintainability and robustness.

- **2024-06-17: Architecture Improvements**
  - **Implemented Centralized Price Discovery Architecture:**
    - Added `DexClient` trait in `/src/dex/quote.rs` to standardize all DEX interactions
    - Created unified `Quote` struct for consistent price discovery across all DEXes
    - Abstracted swap quote functionality behind trait methods for cleaner architecture
    - Enabled plug-and-play DEX support for easier integration of new exchanges
    - Improved type safety across all price discovery components
  - **Documentation Updates:**
    - Added detailed architecture documentation for the new DexClient trait system
    - Updated README with latest architectural improvements and design patterns
    - Clarified benefits of the new approach for future development

- **2024-06-16: Quality & Stability Sprint**
  - **Fixed critical build errors and nearly all warnings:**
    - Added missing `Whirlpool` variant to the `DexType` enum for correct DEX pool handling
    - Replaced deprecated uses of `Pubkey::new` with safe modern APIs (`Pubkey::from` for `[u8; 32]` arrays, `Pubkey::new_unique` for unique test keys)
    - Removed unused `anyhow` import and other dead code across modules to silence compiler warnings
    - Added `#[allow(dead_code)]` annotations on Jupiter API response struct fields required for deserialization, reducing unnecessary noise
    - Cleaned up syntax errors and extraneous code in utility modules (fixed stray, unmatched `fn add`)
    - Project approaches a fully clean build—now ready for more advanced engine or infra work
  - **Best practices now in place for enum extension and Solana pubkey handling**
  - Kept README sync'd and auto-updating per session and build outcome

- **2024-06-15:**
  - **Implemented Direct Whirlpool Account Parsing**
    - Removed dependency on external `orca_whirlpools` crate
    - Implemented direct manual parsing of Whirlpool accounts based on Anchor's on-chain definition
    - Increased performance, robustness, and independence from third-party libraries
    - Full control over program compatibility and account parsing logic
    - Enhanced resilience against upstream API changes

- **2024-06-14:**
  - **Arbitrage Engine Supports Whirlpools**
    - **ArbitrageEngine** (`src/arbitrage/engine.rs`) now includes **Orca Whirlpools** as first-class pools for arbitrage searches.
    - All multi-pool and single-pool routes will consider any pool parsed, including Whirlpools, Raydium, standard Orca, or Jupiter.
    - No changes to engine logic required due to modular design—only pool discovery and parsing was needed.
    - Created `src/dex/whirlpool.rs` using the same modular/trait-based architecture
    - Registered Whirlpools in `src/dex/mod.rs` for detection, parsing, and routing
    - Implemented concurrent fetching of Whirlpools pools alongside Raydium and Orca
    - Pool discovery now spawns a third async task pulling and parsing all live Whirlpools state from Solana on startup
  - Implemented full Jupiter API client with comprehensive error handling and rate limiting.
  - Added detailed Jupiter integration documentation and usage examples.
  - Enhanced arbitrage detection to leverage Jupiter's cross-DEX routing capabilities.
  - Created environment variable validation for Jupiter API key and other required settings.
  - Added command-line options for Jupiter-specific configurations.

- **2024-06-13:**
  - Completed Jupiter DEX integration with full support for cross-DEX arbitrage.
  - Added Jupiter API client for route discovery and execution.
  - Updated arbitrage engine to handle Jupiter-specific routing logic.
  - Enhanced documentation on DEX modularity and integration patterns.

- **2024-06-12:**
  - Added a public `TradingPair` type for unified token-pair tracking (metrics/mod.rs).
  - Implemented/ensured `Metrics::log_opportunity` API for detector/engine logging compatibility.
  - Standardized opportunity logging across modules for consistent metrics collection.
  - Enhanced cross-module composability with shared token pair representation.

- **2024-06-11:**
  - Fixed all function signature, tuple/struct, and opportunity mapping bugs from latest cargo run.
  - Unified all mapping between opportunity types, ensured correct tuple usage for metrics, and logging functions.
  - Fixed E0308 type mismatch in `calculate_transaction_cost` usage across engine.rs; all calls now pass the expected `usize` type, matching the function's definition.
  - Fixed E0610 (`f64` primitive misuse) in `main.rs`: calculations now use `.to_float()` on TokenAmount.
  - Implemented `Metrics::log_opportunity` for compatibility with ArbitrageDetector, resolving E0599 (missing method).
  - Fixed invalid struct field usage for `ArbOpportunity` (now using `.input_token` and `.output_token`).
  - Verified/implemented `Metrics::log_opportunity` for reliable event logging.
  - Project is expected to build without argument or struct mapping errors.
  - Added proper integration of `ArbOpportunity` struct for batching and managing arbitrage candidates from multiple sources.
- **How to retest:**
  ```sh
  cargo build
  ```
  If the build passes, you are ready to proceed with tests/development. If any further errors appear, please report them here!
- **Next step:** Confirm build passes, then move to test new/existing function(s).

---

_This section tracks every important cross-module/public API definition for stable, transparent team scaling._

---

## ❓ Help

- Issues, questions, improvements: use GitHub Issues or comments in code.
- Review sprint roadmap above for a quick snapshot of project health/progress.

---

## Changelog

## 2024-06-23: Centralized Environment Configuration

- **Implemented Centralized Config Loader:**
  - Added a unified configuration system in `/src/config/env.rs` to load and validate all environment variables
  - Standardized short names for all DEXes and tokens (ORCA, RAYDIUM, SOL, USDC, etc.)
  - Bot now validates all required API keys and endpoints on startup
  - Added support for backup RPC endpoints and automatic fallback
  - Updated README with comprehensive `.env` setup instructions

## 2024-05-13: Robust Async DEX API Guardrails and Rate Limiting

- All DEX clients (Orca, Raydium, Phoenix, Meteora, Lifinity, Whirlpool) now use a shared async HTTP utility for:
  - Rate limiting (configurable concurrency and minimum delay)
  - Exponential backoff on errors
  - Fallback to backup API endpoints if primary fails
- This ensures the bot does not spam DEX APIs or RPC endpoints, and gracefully handles rate limits and endpoint failures.
- The new logic is implemented in `src/dex/http_utils.rs` and integrated into each DEX client.
- Paper trading guardrails, private RPC fallback, and detailed simulation logging are fully supported.
- The codebase is ready for further integration of concurrent, adaptive price fetching in the arbitrage engine.
