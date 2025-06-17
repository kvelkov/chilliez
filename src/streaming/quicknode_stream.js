// QuickNode Streaming Setup for Arbitrage Detection
// This connects to QuickNode streams and processes transactions in real-time

const WebSocket = require('ws');
const { ArbitrageDetector } = require('../functions/quicknode_arbitrage_function');

class QuickNodeStream {
  constructor(config) {
    this.wsUrl = config.wsUrl;
    this.apiKey = config.apiKey;
    this.ws = null;
    this.detector = new ArbitrageDetector();
    this.subscriptions = new Map();
    this.isConnected = false;
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = 10;
    this.reconnectDelay = 5000;
    
    // Callbacks
    this.onArbitrageOpportunity = config.onArbitrageOpportunity || (() => {});
    this.onLargeTrade = config.onLargeTrade || (() => {});
    this.onError = config.onError || console.error;
    this.onConnected = config.onConnected || (() => {});
  }

  async connect() {
    try {
      console.log('🔌 Connecting to QuickNode WebSocket...');
      console.log('URL:', this.wsUrl);
      
      this.ws = new WebSocket(this.wsUrl);
      
      this.ws.on('open', () => {
        console.log('✅ Connected to QuickNode!');
        this.isConnected = true;
        this.reconnectAttempts = 0;
        this.onConnected();
        this.setupSubscriptions();
      });
      
      this.ws.on('message', (data) => {
        this.handleMessage(data);
      });
      
      this.ws.on('error', (error) => {
        console.error('❌ WebSocket error:', error);
        this.onError(error);
      });
      
      this.ws.on('close', () => {
        console.log('🔌 WebSocket connection closed');
        this.isConnected = false;
        this.handleReconnect();
      });
      
    } catch (error) {
      console.error('❌ Connection failed:', error);
      this.onError(error);
    }
  }

  setupSubscriptions() {
    console.log('📡 Setting up DEX monitoring subscriptions...');
    
    // Subscribe to transaction logs for all major DEXs
    const dexPrograms = [
      '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools
      '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM
      'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter
      'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',  // Meteora
      '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c', // Lifinity
    ];

    // Subscribe to logs for each DEX
    dexPrograms.forEach((programId, index) => {
      this.subscribeToLogs(programId, index + 1);
    });

    // Subscribe to signature notifications for specific transactions
    this.subscribeToSignatures();
    
    // Subscribe to slot updates for timing
    this.subscribeToSlots();
  }

  subscribeToLogs(programId, id) {
    const subscription = {
      jsonrpc: "2.0",
      id: id,
      method: "logsSubscribe",
      params: [
        { mentions: [programId] },
        { commitment: "finalized" }
      ]
    };
    
    console.log(`📡 Subscribing to ${programId} logs...`);
    this.send(subscription);
    this.subscriptions.set(id, { type: 'logs', programId });
  }

  subscribeToSignatures() {
    // Subscribe to specific transaction signatures if needed
    // This would be used for monitoring specific transactions
    const subscription = {
      jsonrpc: "2.0",
      id: 100,
      method: "signatureSubscribe",
      params: [
        "TRANSACTION_SIGNATURE_HERE", // Replace with actual signature
        { commitment: "finalized" }
      ]
    };
    
    // Uncomment to enable signature monitoring
    // this.send(subscription);
    // this.subscriptions.set(100, { type: 'signature' });
  }

  subscribeToSlots() {
    const subscription = {
      jsonrpc: "2.0",
      id: 200,
      method: "slotSubscribe"
    };
    
    console.log('📡 Subscribing to slot updates...');
    this.send(subscription);
    this.subscriptions.set(200, { type: 'slot' });
  }

  send(message) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    } else {
      console.error('❌ WebSocket not connected, cannot send message');
    }
  }

  handleMessage(data) {
    try {
      const message = JSON.parse(data.toString());
      
      // Handle subscription confirmations
      if (message.result && typeof message.result === 'number') {
        console.log(`✅ Subscription ${message.id} confirmed (ID: ${message.result})`);
        return;
      }
      
      // Handle notifications
      if (message.method) {
        switch (message.method) {
          case 'logsNotification':
            this.handleLogNotification(message);
            break;
          case 'signatureNotification':
            this.handleSignatureNotification(message);
            break;
          case 'slotNotification':
            this.handleSlotNotification(message);
            break;
          default:
            console.log('📨 Unknown notification:', message.method);
        }
      }
      
    } catch (error) {
      console.error('❌ Error parsing message:', error);
      console.log('Raw message:', data.toString());
    }
  }

  async handleLogNotification(message) {
    try {
      const logs = message.params?.result?.value?.logs || [];
      const signature = message.params?.result?.value?.signature;
      
      if (!signature) return;
      
      console.log(`\n🔥 DEX Activity Detected!`);
      console.log(`Signature: ${signature}`);
      console.log(`Logs: ${logs.length} lines`);
      
      // Fetch full transaction details
      const transaction = await this.getTransaction(signature);
      if (transaction) {
        await this.analyzeTransaction(transaction);
      }
      
    } catch (error) {
      console.error('❌ Error handling log notification:', error);
    }
  }

  handleSignatureNotification(message) {
    const signature = message.params?.result?.value;
    console.log(`📝 Signature notification: ${signature}`);
  }

  handleSlotNotification(message) {
    const slot = message.params?.result?.slot;
    if (slot && slot % 100 === 0) { // Log every 100th slot to reduce noise
      console.log(`🎰 Slot: ${slot}`);
    }
  }

  async getTransaction(signature) {
    try {
      // Get transaction details via RPC
      const rpcUrl = this.wsUrl.replace('wss://', 'https://').replace('ws://', 'http://');
      
      const response = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "getTransaction",
          params: [
            signature,
            {
              encoding: "jsonParsed",
              commitment: "finalized",
              maxSupportedTransactionVersion: 0
            }
          ]
        })
      });
      
      const data = await response.json();
      return data.result;
      
    } catch (error) {
      console.error('❌ Error fetching transaction:', error);
      return null;
    }
  }

  async analyzeTransaction(transaction) {
    try {
      // Use our arbitrage detector
      const result = this.detector.analyzeTransaction(transaction);
      
      if (result && result.hasOpportunities) {
        console.log('\n🎯 ARBITRAGE OPPORTUNITY DETECTED!');
        console.log('==========================================');
        console.log(`Signature: ${result.signature}`);
        console.log(`Opportunities: ${result.opportunities.length}`);
        console.log(`Estimated Value: $${result.estimatedValueUSD.toFixed(2)}`);
        console.log(`Is Large Trade: ${result.isLargeTrade}`);
        
        result.opportunities.forEach((opp, index) => {
          console.log(`\n📊 Opportunity ${index + 1}:`);
          console.log(`  Type: ${opp.type}`);
          console.log(`  Estimated Profit: $${opp.estimatedProfit?.toFixed(2) || 'N/A'}`);
          console.log(`  Confidence: ${opp.confidence}`);
          
          if (opp.dexes) {
            console.log(`  DEXs: ${opp.dexes.join(', ')}`);
          }
        });
        
        // Trigger callbacks
        this.onArbitrageOpportunity(result);
        
        if (result.isLargeTrade) {
          this.onLargeTrade(result);
        }
      }
      
    } catch (error) {
      console.error('❌ Error analyzing transaction:', error);
    }
  }

  handleReconnect() {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      console.log(`🔄 Reconnecting... (Attempt ${this.reconnectAttempts}/${this.maxReconnectAttempts})`);
      
      setTimeout(() => {
        this.connect();
      }, this.reconnectDelay);
    } else {
      console.error('❌ Max reconnection attempts reached');
    }
  }

  disconnect() {
    if (this.ws) {
      this.ws.close();
    }
  }
}

// Example usage and configuration
async function startArbitrageStream() {
  // Configuration
  const config = {
    wsUrl: 'wss://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820',
    apiKey: 'QN_635965fc09414ea2becef14f68bcf7bf',
    
    // Callbacks for handling opportunities
    onArbitrageOpportunity: (opportunity) => {
      console.log('\n🚨 EXECUTING ARBITRAGE STRATEGY!');
      // Your arbitrage execution logic here
      executeArbitrage(opportunity);
    },
    
    onLargeTrade: (trade) => {
      console.log('\n🐋 LARGE TRADE DETECTED!');
      // Your large trade monitoring logic here
      monitorLargeTrade(trade);
    },
    
    onError: (error) => {
      console.error('🚨 Stream error:', error);
      // Your error handling logic here
    },
    
    onConnected: () => {
      console.log('🎉 Stream connected and monitoring for arbitrage opportunities!');
    }
  };
  
  // Start the stream
  const stream = new QuickNodeStream(config);
  await stream.connect();
  
  // Keep the process running
  process.on('SIGINT', () => {
    console.log('\n👋 Shutting down stream...');
    stream.disconnect();
    process.exit(0);
  });
}

// Arbitrage execution logic (implement your strategy here)
async function executeArbitrage(opportunity) {
  try {
    console.log('💰 Analyzing arbitrage opportunity...');
    
    for (const opp of opportunity.opportunities) {
      if (opp.estimatedProfit >= 5.0) { // $5 minimum profit
        console.log(`💱 Executing ${opp.type} for $${opp.estimatedProfit.toFixed(2)} profit`);
        
        // Your arbitrage execution logic here
        // Example: Call your Rust bot's execution function
        // await executeArbitrageInRust(opp);
      }
    }
    
  } catch (error) {
    console.error('❌ Arbitrage execution error:', error);
  }
}

// Large trade monitoring logic
async function monitorLargeTrade(trade) {
  try {
    console.log(`🔍 Monitoring large trade: $${trade.estimatedValueUSD.toFixed(2)}`);
    
    // Your large trade analysis logic here
    // Example: Look for follow-up opportunities
    
  } catch (error) {
    console.error('❌ Large trade monitoring error:', error);
  }
}

// Start the stream if this file is run directly
if (require.main === module) {
  console.log('🚀 Starting QuickNode Arbitrage Stream...');
  console.log('========================================');
  
  startArbitrageStream().catch(error => {
    console.error('❌ Failed to start stream:', error);
    process.exit(1);
  });
}

module.exports = {
  QuickNodeStream,
  startArbitrageStream
};
