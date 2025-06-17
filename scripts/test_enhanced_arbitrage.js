#!/usr/bin/env node
// Enhanced Paper Trading with Better Opportunity Analysis
require('dotenv').config({ path: '.env.paper-trading' });
const { PaperTradingStream } = require('../src/streaming/paper_trading_stream');
const fs = require('fs');
const path = require('path');

async function runEnhancedTest() {
  console.log('🚀 Enhanced Paper Trading Test (3 minutes)');
  console.log('📊 Environment:', process.env.SOLANA_NETWORK);
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration');
    process.exit(1);
  }

  let opportunityCount = 0;
  let totalDataReceived = 0;
  const opportunities = [];
  const startTime = Date.now();

  const stream = new PaperTradingStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: (opportunity) => {
      opportunityCount++;
      opportunities.push(opportunity);
      
      console.log(`
🎯 ARBITRAGE OPPORTUNITY #${opportunityCount}:
├─ Signature: ${opportunity.signature.substring(0, 30)}...
├─ Time: ${new Date(opportunity.timestamp).toLocaleTimeString()}
├─ Slot: ${opportunity.slot || 'Unknown'}
├─ Logs: ${opportunity.logs.length} entries
├─ Source: ${opportunity.source}
└─ [PAPER] Profitability analysis needed...
      `);
      
      // Save immediately for each opportunity
      saveOpportunities();
    },
    
    onError: (error) => {
      console.error('❌ Stream Error:', error.message);
    },
    
    onStats: (stats) => {
      totalDataReceived = parseFloat(stats.data_received_mb);
      
      if (totalDataReceived > 10) {
        console.warn(`⚠️  Data usage: ${stats.data_received_mb} MB`);
      }
      
      // Show progress every 30 seconds
      console.log(`
📊 LIVE STATS (${stats.runtime_minutes}min):
├─ Messages: ${stats.total_messages} (${stats.messages_per_minute}/min)
├─ Data: ${stats.data_received_mb} MB
├─ Opportunities: ${opportunityCount}
├─ Rate: ${opportunityCount > 0 ? (opportunityCount / parseFloat(stats.runtime_minutes)).toFixed(1) : '0'} opp/min
└─ Status: ${stats.connected ? '🟢 Live' : '🔴 Offline'}
      `);
    }
  });

  function saveOpportunities() {
    try {
      const logDir = path.join(__dirname, '../paper_trading_logs');
      if (!fs.existsSync(logDir)) {
        fs.mkdirSync(logDir, { recursive: true });
      }
      
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const filename = path.join(logDir, `arbitrage_session_${timestamp.substring(0, 19)}.json`);
      
      const sessionData = {
        session: {
          start_time: new Date(startTime).toISOString(),
          runtime_seconds: (Date.now() - startTime) / 1000,
          total_opportunities: opportunities.length,
          data_received_mb: totalDataReceived
        },
        opportunities: opportunities
      };
      
      fs.writeFileSync(filename, JSON.stringify(sessionData, null, 2));
      console.log(`💾 Saved ${opportunities.length} opportunities to ${path.basename(filename)}`);
    } catch (error) {
      console.error('❌ Error saving:', error.message);
    }
  }

  // Test duration: 3 minutes
  const testDuration = 3 * 60 * 1000;
  
  const stopTest = () => {
    const runtime = (Date.now() - startTime) / 1000;
    console.log(`
🏁 TEST COMPLETED!
📊 Final Results:
├─ Runtime: ${runtime.toFixed(1)} seconds
├─ Opportunities: ${opportunityCount}
├─ Rate: ${(opportunityCount / (runtime / 60)).toFixed(2)} per minute
├─ Data Usage: ${totalDataReceived.toFixed(2)} MB
└─ Efficiency: ${totalDataReceived > 0 ? (opportunityCount / totalDataReceived).toFixed(1) : 'N/A'} opp/MB
    `);
    
    if (opportunityCount > 0) {
      saveOpportunities();
      console.log('✅ All opportunities saved to paper_trading_logs/');
      
      // Show sample opportunity for analysis
      const sample = opportunities[0];
      console.log(`
🔍 SAMPLE OPPORTUNITY ANALYSIS:
├─ Signature: ${sample.signature}
├─ Detected Swaps: ${sample.logs.filter(log => log.toLowerCase().includes('swap')).length}
├─ Program Calls: ${sample.logs.filter(log => log.includes('invoke')).length}
└─ Potential: Needs price/volume analysis
      `);
    }
    
    stream.disconnect();
    process.exit(0);
  };

  // Auto-stop after test duration
  setTimeout(stopTest, testDuration);

  // Handle manual stop
  process.on('SIGINT', stopTest);

  try {
    await stream.connect();
    console.log(`
✅ Enhanced Test Started!
⏱️  Duration: 3 minutes
📡 Monitoring: Orca & Jupiter DEXs
📊 Real-time stats every 30s
📁 Saving to: paper_trading_logs/
🛑 Press Ctrl+C to stop early
    `);
  } catch (error) {
    console.error('❌ Failed to start:', error);
    process.exit(1);
  }
}

runEnhancedTest().catch(console.error);
