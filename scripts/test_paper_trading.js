#!/usr/bin/env node
// Paper Trading Arbitrage Test
// This script tests the QuickNode streaming with rate limits for paper trading

require('dotenv').config({ path: '.env.paper-trading' });
const { PaperTradingStream } = require('../src/streaming/paper_trading_stream');

async function testPaperTrading() {
  console.log('🚀 Starting Paper Trading Arbitrage Test');
  console.log('📊 Environment:', process.env.SOLANA_NETWORK);
  console.log('🔗 QuickNode RPC:', process.env.RPC_URL);
  console.log('📡 QuickNode WS:', process.env.WS_URL);
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration in .env.paper-trading');
    console.error('Required: WS_URL, QUICKNODE_API_KEY');
    process.exit(1);
  }

  const stream = new PaperTradingStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: (opportunity) => {
      console.log(`
🎯 ARBITRAGE OPPORTUNITY DETECTED:
├─ Signature: ${opportunity.signature}
├─ Timestamp: ${opportunity.timestamp}
├─ Slot: ${opportunity.slot}
├─ Type: ${opportunity.type}
└─ Logs: ${opportunity.logs.length} log entries
      `);
      
      // In real bot, this would trigger arbitrage execution
      console.log('📝 [PAPER TRADING] Would execute arbitrage here...');
    },
    
    onError: (error) => {
      console.error('❌ Stream error:', error.message);
    },
    
    onStats: (stats) => {
      // Custom stats logging with data usage warnings
      if (parseFloat(stats.data_received_mb) > 20) {
        console.warn(`⚠️  High data usage: ${stats.data_received_mb} MB`);
      }
      
      if (parseInt(stats.opportunities_found) > 0) {
        console.log(`🎯 Found ${stats.opportunities_found} opportunities so far`);
      }
    }
  });

  // Handle graceful shutdown
  process.on('SIGINT', () => {
    console.log('\n🛑 Shutting down paper trading stream...');
    stream.disconnect();
    process.exit(0);
  });

  process.on('SIGTERM', () => {
    console.log('\n🛑 Terminating paper trading stream...');
    stream.disconnect();
    process.exit(0);
  });

  // Connect and start streaming
  try {
    await stream.connect();
    
    console.log(`
✅ Paper Trading Stream Started!
📊 Monitoring: Orca & Jupiter DEXs only
⏱️  Rate limits: 100 msg/min, 50MB/hour
📝 Logs saved to: paper_trading_logs/arbitrage_opportunities.json
🛑 Press Ctrl+C to stop
    `);
    
    // Keep the process running
    setInterval(() => {
      if (!stream.isConnected) {
        console.log('❌ Stream disconnected, attempting reconnect...');
        stream.connect();
      }
    }, 30000);
    
  } catch (error) {
    console.error('❌ Failed to start paper trading:', error);
    process.exit(1);
  }
}

// Run the test
testPaperTrading().catch(console.error);
