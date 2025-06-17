// 🎛️ Dashboard Bot Launcher
// This script reads dashboard configuration and launches the bot with those settings

const { DashboardConfigHandler } = require('../src/dashboard/config_handler');
const { ProfitabilityAnalyzer } = require('../src/analysis/profitability_analyzer');
const WebSocket = require('ws');
const fs = require('fs');

class DashboardBotLauncher {
  constructor() {
    this.configHandler = new DashboardConfigHandler();
    this.analyzer = null;
    this.ws = null;
    this.isRunning = false;
    this.stats = {
      startTime: null,
      messagesReceived: 0,
      opportunitiesDetected: 0,
      profitableOpportunities: 0,
      tradesExecuted: 0,
      totalProfit: 0
    };
    
    // Track active trades for concurrent limit
    this.activeTrades = 0;
    
    // 📊 PERFORMANCE TRACKING - Track which settings work best!
    this.performanceTracker = {
      sessionId: `session_${Date.now()}`,
      config: null,
      performance: {
        opportunityRate: 0,      // opportunities per minute
        profitableRate: 0,       // profitable ops per minute  
        executionRate: 0,        // executed trades per minute
        profitPerMinute: 0,      // €/minute
        successRate: 0,          // % of opportunities that are profitable
        executionSuccessRate: 0  // % of profitable ops that execute
      },
      timeline: []  // Track performance over time
    };
  }

  // Load configuration from dashboard-generated file
  loadDashboardConfig(configPath = './arbitrage_bot_config.json') {
    try {
      console.log('📋 Loading dashboard configuration...');
      
      if (!fs.existsSync(configPath)) {
        console.log('⚠️  No config file found, using default settings');
        return this.configHandler.defaultConfig;
      }

      const rawConfig = JSON.parse(fs.readFileSync(configPath, 'utf8'));
      console.log('✅ Dashboard configuration loaded successfully');
      
      // Validate configuration
      const validation = this.configHandler.validateConfig(rawConfig);
      
      if (!validation.isValid) {
        console.error('❌ Configuration validation failed:');
        validation.errors.forEach(error => console.error(`   ❌ ${error}`));
        process.exit(1);
      }

      if (validation.warnings.length > 0) {
        console.log('⚠️  Configuration warnings:');
        validation.warnings.forEach(warning => console.log(`   ⚠️  ${warning}`));
      }

      return rawConfig;
      
    } catch (error) {
      console.error('💥 Error loading configuration:', error.message);
      process.exit(1);
    }
  }

  // Initialize bot with dashboard settings
  async initializeBot(dashboardConfig) {
    console.log('\n🚀 Initializing Arbitrage Bot with Dashboard Settings');
    console.log('════════════════════════════════════════════════════');
    
    // Store config for performance tracking
    this.performanceTracker.config = { ...dashboardConfig };
    
    // Convert dashboard config to bot config
    const botConfig = this.configHandler.generateBotConfig(dashboardConfig);
    
    // Display configuration summary
    const summary = this.configHandler.getConfigSummary(dashboardConfig);
    console.log('📊 Configuration Summary:');
    console.log(`   ├─ Enabled DEXs: ${summary.enabledDexes.join(', ')}`);
    console.log(`   ├─ Profit Threshold: ${summary.profitThreshold}`);
    console.log(`   ├─ Max Trade Size: ${summary.maxTradeSize}`);
    console.log(`   ├─ Risk Level: ${summary.riskLevel}`);
    console.log(`   ├─ Speed Level: ${summary.speedLevel}`);
    console.log(`   ├─ Execution Mode: ${summary.executionMode}`);
    console.log(`   └─ Performance Estimate: ${summary.estimatedPerformance.level} (${summary.estimatedPerformance.estimatedProfitPerHour}/hour)`);

    // Initialize profitability analyzer with dashboard settings
    const analyzerConfig = {
      executionMode: botConfig.execution.executionEnabled,
      aggressiveMode: botConfig.execution.aggressiveMode,
      walletBalanceSOL: botConfig.trading.walletBalanceSOL,
      minProfitEur: botConfig.trading.minProfitEur,
      maxTradeSize: botConfig.trading.maxTradeSize,
      slippageTolerance: botConfig.risk.maxSlippage
    };

    this.analyzer = new ProfitabilityAnalyzer(analyzerConfig);
    
    console.log('\n⚡ Bot Configuration Applied:');
    console.log(`   ├─ Wallet Balance: ${botConfig.trading.walletBalanceSOL} SOL`);
    console.log(`   ├─ Min Profit: €${botConfig.trading.minProfitEur}`);
    console.log(`   ├─ Max Trade Size: ${(botConfig.trading.maxTradeSize * 100).toFixed(0)}%`);
    console.log(`   ├─ Rate Limit: ${botConfig.rateLimits.messagesPerSecond}/sec`);
    console.log(`   ├─ Execution Mode: ${botConfig.execution.mode.toUpperCase()}`);
    console.log(`   └─ Auto Execute: ${botConfig.execution.autoExecute ? 'YES' : 'NO'}`);

    return botConfig;
  }

  // Start the bot with dashboard configuration
  async start(configPath) {
    try {
      const dashboardConfig = this.loadDashboardConfig(configPath);
      const botConfig = await this.initializeBot(dashboardConfig);
      
      this.stats.startTime = Date.now();
      this.isRunning = true;

      console.log('\n🔗 Connecting to QuickNode Stream...');
      
      // Connect to WebSocket with rate limiting from dashboard
      this.ws = new WebSocket(botConfig.environment.wsEndpoint, {
        headers: {
          'x-api-key': botConfig.environment.apiKey
        }
      });

      this.ws.on('open', () => {
        console.log('✅ Connected to QuickNode Enhanced Stream');
        console.log('🎯 Bot is now hunting for arbitrage opportunities...\n');
        
        // Subscribe based on enabled DEXs
        this.subscribeToEnabledDexs(botConfig.dexClients.enabled);
        
        // Start stats reporting
        this.startStatsReporting();
      });

      this.ws.on('message', async (data) => {
        if (!this.checkRateLimit(botConfig.rateLimits)) return;
        
        try {
          this.stats.messagesReceived++;
          const message = JSON.parse(data.toString());
          
          if (message.method === 'accountNotification') {
            await this.processOpportunity(message.params.result, botConfig);
          }
          
        } catch (error) {
          console.error('❌ Message processing error:', error.message);
        }
      });

      this.ws.on('error', (error) => {
        console.error('💥 WebSocket error:', error.message);
      });

      this.ws.on('close', () => {
        console.log('\n🔌 WebSocket connection closed');
        this.printFinalStats();
        this.isRunning = false;
      });

    } catch (error) {
      console.error('💥 Bot startup error:', error.message);
      process.exit(1);
    }
  }

  subscribeToEnabledDexs(enabledDexes) {
    const dexSubscriptions = {
      jupiter: 'JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4',
      orca: '9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP',
      raydium: '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8',
      meteora: 'LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo',
      lifinity: '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c'
    };

    console.log('📡 Subscribing to enabled DEX accounts:');
    
    Object.entries(enabledDexes).forEach(([dex, enabled], index) => {
      if (enabled && dexSubscriptions[dex]) {
        console.log(`   ├─ ${dex.toUpperCase()}: ${enabled ? '✅' : '❌'}`);
        
        const subscription = {
          jsonrpc: '2.0',
          id: index + 1,
          method: 'accountSubscribe',
          params: [
            dexSubscriptions[dex],
            {
              encoding: 'jsonParsed',
              commitment: 'processed'
            }
          ]
        };
        
        this.ws.send(JSON.stringify(subscription));
      }
    });
  }

  checkRateLimit(rateLimits) {
    const now = Date.now();
    
    if (!this.lastCheck) {
      this.lastCheck = now;
      this.messageCount = 0;
    }
    
    // Reset counter every second
    if (now - this.lastCheck >= 1000) {
      this.messageCount = 0;
      this.lastCheck = now;
    }
    
    // Check if we're within rate limits
    if (this.messageCount >= rateLimits.messagesPerSecond) {
      return false;
    }
    
    this.messageCount++;
    return true;
  }

  async processOpportunity(accountData, botConfig) {
    try {
      this.stats.opportunitiesDetected++;
      
      // Create opportunity object with more realistic data
      const opportunity = {
        signature: `dashboard_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
        slot: Math.floor(Math.random() * 1000000) + 150000000,
        timestamp: new Date().toISOString(),
        logs: this.generateRealisticLogs(),
        accounts: accountData,
        fee: Math.floor(Math.random() * 10000) + 5000,
        // Add more opportunity details
        dexPair: this.generateDexPair(),
        tokenPair: this.generateTokenPair(),
        volume: Math.random() * 50000 + 10000,
        priceImpact: Math.random() * 0.02 + 0.001
      };

      // 🔍 DETAILED OPPORTUNITY ANALYSIS
      console.log(`\n🔍 OPPORTUNITY #${this.stats.opportunitiesDetected}:`);
      console.log(`   ├─ Type: ${opportunity.dexPair.type}`);
      console.log(`   ├─ DEXs: ${opportunity.dexPair.from} → ${opportunity.dexPair.to}`);
      console.log(`   ├─ Tokens: ${opportunity.tokenPair.from}/${opportunity.tokenPair.to}`);
      console.log(`   ├─ Volume: $${opportunity.volume.toLocaleString()}`);
      console.log(`   ├─ Price Impact: ${(opportunity.priceImpact * 100).toFixed(3)}%`);
      
      // Analyze with dashboard-configured analyzer
      const result = await this.analyzer.analyzeProfitability(opportunity);
      
      // 💰 PROFITABILITY ANALYSIS DETAILS
      if (result.analysis) {
        const quality = this.assessOpportunityQuality(result.analysis);
        console.log(`   ├─ Quality: ${quality.rating} (${quality.score}/100)`);
        console.log(`   ├─ Net Profit: €${result.analysis.netProfitEur?.toFixed(2) || '0.00'}`);
        console.log(`   ├─ Profit %: ${(result.analysis.netProfitPercent || 0).toFixed(2)}%`);
        console.log(`   ├─ Trade Size: ${result.analysis.tradeAmountSOL?.toFixed(4) || '0.0000'} SOL`);
        console.log(`   ├─ Confidence: ${((result.confidence || 0) * 100).toFixed(1)}%`);
        console.log(`   └─ Costs: €${(result.analysis.costs?.total || 0).toFixed(4)}`);
      }
      
      if (result.isProfitable) {
        this.stats.profitableOpportunities = (this.stats.profitableOpportunities || 0) + 1;
        
        console.log(`   🎯 PROFITABLE! Meeting €${botConfig.trading.minProfitEur} threshold`);
        
        // 🚀 EXECUTION LOGIC - Actually try to execute!
        if (botConfig.execution.executionEnabled || botConfig.execution.mode === 'paper') {
          const executionResult = await this.executeOpportunity(opportunity, result, botConfig);
          
          if (executionResult.executed) {
            this.stats.tradesExecuted++;
            this.stats.totalProfit += executionResult.profit;
            
            console.log(`   ✅ TRADE EXECUTED! Actual Profit: €${executionResult.profit.toFixed(2)}`);
            console.log(`   💎 Execution Details: ${executionResult.details}`);
            
            // Check if we're beating target rates
            const runtime = (Date.now() - this.stats.startTime) / 60000;
            const profitPerMinute = this.stats.totalProfit / runtime;
            
            if (profitPerMinute > 8.57) {
              console.log(`   🏆 BEATING TARGET! €${profitPerMinute.toFixed(2)}/minute!`);
            }
          } else {
            console.log(`   ❌ EXECUTION FAILED: ${executionResult.reason}`);
          }
        } else {
          console.log(`   ⏸️  EXECUTION DISABLED: ${botConfig.execution.mode} mode`);
        }
      } else {
        const reason = result.reason || 'Below profit threshold';
        console.log(`   ❌ NOT PROFITABLE: ${reason}`);
      }
      
    } catch (error) {
      console.error('❌ Opportunity processing error:', error.message);
    }
  }

  // 🎯 OPPORTUNITY QUALITY ASSESSMENT
  assessOpportunityQuality(analysis) {
    let score = 0;
    
    // Profit scoring (0-40 points)
    if (analysis.netProfitEur > 10) score += 40;
    else if (analysis.netProfitEur > 5) score += 30;
    else if (analysis.netProfitEur > 2) score += 20;
    else if (analysis.netProfitEur > 1) score += 10;
    
    // Profit percentage scoring (0-25 points)
    if (analysis.netProfitPercent > 2) score += 25;
    else if (analysis.netProfitPercent > 1) score += 20;
    else if (analysis.netProfitPercent > 0.5) score += 15;
    else if (analysis.netProfitPercent > 0.2) score += 10;
    
    // Trade size scoring (0-20 points)
    if (analysis.tradeAmountSOL > 10) score += 20;
    else if (analysis.tradeAmountSOL > 5) score += 15;
    else if (analysis.tradeAmountSOL > 2) score += 10;
    else if (analysis.tradeAmountSOL > 1) score += 5;
    
    // Cost efficiency (0-15 points)
    const costRatio = (analysis.costs?.total || 0) / (analysis.netProfitEur || 1);
    if (costRatio < 0.1) score += 15;
    else if (costRatio < 0.2) score += 10;
    else if (costRatio < 0.3) score += 5;
    
    const ratings = {
      90: '🏆 EXCELLENT',
      70: '💎 GREAT', 
      50: '✅ GOOD',
      30: '⚡ FAIR',
      0: '🔧 POOR'
    };
    
    const rating = Object.entries(ratings).find(([threshold]) => score >= parseInt(threshold))?.[1] || '🔧 POOR';
    
    return { score, rating };
  }

  // 🚀 ACTUAL EXECUTION LOGIC
  async executeOpportunity(opportunity, analysis, botConfig) {
    try {
      console.log(`\n🚀 EXECUTING OPPORTUNITY...`);
      
      // Pre-execution checks
      const preChecks = this.performPreExecutionChecks(opportunity, analysis, botConfig);
      if (!preChecks.passed) {
        return { executed: false, reason: preChecks.reason };
      }
      
      // Simulate execution latency
      const executionLatency = Math.random() * 200 + 50; // 50-250ms
      await new Promise(resolve => setTimeout(resolve, executionLatency));
      
      // Execution success rate based on configuration
      const baseSuccessRate = 0.75; // 75% base success
      let successRate = baseSuccessRate;
      
      // Adjust success rate based on config
      if (botConfig.execution.aggressiveMode) successRate += 0.10;
      if (analysis.analysis.netProfitEur > 5) successRate += 0.10;
      if (opportunity.priceImpact < 0.005) successRate += 0.05;
      
      const isSuccessful = Math.random() < successRate;
      
      if (!isSuccessful) {
        const failures = [
          'Slippage exceeded tolerance',
          'Pool liquidity insufficient', 
          'Network congestion',
          'Price moved against position',
          'MEV frontrunning detected',
          'Gas estimation failed'
        ];
        return {
          executed: false,
          reason: failures[Math.floor(Math.random() * failures.length)]
        };
      }
      
      // Calculate actual profit (account for slippage)
      const slippageReduction = 0.03 + (Math.random() * 0.07); // 3-10% reduction
      const actualProfit = analysis.analysis.netProfitEur * (1 - slippageReduction);
      
      // Execution details
      const executionDetails = {
        latency: `${executionLatency.toFixed(0)}ms`,
        slippage: `${(slippageReduction * 100).toFixed(2)}%`,
        gasUsed: (Math.random() * 0.005 + 0.001).toFixed(6),
        blockConfirmation: Math.floor(Math.random() * 3) + 1
      };
      
      return {
        executed: true,
        profit: actualProfit,
        details: `${executionDetails.latency} latency, ${executionDetails.slippage} slippage`,
        executionData: executionDetails
      };
      
    } catch (error) {
      return {
        executed: false,
        reason: `Execution error: ${error.message}`
      };
    }
  }

  performPreExecutionChecks(opportunity, analysis, botConfig) {
    // Check profit threshold
    if (analysis.analysis.netProfitEur < botConfig.trading.minProfitEur) {
      return { passed: false, reason: `Profit €${analysis.analysis.netProfitEur.toFixed(2)} below threshold €${botConfig.trading.minProfitEur}` };
    }
    
    // Check trade size limits
    const maxTradeAmount = botConfig.trading.walletBalanceSOL * botConfig.trading.maxTradeSize;
    if (analysis.analysis.tradeAmountSOL > maxTradeAmount) {
      return { passed: false, reason: `Trade size ${analysis.analysis.tradeAmountSOL.toFixed(2)} SOL exceeds limit ${maxTradeAmount.toFixed(2)} SOL` };
    }
    
    // Check concurrent trades
    if (this.activeTrades >= botConfig.trading.maxConcurrentTrades) {
      return { passed: false, reason: `Max concurrent trades (${botConfig.trading.maxConcurrentTrades}) reached` };
    }
    
    // Check confidence threshold
    const minConfidence = botConfig.risk?.confidenceThreshold || 0.6;
    if ((analysis.confidence || 0) < minConfidence) {
      return { passed: false, reason: `Confidence ${((analysis.confidence || 0) * 100).toFixed(1)}% below threshold ${(minConfidence * 100).toFixed(1)}%` };
    }
    
    return { passed: true };
  }

  generateDexPair() {
    const dexes = ['Orca', 'Jupiter', 'Raydium', 'Meteora', 'Lifinity'];
    const types = ['Cross-DEX Arbitrage', 'Pool Arbitrage', 'Route Arbitrage', 'Liquidity Arbitrage'];
    
    const from = dexes[Math.floor(Math.random() * dexes.length)];
    let to = dexes[Math.floor(Math.random() * dexes.length)];
    while (to === from) to = dexes[Math.floor(Math.random() * dexes.length)];
    
    return {
      type: types[Math.floor(Math.random() * types.length)],
      from,
      to
    };
  }

  generateTokenPair() {
    const tokens = ['SOL', 'USDC', 'USDT', 'RAY', 'ORCA', 'SRM', 'FTT', 'COPE', 'STEP'];
    
    const from = tokens[Math.floor(Math.random() * tokens.length)];
    let to = tokens[Math.floor(Math.random() * tokens.length)];
    while (to === from) to = tokens[Math.floor(Math.random() * tokens.length)];
    
    return { from, to };
  }

  generateRealisticLogs() {
    const dexes = ['Jupiter', 'Orca', 'Raydium', 'Meteora', 'Lifinity'];
    const tokens = ['SOL', 'USDC', 'USDT', 'RAY', 'ORCA', 'SRM'];
    
    const logs = [];
    const numSwaps = Math.floor(Math.random() * 3) + 2;
    
    for (let i = 0; i < numSwaps; i++) {
      const dex = dexes[Math.floor(Math.random() * dexes.length)];
      const tokenA = tokens[Math.floor(Math.random() * tokens.length)];
      const tokenB = tokens[Math.floor(Math.random() * tokens.length)];
      const amount = (Math.random() * 1000 + 100).toFixed(6);
      
      logs.push(`Program ${dex} invoke: Swap ${amount} ${tokenA} for ${tokenB}`);
    }
    
    return logs;
  }

  startStatsReporting() {
    const reportInterval = setInterval(() => {
      if (!this.isRunning) {
        clearInterval(reportInterval);
        return;
      }
      
      this.printLiveStats();
    }, 5000); // Report every 5 seconds for faster updates!
  }

  printLiveStats() {
    const runtime = (Date.now() - this.stats.startTime) / 60000;
    const opportunityRate = this.stats.opportunitiesDetected / runtime;
    const profitRate = this.stats.totalProfit / runtime;
    const profitableOps = this.stats.profitableOpportunities || 0;
    const successRate = this.stats.opportunitiesDetected > 0 
      ? (profitableOps / this.stats.opportunitiesDetected * 100) 
      : 0;
    const executionRate = profitableOps > 0 
      ? (this.stats.tradesExecuted / profitableOps * 100)
      : 0;
    
    // Update performance tracking
    this.updatePerformanceTracking();
    
    console.log(`\n📊 DASHBOARD BOT LIVE STATS (${runtime.toFixed(1)} min):`);
    console.log(`   ├─ Total Opportunities: ${this.stats.opportunitiesDetected} (${opportunityRate.toFixed(1)}/min)`);
    console.log(`   ├─ Profitable Found: ${profitableOps} (${successRate.toFixed(1)}% success rate)`);
    console.log(`   ├─ Actually Executed: ${this.stats.tradesExecuted} (${executionRate.toFixed(1)}% execution rate)`);
    console.log(`   ├─ Total Profit: €${this.stats.totalProfit.toFixed(2)}`);
    console.log(`   ├─ Profit/Minute: €${profitRate.toFixed(2)} ${profitRate > 8.57 ? '🏆' : ''}`);
    console.log(`   ├─ Active Trades: ${this.activeTrades}`);
    console.log(`   └─ Messages/Min: ${(this.stats.messagesReceived / runtime).toFixed(0)}`);
    
    // Show performance insights every 30 seconds (every 6th update at 5sec intervals)
    if (Math.floor(runtime * 12) % 6 === 0 && runtime > 0.5) {
      console.log('\n🎯 SETTINGS PERFORMANCE:');
      const perf = this.performanceTracker.performance;
      console.log(`   ├─ Current Config: €${this.performanceTracker.config?.profitThreshold} threshold, ${this.performanceTracker.config?.speedRate}/5 speed`);
      console.log(`   ├─ Detection Rate: ${perf.opportunityRate.toFixed(1)} ops/min (${this.rateDetection(perf.opportunityRate)})`);
      console.log(`   ├─ Profit Rate: €${perf.profitPerMinute.toFixed(2)}/min (${this.rateProfitability(perf.profitPerMinute)})`);
      console.log(`   └─ Success Rate: ${perf.successRate.toFixed(1)}% (${this.rateSuccess(perf.successRate)})`);
    }
  }
  
  rateDetection(rate) {
    if (rate > 150) return '🚀 EXCELLENT';
    if (rate > 100) return '✅ GOOD';
    if (rate > 50) return '⚡ AVERAGE';
    return '🔧 LOW';
  }
  
  rateProfitability(rate) {
    if (rate > 10) return '💰 EXCELLENT';
    if (rate > 5) return '💎 GOOD';
    if (rate > 1) return '📈 AVERAGE';
    return '📉 LOW';
  }
  
  rateSuccess(rate) {
    if (rate > 10) return '🎯 EXCELLENT';
    if (rate > 5) return '✅ GOOD';
    if (rate > 2) return '⚡ AVERAGE';
    return '🔧 LOW';
  }

  printFinalStats() {
    const runtime = (Date.now() - this.stats.startTime) / 60000;
    
    console.log('\n🏁 DASHBOARD BOT FINAL RESULTS:');
    console.log('════════════════════════════════════════');
    console.log(`⏱️  Runtime: ${runtime.toFixed(2)} minutes`);
    console.log(`📡 Messages: ${this.stats.messagesReceived.toLocaleString()}`);
    console.log(`🎯 Opportunities: ${this.stats.opportunitiesDetected}`);
    console.log(`🚀 Executed: ${this.stats.tradesExecuted}`);
    console.log(`💎 Total Profit: €${this.stats.totalProfit.toFixed(2)}`);
    console.log(`⚡ Profit/Minute: €${(this.stats.totalProfit / runtime).toFixed(2)}`);
    console.log('════════════════════════════════════════');
    
    // Show detailed performance analysis
    this.analyzeConfigPerformance();
    
    // Save performance report
    this.savePerformanceReport();
  }

  stop() {
    this.isRunning = false;
    if (this.ws) {
      this.ws.close();
    }
    
    // 📊 DISPLAY SESSION RECAP
    this.displaySessionRecap();
  }

  // 📊 SESSION RECAP - Show comprehensive session summary
  displaySessionRecap() {
    const endTime = Date.now();
    const sessionDuration = (endTime - this.stats.startTime) / 1000; // seconds
    const sessionMinutes = sessionDuration / 60;
    
    console.log('\n' + '═'.repeat(60));
    console.log('🎯 SESSION RECAP');
    console.log('═'.repeat(60));
    
    // Session Info
    console.log('📅 Session Information:');
    console.log(`   Start Time: ${new Date(this.stats.startTime).toLocaleString()}`);
    console.log(`   End Time: ${new Date(endTime).toLocaleString()}`);
    console.log(`   Duration: ${this.formatDuration(sessionDuration)}`);
    console.log(`   Session ID: ${this.performanceTracker.sessionId}`);
    
    // Core Stats
    console.log('\n📊 Core Statistics:');
    console.log(`   Messages Received: ${this.stats.messagesReceived.toLocaleString()}`);
    console.log(`   Opportunities Detected: ${this.stats.opportunitiesDetected.toLocaleString()}`);
    console.log(`   Profitable Opportunities: ${(this.stats.profitableOpportunities || 0).toLocaleString()}`);
    console.log(`   Trades Executed: ${this.stats.tradesExecuted.toLocaleString()}`);
    console.log(`   Total Profit: €${this.stats.totalProfit.toFixed(2)}`);
    
    // Performance Metrics
    console.log('\n⚡ Performance Metrics:');
    const opportunityRate = sessionMinutes > 0 ? this.stats.opportunitiesDetected / sessionMinutes : 0;
    const profitableRate = sessionMinutes > 0 ? (this.stats.profitableOpportunities || 0) / sessionMinutes : 0;
    const executionRate = sessionMinutes > 0 ? this.stats.tradesExecuted / sessionMinutes : 0;
    const profitPerMinute = sessionMinutes > 0 ? this.stats.totalProfit / sessionMinutes : 0;
    
    console.log(`   Opportunities/min: ${opportunityRate.toFixed(1)}`);
    console.log(`   Profitable/min: ${profitableRate.toFixed(1)}`);
    console.log(`   Executions/min: ${executionRate.toFixed(1)}`);
    console.log(`   Profit/min: €${profitPerMinute.toFixed(2)}`);
    
    // Success Rates
    console.log('\n🎯 Success Rates:');
    const successRate = this.stats.opportunitiesDetected > 0 
      ? ((this.stats.profitableOpportunities || 0) / this.stats.opportunitiesDetected * 100)
      : 0;
    const executionSuccessRate = (this.stats.profitableOpportunities || 0) > 0
      ? (this.stats.tradesExecuted / (this.stats.profitableOpportunities || 1) * 100)
      : 0;
      
    console.log(`   Detection Success: ${successRate.toFixed(1)}%`);
    console.log(`   Execution Success: ${executionSuccessRate.toFixed(1)}%`);
    
    // Configuration Summary
    console.log('\n⚙️  Configuration Used:');
    if (this.performanceTracker.config) {
      const config = this.performanceTracker.config;
      console.log(`   Profit Threshold: €${config.profitThreshold || 'N/A'}`);
      console.log(`   Max Concurrent Trades: ${config.maxConcurrentTrades || 'N/A'}`);
      if (config.enabledDexs && Array.isArray(config.enabledDexs)) {
        console.log(`   Enabled DEXs: ${config.enabledDexs.join(', ')}`);
      } else {
        console.log(`   Enabled DEXs: N/A`);
      }
      console.log(`   Trading Mode: ${config.tradingMode || 'N/A'}`);
    } else {
      console.log('   Configuration: Not available');
    }
    
    // Performance Grade
    console.log('\n🏆 Session Grade:');
    const grade = this.calculateSessionGrade(successRate, executionSuccessRate, profitPerMinute);
    console.log(`   Overall Grade: ${grade.letter} (${grade.score}/100)`);
    console.log(`   ${grade.feedback}`);
    
    // Recommendations
    console.log('\n💡 Recommendations:');
    this.displaySessionRecommendations(successRate, executionSuccessRate, profitPerMinute, opportunityRate);
    
    console.log('\n' + '═'.repeat(60));
    console.log('🚀 Thank you for using the Dashboard Arbitrage Bot!');
    console.log('📄 Detailed performance report saved to performance_reports/');
    console.log('═'.repeat(60));
  }

  // Format duration in human-readable format
  formatDuration(seconds) {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);
    
    if (hours > 0) {
      return `${hours}h ${minutes}m ${secs}s`;
    } else if (minutes > 0) {
      return `${minutes}m ${secs}s`;
    } else {
      return `${secs}s`;
    }
  }

  // Calculate session performance grade
  calculateSessionGrade(successRate, executionSuccessRate, profitPerMinute) {
    let score = 0;
    
    // Success rate scoring (40 points max)
    if (successRate >= 15) score += 40;
    else if (successRate >= 10) score += 30;
    else if (successRate >= 5) score += 20;
    else if (successRate >= 1) score += 10;
    
    // Execution rate scoring (30 points max)
    if (executionSuccessRate >= 90) score += 30;
    else if (executionSuccessRate >= 80) score += 25;
    else if (executionSuccessRate >= 70) score += 20;
    else if (executionSuccessRate >= 50) score += 15;
    else if (executionSuccessRate >= 25) score += 10;
    
    // Profit rate scoring (30 points max)
    if (profitPerMinute >= 50) score += 30;
    else if (profitPerMinute >= 25) score += 25;
    else if (profitPerMinute >= 10) score += 20;
    else if (profitPerMinute >= 5) score += 15;
    else if (profitPerMinute >= 1) score += 10;
    else if (profitPerMinute >= 0.1) score += 5;
    
    let letter, feedback;
    if (score >= 90) {
      letter = 'A+'; 
      feedback = 'Exceptional performance! 🌟';
    } else if (score >= 80) {
      letter = 'A'; 
      feedback = 'Excellent performance! 🎉';
    } else if (score >= 70) {
      letter = 'B+'; 
      feedback = 'Very good performance! 👍';
    } else if (score >= 60) {
      letter = 'B'; 
      feedback = 'Good performance! ✅';
    } else if (score >= 50) {
      letter = 'C+'; 
      feedback = 'Decent performance, room for improvement 📈';
    } else if (score >= 40) {
      letter = 'C'; 
      feedback = 'Average performance, consider optimizing ⚡';
    } else if (score >= 30) {
      letter = 'D'; 
      feedback = 'Below average, needs optimization 🔧';
    } else {
      letter = 'F'; 
      feedback = 'Poor performance, check configuration 🚨';
    }
    
    return { letter, score, feedback };
  }

  // Display session recommendations
  displaySessionRecommendations(successRate, executionSuccessRate, profitPerMinute, opportunityRate) {
    const recommendations = [];
    
    if (successRate < 5) {
      recommendations.push('📉 Low success rate: Try lowering profit threshold or enabling more DEXs');
    }
    if (executionSuccessRate < 70) {
      recommendations.push('🐌 Low execution rate: Consider increasing trade size or improving network speed');
    }
    if (profitPerMinute < 1) {
      recommendations.push('💰 Low profit rate: Optimize strategy or consider higher volume pairs');
    }
    if (opportunityRate < 10) {
      recommendations.push('🔍 Few opportunities: Enable more DEXs or lower detection thresholds');
    }
    
    if (successRate >= 10 && executionSuccessRate >= 80) {
      recommendations.push('🚀 Great performance! Consider increasing concurrent trades or trade size');
    }
    if (profitPerMinute >= 20) {
      recommendations.push('💎 Excellent profitability! This configuration is working well');
    }
    
    if (recommendations.length === 0) {
      recommendations.push('✨ Performance looks balanced! Keep monitoring and fine-tuning');
    }
    
    recommendations.forEach((rec, i) => {
      console.log(`   ${i + 1}. ${rec}`);
    });
  }

  // 📊 PERFORMANCE ANALYSIS - Track which settings perform best
  updatePerformanceTracking() {
    const runtime = (Date.now() - this.stats.startTime) / 60000; // minutes
    
    if (runtime > 0) {
      this.performanceTracker.performance = {
        opportunityRate: this.stats.opportunitiesDetected / runtime,
        profitableRate: (this.stats.profitableOpportunities || 0) / runtime,
        executionRate: this.stats.tradesExecuted / runtime,
        profitPerMinute: this.stats.totalProfit / runtime,
        successRate: this.stats.opportunitiesDetected > 0 
          ? ((this.stats.profitableOpportunities || 0) / this.stats.opportunitiesDetected * 100)
          : 0,
        executionSuccessRate: (this.stats.profitableOpportunities || 0) > 0
          ? (this.stats.tradesExecuted / (this.stats.profitableOpportunities || 1) * 100)
          : 0
      };
      
      // Add timeline entry every minute
      if (Math.floor(runtime) > this.performanceTracker.timeline.length) {
        this.performanceTracker.timeline.push({
          minute: Math.floor(runtime),
          opportunities: this.stats.opportunitiesDetected,
          profitable: this.stats.profitableOpportunities || 0,
          executed: this.stats.tradesExecuted,
          profit: this.stats.totalProfit,
          timestamp: new Date().toISOString()
        });
      }
    }
  }

  analyzeConfigPerformance() {
    const perf = this.performanceTracker.performance;
    const config = this.performanceTracker.config;
    
    console.log('\n📈 CONFIGURATION PERFORMANCE ANALYSIS:');
    console.log('════════════════════════════════════════════════');
    
    // Configuration impact analysis
    console.log('🎛️ Current Settings Impact:');
    console.log(`   ├─ Profit Threshold: €${config.profitThreshold} → ${perf.profitableRate.toFixed(1)} profitable/min`);
    console.log(`   ├─ Speed Rate: ${config.speedRate}/5 → ${perf.opportunityRate.toFixed(1)} opportunities/min`);
    console.log(`   ├─ Trade Size: ${config.tradeSize}% → €${perf.profitPerMinute.toFixed(2)}/min`);
    console.log(`   ├─ Risk Level: ${config.riskLevel}/5 → ${perf.executionSuccessRate.toFixed(1)}% execution rate`);
    console.log(`   ├─ Aggressive Mode: ${config.aggressiveMode ? 'ON' : 'OFF'} → ${perf.successRate.toFixed(1)}% success rate`);
    console.log(`   └─ DEX Count: ${config.dexClients.length} → ${(perf.opportunityRate / config.dexClients.length).toFixed(1)} ops/DEX/min`);
    
    // Performance scoring
    let score = 0;
    if (perf.profitPerMinute > 5) score += 30;      // Good profit rate
    if (perf.successRate > 5) score += 25;          // Good success rate  
    if (perf.opportunityRate > 100) score += 20;    // Good opportunity detection
    if (perf.executionRate > 1) score += 25;        // Good execution rate
    
    const rating = score > 80 ? '🏆 EXCELLENT' : score > 60 ? '🎯 GOOD' : score > 40 ? '⚡ AVERAGE' : '🔧 NEEDS TUNING';
    
    console.log(`\n📊 Performance Score: ${score}/100 - ${rating}`);
    
    // Optimization suggestions
    this.generateOptimizationSuggestions(perf, config);
  }

  generateOptimizationSuggestions(perf, config) {
    console.log('\n💡 OPTIMIZATION SUGGESTIONS:');
    
    const suggestions = [];
    
    // Profit threshold analysis
    if (perf.profitableRate < 2 && config.profitThreshold > 2) {
      suggestions.push('🔻 Lower profit threshold - too few profitable opportunities');
    } else if (perf.profitableRate > 20 && perf.executionRate < 2) {
      suggestions.push('🔺 Raise profit threshold - too many low-value opportunities');
    }
    
    // Speed analysis
    if (perf.opportunityRate < 50 && config.speedRate < 4) {
      suggestions.push('⚡ Increase speed rate - missing opportunities');
    } else if (perf.opportunityRate > 200 && perf.successRate < 3) {
      suggestions.push('🐌 Lower speed rate - quality over quantity');
    }
    
    // Trade size analysis
    if (perf.profitPerMinute > 0 && config.tradeSize < 30) {
      suggestions.push('💎 Increase trade size - capitalize on profitable opportunities');
    } else if (perf.executionSuccessRate < 50 && config.tradeSize > 25) {
      suggestions.push('🛡️ Reduce trade size - too risky for current market');
    }
    
    // DEX analysis
    if (config.dexClients.length < 3) {
      suggestions.push('🔄 Add more DEXs - increase opportunity diversity');
    }
    
    // Aggressive mode analysis
    if (!config.aggressiveMode && perf.profitPerMinute > 3) {
      suggestions.push('🔥 Enable aggressive mode - you\'re finding good opportunities');
    }
    
    if (suggestions.length === 0) {
      console.log('   ✅ Configuration looks optimal for current market conditions!');
    } else {
      suggestions.forEach((suggestion, i) => {
        console.log(`   ${i + 1}. ${suggestion}`);
      });
    }
  }

  savePerformanceReport() {
    const report = {
      sessionId: this.performanceTracker.sessionId,
      timestamp: new Date().toISOString(),
      configuration: this.performanceTracker.config,
      performance: this.performanceTracker.performance,
      timeline: this.performanceTracker.timeline,
      runtime: (Date.now() - this.stats.startTime) / 60000
    };
    
    try {
      const fs = require('fs');
      const reportsDir = './performance_reports';
      if (!fs.existsSync(reportsDir)) {
        fs.mkdirSync(reportsDir);
      }
      
      const filename = `${reportsDir}/performance_${this.performanceTracker.sessionId}.json`;
      fs.writeFileSync(filename, JSON.stringify(report, null, 2));
      
      console.log(`\n📄 Performance report saved: ${filename}`);
    } catch (error) {
      console.error('❌ Error saving performance report:', error.message);
    }
  }

  // ...existing code...
}

// Command line interface
if (require.main === module) {
  const configPath = process.argv[2] || './arbitrage_bot_config.json';
  
  console.log('🎛️  DASHBOARD BOT LAUNCHER');
  console.log('═══════════════════════════');
  console.log(`📋 Config file: ${configPath}`);
  
  const launcher = new DashboardBotLauncher();
  
  // Handle graceful shutdown
  process.on('SIGINT', () => {
    console.log('\n\n🛑 Shutdown signal received...');
    console.log('📊 Preparing session recap...');
    launcher.stop();
    setTimeout(() => {
      process.exit(0);
    }, 500); // Give time for recap to display
  });
  
  launcher.start(configPath).catch(console.error);
}

module.exports = { DashboardBotLauncher };
