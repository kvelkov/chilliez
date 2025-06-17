// Multi-Hop Arbitrage Detection with Third Pool Integration
// Enhanced detection for complex arbitrage opportunities across multiple pools

const WebSocket = require('ws');
const fs = require('fs');
const path = require('path');

class MultiHopArbitrageStream {
  constructor(config) {
    this.wsUrl = config.wsUrl;
    this.apiKey = config.apiKey;
    this.ws = null;
    this.isConnected = false;
    
    // Optimized for your 1.02GB usage pattern
    this.messageCount = 0;
    this.dataReceived = 0;
    this.startTime = Date.now();
    this.maxMessagesPerMinute = 250; // Slightly higher based on your usage
    this.maxDataPerHour = 120 * 1024 * 1024; // 120MB/hour (you're using more efficiently)
    this.messageBuffer = [];
    this.lastProcessTime = Date.now();
    
    // Multi-hop detection settings
    this.minTransactionValue = 300; // Lower for multi-hop opportunities
    this.multiHopThreshold = 3; // Minimum 3 swaps for multi-hop
    this.maxHopCount = 5; // Track up to 5-hop arbitrage
    
    // Enhanced token set for multi-hop detection
    this.targetTokens = [
      'So11111111111111111111111111111111111111112', // SOL (base)
      'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC 
      'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT
      'mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So', // mSOL
      'bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1', // bSOL
      '7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj', // stSOL
      'J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn', // jitoSOL
    ];
    
    // Multi-hop statistics
    this.stats = {
      singleHop: 0,
      multiHop: 0,
      threePoolArbitrage: 0,
      complexArbitrage: 0,
      dexHits: {},
      hopDistribution: {},
      avgHopsPerTrade: 0
    };
    
    // Pool tracking for arbitrage path detection
    this.poolTracker = new Map();
    this.arbitragePaths = [];
    
    // Paper trading log
    this.logFile = path.join(__dirname, '../../paper_trading_logs/multihop_arbitrage_opportunities.json');
    this.opportunities = [];
    
    // Callbacks
    this.onArbitrageOpportunity = config.onArbitrageOpportunity || this.logOpportunity.bind(this);
    this.onMultiHopDetected = config.onMultiHopDetected || this.logMultiHop.bind(this);
    this.onError = config.onError || console.error;
    this.onStats = config.onStats || this.logStats.bind(this);
    
    // Stats interval
    this.statsInterval = setInterval(() => this.onStats(this.getStats()), 15000); // Every 15s
  }

  async connect() {
    try {
      console.log('🔌 Connecting to QuickNode (MULTI-HOP Mode)...');
      console.log('📊 Optimized for your usage: 250 msg/min, 120MB/hour');
      console.log('🔀 MULTI-HOP Detection: 3+ pool arbitrage paths');
      console.log('🎯 Enhanced DEX + Pool coverage');
      
      this.ws = new WebSocket(this.wsUrl);
      
      this.ws.on('open', () => {
        console.log('✅ Connected to QuickNode (Multi-Hop Mode)!');
        this.isConnected = true;
        this.setupMultiHopSubscriptions();
      });
      
      this.ws.on('message', (data) => {
        this.handleMessage(data);
      });
      
      this.ws.on('error', (error) => {
        console.error('❌ WebSocket error:', error);
        this.onError(error);
      });
      
      this.ws.on('close', () => {
        console.log('🔌 Connection closed');
        this.isConnected = false;
        clearInterval(this.statsInterval);
      });
      
    } catch (error) {
      console.error('❌ Connection failed:', error);
      this.onError(error);
    }
  }

  setupMultiHopSubscriptions() {
    console.log('📡 Setting up MULTI-HOP subscriptions...');
    
    // Enhanced DEX coverage including pools
    const multiHopDexes = [
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter (aggregator)
      '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM
      'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',  // Meteora
      '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c', // Lifinity
      'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY',  // Phoenix
    ];

    // Subscribe to all DEXs for multi-hop detection
    multiHopDexes.forEach((programId, index) => {
      this.subscribeToLogs(programId, index + 1);
      this.stats.dexHits[this.getDexName(programId)] = 0;
    });

    // Initialize hop distribution tracking
    for (let i = 1; i <= this.maxHopCount; i++) {
      this.stats.hopDistribution[`${i}_hop`] = 0;
    }
  }

  getDexName(programId) {
    const dexMap = {
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM': 'Orca',
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB': 'Jupiter', 
      '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8': 'Raydium',
      'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky': 'Meteora',
      '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c': 'Lifinity',
      'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY': 'Phoenix'
    };
    return dexMap[programId] || 'Unknown';
  }

  subscribeToLogs(programId, id) {
    const subscription = {
      jsonrpc: "2.0",
      id: id,
      method: "logsSubscribe",
      params: [
        { 
          mentions: [programId]
        },
        { 
          commitment: "confirmed"
        }
      ]
    };
    
    const dexName = this.getDexName(programId);
    console.log(`📡 Subscribing to ${dexName} for multi-hop detection`);
    this.send(subscription);
  }

  send(message) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  handleMessage(data) {
    if (!this.checkRateLimits(data)) {
      return;
    }

    try {
      const message = JSON.parse(data.toString());
      this.messageCount++;
      this.dataReceived += data.length;
      
      if (message.result && typeof message.result === 'number') {
        console.log(`✅ Multi-hop subscription ${message.id} confirmed`);
        return;
      }
      
      if (message.method === 'logsNotification') {
        this.processMultiHopLogNotification(message.params);
      }
      
    } catch (error) {
      console.error('❌ Error processing message:', error);
    }
  }

  checkRateLimits(data) {
    const now = Date.now();
    const hoursSinceStart = (now - this.startTime) / (1000 * 60 * 60);
    const minutesSinceStart = (now - this.startTime) / (1000 * 60);
    
    // Check data limit
    if (this.dataReceived > this.maxDataPerHour * hoursSinceStart) {
      if (now - this.lastProcessTime > 30000) {
        console.warn(`⚠️  Data rate: ${(this.dataReceived / 1024 / 1024).toFixed(2)}MB (limit: ${this.maxDataPerHour / 1024 / 1024}MB/hour)`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    // Check message limit
    if (this.messageCount > this.maxMessagesPerMinute * minutesSinceStart) {
      if (now - this.lastProcessTime > 30000) {
        console.warn(`⚠️  Message rate: ${this.messageCount} (limit: ${this.maxMessagesPerMinute}/min)`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    return true;
  }

  async processMultiHopLogNotification(params) {
    const { result } = params;
    const { value } = result;
    
    if (!value || !value.logs) {
      return;
    }

    const logs = value.logs;
    const signature = value.signature;
    
    // Analyze for multi-hop arbitrage patterns
    const multiHopAnalysis = this.analyzeMultiHopPattern(logs, signature);
    
    if (multiHopAnalysis.isMultiHopArbitrage) {
      this.handleMultiHopOpportunity(multiHopAnalysis, value);
    } else if (multiHopAnalysis.isSimpleArbitrage) {
      this.handleSimpleArbitrage(multiHopAnalysis, value);
    }
  }

  analyzeMultiHopPattern(logs, signature) {
    const analysis = {
      isMultiHopArbitrage: false,
      isSimpleArbitrage: false,
      hopCount: 0,
      swapCount: 0,
      dexesInvolved: new Set(),
      tokens: new Set(),
      poolsDetected: new Set(),
      arbitrageType: 'none',
      confidence: 0
    };

    let swapScore = 0;
    let routeScore = 0;
    let poolScore = 0;

    for (const log of logs) {
      const logLower = log.toLowerCase();
      
      // Detect DEX involvement
      if (log.includes('9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM')) {
        analysis.dexesInvolved.add('Orca');
        this.stats.dexHits.Orca++;
      } else if (log.includes('JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB')) {
        analysis.dexesInvolved.add('Jupiter');
        this.stats.dexHits.Jupiter++;
      } else if (log.includes('675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8')) {
        analysis.dexesInvolved.add('Raydium');
        this.stats.dexHits.Raydium++;
      } else if (log.includes('MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky')) {
        analysis.dexesInvolved.add('Meteora');
        this.stats.dexHits.Meteora++;
      } else if (log.includes('2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c')) {
        analysis.dexesInvolved.add('Lifinity');
        this.stats.dexHits.Lifinity++;
      } else if (log.includes('PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY')) {
        analysis.dexesInvolved.add('Phoenix');
        this.stats.dexHits.Phoenix++;
      }
      
      // Multi-hop indicators
      if (logLower.includes('swap')) {
        analysis.swapCount++;
        swapScore++;
      }
      
      if (logLower.includes('route') || logLower.includes('routing')) {
        routeScore++;
        analysis.hopCount++;
      }
      
      if (logLower.includes('pool') || logLower.includes('liquidity')) {
        poolScore++;
      }
      
      // Jupiter specific multi-hop patterns
      if (logLower.includes('exactinwithpath') || logLower.includes('exactoutwithpath')) {
        analysis.hopCount += 2;
        routeScore += 2;
      }
      
      // Count program invokes (indicates complexity)
      if (log.includes('Program invoke: [')) {
        analysis.hopCount++;
      }
      
      // Extract token addresses for path analysis
      const tokenMatches = log.match(/[A-Za-z0-9]{32,44}/g);
      if (tokenMatches) {
        tokenMatches.forEach(match => {
          if (match.length >= 32 && this.targetTokens.includes(match)) {
            analysis.tokens.add(match);
          }
        });
      }
    }

    // Determine arbitrage type
    analysis.confidence = Math.min(100, swapScore * 10 + routeScore * 15 + poolScore * 5);
    
    if (analysis.hopCount >= this.multiHopThreshold && analysis.swapCount >= 3) {
      analysis.isMultiHopArbitrage = true;
      
      if (analysis.dexesInvolved.size >= 3) {
        analysis.arbitrageType = 'complex_multi_dex';
        this.stats.complexArbitrage++;
      } else if (analysis.hopCount >= 3) {
        analysis.arbitrageType = 'three_pool_arbitrage';
        this.stats.threePoolArbitrage++;
      } else {
        analysis.arbitrageType = 'multi_hop';
        this.stats.multiHop++;
      }
      
      // Track hop distribution
      const hopKey = `${Math.min(analysis.hopCount, this.maxHopCount)}_hop`;
      this.stats.hopDistribution[hopKey]++;
      
    } else if (analysis.swapCount >= 2) {
      analysis.isSimpleArbitrage = true;
      analysis.arbitrageType = 'simple_arbitrage';
      this.stats.singleHop++;
    }

    return analysis;
  }

  handleMultiHopOpportunity(analysis, value) {
    const opportunity = {
      timestamp: new Date().toISOString(),
      signature: value.signature,
      slot: value.context?.slot,
      logs: value.logs,
      type: 'multi_hop_arbitrage',
      source: 'multihop_stream',
      
      // Multi-hop specific data
      hopCount: analysis.hopCount,
      swapCount: analysis.swapCount,
      arbitrageType: analysis.arbitrageType,
      dexesInvolved: Array.from(analysis.dexesInvolved),
      tokensInvolved: Array.from(analysis.tokens),
      confidence: analysis.confidence,
      complexity: this.calculateComplexity(analysis)
    };
    
    console.log(`🔀 MULTI-HOP ARBITRAGE DETECTED:`);
    console.log(`├─ Signature: ${value.signature.substring(0, 25)}...`);
    console.log(`├─ Type: ${analysis.arbitrageType.toUpperCase()}`);
    console.log(`├─ Hops: ${analysis.hopCount} | Swaps: ${analysis.swapCount}`);
    console.log(`├─ DEXs: ${Array.from(analysis.dexesInvolved).join(', ')}`);
    console.log(`├─ Tokens: ${analysis.tokens.size} different`);
    console.log(`├─ Confidence: ${analysis.confidence}%`);
    console.log(`└─ Complexity: ${opportunity.complexity}/10`);
    
    this.onMultiHopDetected(opportunity);
    this.onArbitrageOpportunity(opportunity);
  }

  handleSimpleArbitrage(analysis, value) {
    const opportunity = {
      timestamp: new Date().toISOString(),
      signature: value.signature,
      slot: value.context?.slot,
      logs: value.logs,
      type: 'simple_arbitrage',
      source: 'multihop_stream',
      
      swapCount: analysis.swapCount,
      dexesInvolved: Array.from(analysis.dexesInvolved),
      confidence: analysis.confidence
    };
    
    console.log(`🔍 Simple arbitrage: ${value.signature.substring(0, 20)}... (${analysis.swapCount} swaps)`);
    this.onArbitrageOpportunity(opportunity);
  }

  calculateComplexity(analysis) {
    let complexity = 0;
    
    // Base complexity from hops
    complexity += Math.min(analysis.hopCount, 5);
    
    // Additional complexity from multiple DEXs
    complexity += analysis.dexesInvolved.size;
    
    // Token diversity adds complexity
    complexity += Math.min(analysis.tokens.size, 3);
    
    return Math.min(10, complexity);
  }

  logMultiHop(opportunity) {
    console.log(`🎯 Multi-hop opportunity logged: ${opportunity.arbitrageType} (${opportunity.hopCount} hops)`);
  }

  logOpportunity(opportunity) {
    this.opportunities.push(opportunity);
    
    // Save every 3 opportunities (frequent saves for multi-hop)
    if (this.opportunities.length % 3 === 0) {
      this.saveOpportunities();
    }
  }

  saveOpportunities() {
    try {
      const dir = path.dirname(this.logFile);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      
      const sessionData = {
        session_stats: this.getStats(),
        multi_hop_summary: {
          total_opportunities: this.opportunities.length,
          multi_hop_count: this.stats.multiHop + this.stats.threePoolArbitrage + this.stats.complexArbitrage,
          simple_arbitrage_count: this.stats.singleHop,
          hop_distribution: this.stats.hopDistribution,
          average_hops: this.calculateAverageHops()
        },
        opportunities: this.opportunities
      };
      
      fs.writeFileSync(this.logFile, JSON.stringify(sessionData, null, 2));
      console.log(`💾 Saved ${this.opportunities.length} multi-hop opportunities`);
    } catch (error) {
      console.error('❌ Error saving opportunities:', error);
    }
  }

  calculateAverageHops() {
    const totalHops = Object.entries(this.stats.hopDistribution)
      .reduce((sum, [key, count]) => {
        const hops = parseInt(key.split('_')[0]);
        return sum + (hops * count);
      }, 0);
    
    const totalTrades = Object.values(this.stats.hopDistribution)
      .reduce((sum, count) => sum + count, 0);
    
    return totalTrades > 0 ? (totalHops / totalTrades).toFixed(2) : '0.00';
  }

  getStats() {
    const now = Date.now();
    const runtimeMinutes = (now - this.startTime) / (1000 * 60);
    const dataMB = this.dataReceived / 1024 / 1024;
    
    const totalOpportunities = this.stats.singleHop + this.stats.multiHop + 
                              this.stats.threePoolArbitrage + this.stats.complexArbitrage;
    
    return {
      runtime_minutes: runtimeMinutes.toFixed(1),
      total_messages: this.messageCount,
      data_received_mb: dataMB.toFixed(2),
      messages_per_minute: (this.messageCount / runtimeMinutes).toFixed(1),
      
      // Multi-hop specific stats
      total_opportunities: totalOpportunities,
      simple_arbitrage: this.stats.singleHop,
      multi_hop_arbitrage: this.stats.multiHop,
      three_pool_arbitrage: this.stats.threePoolArbitrage,
      complex_arbitrage: this.stats.complexArbitrage,
      
      opportunities_per_minute: (totalOpportunities / runtimeMinutes).toFixed(2),
      multi_hop_percentage: totalOpportunities > 0 ? 
        (((this.stats.multiHop + this.stats.threePoolArbitrage + this.stats.complexArbitrage) / totalOpportunities) * 100).toFixed(1) : '0.0',
      
      hop_distribution: this.stats.hopDistribution,
      average_hops_per_trade: this.calculateAverageHops(),
      dex_hits: this.stats.dexHits,
      connected: this.isConnected
    };
  }

  logStats(stats) {
    console.log(`
📊 MULTI-HOP ARBITRAGE STATS:
├─ Runtime: ${stats.runtime_minutes} minutes
├─ Data: ${stats.data_received_mb} MB (${stats.messages_per_minute}/min)
├─ Total Opportunities: ${stats.total_opportunities} (${stats.opportunities_per_minute}/min)
├─ Simple Arbitrage: ${stats.simple_arbitrage}
├─ Multi-Hop: ${stats.multi_hop_arbitrage} (${stats.multi_hop_percentage}%)
├─ Three-Pool: ${stats.three_pool_arbitrage}
├─ Complex: ${stats.complex_arbitrage}
├─ Avg Hops/Trade: ${stats.average_hops_per_trade}
├─ Hop Distribution: ${Object.entries(stats.hop_distribution).map(([k,v]) => `${k}:${v}`).join(' ')}
└─ DEX Hits: ${Object.entries(stats.dex_hits).map(([dex, hits]) => `${dex}:${hits}`).join(' ')}
    `);
  }

  disconnect() {
    if (this.ws) {
      this.ws.close();
    }
    if (this.statsInterval) {
      clearInterval(this.statsInterval);
    }
    this.saveOpportunities();
    console.log('🔌 Disconnected from Multi-Hop QuickNode Stream');
  }
}

module.exports = { MultiHopArbitrageStream };
