# 🛠️ ENGINEERING GUIDE - Solana Arbitrage Bot

Comprehensive Technical Documentation, Operating Guidelines & Implementation Details**

---

## 🔧 OPERATING GUIDELINES

This document must be reviewed by all bots, agents, or assistants before each session and after completing any major task. These principles are non-negotiable and enforced to ensure project integrity, system continuity, and operational clarity.

### 1. Contextual Awareness & System Observability

Before performing any task:

- Traverse and review the full repository file structure
- Identify interdependencies between modules, services, and helper layers (e.g. `websocket`, `dex`, `arbitrage`, `infra`, `utils`, etc.)
- Look for existing implementations of similar logic
- **Understand before acting.** Avoid blindly patching or writing redundant logic

### 2. Functional Consistency

Ensure functions are:

- Properly declared and implemented
- Have consistent naming conventions following Rust best practices
- Include comprehensive error handling with proper Result types
- Are properly tested with unit and integration tests
- Follow the established async/await patterns throughout the codebase

### 3. Documentation Standards

- Update inline documentation for any modified functions
- Maintain README files for new modules
- Document API changes in relevant documentation files
- Include examples for complex functionality
- Keep architecture diagrams updated

### 4. Code Quality Requirements

- Zero compiler warnings (use `cargo clippy` before commits)
- Proper error handling - no unwrap() in production code
- Comprehensive logging at appropriate levels
- Memory-efficient implementations
- Thread-safe code using Arc/Mutex patterns appropriately

---

## 🏗️ SYSTEM ARCHITECTURE

### Core Modules Structure

src/
├── arbitrage/          # Core arbitrage detection and execution
│   ├── engine.rs       # Main arbitrage orchestration
│   ├── detector.rs     # Opportunity detection logic
│   ├── calculator.rs   # Profit/slippage calculations
│   ├── executor.rs     # Transaction execution
│   ├── path_finder.rs  # Advanced multi-hop path finding
│   ├── batch_executor.rs # Batch transaction processing
│   └── opportunity.rs  # Opportunity data structures
├── dex/               # DEX client implementations
│   ├── mod.rs         # DEX client coordination
│   ├── quote.rs       # DexClient trait definition
│   ├── orca.rs        # Orca DEX implementation
│   ├── raydium.rs     # Raydium DEX implementation  
│   ├── meteora.rs     # Meteora DEX implementation
│   ├── lifinity.rs    # Lifinity DEX implementation
│   └── pool.rs        # Pool data structures
├── solana/            # Solana blockchain interactions
│   ├── rpc.rs         # RPC client with failover
│   └── websocket.rs   # WebSocket data streams
├── config/            # Configuration management
├── metrics/           # Performance monitoring
├── cache/             # Redis caching layer
└── utils/             # Shared utilities

### Data Flow Architecture

[DEX APIs] → [DEX Clients] → [Pool Discovery] → [Pools Map]
    ↓
[ArbitrageEngine] → [OpportunityDetector] → [PathFinder] → [Opportunities]
    ↓
[BatchExecutor] → [Transaction Builder] → [Solana RPC] → [Execution]

## 🔌 DEX CLIENT IMPLEMENTATION

### DexClient Trait Interface

```rust
pub trait DexClient: Send + Sync {
    /// Returns the name of the DEX client
    fn get_name(&self) -> &str;

    /// Calculates expected output for a swap
    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> anyhow::Result<Quote>;

    /// Builds swap instruction
    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> anyhow::Result<Instruction>;

    /// Get known pool addresses (to be implemented)
    fn get_known_pool_addresses(&self) -> Vec<Pubkey>;

    /// Fetch current pool state (to be implemented) 
    async fn fetch_pool_state(&self, pool_address: Pubkey, rpc_client: &Arc<SolanaRpcClient>) -> anyhow::Result<PoolInfo>;
}
```

### Pool Data Structure

```rust
pub struct PoolInfo {
    pub address: Pubkey,
    pub name: String,
    pub token_a: PoolToken,
    pub token_b: PoolToken,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub fee_numerator: Option<u64>,
    pub fee_denominator: Option<u64>,
    pub fee_rate_bips: Option<u16>,
    pub last_update_timestamp: u64,
    pub dex_type: DexType,
    // CLMM specific fields
    pub liquidity: Option<u128>,
    pub sqrt_price: Option<u128>,
    pub tick_current_index: Option<i32>,
    pub tick_spacing: Option<u16>,
}
```

### DEX-Specific Implementations

#### Orca (Whirlpools)

- **Pool Type:** Concentrated Liquidity Market Maker (CLMM)
- **Program ID:** `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc`
- **Key Features:** Tick-based pricing, concentrated liquidity
- **Implementation:** `src/dex/orca.rs`

#### Raydium

- **Pool Type:** Automated Market Maker (AMM) and CLMM
- **Program IDs:** Multiple (AMM, CLMM, Stable pools)
- **Key Features:** High liquidity, multiple pool types
- **Implementation:** `src/dex/raydium.rs`

#### Meteora  

- **Pool Type:** Dynamic AMM and DLMM
- **Program IDs:**
  - Dynamic AMM: `Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB`
  - DLMM: `LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo`
- **Key Features:** Dynamic fees, bin-based liquidity
- **Implementation:** `src/dex/meteora.rs`

#### Lifinity

- **Pool Type:** Proactive Market Maker
- **Key Features:** Delta-neutral market making, rebalancing
- **Implementation:** `src/dex/lifinity.rs`

---

## ⚡ ARBITRAGE ENGINE IMPLEMENTATION

### Engine Architecture

```rust
pub struct ArbitrageEngine {
    pub pools: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>,
    pub ws_manager: Option<Arc<Mutex<SolanaWebsocketManager>>>,
    pub price_provider: Option<Arc<dyn PriceDataProvider>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub rpc_client: Option<Arc<SolanaRpcClient>>,
    pub config: Arc<Config>,
    pub detector: Arc<Mutex<ArbitrageDetector>>,
    pub dex_providers: Vec<Arc<dyn DexClient>>,
    // ... other fields
}
```

### Multi-Hop Path Finding

```rust
pub struct AdvancedPathFinder {
    max_hops: usize,
    min_liquidity_threshold: f64,
    max_slippage_pct: f64,
    min_profit_threshold_pct: f64,
    gas_estimator: GasEstimator,
}

impl AdvancedPathFinder {
    pub async fn find_all_profitable_paths(
        &self,
        base_token: Pubkey,
        pools_by_dex: &HashMap<DexType, Vec<Arc<PoolInfo>>>,
        input_amount: f64,
    ) -> Result<Vec<ArbitragePath>, Box<dyn std::error::Error>>;
}
```

### Batch Execution System

```rust
pub struct AdvancedBatchExecutor {
    config: BatchExecutionConfig,
    pending_opportunities: Vec<AdvancedMultiHopOpportunity>,
    current_batch: Vec<BatchOpportunity>,
    execution_metrics: BatchMetrics,
}

impl AdvancedBatchExecutor {
    pub async fn queue_opportunity(&mut self, opportunity: AdvancedMultiHopOpportunity);
    pub async fn execute_batch(&mut self) -> Result<Vec<String>, Box<dyn std::error::Error>>;
}
```

---

## 🔧 API KEYS & CONFIGURATION

### Environment Variables

```bash
# Core RPC Configuration
RPC_URL=https://api.mainnet-beta.solana.com
RPC_URL_BACKUP=https://solana-api.projectserum.com,https://rpc.ankr.com/solana
WS_URL=wss://api.mainnet-beta.solana.com

# Redis Configuration
REDIS_URL=redis://localhost:6379
REDIS_DEFAULT_TTL_SECS=3600

# Trading Configuration
TRADER_WALLET_KEYPAIR_PATH=/path/to/wallet.json
MIN_PROFIT_PCT=0.001
MAX_SLIPPAGE_PCT=0.005
DEFAULT_PRIORITY_FEE_LAMPORTS=5000

# DEX API Keys (Optional - for premium access)
ORCA_API_KEY=your_orca_api_key
RAYDIUM_API_KEY=your_raydium_api_key
METEORA_API_KEY=your_meteora_api_key
LIFINITY_API_KEY=your_lifinity_api_key
JUPITER_API_KEY=your_jupiter_api_key  # Future support
COINAPI_KEY=your_coinapi_key  # Recommended for market data

# Performance Tuning
CYCLE_INTERVAL_SECONDS=5
POOL_REFRESH_INTERVAL_SECS=30
WS_UPDATE_CHANNEL_SIZE=1024
MAX_CONCURRENT_EXECUTIONS=10
```

### Setup Instructions

#### Prerequisites (macOS)

```bash
# Install Rust and Cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Python 3 (if needed)
brew install python

# Verify installations
cargo --version
python3 --version
```

#### Project Setup

```bash
# Clone and setup project
git clone <repository-url>
cd chilliez

# Build project
cargo build

# Run tests
cargo test

# Run demo
cargo run --example advanced_arbitrage_demo

# Run main application
cargo run
```

### Configuration Structure

```rust
pub struct Config {
    // RPC Settings
    pub rpc_url: String,
    pub rpc_url_backup: Option<String>,
    pub ws_url: String,
    
    // Trading Parameters
    pub min_profit_pct: f64,
    pub max_slippage_pct: f64,
    pub sol_price_usd: Option<f64>,
    
    // Performance Settings
    pub cycle_interval_seconds: Option<u64>,
    pub max_concurrent_executions: Option<u32>,
    pub pool_refresh_interval_secs: u64,
    
    // Risk Management
    pub max_tx_fee_lamports_for_acceptance: Option<u64>,
    pub min_profit_usd_threshold: Option<f64>,
    
    // ... other configuration fields
}
```

---

## 🧪 TESTING FRAMEWORK

### Test Categories

1. **Unit Tests:** Individual function testing
2. **Integration Tests:** Module interaction testing  
3. **End-to-End Tests:** Full workflow testing
4. **Performance Tests:** Speed and efficiency validation

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test arbitrage::tests

# Run with output
cargo test -- --nocapture

# Run performance tests
cargo test --release perf_

# Run integration tests
cargo test --test integration_test
```

### Mock Data for Testing

```rust
// Create test pool data
pub fn create_demo_pools() -> HashMap<DexType, Vec<Arc<PoolInfo>>> {
    let mut pools_by_dex = HashMap::new();
    
    // Orca pools
    pools_by_dex.insert(DexType::Orca, vec![
        create_mock_orca_pool("SOL/USDC", 1000000.0, 150000.0),
        create_mock_orca_pool("SOL/USDT", 800000.0, 120000.0),
    ]);
    
    // Add pools for other DEXs...
    pools_by_dex
}
```

---

## 📊 FEATURES & LOGIC IMPLEMENTATION

### Core Features

#### 1. Multi-Hop Arbitrage Detection

- **Path Discovery:** BFS algorithm finds 2-4 hop profitable paths
- **Cross-DEX Routing:** Optimal sequencing across different DEX types
- **Profit Calculation:** Accounts for fees, slippage, and gas costs
- **Risk Assessment:** Confidence scoring for each opportunity

#### 2. Batch Execution Engine

- **Opportunity Grouping:** Compatible opportunities bundled atomically
- **Conflict Detection:** Prevents resource conflicts within batches
- **Priority Ordering:** High-profit opportunities executed first
- **Gas Optimization:** Efficient transaction structure minimizes costs

#### 3. Real-Time Data Processing

- **WebSocket Streams:** Live price and pool state updates
- **Pool State Management:** Efficient tracking of 1000+ pools
- **Update Processing:** Sub-100ms data processing pipeline
- **Staleness Detection:** Automatic detection and refresh of stale data

#### 4. Advanced Risk Management

- **Dynamic Thresholds:** Volatility-adjusted profit requirements
- **Position Sizing:** Intelligent capital allocation
- **Slippage Protection:** Real-time slippage calculation and limits
- **Emergency Systems:** Circuit breakers and automated failsafes

### Execution Logic Flow

1.Pool Data Collection → 2. Opportunity Detection → 3. Path Optimization
                ↓
4.Risk Assessment → 5. Batch Formation → 6. Transaction Execution
                ↓
7.Result Monitoring → 8. Performance Analysis → 9. Strategy Adjustment

---

## 🚨 CRITICAL IMPLEMENTATION NOTES

### Current Status (June 13, 2025)

**✅ COMPLETED:**

- Advanced arbitrage infrastructure
- Multi-hop path finding algorithm
- Batch execution system
- DEX client implementations
- Configuration management system

**❌ CRITICAL MISSING:**

- Pool data pipeline (DEX → Pools Map)
- Real-time pool state updates
- Live transaction execution
- WebSocket integration for pool updates

### Immediate Requirements

1. **Pool Discovery Service** (`src/dex/pool_discovery.rs`)
2. **DEX Client Extensions** (add pool fetching methods)
3. **Pool Population Pipeline** (integrate into main loop)
4. **Real-time Updates** (WebSocket → Pool Map)

### Code Quality Standards

- **Zero Warnings Policy:** Use `cargo clippy --all-targets --all-features`
- **Error Handling:** Proper Result types, no unwrap() in production
- **Async Best Practices:** Consistent tokio usage, proper timeout handling
- **Memory Management:** Efficient Arc/Mutex usage, avoid unnecessary clones
- **Logging Standards:** Structured logging with appropriate levels

---

## 🔍 DEBUGGING & MONITORING

### Logging Configuration

```rust
// Initialize comprehensive logging
fern::Dispatch::new()
    .format(|out, message, record| {
        out.finish(format_args!(
            "[{}][{}][{}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            message
        ))
    })
    .level(log::LevelFilter::Info)
    .level_for("solana_rbpf", log::LevelFilter::Warn)
    .chain(std::io::stdout())
    .apply()?;
```

### Performance Monitoring

```rust
pub struct Metrics {
    pub opportunities_detected: u64,
    pub opportunities_executed: u64,
    pub total_profit_usd: f64,
    pub execution_time_ms: Vec<u64>,
    pub pool_refresh_count: u64,
    // ... additional metrics
}
```

### Health Checks

- **RPC Client Health:** Connection status and latency monitoring
- **WebSocket Health:** Stream connectivity and data freshness
- **Pool Data Health:** Staleness detection and refresh triggers
- **Execution Health:** Success rates and error monitoring

---

## 🛡️ SECURITY CONSIDERATIONS

### Wallet Security

- Use hardware wallets for production
- Implement proper key management
- Separate trading and operational wallets
- Regular security audits

### Transaction Security

- Validate all transaction parameters
- Implement timeout mechanisms
- Use proper slippage protection
- Monitor for MEV attacks

### API Security

- Secure API key storage
- Rate limiting implementation
- Input validation and sanitization
- Comprehensive error handling

---

This engineering guide serves as the definitive technical reference for the Solana Arbitrage Bot. All development should follow these guidelines to ensure consistency, reliability, and maintainability of the codebase.
