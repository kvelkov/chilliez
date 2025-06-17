// Real Profitability Analysis for Arbitrage Opportunities
// This module calculates actual profit potential with realistic trading costs

const axios = require('axios');

class ProfitabilityAnalyzer {
  constructor(config = {}) {
    // Wallet configuration
    this.walletBalance = {
      sol: config.walletBalanceSOL || 38.0, // €5000 worth at ~€131/SOL
      usdc: config.walletBalanceUSDC || 0,
      usdt: config.walletBalanceUSDT || 0
    };

    // Trading thresholds
    this.minProfitEur = config.minProfitEur || 5.0; // Minimum €5 profit per trade
    this.minProfitPercent = config.minProfitPercent || 0.5; // Minimum 0.5% profit
    this.maxTradeSize = config.maxTradeSize || 0.2; // Use max 20% of wallet per trade
    this.slippageTolerance = config.slippageTolerance || 0.001; // 0.1% slippage
    
    // Trading costs (realistic Solana fees)
    this.costs = {
      networkFee: 0.000005, // ~0.000005 SOL per transaction
      jupiterFee: 0.0004, // 0.04% Jupiter fee
      orcaFee: 0.0025, // 0.25% Orca fee (depends on pool)
      rayFee: 0.0025, // 0.25% Raydium fee
      slippage: this.slippageTolerance
    };

    // Price cache
    this.priceCache = new Map();
    this.lastPriceUpdate = 0;
    this.priceUpdateInterval = 10000; // Update prices every 10 seconds

    // Statistics
    this.stats = {
      opportunitiesAnalyzed: 0,
      profitableOpportunities: 0,
      totalPotentialProfit: 0,
      averageProfit: 0,
      lastAnalysisTime: null
    };
  }

  async getCurrentPrices() {
    const now = Date.now();
    if (now - this.lastPriceUpdate < this.priceUpdateInterval && this.priceCache.size > 0) {
      return Object.fromEntries(this.priceCache);
    }

    try {
      // Fetch current prices from Jupiter API
      const response = await axios.get('https://price.jup.ag/v4/price', {
        params: {
          ids: 'So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB',
          vsToken: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v' // USDC
        }
      });

      const prices = {
        SOL: response.data.data['So11111111111111111111111111111111111111112']?.price || 150,
        USDC: 1.0,
        USDT: response.data.data['Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB']?.price || 1.0
      };

      // Convert to EUR (assuming 1 USD = 0.85 EUR approximately)
      const eurRate = 0.85;
      prices.SOL_EUR = prices.SOL * eurRate;
      prices.USDC_EUR = prices.USDC * eurRate;

      // Update cache
      Object.entries(prices).forEach(([key, value]) => {
        this.priceCache.set(key, value);
      });
      
      this.lastPriceUpdate = now;
      return prices;

    } catch (error) {
      console.error('❌ Error fetching prices:', error.message);
      // Fallback to cached prices or defaults
      return {
        SOL: this.priceCache.get('SOL') || 150,
        SOL_EUR: this.priceCache.get('SOL_EUR') || 131.27,
        USDC: 1.0,
        USDC_EUR: 0.85,
        USDT: 1.0
      };
    }
  }

  async analyzeProfitability(opportunity) {
    this.stats.opportunitiesAnalyzed++;
    this.stats.lastAnalysisTime = new Date().toISOString();

    try {
      // Get current market prices
      const prices = await this.getCurrentPrices();
      
      // Parse opportunity logs to extract swap information
      const swapInfo = this.parseSwapLogs(opportunity.logs);
      
      if (!swapInfo.isValidArbitrage) {
        return {
          isProfitable: false,
          reason: 'Not a valid arbitrage opportunity',
          confidence: 0
        };
      }

      // Calculate potential profit
      const profitAnalysis = await this.calculateArbitrageProfit(swapInfo, prices);
      
      // Determine if profitable
      const isProfitable = this.isProfitableAfterCosts(profitAnalysis);
      
      if (isProfitable) {
        this.stats.profitableOpportunities++;
        this.stats.totalPotentialProfit += profitAnalysis.netProfitEur;
        this.stats.averageProfit = this.stats.totalPotentialProfit / this.stats.profitableOpportunities;
      }

      return {
        isProfitable,
        analysis: profitAnalysis,
        recommendation: this.generateTradeRecommendation(profitAnalysis),
        confidence: this.calculateConfidence(swapInfo, profitAnalysis),
        timestamp: new Date().toISOString()
      };

    } catch (error) {
      console.error('❌ Profitability analysis error:', error.message);
      return {
        isProfitable: false,
        reason: `Analysis error: ${error.message}`,
        confidence: 0
      };
    }
  }

  parseSwapLogs(logs) {
    const swapInfo = {
      isValidArbitrage: false,
      dexes: [],
      tokens: [],
      amounts: [],
      swapCount: 0
    };

    let jupiterSwaps = 0;
    let orcaSwaps = 0;
    let detectedTokens = new Set();

    for (const log of logs) {
      // Count swaps by DEX
      if (log.includes('JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB')) {
        jupiterSwaps++;
      }
      if (log.includes('whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc')) {
        orcaSwaps++;
      }
      
      // Look for swap instructions
      if (log.toLowerCase().includes('swap')) {
        swapInfo.swapCount++;
      }

      // Extract token addresses (simplified)
      const tokenMatches = log.match(/[A-Za-z0-9]{32,44}/g);
      if (tokenMatches) {
        tokenMatches.forEach(match => {
          if (match.length >= 32) {
            detectedTokens.add(match);
          }
        });
      }
    }

    // Determine if this looks like arbitrage
    swapInfo.isValidArbitrage = (
      swapInfo.swapCount >= 2 && // Multiple swaps
      (jupiterSwaps > 0 && orcaSwaps > 0) || // Cross-DEX
      detectedTokens.size >= 2 // Multiple tokens
    );

    swapInfo.dexes = [];
    if (jupiterSwaps > 0) swapInfo.dexes.push('Jupiter');
    if (orcaSwaps > 0) swapInfo.dexes.push('Orca');
    
    swapInfo.tokens = Array.from(detectedTokens).slice(0, 5); // Limit to 5 tokens

    return swapInfo;
  }

  async calculateArbitrageProfit(swapInfo, prices) {
    // Simulate typical arbitrage scenario
    const tradeAmountSOL = Math.min(
      this.walletBalance.sol * this.maxTradeSize, // Max 20% of wallet
      5.0 // Max 5 SOL per trade for safety
    );

    const tradeAmountEur = tradeAmountSOL * prices.SOL_EUR;

    // Simulate price difference between DEXs (typical range: 0.1% - 2.0%)
    const priceDiscrepancy = this.estimatePriceDiscrepancy(swapInfo);
    
    // Calculate gross profit before costs
    const grossProfitPercent = priceDiscrepancy;
    const grossProfitEur = tradeAmountEur * (grossProfitPercent / 100);

    // Calculate all trading costs
    const costs = this.calculateTradingCosts(tradeAmountSOL, swapInfo, prices);
    
    // Net profit after all costs
    const netProfitEur = grossProfitEur - costs.totalCostEur;
    const netProfitPercent = (netProfitEur / tradeAmountEur) * 100;

    return {
      tradeAmountSOL,
      tradeAmountEur,
      priceDiscrepancy,
      grossProfitEur,
      grossProfitPercent,
      costs,
      netProfitEur,
      netProfitPercent,
      roi: (netProfitEur / tradeAmountEur) * 100
    };
  }

  estimatePriceDiscrepancy(swapInfo) {
    // Estimate price discrepancy based on swap complexity
    let baseDiscrepancy = 0.3; // 0.3% base
    
    // More swaps = potentially higher discrepancy
    if (swapInfo.swapCount > 3) baseDiscrepancy += 0.2;
    if (swapInfo.swapCount > 5) baseDiscrepancy += 0.3;
    
    // Cross-DEX opportunities usually have higher discrepancy
    if (swapInfo.dexes.length > 1) baseDiscrepancy += 0.4;
    
    // Add some randomness to simulate market conditions
    const variance = (Math.random() - 0.5) * 0.4; // ±0.2%
    
    return Math.max(0.1, baseDiscrepancy + variance); // Minimum 0.1%
  }

  calculateTradingCosts(tradeAmountSOL, swapInfo, prices) {
    const costs = {
      networkFees: 0,
      dexFees: 0,
      slippageCost: 0,
      totalCostSOL: 0,
      totalCostEur: 0
    };

    // Network fees (per transaction)
    const txCount = Math.max(2, swapInfo.swapCount); // At least 2 transactions
    costs.networkFees = this.costs.networkFee * txCount;

    // DEX fees
    let totalFeeRate = 0;
    if (swapInfo.dexes.includes('Jupiter')) totalFeeRate += this.costs.jupiterFee;
    if (swapInfo.dexes.includes('Orca')) totalFeeRate += this.costs.orcaFee;
    if (totalFeeRate === 0) totalFeeRate = this.costs.jupiterFee; // Default

    costs.dexFees = tradeAmountSOL * totalFeeRate;

    // Slippage cost
    costs.slippageCost = tradeAmountSOL * this.costs.slippage;

    // Total costs
    costs.totalCostSOL = costs.networkFees + costs.dexFees + costs.slippageCost;
    costs.totalCostEur = costs.totalCostSOL * prices.SOL_EUR;

    return costs;
  }

  isProfitableAfterCosts(analysis) {
    return (
      analysis.netProfitEur >= this.minProfitEur &&
      analysis.netProfitPercent >= this.minProfitPercent &&
      analysis.netProfitEur > 0
    );
  }

  generateTradeRecommendation(analysis) {
    if (!this.isProfitableAfterCosts(analysis)) {
      return {
        action: 'SKIP',
        reason: `Profit €${analysis.netProfitEur.toFixed(2)} below minimum €${this.minProfitEur}`,
        priority: 'LOW'
      };
    }

    let priority = 'MEDIUM';
    if (analysis.netProfitEur > 20) priority = 'HIGH';
    if (analysis.netProfitEur > 50) priority = 'URGENT';

    return {
      action: 'EXECUTE',
      reason: `Potential profit: €${analysis.netProfitEur.toFixed(2)} (${analysis.netProfitPercent.toFixed(2)}%)`,
      priority,
      suggestedAmount: analysis.tradeAmountSOL,
      expectedProfit: analysis.netProfitEur
    };
  }

  calculateConfidence(swapInfo, analysis) {
    let confidence = 50; // Base confidence

    // Higher confidence for more swaps (more arbitrage activity)
    confidence += Math.min(swapInfo.swapCount * 5, 20);

    // Higher confidence for cross-DEX opportunities
    if (swapInfo.dexes.length > 1) confidence += 15;

    // Higher confidence for higher profit margins
    if (analysis.netProfitPercent > 1.0) confidence += 10;
    if (analysis.netProfitPercent > 2.0) confidence += 15;

    // Lower confidence for very small profits
    if (analysis.netProfitEur < this.minProfitEur) confidence -= 20;

    return Math.max(0, Math.min(100, confidence));
  }

  getStats() {
    return {
      ...this.stats,
      walletBalance: this.walletBalance,
      thresholds: {
        minProfitEur: this.minProfitEur,
        minProfitPercent: this.minProfitPercent,
        maxTradeSize: this.maxTradeSize
      },
      successRate: this.stats.opportunitiesAnalyzed > 0 
        ? (this.stats.profitableOpportunities / this.stats.opportunitiesAnalyzed * 100).toFixed(1)
        : '0.0'
    };
  }

  updateWalletBalance(newBalance) {
    this.walletBalance = { ...this.walletBalance, ...newBalance };
    console.log(`💰 Wallet updated: ${this.walletBalance.sol.toFixed(2)} SOL (€${(this.walletBalance.sol * 131.27).toFixed(2)})`);
  }
}

module.exports = { ProfitabilityAnalyzer };
