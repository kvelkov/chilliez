// 🎛️ Dashboard Configuration Handler
// This module processes dashboard settings and converts them to bot parameters

class DashboardConfigHandler {
  constructor() {
    this.defaultConfig = {
      dexClients: ['orca'],
      profitThreshold: 5.0,
      speedRate: 3,
      tradeSize: 20,
      walletBalance: 38.0,
      riskLevel: 3,
      slippageTolerance: 0.2,
      executionMode: 'paper',
      aggressiveMode: false,
      autoExecute: false,
      maxConcurrentTrades: 3
    };
  }

  // Convert dashboard settings to bot configuration
  convertToBotConfig(dashboardConfig) {
    return {
      // DEX Configuration
      enabledDexes: {
        orca: dashboardConfig.dexClients.includes('orca'),
        jupiter: dashboardConfig.dexClients.includes('jupiter'),
        raydium: dashboardConfig.dexClients.includes('raydium'),
        lifinity: dashboardConfig.dexClients.includes('lifinity'),
        meteora: dashboardConfig.dexClients.includes('meteora'),
        aldrin: dashboardConfig.dexClients.includes('aldrin')
      },

      // Trading Parameters (converted from dashboard values)
      trading: {
        minProfitEur: this.convertProfitThreshold(dashboardConfig.profitThreshold),
        maxTradeSize: dashboardConfig.tradeSize / 100, // Convert percentage to decimal
        slippageTolerance: dashboardConfig.slippageTolerance / 100,
        walletBalanceSOL: dashboardConfig.walletBalance,
        maxConcurrentTrades: dashboardConfig.maxConcurrentTrades
      },

      // Speed/Rate Limits (converted from 1-5 scale)
      rateLimits: this.convertSpeedRate(dashboardConfig.speedRate),

      // Risk Management (converted from 1-5 scale)
      risk: this.convertRiskLevel(dashboardConfig.riskLevel),

      // Execution Settings
      execution: {
        mode: dashboardConfig.executionMode, // 'paper', 'live', 'simulation'
        aggressiveMode: dashboardConfig.aggressiveMode,
        autoExecute: dashboardConfig.autoExecute,
        executionEnabled: dashboardConfig.executionMode === 'live'
      },

      // Generated timestamps
      metadata: {
        configVersion: '1.0',
        generatedAt: new Date().toISOString(),
        generatedBy: 'dashboard'
      }
    };
  }

  // Convert profit threshold (1-10 scale to actual EUR values)
  convertProfitThreshold(dashboardValue) {
    // 1 = €1, 2 = €2, ..., 10 = €50 (exponential growth for higher values)
    if (dashboardValue <= 5) {
      return dashboardValue * 1; // €1-€5
    } else {
      return (dashboardValue - 5) * 9 + 5; // €5, €14, €23, €32, €41, €50
    }
  }

  // Convert speed rate (1-5 scale to actual rate limits)
  convertSpeedRate(speedValue) {
    const speedConfigs = {
      1: { // Slowest - Conservative
        messagesPerSecond: 2,
        burstLimit: 5,
        cooldownMs: 500,
        description: 'Very conservative, no risk of rate limiting'
      },
      2: { // Slow - Safe
        messagesPerSecond: 4,
        burstLimit: 8,
        cooldownMs: 300,
        description: 'Safe rate, good for stable connections'
      },
      3: { // Medium - Balanced
        messagesPerSecond: 6,
        burstLimit: 12,
        cooldownMs: 200,
        description: 'Balanced speed and safety'
      },
      4: { // Fast - Aggressive
        messagesPerSecond: 10,
        burstLimit: 20,
        cooldownMs: 100,
        description: 'Fast rate, monitor for throttling'
      },
      5: { // Maximum - Risk of kick
        messagesPerSecond: 15,
        burstLimit: 30,
        cooldownMs: 50,
        description: 'Maximum speed, high risk of rate limiting'
      }
    };

    return speedConfigs[speedValue] || speedConfigs[3];
  }

  // Convert risk level (1-5 scale to actual risk parameters)
  convertRiskLevel(riskValue) {
    const riskConfigs = {
      1: { // Very Safe
        maxSlippage: 0.001,
        confidenceThreshold: 0.9,
        maxTradeSize: 0.05, // Max 5% of wallet
        stopLoss: 0.02, // 2% stop loss
        description: 'Ultra-conservative, minimal risk'
      },
      2: { // Safe
        maxSlippage: 0.002,
        confidenceThreshold: 0.8,
        maxTradeSize: 0.10, // Max 10% of wallet
        stopLoss: 0.03, // 3% stop loss
        description: 'Conservative with some flexibility'
      },
      3: { // Medium
        maxSlippage: 0.005,
        confidenceThreshold: 0.7,
        maxTradeSize: 0.20, // Max 20% of wallet
        stopLoss: 0.05, // 5% stop loss
        description: 'Balanced risk/reward'
      },
      4: { // High
        maxSlippage: 0.01,
        confidenceThreshold: 0.6,
        maxTradeSize: 0.35, // Max 35% of wallet
        stopLoss: 0.08, // 8% stop loss
        description: 'Aggressive trading, higher potential'
      },
      5: { // Maximum
        maxSlippage: 0.02,
        confidenceThreshold: 0.5,
        maxTradeSize: 0.50, // Max 50% of wallet
        stopLoss: 0.10, // 10% stop loss
        description: 'Maximum risk, maximum potential'
      }
    };

    return riskConfigs[riskValue] || riskConfigs[3];
  }

  // Generate complete bot configuration file
  generateBotConfig(dashboardConfig) {
    const botConfig = this.convertToBotConfig(dashboardConfig);
    
    return {
      // Environment Configuration
      environment: {
        network: 'mainnet',
        rpcEndpoint: process.env.QUICKNODE_RPC_URL || 'https://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820',
        wsEndpoint: process.env.QUICKNODE_WS_URL || 'wss://little-convincing-borough.solana-mainnet.quiknode.pro/2c994d6eb71f3f58812833ce0783ae95f75e1820',
        apiKey: process.env.QUICKNODE_API_KEY || 'QN_635965fc09414ea2becef14f68bcf7bf'
      },

      // DEX Client Configuration
      dexClients: {
        enabled: botConfig.enabledDexes,
        endpoints: {
          orca: 'https://api.orca.so',
          jupiter: 'https://quote-api.jup.ag/v6',
          raydium: 'https://api.raydium.io/v2',
          lifinity: 'https://lifinity.io/api',
          meteora: 'https://app.meteora.ag/api',
          aldrin: 'https://api.aldrin.com'
        }
      },

      // Trading Configuration
      trading: botConfig.trading,
      
      // Rate Limiting
      rateLimits: botConfig.rateLimits,
      
      // Risk Management
      risk: botConfig.risk,
      
      // Execution Settings
      execution: botConfig.execution,
      
      // Logging Configuration
      logging: {
        level: dashboardConfig.executionMode === 'live' ? 'info' : 'debug',
        enableFileLogging: true,
        logDirectory: './logs',
        enableMetrics: true
      },

      // Metadata
      metadata: botConfig.metadata
    };
  }

  // Validate dashboard configuration
  validateConfig(dashboardConfig) {
    const errors = [];
    const warnings = [];

    // Check required fields
    if (!dashboardConfig.dexClients || dashboardConfig.dexClients.length === 0) {
      errors.push('At least one DEX must be selected');
    }

    if (dashboardConfig.walletBalance < 1) {
      errors.push('Wallet balance must be at least 1 SOL');
    }

    if (dashboardConfig.profitThreshold < 1 || dashboardConfig.profitThreshold > 10) {
      errors.push('Profit threshold must be between 1-10');
    }

    // Check warnings
    if (dashboardConfig.speedRate === 5) {
      warnings.push('Maximum speed rate may cause rate limiting');
    }

    if (dashboardConfig.riskLevel >= 4) {
      warnings.push('High risk level - monitor trades carefully');
    }

    if (dashboardConfig.executionMode === 'live' && !dashboardConfig.autoExecute) {
      warnings.push('Live mode without auto-execute requires manual confirmation');
    }

    if (dashboardConfig.tradeSize > 30) {
      warnings.push('Trade size >30% is considered high risk');
    }

    return { errors, warnings, isValid: errors.length === 0 };
  }

  // Load configuration from file
  loadConfigFromFile(filePath) {
    try {
      const fs = require('fs');
      const configData = fs.readFileSync(filePath, 'utf8');
      return JSON.parse(configData);
    } catch (error) {
      console.error('Error loading config file:', error.message);
      return this.defaultConfig;
    }
  }

  // Save configuration to file
  saveConfigToFile(config, filePath) {
    try {
      const fs = require('fs');
      const configData = JSON.stringify(config, null, 2);
      fs.writeFileSync(filePath, configData);
      return true;
    } catch (error) {
      console.error('Error saving config file:', error.message);
      return false;
    }
  }

  // Get configuration summary for display
  getConfigSummary(dashboardConfig) {
    const botConfig = this.convertToBotConfig(dashboardConfig);
    
    return {
      enabledDexes: Object.keys(botConfig.enabledDexes).filter(dex => botConfig.enabledDexes[dex]),
      profitThreshold: `€${botConfig.trading.minProfitEur}`,
      maxTradeSize: `${(botConfig.trading.maxTradeSize * 100).toFixed(0)}%`,
      riskLevel: botConfig.risk.description,
      speedLevel: botConfig.rateLimits.description,
      executionMode: botConfig.execution.mode.toUpperCase(),
      estimatedPerformance: this.estimatePerformance(botConfig)
    };
  }

  // Estimate performance based on configuration
  estimatePerformance(botConfig) {
    let score = 0;
    
    // DEX diversity bonus
    const enabledDexCount = Object.values(botConfig.enabledDexes).filter(Boolean).length;
    score += enabledDexCount * 10;
    
    // Speed bonus
    score += botConfig.rateLimits.messagesPerSecond * 2;
    
    // Risk adjustment
    if (botConfig.risk.maxTradeSize > 0.3) score += 20; // High risk bonus
    if (botConfig.risk.maxTradeSize < 0.1) score -= 10; // Conservative penalty
    
    // Mode adjustment
    if (botConfig.execution.mode === 'live') score += 30;
    if (botConfig.execution.aggressiveMode) score += 20;
    
    const performance = Math.min(100, Math.max(0, score));
    
    return {
      score: performance,
      level: performance > 80 ? 'High' : performance > 60 ? 'Medium' : 'Conservative',
      estimatedProfitPerHour: `€${(performance * 2).toFixed(0)}-${(performance * 5).toFixed(0)}`
    };
  }
}

module.exports = { DashboardConfigHandler };
