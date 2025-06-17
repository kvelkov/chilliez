#!/usr/bin/env node
// Paper Trading with Real Profitability Analysis
require('dotenv').config({ path: '.env.paper-trading' });
const { PaperTradingStream } = require('../src/streaming/paper_trading_stream');
const { ProfitabilityAnalyzer } = require('../src/analysis/profitability_analyzer');
const fs = require('fs');
const path = require('path');

async function runProfitabilityTest() {
  console.log('🚀 Paper Trading with Real Profitability Analysis');
  console.log('💰 Wallet: €5000 (~38 SOL at €131.27/SOL)');
  console.log('🎯 Min Profit: €5 per trade (0.5%)');
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration');
    process.exit(1);
  }

  // Initialize profitability analyzer with realistic settings
  const analyzer = new ProfitabilityAnalyzer({
    walletBalanceSOL: 38.0, // €5000 worth
    minProfitEur: 5.0, // Minimum €5 profit
    minProfitPercent: 0.5, // Minimum 0.5% ROI
    maxTradeSize: 0.2, // Max 20% of wallet per trade
    slippageTolerance: 0.001 // 0.1% slippage tolerance
  });

  let opportunityCount = 0;
  let profitableCount = 0;
  let totalPotentialProfit = 0;
  const results = [];
  const startTime = Date.now();

  const stream = new PaperTradingStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: async (opportunity) => {
      opportunityCount++;
      console.log(`\n🔍 ANALYZING OPPORTUNITY #${opportunityCount}:`);
      console.log(`├─ Signature: ${opportunity.signature.substring(0, 30)}...`);
      console.log(`├─ Logs: ${opportunity.logs.length} entries`);
      console.log(`└─ Time: ${new Date(opportunity.timestamp).toLocaleTimeString()}`);

      try {
        // Perform real profitability analysis
        const analysis = await analyzer.analyzeProfitability(opportunity);
        
        if (analysis.isProfitable) {
          profitableCount++;
          totalPotentialProfit += analysis.analysis.netProfitEur;
          
          console.log(`
✅ PROFITABLE OPPORTUNITY FOUND!
├─ Net Profit: €${analysis.analysis.netProfitEur.toFixed(2)} (${analysis.analysis.netProfitPercent.toFixed(2)}%)
├─ Trade Size: ${analysis.analysis.tradeAmountSOL.toFixed(2)} SOL (€${analysis.analysis.tradeAmountEur.toFixed(2)})
├─ Price Discrepancy: ${analysis.analysis.priceDiscrepancy.toFixed(2)}%
├─ Trading Costs: €${analysis.analysis.costs.totalCostEur.toFixed(2)}
├─ Confidence: ${analysis.confidence}%
├─ Recommendation: ${analysis.recommendation.action} (${analysis.recommendation.priority})
└─ [PAPER TRADE] Would execute: ${analysis.recommendation.suggestedAmount.toFixed(2)} SOL
          `);
        } else {
          console.log(`
❌ NOT PROFITABLE:
├─ Reason: ${analysis.reason || analysis.recommendation?.reason || 'Below thresholds'}
├─ Potential Profit: €${analysis.analysis?.netProfitEur?.toFixed(2) || 'N/A'}
├─ Confidence: ${analysis.confidence}%
└─ Action: SKIP
          `);
        }

        // Store result
        results.push({
          opportunity_id: opportunityCount,
          signature: opportunity.signature,
          timestamp: opportunity.timestamp,
          isProfitable: analysis.isProfitable,
          analysis: analysis.analysis,
          recommendation: analysis.recommendation,
          confidence: analysis.confidence
        });

      } catch (error) {
        console.error(`❌ Analysis error for opportunity #${opportunityCount}:`, error.message);
      }
    },
    
    onError: (error) => {
      console.error('❌ Stream Error:', error.message);
    },
    
    onStats: (stats) => {
      // Show live statistics
      if (opportunityCount > 0) {
        const successRate = (profitableCount / opportunityCount * 100).toFixed(1);
        console.log(`
📊 LIVE PROFITABILITY STATS:
├─ Opportunities: ${opportunityCount} analyzed
├─ Profitable: ${profitableCount} (${successRate}%)
├─ Potential Profit: €${totalPotentialProfit.toFixed(2)}
├─ Avg Profit: €${profitableCount > 0 ? (totalPotentialProfit / profitableCount).toFixed(2) : '0.00'}
└─ Stream: ${stats.data_received_mb} MB, ${stats.messages_per_minute}/min
        `);
      }
    }
  });

  // Test duration: 5 minutes for thorough analysis
  const testDuration = 5 * 60 * 1000;
  
  const stopTest = async () => {
    const runtime = (Date.now() - startTime) / 1000;
    console.log(`
🏁 PROFITABILITY TEST COMPLETED!
⏱️  Runtime: ${runtime.toFixed(1)} seconds
📊 Final Results:
├─ Total Opportunities: ${opportunityCount}
├─ Profitable Opportunities: ${profitableCount}
├─ Success Rate: ${opportunityCount > 0 ? (profitableCount / opportunityCount * 100).toFixed(1) : '0'}%
├─ Total Potential Profit: €${totalPotentialProfit.toFixed(2)}
├─ Average Profit per Trade: €${profitableCount > 0 ? (totalPotentialProfit / profitableCount).toFixed(2) : '0.00'}
└─ Hourly Profit Rate: €${((totalPotentialProfit / runtime) * 3600).toFixed(2)}/hour
    `);

    // Show analyzer statistics
    const analyzerStats = analyzer.getStats();
    console.log(`
📈 ANALYZER PERFORMANCE:
├─ Analysis Success Rate: ${analyzerStats.successRate}%
├─ Wallet Balance: ${analyzerStats.walletBalance.sol.toFixed(2)} SOL
├─ Min Profit Threshold: €${analyzerStats.thresholds.minProfitEur}
├─ Max Trade Size: ${(analyzerStats.thresholds.maxTradeSize * 100)}% of wallet
└─ Average Profit: €${analyzerStats.averageProfit.toFixed(2)}
    `);

    // Save detailed results
    if (results.length > 0) {
      const sessionData = {
        session: {
          start_time: new Date(startTime).toISOString(),
          end_time: new Date().toISOString(),
          runtime_seconds: runtime,
          wallet_balance_sol: 38.0,
          wallet_value_eur: 38.0 * 131.27,
          min_profit_eur: 5.0,
          min_profit_percent: 0.5
        },
        summary: {
          total_opportunities: opportunityCount,
          profitable_opportunities: profitableCount,
          success_rate_percent: opportunityCount > 0 ? (profitableCount / opportunityCount * 100) : 0,
          total_potential_profit_eur: totalPotentialProfit,
          average_profit_eur: profitableCount > 0 ? (totalPotentialProfit / profitableCount) : 0,
          hourly_profit_rate_eur: (totalPotentialProfit / runtime) * 3600
        },
        analyzer_stats: analyzerStats,
        opportunities: results
      };

      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const filename = path.join(__dirname, '../paper_trading_logs', `profitability_analysis_${timestamp.substring(0, 19)}.json`);
      
      // Ensure directory exists
      const dir = path.dirname(filename);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      
      fs.writeFileSync(filename, JSON.stringify(sessionData, null, 2));
      console.log(`💾 Detailed analysis saved to ${path.basename(filename)}`);

      // Show best opportunity
      const bestOpp = results.filter(r => r.isProfitable)
        .sort((a, b) => (b.analysis?.netProfitEur || 0) - (a.analysis?.netProfitEur || 0))[0];
      
      if (bestOpp) {
        console.log(`
🏆 BEST OPPORTUNITY:
├─ Signature: ${bestOpp.signature.substring(0, 30)}...
├─ Profit: €${bestOpp.analysis.netProfitEur.toFixed(2)} (${bestOpp.analysis.netProfitPercent.toFixed(2)}%)
├─ Trade Size: ${bestOpp.analysis.tradeAmountSOL.toFixed(2)} SOL
├─ Confidence: ${bestOpp.confidence}%
└─ Would have been: ${bestOpp.recommendation.action}
        `);
      }
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
✅ Profitability Analysis Started!
⏱️  Duration: 5 minutes
💰 Wallet: 38 SOL (€5000)
🎯 Thresholds: €5 min profit, 0.5% ROI
📊 Real-time profitability analysis
🛑 Press Ctrl+C to stop early
    `);
  } catch (error) {
    console.error('❌ Failed to start:', error);
    process.exit(1);
  }
}

runProfitabilityTest().catch(console.error);
