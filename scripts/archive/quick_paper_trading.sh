#!/bin/bash
# quick_paper_trading.sh - Quick setup for paper trading with virtual funds

echo "🧪 Setting up Paper Trading with Virtual Funds"
echo "=============================================="

# Navigate to project directory
cd /Users/kiril/Desktop/chilliez

# Build the project
echo "🔨 Building project..."
cargo build --release

# Run paper trading mode with default virtual funds
echo "💰 Starting paper trading with virtual portfolio:"
echo "  - 10 SOL (simulated)"
echo "  - 100,000 USDC (simulated)" 
echo "  - 0.000005 SOL transaction fees (simulated)"
echo ""
echo "📈 Starting arbitrage bot in paper trading mode..."

# Start paper trading (will create virtual portfolio automatically)
cargo run --release -- --paper-trading --paper-logs-dir "./my_paper_logs"

echo "✅ Paper trading session complete!"
echo "📊 Check ./my_paper_logs/ for trading reports"
