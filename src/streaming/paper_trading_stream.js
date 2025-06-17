// Paper Trading Stream Setup - Rate Limited and Filtered
// This version includes throttling and filtering for paper trading testing

const WebSocket = require('ws');
const fs = require('fs');
const path = require('path');

class PaperTradingStream {
  constructor(config) {
    this.wsUrl = config.wsUrl;
    this.apiKey = config.apiKey;
    this.ws = null;
    this.isConnected = false;
    
    // Rate limiting and data management
    this.messageCount = 0;
    this.dataReceived = 0;
    this.startTime = Date.now();
    this.maxMessagesPerMinute = 100; // Limit messages
    this.maxDataPerHour = 50 * 1024 * 1024; // 50MB per hour limit
    this.messageBuffer = [];
    this.lastProcessTime = Date.now();
    
    // Filtering settings
    this.minTransactionValue = 1000; // Only process swaps > $1000
    this.targetTokens = [
      'So11111111111111111111111111111111111111112', // SOL
      'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC
      'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT
    ];
    
    // Paper trading log
    this.logFile = path.join(__dirname, '../../paper_trading_logs/arbitrage_opportunities.json');
    this.opportunities = [];
    
    // Callbacks
    this.onArbitrageOpportunity = config.onArbitrageOpportunity || this.logOpportunity.bind(this);
    this.onError = config.onError || console.error;
    this.onStats = config.onStats || this.logStats.bind(this);
    
    // Stats interval
    this.statsInterval = setInterval(() => this.onStats(this.getStats()), 30000); // Every 30s
  }

  async connect() {
    try {
      console.log('🔌 Connecting to QuickNode for Paper Trading...');
      console.log('📊 Rate limits: 100 msg/min, 50MB/hour');
      
      this.ws = new WebSocket(this.wsUrl);
      
      this.ws.on('open', () => {
        console.log('✅ Connected to QuickNode (Paper Trading Mode)!');
        this.isConnected = true;
        this.setupLimitedSubscriptions();
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

  setupLimitedSubscriptions() {
    console.log('📡 Setting up LIMITED subscriptions for paper trading...');
    
    // Only subscribe to major DEXs with high volume
    const majorDexes = [
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools (highest volume)
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter (aggregator)
    ];

    // Subscribe to only these 2 DEXs to reduce data
    majorDexes.forEach((programId, index) => {
      this.subscribeToLogs(programId, index + 1);
    });
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
          commitment: "confirmed" // Use confirmed instead of finalized for faster updates
        }
      ]
    };
    
    console.log(`📡 Subscribing to ${programId} logs...`);
    this.send(subscription);
  }

  send(message) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  handleMessage(data) {
    // Check rate limits
    if (!this.checkRateLimits(data)) {
      return;
    }

    try {
      const message = JSON.parse(data.toString());
      this.messageCount++;
      this.dataReceived += data.length;
      
      // Handle subscription confirmations
      if (message.result && typeof message.result === 'number') {
        console.log(`✅ Subscription ${message.id} confirmed`);
        return;
      }
      
      // Handle log notifications
      if (message.method === 'logsNotification') {
        this.processLogNotification(message.params);
      }
      
    } catch (error) {
      console.error('❌ Error processing message:', error);
    }
  }

  checkRateLimits(data) {
    const now = Date.now();
    const hoursSinceStart = (now - this.startTime) / (1000 * 60 * 60);
    const minutesSinceStart = (now - this.startTime) / (1000 * 60);
    
    // Check data limit (50MB per hour)
    if (this.dataReceived > this.maxDataPerHour * hoursSinceStart) {
      if (now - this.lastProcessTime > 60000) { // Log every minute
        console.warn(`⚠️  Data rate limit approaching: ${(this.dataReceived / 1024 / 1024).toFixed(2)}MB received`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    // Check message limit (100 per minute)
    if (this.messageCount > this.maxMessagesPerMinute * minutesSinceStart) {
      if (now - this.lastProcessTime > 60000) {
        console.warn(`⚠️  Message rate limit exceeded: ${this.messageCount} messages`);
        this.lastProcessTime = now;
      }
      return false;
    }
    
    return true;
  }

  async processLogNotification(params) {
    const { result } = params;
    const { value } = result;
    
    if (!value || !value.logs) {
      return;
    }

    // Simple arbitrage detection based on logs
    const logs = value.logs;
    const signature = value.signature;
    
    // Look for swap-related logs
    const swapIndicators = [
      'Program log: Instruction: Swap',
      'Program log: SwapEvent',
      'Program invoke: [',
      'swap'
    ];
    
    const hasSwap = logs.some(log => 
      swapIndicators.some(indicator => 
        log.toLowerCase().includes(indicator.toLowerCase())
      )
    );
    
    if (hasSwap) {
      // This is a potential arbitrage opportunity
      const opportunity = {
        timestamp: new Date().toISOString(),
        signature: signature,
        slot: value.context?.slot,
        logs: logs,
        type: 'swap_detected',
        source: 'paper_trading_stream'
      };
      
      console.log(`🔍 Potential arbitrage detected: ${signature}`);
      this.onArbitrageOpportunity(opportunity);
    }
  }

  logOpportunity(opportunity) {
    this.opportunities.push(opportunity);
    
    // Save to file every 10 opportunities
    if (this.opportunities.length % 10 === 0) {
      this.saveOpportunities();
    }
    
    console.log(`📝 Logged opportunity #${this.opportunities.length}: ${opportunity.signature}`);
  }

  saveOpportunities() {
    try {
      // Ensure directory exists
      const dir = path.dirname(this.logFile);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      
      fs.writeFileSync(this.logFile, JSON.stringify(this.opportunities, null, 2));
      console.log(`💾 Saved ${this.opportunities.length} opportunities to ${this.logFile}`);
    } catch (error) {
      console.error('❌ Error saving opportunities:', error);
    }
  }

  getStats() {
    const now = Date.now();
    const runtimeMinutes = (now - this.startTime) / (1000 * 60);
    const dataMB = this.dataReceived / 1024 / 1024;
    
    return {
      runtime_minutes: runtimeMinutes.toFixed(1),
      total_messages: this.messageCount,
      data_received_mb: dataMB.toFixed(2),
      messages_per_minute: (this.messageCount / runtimeMinutes).toFixed(1),
      opportunities_found: this.opportunities.length,
      connected: this.isConnected
    };
  }

  logStats(stats) {
    console.log(`
📊 PAPER TRADING STATS:
├─ Runtime: ${stats.runtime_minutes} minutes
├─ Messages: ${stats.total_messages} (${stats.messages_per_minute}/min)
├─ Data: ${stats.data_received_mb} MB
├─ Opportunities: ${stats.opportunities_found}
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
    console.log('🔌 Disconnected from QuickNode');
  }
}

module.exports = { PaperTradingStream };
