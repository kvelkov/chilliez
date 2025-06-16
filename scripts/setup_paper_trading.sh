#!/bin/bash
# Paper Trading Environment Setup Script

set -e

echo "🚀 Setting up Paper Trading Environment for Real Market Testing"
echo "=============================================================="

# Create necessary directories
echo "📁 Creating directories..."
mkdir -p logs
mkdir -p paper_trading_reports  
mkdir -p wallets
mkdir -p config

# Set environment
echo "⚙️  Setting up environment..."
export SOLANA_NETWORK=devnet
export SOLANA_RPC_URL=https://api.devnet.solana.com

echo "💰 Collector Wallet: 3hsSVPZXWhc58XviagCVWvaC1GVn1Ff2CQ2kna9mc4yq"
echo "📍 Network: ${SOLANA_NETWORK}"
echo "🔗 RPC URL: ${SOLANA_RPC_URL}"

# Test network connectivity
echo "🌐 Testing network connectivity..."
if curl -s --max-time 5 "$SOLANA_RPC_URL" >/dev/null; then
    echo "✅ Network connection successful"
else
    echo "❌ Network connection failed"
    exit 1
fi

# Test Jupiter API
echo "🪐 Testing Jupiter API..."
if curl -s --max-time 5 "https://quote-api.jup.ag/v6/tokens" >/dev/null; then
    echo "✅ Jupiter API accessible"
else
    echo "❌ Jupiter API connection failed"
fi

echo ""
echo "✅ Paper Trading Environment Setup Complete!"
echo ""
echo "📋 Next Steps:"
echo "1. Get API keys (optional but recommended):"
echo "   - Helius: https://www.helius.dev/"
echo "   - QuickNode: https://www.quicknode.com/"
echo ""
echo "2. Update .env with your API keys"
echo ""
echo "3. Run paper trading demo:"
echo "   cargo run --example paper_trading_demo"
echo ""
echo "4. Run real environment paper trading:"
echo "   cargo run --example real_environment_paper_trading"
echo ""
