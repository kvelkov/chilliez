# Arbitrage Module Documentation

## Structure of the `arbitrage` Folder

The `arbitrage` folder contains all logic related to decentralized exchange (DEX) arbitrage on Solana. The code is organized into modular components for clarity, maintainability, and extensibility.

### Directory and File Map

```
arbitrage/
├── analysis/                  # Mathematical analysis, fee logic, thresholds
│   ├── fee.rs                 # Fee calculation logic and FeeManager
│   ├── math.rs                # Advanced math, slippage models, simulation logic
│   └── mod.rs                 # Re-exports, ArbitrageAnalyzer, high-level orchestration
├── orchestrator/              # Central orchestrator and coordination logic
│   ├── core.rs                # Main orchestrator struct and core logic
│   ├── detection_engine.rs    # Arbitrage opportunity detection engine
│   ├── execution_manager.rs   # Execution management for arbitrage trades
│   └── concurrency_manager.rs # Concurrency and async coordination
├── calculator_tests.rs        # Unit tests for math and calculator logic
├── execution.rs               # Trade execution logic (HFT, batch, Jito)
├── execution_manager.rs       # Execution coordination (legacy)
├── market_data.rs             # Market data and price feeds
├── mev.rs                     # MEV protection and Jito integration
├── mod.rs                     # Main module, imports and exposes all submodules
├── opportunity.rs             # Opportunity and pathfinding types
├── safety.rs                  # Transaction safety, retry logic, and recovery
├── strategy.rs                # Arbitrage strategy and pathfinding logic
├── strategy_manager.rs        # Strategy coordination (legacy)
├── tests.rs                   # Integration and unit tests
├── types.rs                   # Common types and enums
```

## File Purposes and Usage

- **analysis/**: Contains all math, fee, and threshold logic for arbitrage analysis.
  - `fee.rs`: Implements `FeeManager` and fee breakdown logic for multi-hop trades.
  - `math.rs`: Advanced math, slippage models, simulation, and profit calculation.
  - `mod.rs`: Re-exports all analysis types and provides `ArbitrageAnalyzer` for high-level analysis.
- **orchestrator/**: Central coordination of arbitrage operations.
  - `core.rs`: Main orchestrator struct, manages all arbitrage components and workflow.
  - `detection_engine.rs`: Detects arbitrage opportunities in real time.
  - `execution_manager.rs`: Manages execution of arbitrage trades.
  - `concurrency_manager.rs`: Handles async and concurrency logic for orchestrator.
- **execution.rs**: Unified trade execution logic, including HFT and batch execution, Jito integration.
- **mev.rs**: MEV protection and Jito bundle logic.
- **opportunity.rs**: Types and logic for representing arbitrage opportunities and paths.
- **safety.rs**: Ensures transaction safety, retry logic, and error recovery.
- **strategy.rs**: Arbitrage strategy and pathfinding logic.
- **tests.rs**: Integration and unit tests for arbitrage logic.
- **types.rs**: Common types and enums used throughout arbitrage modules.

## How the Arbitrage Logic Works

1. **Detection**: The orchestrator (in `orchestrator/core.rs` and `detection_engine.rs`) continuously scans for arbitrage opportunities using real-time market data and the strategy logic (`strategy.rs`).
2. **Analysis**: When a potential opportunity is found, the analysis module (`analysis/`) is used to:
   - Calculate optimal trade amounts and expected profit (`math.rs`)
   - Estimate fees and slippage (`fee.rs`, `math.rs`)
   - Assess risk and thresholds (`math.rs`, `safety.rs`)
3. **Execution**: If the opportunity passes all checks, the orchestrator triggers trade execution (`execution.rs`), which may use HFT or batch execution, and applies MEV protection (`mev.rs`).
4. **Safety and Recovery**: All trades are monitored for safety and correctness (`safety.rs`). If errors occur, retry and recovery logic is applied.
5. **Testing**: All logic is covered by unit and integration tests (`tests.rs`, `calculator_tests.rs`).

## Usage Instructions

- **Before working on any arbitrage logic, read and understand this file.**
- Each module is responsible for a specific aspect of the arbitrage workflow. When making changes, ensure you update related modules and tests.
- For new features, follow the modular structure and add documentation.
- Run all tests after making changes to ensure correctness.

---

_Last updated: 15 June 2025_
