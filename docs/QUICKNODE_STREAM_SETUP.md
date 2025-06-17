# 🎯 QuickNode Stream Setup Guide for Arbitrage Detection

## 🚀 Quick Start (5 minutes)

### Step 1: Run the Setup Script
```bash
cd /Users/kiril/Desktop/chilliez
./scripts/setup_quicknode_arbitrage.sh
```

### Step 2: Go to QuickNode Dashboard
1. Open: https://dashboard.quicknode.com/
2. Login with your account
3. Navigate to **Streams** → **Create Stream**

### Step 3: Configure the Stream
```
Dataset: Solana Mainnet
Stream Type: Block
☑️ Include Transactions
☑️ Include Transaction Details  
☑️ Include Token Balances
☑️ Include Inner Instructions
```

### Step 4: Add the Filter Function
1. In the **Function** section, paste the entire content from:
   `src/streams/quicknode_arbitrage_filter.js`

2. The filter will automatically detect:
   - ✅ DEX swaps (Orca, Raydium, Jupiter, etc.)
   - ✅ Arbitrage opportunities
   - ✅ Large trades ($10,000+)
   - ✅ MEV bot activity
   - ✅ Cross-DEX transactions

### Step 5: Set Destination
Choose one:
- **Webhook**: Your server endpoint to receive real-time data
- **Queue**: Store for batch processing
- **Database**: Direct database insertion

---

## 🔧 Detailed Configuration Options

### Option A: Block Stream (Recommended)
**Best for**: Active arbitrage trading

```javascript
// Stream Configuration
{
  "type": "block",
  "network": "solana-mainnet", 
  "include_transactions": true,
  "include_inner_instructions": true,
  "include_token_balances": true,
  "commitment": "finalized"
}
```

### Option B: Account Subscription  
**Best for**: Pool monitoring

```javascript
// Monitor specific DEX pools
{
  "method": "accountSubscribe",
  "params": [
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", // Orca
    {
      "encoding": "base64",
      "commitment": "finalized"
    }
  ]
}
```

### Option C: Program Logs
**Best for**: Comprehensive DEX monitoring

```javascript
// Monitor all DEX program activity
{
  "method": "logsSubscribe", 
  "params": [
    {
      "mentions": [
        "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM" // Orca
      ]
    },
    {
      "commitment": "finalized"
    }
  ]
}
```

---

## 📊 What the Filter Detects

### 🎯 Arbitrage Opportunities
- **Cross-DEX arbitrage**: Same token pair trading at different prices
- **Price impact arbitrage**: Large trades creating temporary price differences
- **MEV sandwich opportunities**: Profitable front/back-running opportunities

### 💰 Trading Patterns
- **Large trades**: Transactions over $10,000 USD
- **Whale activity**: Monitored whale wallet transactions  
- **Bot activity**: Known MEV bot transactions
- **Token transfers**: Significant SPL token movements

### 🔄 DEX Operations
- **Swaps**: Token swaps across all major DEXs
- **Liquidity changes**: Pool additions/removals
- **Price impacts**: Transactions causing >5% price impact

---

## ⚡ Real-Time Integration

### WebSocket Connection Example
```javascript
const ws = new WebSocket('wss://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  
  // Your filter has already processed this data
  if (data.arbitrageTransactions) {
    console.log(`🎯 Found ${data.arbitrageTransactions.length} opportunities`);
    
    // Process each opportunity
    data.arbitrageTransactions.forEach(tx => {
      if (tx.arbitrageOpportunities.length > 0) {
        console.log(`💰 Potential profit: $${tx.arbitrageOpportunities[0].estimatedProfit}`);
        // Execute your arbitrage logic here
      }
    });
  }
};
```

### Webhook Integration
```javascript
// Express.js webhook endpoint
app.post('/quicknode-webhook', (req, res) => {
  const streamData = req.body;
  
  if (streamData.data && streamData.data.length > 0) {
    streamData.data.forEach(block => {
      if (block.arbitrageTransactions) {
        // Process arbitrage opportunities
        processArbitrageOpportunities(block.arbitrageTransactions);
      }
    });
  }
  
  res.status(200).send('OK');
});
```

---

## 🎯 **Recommended: Custom Function Setup**

### **Why Custom Function over Webhook?**
- ⚡ **Lower latency** - Processing at QuickNode's edge
- 💰 **Cost effective** - Only pay for filtered data
- 🔧 **No server maintenance** - QuickNode handles infrastructure
- 📊 **Pre-filtered data** - Only arbitrage opportunities reach you

### **Setup Steps:**
1. **QuickNode Dashboard** → Streams → Create Stream
2. **Choose**: Custom Function (not Webhook)
3. **Paste**: Content from `src/streams/quicknode_arbitrage_filter.js`
4. **Output to**: Webhook endpoint for your bot

### **Function Configuration:**
```
Stream Type: Block
Destination: Custom Function
Function Code: [Your arbitrage filter]
Output Method: Webhook to your bot
```

### **What You'll Receive:**
Instead of 10MB raw blocks, you get clean arbitrage data:
```json
{
  "arbitrageOpportunities": [
    {
      "type": "cross_dex_arbitrage", 
      "estimatedProfit": 15.75,
      "dexes": ["Orca", "Raydium"]
    }
  ]
}
```

---

## 🛠️ Customization

### Adjust Thresholds
Edit these values in the filter:

```javascript
const CONFIG = {
  THRESHOLDS: {
    MIN_SWAP_VALUE_USD: 100,        // Minimum $100 swaps
    MIN_ARBITRAGE_PROFIT_USD: 5,    // Minimum $5 profit
    MIN_LIQUIDITY_CHANGE: 1000,     // Minimum $1000 liquidity change
    LARGE_TRADE_THRESHOLD: 10000    // $10,000+ = large trade
  }
};
```

### Add Custom Addresses
Use QuickNode's Key-Value Store:

```bash
# Add your addresses to monitor
curl -X POST "https://api.quicknode.com/quicknode/rest/v1/lists" \
  -H "x-api-key: QN_635965fc09414ea2becef14f68bcf7bf" \
  -d '{
    "name": "ARB_ADDRESSES",
    "addresses": ["YOUR_WALLET", "COMPETITOR_WALLET"]
  }'
```

### Monitor Specific Tokens
```bash
# Add tokens to prioritize
curl -X POST "https://api.quicknode.com/quicknode/rest/v1/lists" \
  -H "x-api-key: QN_635965fc09414ea2becef14f68bcf7bf" \
  -d '{
    "name": "ARB_TOKENS", 
    "addresses": ["SOL_MINT", "USDC_MINT", "CUSTOM_TOKEN"]
  }'
```

---

## 📈 Expected Results

### Sample Filter Output
```json
{
  "slot": 123456789,
  "blockTime": 1703123456,
  "arbitrageTransactions": [
    {
      "signature": "abc123...",
      "dexSwaps": [
        {
          "dex": "Orca",
          "tokenIn": "So11111111111111111111111111111111111111112",
          "tokenOut": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
          "amountIn": 10.5,
          "amountOut": 1575.0
        }
      ],
      "arbitrageOpportunities": [
        {
          "type": "cross_dex_arbitrage",
          "dexes": ["Orca", "Raydium"],
          "estimatedProfit": 15.75,
          "confidence": "high"
        }
      ],
      "estimatedValueUSD": 1575.0,
      "isLargeTrade": false
    }
  ],
  "summary": {
    "totalArbitrageOpportunities": 1,
    "totalVolumeUSD": 1575.0,
    "dexesInvolved": ["Orca"],
    "hasLargeTrades": false
  }
}
```

---

## 🚨 Troubleshooting

### Common Issues

1. **No data received**
   - Check QuickNode endpoint URL
   - Verify API key is correct
   - Ensure stream is active in dashboard

2. **Filter not working**
   - Check JavaScript syntax in filter function
   - Verify all DEX program IDs are correct
   - Test with lower thresholds

3. **Missing opportunities**
   - Lower `MIN_SWAP_VALUE_USD` threshold
   - Add more DEX programs to monitor
   - Check `commitment` level (use "confirmed" for faster detection)

### Debug Mode
Add this to your filter for debugging:

```javascript
// Add at the top of main() function
const DEBUG = true;
if (DEBUG) {
  console.log('Processing block with', data.length, 'transactions');
}
```

---

## ✅ Verification

Test your setup:

1. **Check connection**: Run setup script
2. **Verify stream**: Check QuickNode dashboard for active stream
3. **Test filter**: Look for console logs in stream dashboard  
4. **Monitor data**: Watch for incoming arbitrage opportunities

Your stream is now configured to detect profitable arbitrage opportunities in real-time! 🎉
