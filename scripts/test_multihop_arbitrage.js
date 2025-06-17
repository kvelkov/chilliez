#!/usr/bin/env node
// Multi-Hop Arbitrage Test - Optimized for Your 1.02GB Usage Pattern
require('dotenv').config({ path: '.env.paper-trading' });
const { MultiHopArbitrageStream } = require('../src/streaming/multihop_arbitrage_stream');
const { ProfitabilityAnalyzer } = require('../src/analysis/profitability_analyzer');

async function runMultiHopTest() {
  console.log('🚀 MULTI-HOP ARBITRAGE TEST');
  console.log('📊 Based on your usage: 1.02GB, 2323 blocks, 1 error');
  console.log('🔀 NEW FEATURES:');
  console.log('├─ Third pool arbitrage detection');
  console.log('├─ Multi-hop path analysis (up to 5 hops)');
  console.log('├─ Complex cross-DEX arbitrage');
  console.log('├─ Jupiter routing pattern detection');
  console.log('└─ Enhanced token path tracking');
  
  if (!process.env.WS_URL || !process.env.QUICKNODE_API_KEY) {
    console.error('❌ Missing QuickNode configuration');
    process.exit(1);
  }

  // Multi-hop optimized analyzer
  const analyzer = new ProfitabilityAnalyzer({
    walletBalanceSOL: 38.0,
    minProfitEur: 3.0, // Lower threshold for multi-hop (higher volume)
    minProfitPercent: 0.3, // Lower % threshold (multi-hop can be smaller margins)
    maxTradeSize: 0.15, // Slightly smaller for multi-hop safety
    slippageTolerance: 0.002 // Higher slippage tolerance for multi-hop
  });

  let opportunityCount = 0;
  let simpleArbitrage = 0;
  let multiHopArbitrage = 0;
  let threePoolArbitrage = 0;
  let complexArbitrage = 0;
  let profitableCount = 0;
  let totalPotentialProfit = 0;
  const startTime = Date.now();
  const arbitrageTypes = {};

  const stream = new MultiHopArbitrageStream({
    wsUrl: process.env.WS_URL,
    apiKey: process.env.QUICKNODE_API_KEY,
    
    onArbitrageOpportunity: async (opportunity) => {
      opportunityCount++;
      
      // Track arbitrage types
      const type = opportunity.type;
      arbitrageTypes[type] = (arbitrageTypes[type] || 0) + 1;
      
      if (type === 'simple_arbitrage') {
        simpleArbitrage++;
      } else {
        multiHopArbitrage++;
        
        if (opportunity.arbitrageType === 'three_pool_arbitrage') {
          threePoolArbitrage++;
        } else if (opportunity.arbitrageType === 'complex_multi_dex') {
          complexArbitrage++;
        }
      }
      
      console.log(`
🎯 OPPORTUNITY #${opportunityCount} (${type.toUpperCase()}):
├─ Signature: ${opportunity.signature.substring(0, 25)}...
├─ ${type === 'multi_hop_arbitrage' ? 
  `Hops: ${opportunity.hopCount} | DEXs: ${opportunity.dexesInvolved.join(',')}` :
  `Swaps: ${opportunity.swapCount} | DEXs: ${opportunity.dexesInvolved.join(',')}`}
├─ ${type === 'multi_hop_arbitrage' ? `Complexity: ${opportunity.complexity}/10` : `Confidence: ${opportunity.confidence}%`}
└─ Time: ${new Date(opportunity.timestamp).toLocaleTimeString()}
      `);

      // Analyze profitability for high-potential opportunities
      const shouldAnalyze = (
        (type === 'multi_hop_arbitrage' && opportunity.hopCount >= 3) ||
        (type === 'simple_arbitrage' && opportunity.confidence >= 60)
      );
      
      if (shouldAnalyze) {
        try {
          const analysis = await analyzer.analyzeProfitability(opportunity);
          
          if (analysis.isProfitable) {
            profitableCount++;
            totalPotentialProfit += analysis.analysis.netProfitEur;
            
            console.log(`
✅ PROFITABLE ${type.toUpperCase()}!
├─ Profit: €${analysis.analysis.netProfitEur.toFixed(2)} (${analysis.analysis.netProfitPercent.toFixed(2)}%)
├─ Trade Size: ${analysis.analysis.tradeAmountSOL.toFixed(2)} SOL
├─ ${type === 'multi_hop_arbitrage' ? 
  `Arbitrage Type: ${opportunity.arbitrageType}` : 
  `Simple Type: ${opportunity.dexesInvolved.length} DEX`}
├─ Analysis Confidence: ${analysis.confidence}%
└─ Recommendation: ${analysis.recommendation.action}
            `);
          } else {
            console.log(`❌ ${type}: €${analysis.analysis?.netProfitEur?.toFixed(2) || 'N/A'} profit (below threshold)`);
          }
        } catch (error) {
          console.log(`⚠️  Analysis skipped for ${type} opportunity`);
        }
      }
    },
    
    onMultiHopDetected: (opportunity) => {
      if (opportunity.arbitrageType === 'three_pool_arbitrage') {
        console.log(`🎯 THREE-POOL ARBITRAGE: ${opportunity.hopCount} hops across ${opportunity.dexesInvolved.length} DEXs`);
      } else if (opportunity.arbitrageType === 'complex_multi_dex') {
        console.log(`🌟 COMPLEX ARBITRAGE: ${opportunity.dexesInvolved.length} DEXs, complexity ${opportunity.complexity}/10`);
      }
    },
    
    onError: (error) => {
      console.error('❌ Multi-Hop Stream Error:', error.message);
    },
    
    onStats: (stats) => {
      const runtime = parseFloat(stats.runtime_minutes);
      const multiHopPercent = parseFloat(stats.multi_hop_percentage);
      
      console.log(`
📊 MULTI-HOP PERFORMANCE:
├─ Total: ${stats.total_opportunities} (${stats.opportunities_per_minute}/min)
├─ Simple: ${stats.simple_arbitrage} | Multi-Hop: ${stats.multi_hop_arbitrage} (${multiHopPercent}%)
├─ Three-Pool: ${stats.three_pool_arbitrage} | Complex: ${stats.complex_arbitrage}
├─ Profitable: ${profitableCount}/${opportunityCount} analyzed
├─ Avg Hops: ${stats.average_hops_per_trade}
├─ Data: ${stats.data_received_mb}MB (${stats.messages_per_minute}/min)
└─ Profit: €${totalPotentialProfit.toFixed(2)} potential
      `);
      
      // Show hop distribution
      if (Object.values(stats.hop_distribution).some(v => v > 0)) {
        const hopStats = Object.entries(stats.hop_distribution)
          .filter(([_, count]) => count > 0)
          .map(([hop, count]) => `${hop}:${count}`)
          .join(' ');
        console.log(`🔀 Hop Distribution: ${hopStats}`);
      }
    }
  });

  // Test duration: 4 minutes for comprehensive multi-hop analysis
  const testDuration = 4 * 60 * 1000;
  
  const stopTest = () => {
    const runtime = (Date.now() - startTime) / 1000;
    const ratePerMinute = (opportunityCount / (runtime / 60)).toFixed(2);
    const profitRate = totalPotentialProfit / (runtime / 3600);
    
    console.log(`
🏁 MULTI-HOP ARBITRAGE TEST COMPLETED!
📊 FINAL RESULTS (${runtime.toFixed(1)}s):
├─ Total Opportunities: ${opportunityCount} (${ratePerMinute}/min)
├─ Simple Arbitrage: ${simpleArbitrage} (${(simpleArbitrage/opportunityCount*100).toFixed(1)}%)
├─ Multi-Hop Arbitrage: ${multiHopArbitrage} (${(multiHopArbitrage/opportunityCount*100).toFixed(1)}%)
│  ├─ Three-Pool: ${threePoolArbitrage}
│  └─ Complex: ${complexArbitrage}
├─ Profitable: ${profitableCount}/${opportunityCount} (${opportunityCount > 0 ? (profitableCount/opportunityCount*100).toFixed(1) : '0'}%)
├─ Total Profit Potential: €${totalPotentialProfit.toFixed(2)}
├─ Hourly Profit Rate: €${profitRate.toFixed(2)}/hour
└─ Avg Profit/Trade: €${profitableCount > 0 ? (totalPotentialProfit/profitableCount).toFixed(2) : '0.00'}

🔀 ARBITRAGE TYPE BREAKDOWN:
${Object.entries(arbitrageTypes).map(([type, count]) => 
  `├─ ${type.replace('_', ' ').toUpperCase()}: ${count} (${(count/opportunityCount*100).toFixed(1)}%)`
).join('\n')}

💡 MULTI-HOP INSIGHTS:
├─ Multi-hop opportunities: ${multiHopArbitrage > 0 ? 'DETECTED ✅' : 'Not found ❌'}
├─ Three-pool arbitrage: ${threePoolArbitrage > 0 ? `${threePoolArbitrage} found ✅` : 'Not detected ❌'}
├─ Complex cross-DEX: ${complexArbitrage > 0 ? `${complexArbitrage} found ✅` : 'Not detected ❌'}
└─ Detection effectiveness: ${opportunityCount > 10 ? 'High volume ✅' : 'Low volume ⚠️'}

📈 OPTIMIZATION SUGGESTIONS:
${multiHopArbitrage > simpleArbitrage ? 
  '✅ Multi-hop detection is working well! Consider focusing on complex arbitrage.' :
  '💡 Consider adjusting multi-hop thresholds if simple arbitrage dominates.'
}
${profitableCount > opportunityCount * 0.2 ?
  '✅ Good profitability rate! Current thresholds are appropriate.' :
  '💡 Consider lowering profit thresholds to capture more multi-hop opportunities.'
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
✅ Multi-Hop Arbitrage Test Started!
🎯 Monitoring: 6 DEXs with multi-hop detection
🔀 Detecting: 1-5 hop arbitrage paths
💰 Wallet: 38 SOL (€5000) with multi-hop optimized settings
⏱️  Duration: 4 minutes
🛑 Press Ctrl+C to stop early
    `);
  } catch (error) {
    console.error('❌ Failed to start multi-hop test:', error);
    process.exit(1);
  }
}

runMultiHopTest().catch(console.error);
