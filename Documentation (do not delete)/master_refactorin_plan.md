"You are an expert Rust software architect specializing in high-performance trading systems. Your task is to refactor my entire Solana arbitrage bot project according to a precise, phased plan.
Your Goal: To transform the current project structure into a more logical, modular, and maintainable architecture. You will be provided with the complete current file structure. You must follow the Proposed Project Structure and the Step-by-Step Execution Guide outlined below.
Critical Instruction: You must proceed one step at a time. After you complete each numbered step, you will present the changes you made and then wait for my confirmation before you proceed to the next step. This is crucial for ensuring the refactoring is done correctly.
Here is the complete plan:

[Proposed Project Structure]
.
├── Cargo.toml
├── config/
│   ├── mainnet.toml
│   ├── paper_trading.toml
│   └── dex_programs.json
├── docs/
├── examples/
├── scripts/
├── tests/
│   ├── common.rs
│   ├── arbitrage_e2e.rs
│   └── dex_integrations.rs
└── src/
    ├── main.rs                 
    ├── lib.rs                  
    └── error.rs                

    ├── config.rs               
    ├── arbitrage/
    │   ├── mod.rs
    │   ├── engine.rs           
    │   ├── pathfinder.rs       
    │   ├── opportunity.rs      
    │   ├── execution.rs        
    │   └── safety.rs           
    │
    ├── dex/                    
    │   ├── mod.rs
    │   ├── api.rs              
    │   ├── discovery.rs        
    │   └── protocols/          
    │       ├── mod.rs
    │       ├── jupiter.rs
    │       ├── orca.rs
    │       └── ...
    │
    ├── blockchain/             
    │   ├── mod.rs
    │   ├── client.rs           
    │   ├── wallet.rs           
    │   └── accounts.rs         
    │
    ├── data/                   
    │   ├── mod.rs
    │   ├── cache.rs            
    │   ├── webhooks.rs         
    │   └── websockets.rs       
    │
    ├── simulation/             
    │   ├── mod.rs
    │   ├── engine.rs
    │   ├── portfolio.rs
    │   └── analytics.rs
    │
    ├── monitoring/             
    │   ├── mod.rs
    │   ├── metrics.rs          
    │   ├── health.rs           
    │   └── balance_monitor.rs  
    │
    └── utils/                  
        ├── mod.rs
        └── timing.rs

[Step-by-Step Execution Guide]
You will now begin the refactoring process. Start with Phase 1, Step 1.1.
Phase 1: Foundational Cleanup
Step 1.1: Create New Directory Structure
* Objective: Create the new folder layout.
* Task:
    1. Create the following new directories at the project root: config, scripts.
    2. Create the following new directories inside src: blockchain, data, monitoring, simulation.
    3. Rename src/paper_trading to src/simulation.
* Action: Execute this step now and await my confirmation.
Step 1.2: Relocate Non-Source Files
* Objective: Move all non-Rust files to their correct locations outside of src.
* Task:
    1. Move all .toml and .json files from the root /config directory to the new top-level /config directory.
    2. Move all .js files from anywhere inside /src to the new /scripts directory.
    3. Move the top-level paper_trading_logs and demo_paper_logs folders to a new top-level /logs directory.
* Verification: The /src directory should no longer contain any .js, .toml, or .json files. The new /config, /scripts, and /logs directories should be populated.
Phase 2: Refactoring Core Service Modules
Step 2.1: Refactor config.rs
* Objective: Create a single, unified configuration module.
* Input Files: src/config/mod.rs, src/config/settings.rs, src/config/env.rs.
* Output File: src/config.rs.
* Tasks:
    1. Create a new file src/config.rs.
    2. Merge the logic from settings.rs (the main Config struct) and env.rs (environment variable loading) into src/config.rs.
    3. Ensure the new module correctly loads configurations from the files now located in the /config directory.
    4. Delete the old src/config directory.
* Verification: The application should compile, and all configuration values should be loadable from the new config.rs module.
Step 2.2: Refactor monitoring Module
* Objective: Consolidate all metrics, performance, and monitoring logic.
* Input Files: src/monitoring/, src/metrics/, src/local_metrics/, src/performance/.
* Output Directory: src/monitoring/.
* Tasks:
    1. Merge the contents of src/metrics/metrics.rs and src/local_metrics/metrics.rs into a new, unified src/monitoring/metrics.rs.
    2. Move the logic from src/performance/benchmark.rs and src/performance/metrics.rs into the new src/monitoring/metrics.rs or a new src/monitoring/performance.rs if the logic is substantial.
    3. Move src/monitoring/balance_monitor_enhanced.rs to src/monitoring/balance_monitor.rs.
    4. Create a src/monitoring/health.rs for health check logic.
    5. Delete the old src/metrics, src/local_metrics, and src/performance directories.
* Verification: All metrics and monitoring logic should now be accessible via the monitoring module.
Step 2.3: Refactor blockchain Module
* Objective: Group all direct on-chain interactions.
* Input Files: src/solana/, src/wallet/, src/api/.
* Output Directory: src/blockchain/.
* Tasks:
    1. Move the contents of src/api/ (connection pool, rate limiter) into src/blockchain/client.rs.
    2. Move the contents of src/wallet/ into src/blockchain/wallet.rs.
    3. Move the relevant parts of src/solana/ (rpc.rs, accounts.rs) into the src/blockchain/ module.
    4. Delete the old src/solana, src/wallet, and src/api directories.
* Verification: All RPC calls, wallet management, and direct Solana interactions should now be handled by the blockchain module.
Step 2.4: Refactor data Module
* Objective: Consolidate all data ingestion and caching logic.
* Input Files: src/websocket/, src/webhooks/, src/streams/, src/cache.rs.
* Output Directory: src/data/.
* Tasks:
    1. Move the contents of src/websocket/ into a new src/data/websockets.rs file.
    2. Move the contents of src/webhooks/ into a new src/data/webhooks.rs file.
    3. Merge the logic from src/streams/ into the appropriate websockets.rs or webhooks.rs file, as they are a form of data stream.
    4. Move the existing src/cache.rs to src/data/cache.rs.
    5. Delete the old src/websocket, src/webhooks, and src/streams directories.
* Verification: All external data feeds and caching are now managed by the data module.
Phase 3: Refactoring Core Application Logic
Step 3.1: Refactor dex Module
* Objective: Clean up the DEX interaction layer.
* Input Files: src/dex/.
* Output Directory: src/dex/.
* Tasks:
    1. Verify that src/dex/api.rs correctly defines the DexClient trait.
    2. Move all individual client implementations (e.g., jupiter.rs, orca.rs) into a new src/dex/protocols/ subdirectory.
    3. Ensure src/dex/discovery.rs handles pool discovery and banned pair logic.
    4. Move DEX-specific math from src/dex/math/ into the respective client file within src/dex/protocols/. Delete the src/dex/math folder.
* Verification: The dex module should provide a clean API for interacting with any supported DEX via the DexClient trait.
Step 3.2: Refactor arbitrage Module
* Objective: Consolidate the core arbitrage logic into a clean engine/pathfinder model.
* Input Files: src/arbitrage/, src/jito_bundle.rs, src/execution.rs.
* Output Directory: src/arbitrage/.
* Tasks:
    1. Create src/arbitrage/engine.rs and move the high-level logic from the orchestrator sub-directory into it.
    2. Create src/arbitrage/pathfinder.rs and move all logic from routing and strategy into it.
    3. Create src/arbitrage/execution.rs and move the logic from the old src/execution.rs and src/jito_bundle.rs into it.
    4. Create src/arbitrage/safety.rs for pre-flight transaction checks.
    5. Delete the now-empty sub-directories (orchestrator, routing, analysis, etc.).
* Verification: The arbitrage module is now flatter and more focused. The engine coordinates, the pathfinder finds routes, and execution performs the trades.
Phase 4: Final Wiring and Cleanup
Step 4.1: Update lib.rs and main.rs
* Objective: Update the crate root and binary entry point to reflect the new structure.
* Task:
    1. Open src/lib.rs and src/main.rs.
    2. Update all mod declarations (mod arbitrage;, mod data;, etc.) to match the new top-level modules.
    3. Update all use statements throughout the entire project to reflect the new paths (e.g., use crate::solana::rpc becomes use crate::blockchain::client). Use cargo check or clippy to find all compilation errors.
* Verification: The project should compile successfully.
Step 4.2: Update Integration Tests
* Objective: Fix the integration tests to work with the new module structure.
* Input Files: /tests/.
* Task: Go through each file in the /tests directory and update all use statements to point to the new module paths.
* Verification: All integration tests should pass when run with cargo test.

## Refactor Progress Checklist

- [x] Create new directory structure (config, scripts, src/blockchain, src/data, src/monitoring, src/simulation)
- [x] Rename src/paper_trading to src/simulation
- [x] Move all .toml and .json files from the root /config directory to the new top-level /config directory
- [x] Remove obsolete fconfig folder and its files
- [x] Move all .js files from anywhere inside /src to the new /scripts directory (none found)
- [x] Move paper_trading_logs and demo_paper_logs to new top-level /logs directory
- [x] Verified /config contains only: dex_programs.json, orca_whirlpool_pools.json, paper-trading.toml, quicknode_streams.toml
- [x] Refactored config.rs: merged settings.rs and env.rs, deleted old src/config directory
- [x] Refactored monitoring module: merged metrics, performance, and health logic into src/monitoring/
- [x] Refactored simulation and monitoring modules: moved and unified all metrics and performance logic, updated all code references
- [x] Refactored blockchain module: moved and merged all on-chain interaction code into src/blockchain/, resolved all import/type errors
- [x] Fixed all module import errors and updated all references to the new simulation and monitoring modules
- [x] Fixed all stubs/dummies with // TODO: comments for future implementation
- [x] Fixed all errors in examples and tests after monitoring and blockchain refactor
- [x] Fixed duplicate re-export and type mismatch errors for ArbHop and OrchestratorDeps
- [x] Fixed all test errors related to missing or mismatched ArbHop types
- [x] Refactored data module: moved websocket, webhooks, streams, and cache logic into src/data/ and deleted old directories

_Next steps will be checked off as completed. Proceeding to Phase 3, Step 3.1: Refactor dex module._

