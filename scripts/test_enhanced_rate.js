#!/usr/bin/env node
// Enhanced Rate Test - More DEXs, Higher Limits, More Opportunities
require('dotenv').config({ path: '.env.paper-trading' });
const { EnhancedPaperTradingStream } = require('../src/streaming/enhanced_paper_trading_stream');
const { ProfitabilityAnalyzer } = require('../src/analysis/profitability_analyzer');

async function runEnhancedRateTest() {
  console.log('🚀 ENHANCED RATE TEST - Increased Search Scope');
  console.log('📈 RATE INCREASES:');
  console.log('├─ Messages: 100 → 200 per minute (+100%)');
  console.log('├─ Data: 50MB → 100MB per hour (+100%)');
  console.log('├─ DEXs: 2 → 5 major DEXs (+150%)');
  console.log('├─ Min Trade: $1000 → $500 (+50% more opportunities)');
  console.log('└─ Detection: Lower threshold (more sensitive)');
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration');
    process.exit(1);
  }

  // Enhanced analyzer with €5000 wallet
  const analyzer = new ProfitabilityAnalyzer({
    walletBalanceSOL: 38.0,
    minProfitEur: 5.0,
    minProfitPercent: 0.5,
    maxTradeSize: 0.2, // 20% max
    slippageTolerance: 0.001
  });

  let opportunityCount = 0;
  let profitableCount = 0;
  let totalPotentialProfit = 0;
  const startTime = Date.now();
  const dexStats = {};

  const stream = new EnhancedPaperTradingStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: async (opportunity) => {
      opportunityCount++;
      
      // Track DEX statistics
      const dex = opportunity.detectedDex;
      dexStats[dex] = (dexStats[dex] || 0) + 1;
      
      console.log(`
🎯 ENHANCED OPPORTUNITY #${opportunityCount}:
├─ Signature: ${opportunity.signature.substring(0, 25)}...
├─ DEX: ${opportunity.detectedDex}
├─ Score: ${opportunity.swapScore}/10 confidence
├─ Logs: ${opportunity.logCount} entries
└─ Time: ${new Date(opportunity.timestamp).toLocaleTimeString()}
      `);

      // Quick profitability check for promising opportunities
      if (opportunity.swapScore >= 3) { // High-confidence opportunities
        try {
          const analysis = await analyzer.analyzeProfitability(opportunity);
          
          if (analysis.isProfitable) {
            profitableCount++;
            totalPotentialProfit += analysis.analysis.netProfitEur;
            
            console.log(`
✅ PROFITABLE! (${dex})
├─ Profit: €${analysis.analysis.netProfitEur.toFixed(2)} (${analysis.analysis.netProfitPercent.toFixed(2)}%)
├─ Trade: ${analysis.analysis.tradeAmountSOL.toFixed(2)} SOL
├─ Confidence: ${analysis.confidence}%
└─ Action: ${analysis.recommendation.action}
            `);
          }
        } catch (error) {
          // Continue processing even if analysis fails
          console.log(`⚠️  Analysis skipped for ${dex} opportunity`);
        }
      }
    },
    
    onError: (error) => {
      console.error('❌ Enhanced Stream Error:', error.message);
    },
    
    onStats: (stats) => {
      const runtime = parseFloat(stats.runtime_minutes);
      const currentRate = parseFloat(stats.opportunities_per_minute);
      
      // Show enhanced statistics
      console.log(`
📊 ENHANCED RATE PERFORMANCE:
├─ Opportunities: ${stats.opportunities_found} (${currentRate}/min)
├─ Profitable: ${profitableCount} (${opportunityCount > 0 ? (profitableCount/opportunityCount*100).toFixed(1) : '0'}%)
├─ Data Usage: ${stats.data_received_mb}/${stats.limits.data_limit_mb}MB/hour
├─ Message Rate: ${stats.messages_per_minute}/${stats.limits.message_limit}/min
├─ DEX Breakdown: ${Object.entries(stats.dex_hits).map(([dex, hits]) => `${dex}:${hits}`).join(' ')}
└─ Profit Rate: €${totalPotentialProfit.toFixed(2)} total
      `);
      
      // Rate comparison
      if (runtime > 1) {
        const projectedHourly = currentRate * 60;
        const projectedDaily = projectedHourly * 24;
        console.log(`📈 PROJECTIONS: ${projectedHourly.toFixed(0)}/hour, ${projectedDaily.toFixed(0)}/day`);
      }
    }
  });

  // Test duration: 3 minutes for rate comparison
  const testDuration = 3 * 60 * 1000;
  
  const stopTest = () => {
    const runtime = (Date.now() - startTime) / 1000;
    const ratePerMinute = (opportunityCount / (runtime / 60)).toFixed(2);
    const profitRate = totalPotentialProfit / (runtime / 3600); // per hour
    
    console.log(`
🏁 ENHANCED RATE TEST COMPLETED!
📊 FINAL RATE COMPARISON:
├─ Total Runtime: ${runtime.toFixed(1)}s
├─ Opportunities: ${opportunityCount}
├─ Rate: ${ratePerMinute}/minute (vs 1.0/min before)
├─ Improvement: ${((parseFloat(ratePerMinute) / 1.0 - 1) * 100).toFixed(0)}% more opportunities
├─ Profitable: ${profitableCount}/${opportunityCount} (${opportunityCount > 0 ? (profitableCount/opportunityCount*100).toFixed(1) : '0'}%)
└─ Profit Rate: €${profitRate.toFixed(2)}/hour

📡 DEX PERFORMANCE:
${Object.entries(dexStats).map(([dex, count]) => 
  `├─ ${dex}: ${count} opportunities (${(count/opportunityCount*100).toFixed(1)}%)`
).join('\n')}

💡 RECOMMENDATION:
${parseFloat(ratePerMinute) > 2.0 ? 
  '✅ Enhanced rate is working well! Consider keeping these settings.' :
  '⚠️  Rate not significantly improved. May need further optimization.'
}
    `);
    
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
✅ Enhanced Rate Test Started!
🎯 Monitoring 5 DEXs: Orca, Jupiter, Raydium, Meteora, Lifinity
📈 Higher limits: 200 msg/min, 100MB/hour
⏱️  Test duration: 3 minutes
🛑 Press Ctrl+C to stop early
    `);
  } catch (error) {
    console.error('❌ Failed to start enhanced test:', error);
    process.exit(1);
  }
}

runEnhancedRateTest().catch(console.error);
