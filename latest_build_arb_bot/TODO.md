# 🚀 2024-06-24 → 2025-05-14: Major Refactor Plan for Cross-DEX, Multi-Hop Arbitrage

## 🟢 Architectural Direction & Workflow (APPROVED)
- **Modular, testable, and future-proof architecture** for robust cross-DEX, multi-hop arbitrage.
- **Key modules:**
  - `detector.rs`: Opportunity detection (now multi-hop, cross-DEX aware)
  - `calculator.rs`: Analytics, profit/slippage/risk/rebate logic (multi-hop aware)
  - `engine.rs`: State management, live updates, orchestration
  - `executor.rs`: Execution of arbitrage paths (atomic, multi-leg)
- **All analytics, detection, and execution logic must be modular and testable.**
- **All major architectural/code changes must be documented and approved here before implementation.**

## 📝 Action Plan & Progress Log

### 1. Design & Planning
- [x] Review current project structure, code, and README for architectural clarity and limitations.
- [x] Confirm analytics functions in `calculator.rs` are actively used in detection pipeline.
- [x] Draft new modular architecture and workflow (see above).
- [x] Confirm current arbitrage model is insufficient for cross-DEX, multi-hop; new model needed.
- [x] Draft this action plan and progress log.

### 2. Core Refactor Steps
- [x] **Design new arbitrage opportunity struct and pipeline** to support multi-hop, cross-DEX routes (`src/arbitrage/opportunity.rs`).
- [x] **Integrate and extend existing WebSocket and Solana RPC/account modules** for multi-hop arbitrage state management and detection (reuse, don't rebuild).
- [x] **Refactor `detector.rs`** to find and represent multi-hop, cross-DEX opportunities using the new struct.
- [x] **Refactor `calculator.rs`** to support multi-hop profit, slippage, risk, and rebate calculations.
- [x] **Remove legacy blacklist/tempban logic**: migrated all ban logic to CSV-based functions in `detector.rs` and deleted `pair_blacklist.rs`/`pair_tempban.rs`.
- [x] **Update all detection and execution logic** to use new CSV-based ban functions.
- [x] **Update TODO.md** with detailed progress and next steps.
- [ ] **Refactor `engine.rs` and `executor.rs`** for state management and execution of complex arbitrage paths.
- [ ] **Add/expand tests** for new models and logic.
- [ ] **Clean up warnings and dead code** after refactor.

## ♻️ Reusable Infrastructure
- WebSocket subscription, reconnection, and event channel logic (`solana/websocket.rs`, `websocket/`)
- High-availability Solana RPC client (`solana/rpc.rs`)
- Token/account parsing and metadata caching (`solana/accounts.rs`)

### 3. Documentation & Tracking
- [x] **Keep this TODO.md updated** with progress, decisions, and architectural changes.
- [ ] **Update README.md** with new architecture, workflow, and progress.

---

## 🟢 Recent Progress (2025-05-14)
- 🚀 **Multi-hop, cross-DEX arbitrage detection and analytics fully integrated.**
- 🚀 **All ban/blacklist logic unified in `detector.rs` with CSV logging.**
- 🚀 **Legacy ban files (`pair_blacklist.rs`, `pair_tempban.rs`) removed.**
- 🚀 **All detection and execution logic now uses new CSV-based ban functions.**
- 🚀 **Project builds cleanly; only dead code warnings remain.**
- 🚀 **TODO.md and documentation updated to reflect new architecture and progress.**

---

## 🟡 Pending/Next Steps
- [ ] Refactor `engine.rs` and `executor.rs` for robust multi-hop execution and logging.
- [ ] Expand/refactor tests for new ban logic and multi-hop analytics.
- [ ] Clean up remaining warnings and dead code.
- [ ] Continue updating documentation and TODO.md as progress is made.

---

_This file is now the authoritative to-do list for the project. Please update whenever work progresses or key items are completed!_

# 📝 Comprehensive To-Do & Audit List: Solana Arbitrage Bot

This document centralizes all actionable tasks, ideas, and tracks progress.  
Includes every item from the previous README.md audit section, plus new expert suggestions.

---

## ✅ Recent, Adopted, or Ongoing Improvements

- ✅ **WebSocket Optimization:** Use Tokio broadcast channels for update handling efficiency instead of direct receiver subscriptions.
- ✅ **Centralized Profitability Check:** Rely solely on a shared, canonical `is_profitable` method for all modules.
- ✅ **Canonical Imports for Cleaner Code:** Use standard function calls and remove redundant methods/dependencies.
- ✅ **Dead Code Cleanup:** Prune suppressions and unused code to streamline compilation and reduce lint warnings.
- ✅ **Simulated Swaps:** Perform virtual transactions before executing real ones for accuracy.
- ✅ **Price Discrepancy Thresholds:** Set dynamic trade limits to avoid unnecessary trades.
- ✅ **Multi-DEX Optimization:** Adjust routes based on network congestion.
- ✅ **Jupiter DEX Support:** Added parser and executor for Jupiter integration.
- ✅ **Dynamic Trade Size Scaling:** Adjust trade size based on order book liquidity depth.
- ✅ **Anti-Front-Running Protection:** Randomize order sending (ms-level) to avoid manipulation.
- ✅ **Pool Health Scoring:** Score pools on liquidity, slippage, and age—not just blacklist.
- ✅ **Multi-TX Atomic Execution:** Bundle arbitrage legs into a single transaction.
- ✅ **DEX Liquidity Mirroring:** Mirror real-time order books in memory for fast decisions.
- ✅ **Dynamic Fee-Based Routing:** Reroute trades to cheaper DEXs when network fees spike.
- ✅ **Parallel Price Discovery:** Use Tokio tasks for concurrent DEX pool updates.
- ✅ **Adaptive WebSocket Scaling:** Dynamically adjust subscription intensity by volatility.
- ✅ **Profitability Trends per DEX:** Track historical trade success for pattern discovery.
- ✅ **Slippage Heatmap Logging:** Log average slippage per trade/DEX for better future trades.
- ✅ **Trade Execution Latency Profiling:** Monitor time from detection to trade for efficiency.
- ✅ **Reinforcement Learning Trade Optimization:** Self-improving trade logic.
- ✅ **Auto-Liquidity Prediction:** Predict liquidity spikes using historical AI models.
- ✅ **Adaptive Strategy Switching:** Auto-switch between market-making, arbitrage, and passive strategies.

---

## ��� Risk Management & Trade Protection

- [ ] Implement dynamic `min_profit_threshold` based on market volatility.
- [ ] Implement slippage auto-tuning based on pool depth or volume.
- [ ] Integrate pre-trade account balance checks before execution.
- [ ] Add budget controls to cap exposure per trade/session.
- [ ] Add stale pool timeout logic (e.g., pools not updated within 10s).
- [ ] Add blacklist or scoring for illiquid/spoofed pairs.

---

## 🧠 Execution Logic & Transaction Strategy

- [ ] Implement fee-aware trade logic (incorporating per-DEX fee models).
- [ ] Add per-DEX fee model awareness (Raydium, Orca, Whirlpool, etc.).
- [ ] Build dynamic gas estimation based on congestion.
- [ ] Add transaction timeout fallback (graceful skip/abort on failure).
- [ ] Integrate retry logic for network errors (`BlockhashNotFound`, `NodeIsBehind`).
- [ ] Consolidate all error types (e.g., `ArbError`) in `src/error/mod.rs`.
- [ ] Refactor/remove dead code suppression in `src/dead_code_suppression.rs`.
- [ ] Improve error reporting logic in `src/solana/websocket.rs`.
- [ ] Establish a unified/centralized error handling approach.

---

## ⚙️ Configurability & Control Interface

- [ ] Create a `runtime.yaml` or `.env.live` override system.
- [ ] Support runtime CLI flags or admin endpoint for live/hot updates.
- [ ] Allow per-DEX toggles (via config or dashboard).
- [ ] Add a runtime mode toggle: paper/live/simulation.

---

## 📡 WebSocket Infrastructure & Monitoring

- [ ] Implement a watchdog for frozen/dropped streams.
- [ ] Add keep-alive and auto-reconnect (with jitter).
- [ ] WebSocket heartbeat validator (no updates in 15s = alert).
- [ ] Debounce/throttle noisy update streams.
- [ ] Group events by pool for efficient batch processing.
- [ ] Harden error/backoff, especially network errors.

---

## 🧾 Logging & Observability

- [ ] Switch from ad-hoc file logs to structured logging (e.g. JSON lines).
- [ ] Add log level override via config.
- [ ] Support external telemetry (Prometheus, file tailing).
- [ ] Track success/fail rates per DEX.
- [ ] Expose latency stats per DEX and route.
- [ ] Add more tests, especially error and edge cases.

---

## 🧪 Test & Simulation Improvements

- [ ] Expand simulation mode to track unrealized PnL.
- [ ] Enable pool state mocking for CI tests.
- [ ] Support trade path simulation/dry-run confirmation.
- [ ] Add replay mode for historical pools.

---

## 🤖 AI/ML Future Hooks

- [ ] Integrate `CryptoDataProvider` trait for prediction models.
- [ ] Allow AI/ML for dynamic opportunity ranking.
- [ ] Add feedback loop for RL models.
- [ ] Stub/define "AI filter" trait, wire into engine.

---

## 📊 Optional Enhancements

- [ ] Runtime dashboard (HTML/JS) for live state.
- [ ] Telegram/Slack alerts for errors and trade events.
- [ ] Trade journaling—record reasons for trade decisions.
- [ ] Support multiple wallets for parallel routes.
- [ ] Start `infra.md` for deployment, multi-node, on-chain batching.
- [ ] Prototype Anchor-based on-chain batch contract.

---

# 2025-05-14: Progress Update & Next Steps

## ✅ Completed
- Multi-hop detection pipeline implemented and integrated into engine and main.rs (`detector.rs` multi-hop refactor complete).
- Existing WebSocket, Solana RPC, and account modules are reused for state management and detection.
- All ban/blacklist logic unified in `detector.rs` with CSV logging.
- Legacy ban files (`pair_blacklist.rs`, `pair_tempban.rs`) removed.
- All detection and execution logic now uses new CSV-based ban functions.
- Project builds cleanly; only dead code warnings remain.
- TODO.md and documentation updated to reflect new architecture and progress.

## 🟡 Next Steps
- [ ] Refactor `engine.rs` and `executor.rs` for robust multi-hop execution and logging:
    - Accept `MultiHopArbOpportunity` and iterate over hops, building DEX-specific instructions for each.
    - Add clear logging for "happy journey" (success) and "sad journey" (failure/miss).
    - Ensure robust error handling and logging for each hop.
- [ ] Expand/refactor tests for new ban logic and multi-hop analytics.
- [ ] Clean up remaining warnings and dead code.
- [ ] Continue updating documentation and TODO.md as progress is made.

## 💡 Suggestions
- All analytics and execution logic should remain modular and testable.
- The opportunity struct is now flexible for multi-hop, multi-DEX routes and future expansion.
- Continue to centralize all calculations in `calculator.rs` for consistency.
- Use `FeeManager` for all fee/slippage/gas analytics, especially for multi-hop.
- Add product logging for both successful and missed arbitrage journeys.

---

_Last updated: 2025-05-14 · This file is now the authoritative to-do list for the project. Please update whenever work progresses or key items are completed!_