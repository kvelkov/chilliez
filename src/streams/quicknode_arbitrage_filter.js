/**
 * Solana DEX Arbitrage Opportunity Filter
 * ----------------------------------------
 * Tracks DEX swaps, liquidity changes, and arbitrage opportunities across major Solana DEXs.
 * 
 * FEATURES:
 * 1. DEX Swap Detection
 *    - Orca Whirlpool swaps
 *    - Raydium AMM swaps
 *    - Jupiter aggregated swaps
 *    - Meteora and Lifinity swaps
 *    - Cross-DEX arbitrage detection
 * 
 * 2. Liquidity Pool Monitoring
 *    - Pool state changes
 *    - Large liquidity additions/removals
 *    - Price impact calculations
 * 
 * 3. Arbitrage Opportunity Detection
 *    - Price discrepancies between DEXs
 *    - Token pair monitoring
 *    - Profit threshold filtering
 * 
 * 4. Address Filtering
 *    - MEV bot addresses
 *    - Large trader wallets
 *    - Your bot's wallet monitoring
 * 
 * USAGE:
 * 1. Configure thresholds
 *    - CONFIG.THRESHOLDS.MIN_SWAP_VALUE (default: 100 USDC)
 *    - CONFIG.THRESHOLDS.MIN_ARBITRAGE_PROFIT (default: 5 USDC)
 * 
 * 2. Set up address monitoring
 *    https://www.quicknode.com/docs/key-value-store/rest-api/lists/create-list 'ARB_ADDRESSES' ['your_wallet', 'competitor_wallets']
 * 
 * 3. Configure token pairs for arbitrage
 *    https://www.quicknode.com/docs/key-value-store/rest-api/lists/create-list 'ARB_TOKENS' ['SOL', 'USDC', 'USDT', 'mSOL']
 */

const CONFIG = {
    LAMPORTS_PER_SOL: 1000000000,
    USDC_DECIMALS: 6,
    
    // DEX Program IDs
    PROGRAMS: {
        SYSTEM: '11111111111111111111111111111111',
        SPL_TOKEN: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
        ORCA_WHIRLPOOL: '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM',
        RAYDIUM_AMM: '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8',
        JUPITER: 'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',
        METEORA: 'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',
        LIFINITY: '2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c',
        PHOENIX: 'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY',
        OPENBOOK: 'EhpbDwV1vhwtoSZtZpGwVo3F8wq8AQy5i3H5N8rYZMEZ'
    },
    
    // High-value tokens for arbitrage
    TOKENS: {
        SOL: 'So11111111111111111111111111111111111111112',
        USDC: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v',
        USDT: 'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB',
        MSOL: 'mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So',
        STSOL: '7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj',
        BSOL: 'bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1',
        JITOSOL: 'J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn',
        BONK: 'DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263'
    },
    
    // Detection thresholds
    THRESHOLDS: {
        MIN_SWAP_VALUE_USD: 100,        // $100 minimum swap value
        MIN_ARBITRAGE_PROFIT_USD: 5,    // $5 minimum arbitrage profit
        MIN_LIQUIDITY_CHANGE: 1000,     // $1000 minimum liquidity change
        MAX_PRICE_IMPACT: 0.05,         // 5% maximum price impact
        MIN_SOL_TRANSFER: 0.1,          // 0.1 SOL minimum transfer
        LARGE_TRADE_THRESHOLD: 10000    // $10,000 for large trade alerts
    },
    
    // QuickNode lists for monitoring
    LISTS: {
        ARBITRAGE_ADDRESSES: 'ARB_ADDRESSES',
        MONITORED_TOKENS: 'ARB_TOKENS',
        MEV_BOTS: 'MEV_BOTS',
        WHALE_WALLETS: 'WHALE_WALLETS'
    }
};

class ArbitrageFilter {
    constructor(watchedAddresses = [], monitoredTokens = [], mevBots = [], whaleWallets = []) {
        this.watchedAddresses = new Set(watchedAddresses);
        this.monitoredTokens = new Set(monitoredTokens);
        this.mevBots = new Set(mevBots);
        this.whaleWallets = new Set(whaleWallets);
        this.dexPrograms = new Set(Object.values(CONFIG.PROGRAMS));
        this.arbitrageTokens = new Set(Object.values(CONFIG.TOKENS));
    }

    static async init() {
        try {
            const [arbAddresses, monitoredTokens, mevBots, whaleWallets] = await Promise.all([
                qnLib.qnGetList(CONFIG.LISTS.ARBITRAGE_ADDRESSES).catch(() => []),
                qnLib.qnGetList(CONFIG.LISTS.MONITORED_TOKENS).catch(() => Object.values(CONFIG.TOKENS)),
                qnLib.qnGetList(CONFIG.LISTS.MEV_BOTS).catch(() => []),
                qnLib.qnGetList(CONFIG.LISTS.WHALE_WALLETS).catch(() => [])
            ]);
            
            return new ArbitrageFilter(
                arbAddresses || [],
                monitoredTokens || Object.values(CONFIG.TOKENS),
                mevBots || [],
                whaleWallets || []
            );
        } catch (error) {
            console.error('Filter initialization error:', error);
            return new ArbitrageFilter();
        }
    }

    processTransaction(tx) {
        if (!this.isValidTransaction(tx)) return null;

        const analysis = {
            signature: tx.transaction.signatures[0],
            slot: tx.slot || null,
            blockTime: tx.blockTime || null,
            dexSwaps: this.extractDexSwaps(tx),
            tokenTransfers: this.extractTokenTransfers(tx),
            liquidityChanges: this.extractLiquidityChanges(tx),
            arbitrageOpportunities: [],
            addressFlags: this.analyzeAddresses(tx),
            priceImpact: this.calculatePriceImpact(tx),
            isLargeTrade: false,
            estimatedValueUSD: 0
        };

        // Calculate estimated USD value
        analysis.estimatedValueUSD = this.estimateTransactionValueUSD(analysis);
        analysis.isLargeTrade = analysis.estimatedValueUSD >= CONFIG.THRESHOLDS.LARGE_TRADE_THRESHOLD;

        // Detect potential arbitrage opportunities
        analysis.arbitrageOpportunities = this.detectArbitrageOpportunities(analysis);

        // Filter based on thresholds and relevance
        if (this.isRelevantTransaction(analysis)) {
            return analysis;
        }

        return null;
    }

    isValidTransaction(tx) {
        return tx && tx.transaction && tx.meta && !tx.meta.err;
    }

    extractDexSwaps(tx) {
        const swaps = [];
        const instructions = [
            ...(tx.transaction.message.instructions || []),
            ...(tx.meta.innerInstructions || []).flatMap(inner => inner?.instructions || [])
        ];

        for (const ix of instructions) {
            if (this.dexPrograms.has(ix.programId)) {
                const swap = this.parseSwapInstruction(ix, tx);
                if (swap) {
                    swaps.push({
                        dex: this.identifyDex(ix.programId),
                        programId: ix.programId,
                        ...swap
                    });
                }
            }
        }

        return swaps;
    }

    parseSwapInstruction(instruction, tx) {
        // Parse different DEX instruction formats
        if (instruction.parsed) {
            // Handle parsed instructions
            if (instruction.parsed.type === 'swap' || 
                instruction.parsed.type === 'swapV2' ||
                instruction.parsed.type === 'exactInputOutput') {
                return {
                    type: instruction.parsed.type,
                    tokenIn: instruction.parsed.info?.tokenIn || instruction.parsed.info?.inputMint,
                    tokenOut: instruction.parsed.info?.tokenOut || instruction.parsed.info?.outputMint,
                    amountIn: instruction.parsed.info?.amountIn,
                    amountOut: instruction.parsed.info?.amountOut,
                    slippage: instruction.parsed.info?.slippage
                };
            }
        }

        // For non-parsed instructions, extract from token balance changes
        return this.extractSwapFromBalanceChanges(tx, instruction);
    }

    extractSwapFromBalanceChanges(tx, instruction) {
        const preTokenBalances = tx.meta.preTokenBalances || [];
        const postTokenBalances = tx.meta.postTokenBalances || [];
        
        const balanceChanges = this.calculateTokenBalanceChanges(preTokenBalances, postTokenBalances);
        
        // Look for two significant balance changes (in and out)
        const significantChanges = balanceChanges.filter(change => 
            Math.abs(change.change) > CONFIG.THRESHOLDS.MIN_SOL_TRANSFER
        );
        
        if (significantChanges.length >= 2) {
            const tokenIn = significantChanges.find(c => c.change < 0); // Decrease
            const tokenOut = significantChanges.find(c => c.change > 0); // Increase
            
            if (tokenIn && tokenOut) {
                return {
                    type: 'swap_detected',
                    tokenIn: tokenIn.mint,
                    tokenOut: tokenOut.mint,
                    amountIn: Math.abs(tokenIn.change),
                    amountOut: tokenOut.change
                };
            }
        }
        
        return null;
    }

    extractTokenTransfers(tx) {
        const transfers = [];
        const instructions = [
            ...(tx.transaction.message.instructions || []),
            ...(tx.meta.innerInstructions || []).flatMap(inner => inner?.instructions || [])
        ];

        for (const ix of instructions) {
            if (ix.programId === CONFIG.PROGRAMS.SPL_TOKEN && ix.parsed?.type === 'transfer') {
                const amount = parseFloat(ix.parsed.info.amount);
                
                transfers.push({
                    source: ix.parsed.info.source,
                    destination: ix.parsed.info.destination,
                    amount,
                    mint: ix.parsed.info.mint,
                    isSignificant: amount >= CONFIG.THRESHOLDS.MIN_SOL_TRANSFER
                });
            }
        }

        return transfers;
    }

    extractLiquidityChanges(tx) {
        const changes = [];
        const preTokenBalances = tx.meta.preTokenBalances || [];
        const postTokenBalances = tx.meta.postTokenBalances || [];

        const balanceChanges = this.calculateTokenBalanceChanges(preTokenBalances, postTokenBalances);
        
        // Look for large liquidity changes in known tokens
        for (const change of balanceChanges) {
            if (this.arbitrageTokens.has(change.mint) && 
                Math.abs(change.changeUSD || 0) >= CONFIG.THRESHOLDS.MIN_LIQUIDITY_CHANGE) {
                changes.push({
                    mint: change.mint,
                    owner: change.owner,
                    change: change.change,
                    changeUSD: change.changeUSD,
                    type: change.change > 0 ? 'addition' : 'removal'
                });
            }
        }

        return changes;
    }

    calculateTokenBalanceChanges(preBalances, postBalances) {
        const changes = [];
        
        for (const post of postBalances) {
            const pre = preBalances.find(p => 
                p.accountIndex === post.accountIndex && p.mint === post.mint
            );
            
            const preAmount = pre?.uiTokenAmount?.uiAmount || 0;
            const postAmount = post.uiTokenAmount?.uiAmount || 0;
            const change = postAmount - preAmount;
            
            if (Math.abs(change) > 0.001) { // Ignore dust
                changes.push({
                    mint: post.mint,
                    owner: post.owner,
                    pre: preAmount,
                    post: postAmount,
                    change,
                    changeUSD: this.estimateUSDValue(change, post.mint),
                    decimals: post.uiTokenAmount.decimals
                });
            }
        }
        
        return changes;
    }

    analyzeAddresses(tx) {
        const flags = {
            isWatched: false,
            isMevBot: false,
            isWhale: false,
            hasArbitrageToken: false
        };

        const txAddresses = new Set(
            tx.transaction.message.accountKeys.map(key => key.pubkey || key)
        );

        // Check for watched addresses
        flags.isWatched = [...this.watchedAddresses].some(addr => txAddresses.has(addr));
        
        // Check for MEV bots
        flags.isMevBot = [...this.mevBots].some(addr => txAddresses.has(addr));
        
        // Check for whale wallets
        flags.isWhale = [...this.whaleWallets].some(addr => txAddresses.has(addr));
        
        // Check for arbitrage tokens
        flags.hasArbitrageToken = [...this.arbitrageTokens].some(token => txAddresses.has(token));

        return flags;
    }

    calculatePriceImpact(tx) {
        // Simplified price impact calculation
        // In production, you'd want more sophisticated price impact detection
        const balanceChanges = this.calculateTokenBalanceChanges(
            tx.meta.preTokenBalances || [],
            tx.meta.postTokenBalances || []
        );
        
        let maxImpact = 0;
        for (const change of balanceChanges) {
            if (change.pre > 0) {
                const impact = Math.abs(change.change) / change.pre;
                maxImpact = Math.max(maxImpact, impact);
            }
        }
        
        return maxImpact;
    }

    detectArbitrageOpportunities(analysis) {
        const opportunities = [];
        
        // Cross-DEX arbitrage detection
        if (analysis.dexSwaps.length >= 2) {
            const dexes = [...new Set(analysis.dexSwaps.map(swap => swap.dex))];
            if (dexes.length >= 2) {
                opportunities.push({
                    type: 'cross_dex_arbitrage',
                    dexes: dexes,
                    confidence: 'high',
                    estimatedProfit: this.estimateArbitrageProfit(analysis.dexSwaps)
                });
            }
        }
        
        // Large price impact opportunities
        if (analysis.priceImpact > CONFIG.THRESHOLDS.MAX_PRICE_IMPACT) {
            opportunities.push({
                type: 'price_impact_arbitrage',
                priceImpact: analysis.priceImpact,
                confidence: 'medium'
            });
        }
        
        // MEV sandwich opportunity
        if (analysis.addressFlags.isMevBot && analysis.isLargeTrade) {
            opportunities.push({
                type: 'mev_sandwich',
                confidence: 'high',
                tradeValue: analysis.estimatedValueUSD
            });
        }
        
        return opportunities.filter(opp => 
            !opp.estimatedProfit || opp.estimatedProfit >= CONFIG.THRESHOLDS.MIN_ARBITRAGE_PROFIT_USD
        );
    }

    estimateArbitrageProfit(swaps) {
        // Simplified profit estimation
        // In production, you'd calculate based on actual price differences
        let totalVolume = 0;
        for (const swap of swaps) {
            totalVolume += this.estimateUSDValue(swap.amountIn || 0, swap.tokenIn);
        }
        
        // Estimate 0.1-0.5% profit potential
        return totalVolume * 0.002;
    }

    estimateUSDValue(amount, mint) {
        // Simplified USD estimation - in production, use real price feeds
        const priceEstimates = {
            [CONFIG.TOKENS.SOL]: 150,     // $150 per SOL
            [CONFIG.TOKENS.USDC]: 1,      // $1 per USDC
            [CONFIG.TOKENS.USDT]: 1,      // $1 per USDT
            [CONFIG.TOKENS.MSOL]: 165,    // $165 per mSOL
            [CONFIG.TOKENS.STSOL]: 160,   // $160 per stSOL
            [CONFIG.TOKENS.BSOL]: 155,    // $155 per bSOL
            [CONFIG.TOKENS.JITOSOL]: 170, // $170 per jitoSOL
        };
        
        const price = priceEstimates[mint] || 0;
        return amount * price;
    }

    estimateTransactionValueUSD(analysis) {
        let totalValue = 0;
        
        // Sum up swap values
        for (const swap of analysis.dexSwaps) {
            totalValue += this.estimateUSDValue(swap.amountIn || 0, swap.tokenIn);
        }
        
        // Sum up significant transfers
        for (const transfer of analysis.tokenTransfers) {
            if (transfer.isSignificant) {
                totalValue += this.estimateUSDValue(transfer.amount, transfer.mint);
            }
        }
        
        return totalValue;
    }

    isRelevantTransaction(analysis) {
        // Filter criteria for arbitrage relevance
        return (
            // Has DEX swaps
            analysis.dexSwaps.length > 0 ||
            
            // Has arbitrage opportunities
            analysis.arbitrageOpportunities.length > 0 ||
            
            // Is a large trade
            analysis.isLargeTrade ||
            
            // Involves watched addresses
            analysis.addressFlags.isWatched ||
            
            // MEV bot activity
            analysis.addressFlags.isMevBot ||
            
            // Significant liquidity changes
            analysis.liquidityChanges.length > 0 ||
            
            // High price impact
            analysis.priceImpact > CONFIG.THRESHOLDS.MAX_PRICE_IMPACT ||
            
            // Minimum value threshold
            analysis.estimatedValueUSD >= CONFIG.THRESHOLDS.MIN_SWAP_VALUE_USD
        );
    }

    identifyDex(programId) {
        const dexMap = {
            [CONFIG.PROGRAMS.ORCA_WHIRLPOOL]: 'Orca',
            [CONFIG.PROGRAMS.RAYDIUM_AMM]: 'Raydium',
            [CONFIG.PROGRAMS.JUPITER]: 'Jupiter',
            [CONFIG.PROGRAMS.METEORA]: 'Meteora',
            [CONFIG.PROGRAMS.LIFINITY]: 'Lifinity',
            [CONFIG.PROGRAMS.PHOENIX]: 'Phoenix',
            [CONFIG.PROGRAMS.OPENBOOK]: 'OpenBook'
        };
        
        return dexMap[programId] || 'Unknown';
    }
}

/**
 * Main entry point for QuickNode Streams filter
 */
async function main(payload) {
    const { data, metadata } = payload;
    if (!Array.isArray(data)) return payload;

    try {
        const filter = await ArbitrageFilter.init();
        
        const results = data
            .filter(block => block?.transactions?.length > 0)
            .map(block => {
                const relevantTransactions = block.transactions
                    .map(tx => filter.processTransaction(tx))
                    .filter(Boolean);
                
                if (relevantTransactions.length > 0) {
                    return {
                        slot: block.parentSlot + 1,
                        blockTime: block.blockTime,
                        transactionCount: block.transactions.length,
                        arbitrageTransactions: relevantTransactions,
                        summary: {
                            totalArbitrageOpportunities: relevantTransactions.reduce(
                                (sum, tx) => sum + tx.arbitrageOpportunities.length, 0
                            ),
                            totalVolumeUSD: relevantTransactions.reduce(
                                (sum, tx) => sum + tx.estimatedValueUSD, 0
                            ),
                            dexesInvolved: [...new Set(
                                relevantTransactions.flatMap(tx => 
                                    tx.dexSwaps.map(swap => swap.dex)
                                )
                            )],
                            hasLargeTrades: relevantTransactions.some(tx => tx.isLargeTrade),
                            hasMevActivity: relevantTransactions.some(tx => tx.addressFlags.isMevBot)
                        }
                    };
                }
                return null;
            })
            .filter(Boolean);

        // Only return data if we found arbitrage opportunities
        if (results.length > 0) {
            console.log(`🎯 Found ${results.length} blocks with arbitrage opportunities`);
            console.log(`📊 Total opportunities: ${results.reduce((sum, block) => 
                sum + block.summary.totalArbitrageOpportunities, 0)}`);
            console.log(`💰 Total volume: $${results.reduce((sum, block) => 
                sum + block.summary.totalVolumeUSD, 0).toFixed(2)}`);
            
            return {
                data: results,
                metadata: {
                    ...metadata,
                    arbitrageFilter: {
                        blocksProcessed: data.length,
                        blocksWithOpportunities: results.length,
                        totalOpportunities: results.reduce((sum, block) => 
                            sum + block.summary.totalArbitrageOpportunities, 0),
                        filterTimestamp: Date.now()
                    }
                }
            };
        }

        // Return original data if no opportunities found
        return payload;

    } catch (error) {
        console.error('🚨 Arbitrage filter error:', error);
        return payload;
    }
}
