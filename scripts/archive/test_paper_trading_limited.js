#!/usr/bin/env node
// Limited Paper Trading Test - Runs for 2 minutes then stops
require('dotenv').config({ path: '.env.paper-trading' });
const { PaperTradingStream } = require('../src/streaming/paper_trading_stream');

async function runLimitedTest() {
  console.log('🚀 Starting LIMITED Paper Trading Test (2 minutes)');
  console.log('📊 Environment:', process.env.SOLANA_NETWORK);
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration');
    process.exit(1);
  }

  let opportunityCount = 0;
  const startTime = Date.now();

  const stream = new PaperTradingStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: (opportunity) => {
      opportunityCount++;
      console.log(`
🎯 OPPORTUNITY #${opportunityCount}:
├─ Signature: ${opportunity.signature.substring(0, 20)}...
├─ Timestamp: ${opportunity.timestamp}
├─ Type: ${opportunity.type}
└─ [PAPER] Would execute arbitrage...
      `);
    },
    
    onError: (error) => {
      console.error('❌ Error:', error.message);
    }
  });

  // Auto-stop after 2 minutes
  const testDuration = 2 * 60 * 1000; // 2 minutes
  setTimeout(() => {
    console.log('\n⏰ Test time limit reached (2 minutes)');
    console.log(`📊 Final Stats: ${opportunityCount} opportunities found`);
    stream.disconnect();
    process.exit(0);
  }, testDuration);

  // Handle manual shutdown
  process.on('SIGINT', () => {
    const runtime = (Date.now() - startTime) / 1000;
    console.log(`\n🛑 Manual stop after ${runtime.toFixed(1)}s`);
    console.log(`📊 Found ${opportunityCount} opportunities`);
    stream.disconnect();
    process.exit(0);
  });

  try {
    await stream.connect();
    console.log(`
✅ Test Started! Monitoring for 2 minutes...
📊 Watching: Orca & Jupiter DEXs
⚡ Rate limits: 100 msg/min, 50MB/hour
🛑 Press Ctrl+C to stop early
    `);
  } catch (error) {
    console.error('❌ Failed to start:', error);
    process.exit(1);
  }
}

runLimitedTest().catch(console.error);
