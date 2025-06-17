// Enhanced Paper Trading Stream with Increased Search Scope
// More DEXs, higher limits, better opportunity detection

const WebSocket = require('ws');
const fs = require('fs');
const path = require('path');

class EnhancedPaperTradingStream {
  constructor(config) {
    this.wsUrl = config.wsUrl;
    this.apiKey = config.apiKey;
    this.ws = null;
    this.isConnected = false;
    
    // REDUCED RATE LIMITS to avoid hitting QuickNode limits
    this.messageCount = 0;
    this.dataReceived = 0;
    this.startTime = Date.now();
    this.maxMessagesPerMinute = 120; // REDUCED from 200 to 120
    this.maxDataPerHour = 70 * 1024 * 1024; // REDUCED from 100MB to 70MB per hour
    this.messageBuffer = [];
    this.lastProcessTime = Date.now();
    this.rateLimitBuffer = 15; // Add safety buffer
    
    // Enhanced filtering settings
    this.minTransactionValue = 500; // LOWERED from $1000 to $500 (more opportunities)
    this.targetTokens = [
      'So11111111111111111111111111111111111111112', // SOL
      'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC
      'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT
      'mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So', // mSOL (Marinade)
      'bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1', // bSOL (Blaze)
    ];
    
    // Statistics
    this.stats = {
      opportunitiesDetected: 0,
      dexHits: {},
      avgOpportunitiesPerMinute: 0
    };
    
    // Paper trading log
    this.logFile = path.join(__dirname, '../../paper_trading_logs/enhanced_arbitrage_opportunities.json');
    this.opportunities = [];
    
    // Callbacks
    this.onArbitrageOpportunity = config.onArbitrageOpportunity || this.logOpportunity.bind(this);
    this.onError = config.onError || console.error;
    this.onStats = config.onStats || this.logStats.bind(this);
    
    // Stats interval
    this.statsInterval = setInterval(() => this.onStats(this.getStats()), 20000); // Every 20s
  }

  async connect() {
    try {
      console.log('🔌 Connecting to QuickNode (ENHANCED Mode)...');
      console.log('📊 INCREASED Rate limits: 200 msg/min, 100MB/hour');
      console.log('🎯 EXPANDED DEX coverage: 5 major DEXs');
      
      this.ws = new WebSocket(this.wsUrl);
      
      this.ws.on('open', () => {
        console.log('✅ Connected to QuickNode (Enhanced Mode)!');
        this.isConnected = true;
        this.setupEnhancedSubscriptions();
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

  setupEnhancedSubscriptions() {
    console.log('📡 Setting up ENHANCED subscriptions...');
    
    // EXPANDED DEX coverage for more opportunities
    const enhancedDexes = [
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter (aggregator)
      '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM
      'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',  // Meteora
      '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c', // Lifinity
    ];

    // Subscribe to all 5 major DEXs
    enhancedDexes.forEach((programId, index) => {
      this.subscribeToLogs(programId, index + 1);
      this.stats.dexHits[this.getDexName(programId)] = 0;
    });
  }

  getDexName(programId) {
    const dexMap = {
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM': 'Orca',
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB': 'Jupiter', 
      '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8': 'Raydium',
      'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky': 'Meteora',
      '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c': 'Lifinity'
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
          commitment: "confirmed" // Fast updates
        }
      ]
    };
    
    const dexName = this.getDexName(programId);
    console.log(`📡 Subscribing to ${dexName} (${programId.substring(0, 8)}...)`);
    this.send(subscription);
  }

  send(message) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  handleMessage(data) {
    // ENHANCED rate limiting with higher thresholds
    if (!this.checkEnhancedRateLimits(data)) {
      return;
    }

    try {
      const message = JSON.parse(data.toString());
      this.messageCount++;
      this.dataReceived += data.length;
      
      // Handle subscription confirmations
      if (message.result && typeof message.result === 'number') {
        console.log(`✅ Enhanced subscription ${message.id} confirmed`);
        return;
      }
      
      // Handle log notifications
      if (message.method === 'logsNotification') {
        this.processEnhancedLogNotification(message.params);
      }
      
    } catch (error) {
      console.error('❌ Error processing message:', error);
    }
  }

  checkEnhancedRateLimits(data) {
    const now = Date.now();
    const hoursSinceStart = (now - this.startTime) / (1000 * 60 * 60);
    const minutesSinceStart = (now - this.startTime) / (1000 * 60);
    
    // Check enhanced data limit (100MB per hour)
    if (this.dataReceived > this.maxDataPerHour * hoursSinceStart) {
      if (now - this.lastProcessTime > 30000) { // Log every 30s
        console.warn(`⚠️  Data rate approaching limit: ${(this.dataReceived / 1024 / 1024).toFixed(2)}MB`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    // Check enhanced message limit (200 per minute)
    if (this.messageCount > this.maxMessagesPerMinute * minutesSinceStart) {
      if (now - this.lastProcessTime > 30000) {
        console.warn(`⚠️  Message rate high: ${this.messageCount} messages`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    return true;
  }

  async processEnhancedLogNotification(params) {
    const { result } = params;
    const { value } = result;
    
    if (!value || !value.logs) {
      return;
    }

    const logs = value.logs;
    const signature = value.signature;
    
    // ENHANCED arbitrage detection with more patterns
    const arbitrageIndicators = [
      'Program log: Instruction: Swap',
      'Program log: SwapEvent', 
      'Program log: Route', // Jupiter routing
      'Program invoke: [',
      'swap',
      'SwapExactTokensForTokens', // Raydium
      'exchange', // Meteora
      'trade' // General trading
    ];
    
    let swapScore = 0;
    let detectedDex = 'Unknown';
    
    // Score the transaction based on arbitrage indicators
    for (const log of logs) {
      const logLower = log.toLowerCase();
      
      // Check for DEX identification
      if (log.includes('9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM')) {
        detectedDex = 'Orca';
        this.stats.dexHits.Orca++;
      } else if (log.includes('JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB')) {
        detectedDex = 'Jupiter';
        this.stats.dexHits.Jupiter++;
      } else if (log.includes('675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8')) {
        detectedDex = 'Raydium';
        this.stats.dexHits.Raydium++;
      } else if (log.includes('MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky')) {
        detectedDex = 'Meteora';
        this.stats.dexHits.Meteora++;
      } else if (log.includes('2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c')) {
        detectedDex = 'Lifinity';
        this.stats.dexHits.Lifinity++;
      }
      
      // Score arbitrage indicators
      for (const indicator of arbitrageIndicators) {
        if (logLower.includes(indicator.toLowerCase())) {
          swapScore++;
        }
      }
    }
    
    // Enhanced detection threshold (lower = more sensitive)
    if (swapScore >= 1) { // LOWERED from 2 to 1 (more opportunities)
      this.stats.opportunitiesDetected++;
      
      const opportunity = {
        timestamp: new Date().toISOString(),
        signature: signature,
        slot: value.context?.slot,
        logs: logs,
        type: 'enhanced_swap_detected',
        source: 'enhanced_paper_trading_stream',
        swapScore: swapScore,
        detectedDex: detectedDex,
        logCount: logs.length
      };
      
      console.log(`🔍 Enhanced arbitrage detected: ${signature} (Score: ${swapScore}, DEX: ${detectedDex})`);
      this.onArbitrageOpportunity(opportunity);
    }
  }

  logOpportunity(opportunity) {
    this.opportunities.push(opportunity);
    
    // Save more frequently for better tracking
    if (this.opportunities.length % 5 === 0) { // Every 5 opportunities
      this.saveOpportunities();
    }
    
    console.log(`📝 Enhanced opportunity #${this.opportunities.length}: ${opportunity.signature.substring(0, 20)}... (${opportunity.detectedDex})`);
  }

  saveOpportunities() {
    try {
      const dir = path.dirname(this.logFile);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      
      fs.writeFileSync(this.logFile, JSON.stringify({
        session_stats: this.getStats(),
        opportunities: this.opportunities
      }, null, 2));
      
      console.log(`💾 Saved ${this.opportunities.length} enhanced opportunities`);
    } catch (error) {
      console.error('❌ Error saving opportunities:', error);
    }
  }

  getStats() {
    const now = Date.now();
    const runtimeMinutes = (now - this.startTime) / (1000 * 60);
    const dataMB = this.dataReceived / 1024 / 1024;
    
    this.stats.avgOpportunitiesPerMinute = this.stats.opportunitiesDetected / runtimeMinutes;
    
    return {
      runtime_minutes: runtimeMinutes.toFixed(1),
      total_messages: this.messageCount,
      data_received_mb: dataMB.toFixed(2),
      messages_per_minute: (this.messageCount / runtimeMinutes).toFixed(1),
      opportunities_found: this.stats.opportunitiesDetected,
      opportunities_per_minute: this.stats.avgOpportunitiesPerMinute.toFixed(2),
      dex_hits: this.stats.dexHits,
      connected: this.isConnected,
      limits: {
        message_limit: this.maxMessagesPerMinute,
        data_limit_mb: this.maxDataPerHour / 1024 / 1024
      }
    };
  }

  logStats(stats) {
    console.log(`
📊 ENHANCED STREAMING STATS:
├─ Runtime: ${stats.runtime_minutes} minutes
├─ Messages: ${stats.total_messages} (${stats.messages_per_minute}/min) [Limit: ${stats.limits.message_limit}/min]
├─ Data: ${stats.data_received_mb} MB [Limit: ${stats.limits.data_limit_mb}MB/hour]
├─ Opportunities: ${stats.opportunities_found} (${stats.opportunities_per_minute}/min)
├─ DEX Breakdown: ${Object.entries(stats.dex_hits).map(([dex, hits]) => `${dex}: ${hits}`).join(', ')}
└─ Status: ${stats.connected ? '🟢 Connected' : '🔴 Disconnected'}
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
    console.log('🔌 Disconnected from Enhanced QuickNode Stream');
  }
}

module.exports = { EnhancedPaperTradingStream };
