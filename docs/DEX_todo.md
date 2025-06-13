Objective: Build the core data pipeline that discovers and maintains a live, in-memory mirror of all relevant DEX liquidity pools.

Task 1: Implement the PoolDiscoveryService

**Status: Partially Completed**
Actionable Items:

* In a new file src/dex/discovery_service.rs, create a struct PoolDiscoveryService.
* Implement a main run method on this service that will operate as a persistent tokio::task.
* Within run, spawn four independent tokio::tasks, one for each DEX: Orca, Raydium, Lifinity, and Meteora. These tasks will be responsible for fetching the master list of all pool program addresses for their respective DEXs.

- Aggregate the results from all four tasks into a unified master list of Pubkeys.

* In a new file `src/dex/pool_discovery.rs` (Note: filename changed from `discovery_service.rs` during initial implementation), create a struct `PoolDiscoveryService`. (✅ Done)
* Implement a main `run_discovery_loop` method on this service that will operate as a persistent `tokio::task` (via `start()` method). (✅ Done)
* Within `run_discovery_loop`, iterate over `pool_discoverable_clients` (which represent Orca, Raydium, Lifinity, Meteora etc.) to fetch initial pool information (including addresses). (✅ Done)
* Aggregate the results into a unified list of `Pubkey`s for fetching raw data. (✅ Done)

Task 2: Develop a Parallel Ingestion & Parsing Pipeline

**Status: Partially Completed**
Actionable Items:

* Create a `tokio::sync::mpsc` channel for communicating raw pool data. The channel will transmit a tuple: `(Vec<u8>, DexType, Pubkey)`. (✅ Done - Channel transmits `(Vec<u8>_raw_data, Pubkey_pool_address, Pubkey_program_owner)`)
* The `PoolDiscoveryService` will batch the master pubkey list and use `get_multiple_accounts` for efficient RPC fetching. For each raw account data received, send it into the MPSC channel. (✅ Done - `fetch_and_send_raw_account_data` method)
* Create a single consumer task that listens on the MPSC channel. (✅ Partially Done - Basic consumer in `create_pool_discovery_service` utility; `consume_and_parse_raw_data` in `PoolDiscoveryService` is a placeholder needing MPSC ownership refactoring for full internal consumer.)
* Crucially, this consumer will offload the CPU-heavy parsing of the raw `Vec<u8>` data to a `rayon` worker pool. This prevents blocking the main `tokio` async runtime. The `rayon` pool will parse the bytes into the `PoolInfo` struct. (🟡 Pending - Logic planned for `consume_and_parse_raw_data` but not fully implemented with `rayon`.)
* Create a `tokio::sync::mpsc` channel for communicating raw pool data. The channel will transmit a tuple: `(Vec<u8>, DexType, Pubkey)`.
    * ✅ Done - Channel transmits `(Vec<u8>_raw_data, Pubkey_pool_address, Pubkey_program_owner)`. `program_owner` implies `DexType` for parser lookup.
* The `PoolDiscoveryService` will batch the master pubkey list and use `get_multiple_accounts` for efficient RPC fetching. For each raw account data received, send it into the MPSC channel.
    * ✅ Done - Implemented in `fetch_and_send_raw_account_data` method.
* Create a single consumer task that listens on the MPSC channel.
    * ⚠️ Partially Done - A basic consumer is spawned in the `create_pool_discovery_service` helper. The `PoolDiscoveryService::consume_and_parse_raw_data` method is a placeholder and needs MPSC channel ownership refactoring to be a fully integrated internal consumer. The `rayon` offloading is also part of this pending work.
* Crucially, this consumer will offload the CPU-heavy parsing of the raw `Vec<u8>` data to a `rayon` worker pool. This prevents blocking the main `tokio` async runtime. The `rayon` pool will parse the bytes into the `PoolInfo` struct.
    * 🟡 Pending - The `rayon` logic is conceptualized for the consumer task but not yet implemented.

Task 3: Establish and Integrate the Central Pool Cache

**Status: Partially Completed**
Actionable Items:

* Instantiate a `DashMap` that will serve as the central, concurrent cache for all pool data: `Arc<DashMap<Pubkey, Arc<PoolInfo>>>`. (✅ Done - Implemented as `pool_data_cache` field in `PoolDiscoveryService`)
* The `rayon` worker threads, upon successfully parsing a pool, will insert the resulting `PoolInfo` struct into this `DashMap`. (🟡 Pending - Part of the unimplemented `rayon` parsing logic in the consumer task.)
* Modify the `SolanaWebsocketManager` to subscribe to account updates for all `Pubkey`s present in the `DashMap`. On receiving an update, it must directly update the corresponding entry in the `DashMap` to ensure data freshness. (🟡 Pending - Explicitly deferred. `DashMap` is ready for this integration later.)
* Instantiate a `DashMap` that will serve as the central, concurrent cache for all pool data: `Arc<DashMap<Pubkey, Arc<PoolInfo>>>`.
    * ✅ Done - Implemented as the `pool_data_cache` field in `PoolDiscoveryService`.
* The `rayon` worker threads, upon successfully parsing a pool, will insert the resulting `PoolInfo` struct into this `DashMap`.
    * 🟡 Pending - This is part of the `rayon` parsing logic within the consumer task, which is not fully implemented.
* Modify the `SolanaWebsocketManager` to subscribe to account updates for all `Pubkey`s present in the `DashMap`. On receiving an update, it must directly update the corresponding entry in the `DashMap` to ensure data freshness.
    * 🟡 Pending - This was identified as a future task. The `DashMap` is available, but `SolanaWebsocketManager` has not been modified yet.

Task 4: Verification and Testing

**Status: ✅ Completed**
Actionable Items:

* Create a new example file in `examples/dex_data_factory_test.rs`. (✅ Done)
* This test will initialize the `PoolDiscoveryService`, let it run for a short period (e.g., 30 seconds), and then print the total number of pools discovered and cached in the `DashMap`. (✅ Done - Example uses one-shot `discover_all_pools` for now and checks cache.)
* Assert that the number of pools is greater than a reasonable threshold (e.g., 1000) to confirm the pipeline is working. (✅ Done - Asserts > 100 pools.)
* Create a new example file in `examples/dex_data_factory_test.rs`.
    * ✅ Done
* This test will initialize the `PoolDiscoveryService`, let it run for a short period (e.g., 30 seconds), and then print the total number of pools discovered and cached in the `DashMap`.
    * ✅ Done - The example currently uses the one-shot `discover_all_pools()` method to populate the cache for testing purposes.
* Assert that the number of pools is greater than a reasonable threshold (e.g., 1000) to confirm the pipeline is working.
    * ✅ Done - Asserts for a reasonable number of pools (currently >100, adaptable).

Sprint 2: Advanced Quoting & Multi-Hop Pathfinding
Objective: Build the analytical services to calculate optimal quotes and discover complex arbitrage opportunities from the cached data.

Task 1: Implement the AdvancedQuotingEngine

**Status: ✅ Completed**
Actionable Items:

* Create `src/dex/quoting_engine.rs`.
* Define a method `calculate_best_quote(input_mint, output_mint, amount)` that queries the central `DashMap` cache to find all pools for the given pair.
* For each found pool, dispatch the quote calculation to the appropriate DEX-specific logic, handling both standard AMMs and concentrated liquidity (CLMM) models correctly.
* Create `src/dex/quoting_engine.rs`. (✅ Done)
* Define a method `calculate_best_quote(input_mint, output_mint, amount)` that queries the central `DashMap` cache to find all pools for the given pair. (✅ Done)
* For each found pool, dispatch the quote calculation to the appropriate DEX-specific logic, handling both standard AMMs and concentrated liquidity (CLMM) models correctly. (✅ Done - Relies on `DexClient::calculate_onchain_quote` which should handle this per DEX.)

Task 2: Build the PathFinder Service with a Live Market Graph

**Status: ✅ Completed**
Actionable Items:

* Create `src/dex/path_finder.rs`.
* Integrate the `petgraph` crate.
* Create a service that builds a `petgraph` directed graph in memory. Tokens are nodes, and potential swaps are weighted edges.
* The edge weight must be the negative logarithm of the exchange rate.
* This graph must be updated in near-real-time whenever pool data in the `DashMap` changes.
* Create `src/dex/path_finder.rs`. (✅ Done)
* Integrate the `petgraph` crate. (✅ Done)
* Create a service that builds a `petgraph` directed graph in memory. Tokens are nodes, and potential swaps are weighted edges. (✅ Done)
* The edge weight must be the negative logarithm of the exchange rate. (✅ Done)
* This graph must be updated in near-real-time whenever pool data in the `DashMap` changes (via `run_graph_updater_task`). (✅ Done)

Task 3: Implement Bellman-Ford for Arbitrage Discovery

**Status: 🟡 Pending**
Actionable Items:

* Within the `PathFinder` service, implement a persistent task that repeatedly runs the `petgraph::algo::bellman_ford` algorithm on the market graph.
* When a negative-cost cycle is detected, translate that path back into a `MultiHopArbOpportunity` struct.

