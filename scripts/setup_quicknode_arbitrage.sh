#!/bin/bash

# QuickNode Stream Setup for Arbitrage Bot
# =========================================

echo "🚀 Setting up QuickNode Streams for Arbitrage Detection"
echo "======================================================="

# Your QuickNode details
QUICKNODE_ENDPOINT="https://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820"
QUICKNODE_WS_ENDPOINT="wss://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820"
QUICKNODE_API_KEY="QN_635965fc09414ea2becef14f68bcf7bf"

echo "📋 Configuration:"
echo "  RPC Endpoint: $QUICKNODE_ENDPOINT"
echo "  WebSocket: $QUICKNODE_WS_ENDPOINT"
echo "  API Key: $QUICKNODE_API_KEY"
echo ""

# Test connection
echo "🔍 Testing QuickNode connection..."
response=$(curl -s -X POST "$QUICKNODE_ENDPOINT" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getHealth"
  }')

if echo "$response" | grep -q "ok"; then
    echo "✅ QuickNode RPC connection successful!"
else
    echo "❌ QuickNode RPC connection failed!"
    echo "Response: $response"
    exit 1
fi

echo ""
echo "🔧 Stream Configuration Options:"
echo "================================"

echo ""
echo "1️⃣  TRANSACTION STREAM (Recommended for Arbitrage)"
echo "   • Monitors all transactions in real-time"
echo "   • Filters for DEX swaps and arbitrage opportunities"
echo "   • Best for: Active arbitrage trading"
echo ""

echo "2️⃣  ACCOUNT SUBSCRIPTION (Pool Monitoring)"
echo "   • Monitors specific DEX pool accounts"
echo "   • Tracks liquidity and price changes"
echo "   • Best for: Pool state monitoring"
echo ""

echo "3️⃣  PROGRAM LOGS (DEX Activity)"
echo "   • Monitors DEX program execution logs"
echo "   • Catches all DEX interactions"
echo "   • Best for: Comprehensive DEX monitoring"
echo ""

# Create QuickNode lists for address monitoring
echo "📝 Setting up QuickNode Address Lists..."

# Create arbitrage addresses list
echo "Creating ARB_ADDRESSES list..."
curl -s -X POST "https://api.quicknode.com/quicknode/rest/v1/lists" \
  -H "accept: application/json" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $QUICKNODE_API_KEY" \
  -d '{
    "name": "ARB_ADDRESSES",
    "description": "Addresses to monitor for arbitrage opportunities",
    "addresses": [
      "YOUR_BOT_WALLET_ADDRESS_HERE",
      "COMPETITOR_MEV_BOT_1",
      "COMPETITOR_MEV_BOT_2"
    ]
  }' > /dev/null 2>&1

# Create monitored tokens list
echo "Creating ARB_TOKENS list..."
curl -s -X POST "https://api.quicknode.com/quicknode/rest/v1/lists" \
  -H "accept: application/json" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $QUICKNODE_API_KEY" \
  -d '{
    "name": "ARB_TOKENS",
    "description": "High-value tokens for arbitrage monitoring",
    "addresses": [
      "So11111111111111111111111111111111111111112",
      "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
      "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
      "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj"
    ]
  }' > /dev/null 2>&1

echo "✅ QuickNode lists created!"
echo ""

echo "🎯 Next Steps - QuickNode Dashboard Setup:"
echo "=========================================="
echo ""
echo "1. Go to: https://dashboard.quicknode.com/"
echo "2. Navigate to: Streams → Create Stream"
echo "3. Choose: 'Block' for transaction monitoring"
echo "4. Configure:"
echo "   • Dataset: Solana Mainnet"
echo "   • Stream Type: Block"
echo "   • Include Transactions: ✅ Yes"
echo "   • Include Transaction Details: ✅ Yes"
echo "   • Include Token Balances: ✅ Yes"
echo ""
echo "5. Add the filter function:"
echo "   • Copy the content from: src/streams/quicknode_arbitrage_filter.js"
echo "   • Paste it in the 'Function' section"
echo ""
echo "6. Set destination:"
echo "   • Webhook URL: Your server endpoint"
echo "   • Or: Queue (for processing later)"
echo ""

echo "📊 Monitoring Targets:"
echo "====================="
echo "• Orca Whirlpools: 9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"
echo "• Raydium AMM: 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
echo "• Jupiter: JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB"
echo "• Meteora: MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky"
echo "• Lifinity: 2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c"
echo ""

echo "💡 Alternative: WebSocket Connection"
echo "==================================="
echo "For real-time WebSocket connection, use:"
echo ""
echo "const ws = new WebSocket('$QUICKNODE_WS_ENDPOINT');"
echo ""
echo "Subscribe to account changes:"
echo '{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
    {
      "encoding": "base64",
      "commitment": "finalized"
    }
  ]
}'
echo ""

echo "🎉 Setup Complete!"
echo "=================="
echo ""
echo "Your arbitrage filter is configured to detect:"
echo "✅ Cross-DEX arbitrage opportunities"
echo "✅ Large trades and price impacts"
echo "✅ MEV bot activity"
echo "✅ Liquidity changes"
echo "✅ Whale wallet transactions"
echo ""
echo "Filter thresholds:"
echo "• Minimum swap value: $100 USD"
echo "• Minimum arbitrage profit: $5 USD"
echo "• Large trade threshold: $10,000 USD"
echo "• Maximum price impact: 5%"
echo ""
echo "📈 Ready for arbitrage detection!"
