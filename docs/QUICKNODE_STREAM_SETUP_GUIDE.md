# QuickNode Stream Setup Guide
# Complete step-by-step guide to set up real-time streams for arbitrage detection

## 🎯 What Are QuickNode Streams?

QuickNode Streams are real-time WebSocket connections that notify you instantly when:
- Transactions occur on specific programs (like DEXs)
- Account balances change
- New blocks are produced
- Specific events happen on-chain

Think of it like getting a phone notification every time someone makes a trade on Orca or Raydium!

## 📋 Step 1: Understanding Stream Types

### Account Streams
Monitor when specific accounts (like pool addresses) change:
```json
{
  "method": "accountSubscribe", 
  "params": ["POOL_ADDRESS_HERE"]
}
```

### Program Log Streams  
Monitor all transactions that interact with a specific program:
```json
{
  "method": "logsSubscribe",
  "params": [{"mentions": ["DEX_PROGRAM_ID_HERE"]}]
}
```

### Signature Streams
Monitor specific transactions by their signature:
```json
{
  "method": "signatureSubscribe",
  "params": ["TRANSACTION_SIGNATURE_HERE"]
}
```

## 🔧 Step 2: QuickNode Dashboard Setup

### A. Log into QuickNode Dashboard
1. Go to https://dashboard.quicknode.com/
2. Select your Solana endpoint
3. Click on "Streams" in the sidebar

### B. Create a New Stream
1. Click "Create Stream"
2. Choose "Solana" as the blockchain
3. Select your endpoint from the dropdown

### C. Configure Stream Filters
This is where you'll use the JavaScript filter I created!

## 🚀 Step 3: Create Your First Stream

Let me create a simple setup script for you:
