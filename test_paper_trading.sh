#!/bin/bash
# test_paper_trading.sh - Test script for paper trading mode

echo "🧪 Testing Paper Trading Mode"
echo "=============================="

# Build the project
echo "📦 Building project..."
cargo build --release

if [ $? -ne 0 ]; then
    echo "❌ Build failed!"
    exit 1
fi

echo "✅ Build successful!"

# Test paper trading mode
echo ""
echo "📄 Testing paper trading mode..."
echo "This will run the bot in paper trading mode for 30 seconds"
echo "Press Ctrl+C to stop early"

# Set environment variables for testing
export PAPER_TRADING=true
export RPC_URL="https://api.mainnet-beta.solana.com"
export WS_URL="wss://api.mainnet-beta.solana.com"
export WALLET_PATH="test_wallet.json"
export REDIS_URL="redis://127.0.0.1/"
export MIN_PROFIT_PCT="0.001"
export MAX_SLIPPAGE_PCT="0.01"

# Run with paper trading flag and timeout after 30 seconds
timeout 30s ./target/release/solana-arb-bot --paper-trading || {
    echo ""
    echo "⏰ Test completed (timeout or manual stop)"
}

echo ""
echo "🎯 Paper trading test finished!"
echo "📁 Check ./paper_trading_logs/ for generated logs"
echo ""
echo "Expected files:"
echo "  - paper_trades_*.jsonl (trade logs)"  
echo "  - paper_analytics_*.json (performance analytics)"
