// QuickNode Function: Solana DEX Arbitrage Opportunity Detector
// This function processes Solana transaction data to detect arbitrage opportunities
// Compatible with QuickNode Functions Node.js 20 runtime

function main(params) {
  try {
    // Extract transaction data from params
    const transaction = params.transaction || params;
    
    // Initialize arbitrage detector
    const detector = new ArbitrageDetector();
    
    // Process the transaction for arbitrage opportunities
    const result = detector.analyzeTransaction(transaction);
    
    // Return results (only if opportunities found)
    if (result && result.hasOpportunities) {
      return {
        success: true,
        timestamp: Date.now(),
        opportunities: result.opportunities,
        transaction: {
          signature: result.signature,
          slot: result.slot,
          dexSwaps: result.dexSwaps,
          estimatedValueUSD: result.estimatedValueUSD,
          isLargeTrade: result.isLargeTrade
        },
        metadata: {
          totalOpportunities: result.opportunities.length,
          maxProfit: Math.max(...result.opportunities.map(o => o.estimatedProfit || 0)),
          dexesInvolved: [...new Set(result.dexSwaps.map(s => s.dex))]
        }
      };
    }
    
    // Return null if no opportunities (saves bandwidth)
    return null;
    
  } catch (error) {
    return {
      success: false,
      error: error.message,
      timestamp: Date.now()
    };
  }
}

class ArbitrageDetector {
  constructor() {
    // DEX Program IDs
    this.dexPrograms = {
      'whirlpool': '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca
      'raydium': '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8',   // Raydium
      'jupiter': 'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',    // Jupiter
      'meteora': 'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',    // Meteora
      'lifinity': '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c',  // Lifinity
      'phoenix': 'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY',   // Phoenix
      'openbook': 'EhpbDwV1vhwtoSZtZpGwVo3F8wq8AQy5i3H5N8rYZMEZ'   // OpenBook
    };
    
    // High-value tokens for arbitrage
    this.arbitrageTokens = {
      'SOL': 'So11111111111111111111111111111111111111112',
      'USDC': 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v',
      'USDT': 'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB',
      'mSOL': 'mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So',
      'stSOL': '7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj',
      'bSOL': 'bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1',
      'jitoSOL': 'J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn'
    };
    
    // Detection thresholds
    this.thresholds = {
      minSwapValueUSD: 100,        // $100 minimum
      minArbitrageProfitUSD: 5,    // $5 minimum profit
      largeTradeThreshold: 10000,  // $10,000 large trade
      maxPriceImpact: 0.05         // 5% max price impact
    };
    
    // Token price estimates (in production, use real price feeds)
    this.tokenPrices = {
      [this.arbitrageTokens.SOL]: 150,
      [this.arbitrageTokens.USDC]: 1,
      [this.arbitrageTokens.USDT]: 1,
      [this.arbitrageTokens.mSOL]: 165,
      [this.arbitrageTokens.stSOL]: 160,
      [this.arbitrageTokens.bSOL]: 155,
      [this.arbitrageTokens.jitoSOL]: 170
    };
  }
  
  analyzeTransaction(tx) {
    if (!this.isValidTransaction(tx)) {
      return null;
    }
    
    const signature = tx.transaction?.signatures?.[0] || 'unknown';
    const slot = tx.slot || null;
    
    // Extract DEX swaps
    const dexSwaps = this.extractDexSwaps(tx);
    
    // Calculate transaction value
    const estimatedValueUSD = this.calculateTransactionValue(tx, dexSwaps);
    
    // Detect arbitrage opportunities
    const opportunities = this.detectArbitrageOpportunities(tx, dexSwaps);
    
    const result = {
      signature,
      slot,
      dexSwaps,
      opportunities,
      estimatedValueUSD,
      isLargeTrade: estimatedValueUSD >= this.thresholds.largeTradeThreshold,
      hasOpportunities: opportunities.length > 0
    };
    
    return result;
  }
  
  isValidTransaction(tx) {
    return tx && 
           tx.transaction && 
           tx.meta && 
           !tx.meta.err &&
           tx.transaction.message &&
           tx.transaction.message.instructions;
  }
  
  extractDexSwaps(tx) {
    const swaps = [];
    const instructions = tx.transaction.message.instructions || [];
    const innerInstructions = tx.meta.innerInstructions || [];
    
    // Process main instructions
    instructions.forEach((ix, index) => {
      const swap = this.parseInstruction(ix, tx, index, false);
      if (swap) {
        swaps.push(swap);
      }
    });
    
    // Process inner instructions
    innerInstructions.forEach(innerGroup => {
      if (innerGroup.instructions) {
        innerGroup.instructions.forEach((ix, index) => {
          const swap = this.parseInstruction(ix, tx, `${innerGroup.index}_${index}`, true);
          if (swap) {
            swaps.push(swap);
          }
        });
      }
    });
    
    return swaps;
  }
  
  parseInstruction(instruction, tx, index, isInner) {
    const programId = this.getProgramId(instruction, tx);
    const dexName = this.identifyDex(programId);
    
    if (!dexName) {
      return null;
    }
    
    // Extract swap data from token balance changes
    const swapData = this.extractSwapFromBalances(tx);
    
    if (swapData) {
      return {
        index: index.toString(),
        dex: dexName,
        programId,
        isInner,
        ...swapData
      };
    }
    
    return null;
  }
  
  getProgramId(instruction, tx) {
    if (instruction.programId) {
      return instruction.programId;
    }
    
    if (instruction.programIdIndex !== undefined) {
      const accountKeys = tx.transaction.message.accountKeys || [];
      const account = accountKeys[instruction.programIdIndex];
      return account?.pubkey || account;
    }
    
    return null;
  }
  
  identifyDex(programId) {
    for (const [name, id] of Object.entries(this.dexPrograms)) {
      if (id === programId) {
        return name.charAt(0).toUpperCase() + name.slice(1);
      }
    }
    return null;
  }
  
  extractSwapFromBalances(tx) {
    const preBalances = tx.meta.preTokenBalances || [];
    const postBalances = tx.meta.postTokenBalances || [];
    
    const changes = this.calculateBalanceChanges(preBalances, postBalances);
    
    // Look for swap pattern: one token decreases, another increases
    const decreases = changes.filter(c => c.change < -0.001);
    const increases = changes.filter(c => c.change > 0.001);
    
    if (decreases.length >= 1 && increases.length >= 1) {
      const tokenIn = decreases[0];
      const tokenOut = increases[0];
      
      return {
        tokenIn: tokenIn.mint,
        tokenOut: tokenOut.mint,
        amountIn: Math.abs(tokenIn.change),
        amountOut: tokenOut.change,
        amountInUSD: Math.abs(tokenIn.changeUSD || 0),
        amountOutUSD: tokenOut.changeUSD || 0
      };
    }
    
    return null;
  }
  
  calculateBalanceChanges(preBalances, postBalances) {
    const changes = [];
    
    postBalances.forEach(post => {
      const pre = preBalances.find(p => 
        p.accountIndex === post.accountIndex && 
        p.mint === post.mint
      );
      
      const preAmount = pre?.uiTokenAmount?.uiAmount || 0;
      const postAmount = post.uiTokenAmount?.uiAmount || 0;
      const change = postAmount - preAmount;
      
      if (Math.abs(change) > 0.001) {
        const price = this.tokenPrices[post.mint] || 0;
        changes.push({
          mint: post.mint,
          owner: post.owner,
          change,
          changeUSD: change * price,
          decimals: post.uiTokenAmount.decimals
        });
      }
    });
    
    return changes;
  }
  
  calculateTransactionValue(tx, dexSwaps) {
    let totalValue = 0;
    
    // Sum swap values
    dexSwaps.forEach(swap => {
      totalValue += swap.amountInUSD || 0;
    });
    
    // If no swap data, estimate from balance changes
    if (totalValue === 0) {
      const changes = this.calculateBalanceChanges(
        tx.meta.preTokenBalances || [],
        tx.meta.postTokenBalances || []
      );
      
      changes.forEach(change => {
        totalValue += Math.abs(change.changeUSD || 0);
      });
    }
    
    return totalValue;
  }
  
  detectArbitrageOpportunities(tx, dexSwaps) {
    const opportunities = [];
    
    // Cross-DEX arbitrage
    if (dexSwaps.length >= 2) {
      const dexes = [...new Set(dexSwaps.map(s => s.dex))];
      if (dexes.length >= 2) {
        const profit = this.estimateCrossDexProfit(dexSwaps);
        if (profit >= this.thresholds.minArbitrageProfitUSD) {
          opportunities.push({
            type: 'cross_dex_arbitrage',
            dexes,
            estimatedProfit: profit,
            confidence: 'high',
            swaps: dexSwaps.length
          });
        }
      }
    }
    
    // Large trade opportunity
    const totalValue = dexSwaps.reduce((sum, swap) => sum + (swap.amountInUSD || 0), 0);
    if (totalValue >= this.thresholds.largeTradeThreshold) {
      opportunities.push({
        type: 'large_trade',
        tradeValue: totalValue,
        estimatedProfit: totalValue * 0.001, // 0.1% potential profit
        confidence: 'medium'
      });
    }
    
    // Price impact opportunity
    const priceImpact = this.calculatePriceImpact(tx);
    if (priceImpact > this.thresholds.maxPriceImpact) {
      opportunities.push({
        type: 'price_impact',
        priceImpact,
        estimatedProfit: totalValue * 0.002, // 0.2% potential profit
        confidence: 'medium'
      });
    }
    
    // Token pair arbitrage
    const tokenPairs = this.identifyTokenPairs(dexSwaps);
    tokenPairs.forEach(pair => {
      if (pair.priceDifference > 0.01) { // 1% price difference
        opportunities.push({
          type: 'token_pair_arbitrage',
          tokenA: pair.tokenA,
          tokenB: pair.tokenB,
          priceDifference: pair.priceDifference,
          estimatedProfit: pair.estimatedProfit,
          confidence: 'high'
        });
      }
    });
    
    return opportunities.filter(opp => 
      opp.estimatedProfit >= this.thresholds.minArbitrageProfitUSD
    );
  }
  
  estimateCrossDexProfit(swaps) {
    const totalVolume = swaps.reduce((sum, swap) => sum + (swap.amountInUSD || 0), 0);
    // Conservative estimate: 0.2% profit on cross-DEX arbitrage
    return totalVolume * 0.002;
  }
  
  calculatePriceImpact(tx) {
    const changes = this.calculateBalanceChanges(
      tx.meta.preTokenBalances || [],
      tx.meta.postTokenBalances || []
    );
    
    let maxImpact = 0;
    changes.forEach(change => {
      if (change.change < 0) { // Token being sold
        const impact = Math.abs(change.change) / (Math.abs(change.change) + 1000); // Simplified
        maxImpact = Math.max(maxImpact, impact);
      }
    });
    
    return maxImpact;
  }
  
  identifyTokenPairs(swaps) {
    const pairs = [];
    
    // Simple pair analysis - compare swaps of same token pairs
    for (let i = 0; i < swaps.length; i++) {
      for (let j = i + 1; j < swaps.length; j++) {
        const swap1 = swaps[i];
        const swap2 = swaps[j];
        
        // Check if swaps involve same token pair
        if ((swap1.tokenIn === swap2.tokenIn && swap1.tokenOut === swap2.tokenOut) ||
            (swap1.tokenIn === swap2.tokenOut && swap1.tokenOut === swap2.tokenIn)) {
          
          const rate1 = (swap1.amountOutUSD || 0) / (swap1.amountInUSD || 1);
          const rate2 = (swap2.amountOutUSD || 0) / (swap2.amountInUSD || 1);
          const priceDifference = Math.abs(rate1 - rate2) / Math.max(rate1, rate2);
          
          if (priceDifference > 0.005) { // 0.5% minimum difference
            pairs.push({
              tokenA: swap1.tokenIn,
              tokenB: swap1.tokenOut,
              priceDifference,
              estimatedProfit: Math.min(swap1.amountInUSD || 0, swap2.amountInUSD || 0) * priceDifference,
              swap1: swap1.dex,
              swap2: swap2.dex
            });
          }
        }
      }
    }
    
    return pairs;
  }
}

// Export for QuickNode Functions (if needed)
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { main, ArbitrageDetector };
}
