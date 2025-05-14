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

_Last updated: 2024-06-22 · This file is now the authoritative to-do list for the project. Please update whenever work progresses or key items are completed!_