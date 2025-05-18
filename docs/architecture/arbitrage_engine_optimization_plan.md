# Arbitrage Engine Optimization Plan

## 1. Current Architecture & Bottlenecks

### Transaction Propagation
- Transactions are built and signed in [`ArbitrageExecutor::execute()`](../../src/arbitrage/executor.rs#L52) and [`execute_multihop()`](../../src/arbitrage/executor.rs#L133).
- Propagation is synchronous: blockhash is fetched, transaction is signed, then sent and confirmed sequentially.
- No batching, pipelining, or parallel submission of transactions.
- No retry/fallback logic for transaction submission.

### RPC Calls
- Direct use of `solana_client::nonblocking::rpc_client::RpcClient` in the executor.
- No use of the custom [`SolanaRpcClient`](../../src/solana/rpc.rs#L13) wrapper, which provides retries, fallback endpoints, and jittered backoff.
- All critical RPC calls (blockhash, send/confirm) are single-threaded and not instrumented for latency.

### Async Handling
- Async/await is used, but execution is mostly sequential.
- No explicit concurrency for opportunity detection, transaction building, or submission.
- No use of task pools or parallelism for high-throughput scenarios.

### Metrics & Logging
- Some logging of elapsed time for execution, but no granular metrics for RPC latency, transaction propagation, or confirmation delays.
- No tracing of async task scheduling or queueing delays.

---

## 2. Proposed Improvements

### A. Integrate High-Availability RPC Client
- Replace direct `RpcClient` usage in `ArbitrageExecutor` with the [`SolanaRpcClient`](../../src/solana/rpc.rs#L13) wrapper.
- Leverage retry and fallback logic for all critical RPC calls (blockhash, send/confirm).
- Add metrics for retries, fallback usage, and RPC latency.

### B. Parallelize Opportunity Execution
- Use `tokio::spawn` or a task pool to execute multiple arbitrage opportunities in parallel, up to a configurable concurrency limit.
- Batch blockhash fetches and transaction submissions where possible.

### C. Instrument Latency Metrics
- Add detailed timing for:
  - Blockhash fetch
  - Transaction signing
  - Transaction submission
  - Confirmation time
- Log and export these metrics for profiling.

### D. Optimize Transaction Propagation
- Consider pipelining: prepare and sign transactions while waiting for blockhash or confirmation.
- Optionally, use `send_transaction` (fire-and-forget) for non-critical paths, and `send_and_confirm_transaction` only for high-value trades.

### E. Async Improvements
- Use `tokio::select!` or similar patterns to handle timeouts and cancellations more robustly.
- Add explicit timeouts to all RPC calls and transaction submissions.

---

## 3. High-Level Plan (Mermaid Diagram)

```mermaid
flowchart TD
    A[Detect Opportunities] --> B{Parallel Execution?}
    B -- Yes --> C[Spawn Tasks for Each Opportunity]
    B -- No --> D[Sequential Execution]
    C --> E[Prepare Transaction]
    E --> F[Fetch Blockhash (with Retry/Fallback)]
    F --> G[Sign Transaction]
    G --> H[Send & Confirm (with Retry/Fallback)]
    H --> I[Record Metrics]
    D --> E
    I --> J[Log/Export Metrics]
```

---

## 4. Next Steps

1. Refactor the arbitrage executor to use the high-availability RPC client.
2. Add parallel execution for arbitrage opportunities.
3. Instrument and log detailed latency metrics.
4. Optimize transaction propagation and async handling.
5. Document all changes and update architecture diagrams as needed.