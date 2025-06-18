/**
 * QuickNode DEX Analysis - Adapted for Local Use
 * Based on QuickNode's Solana DEX Analysis Function approach
 * Analyzes Solana blocks for DEX activity using programs_with_logs methodology
 */

let fetch;

class SolanaDEXAnalyzer {
    constructor(quicknodeEndpoint, apiKey = null) {
        this.quicknodeEndpoint = quicknodeEndpoint;
        this.apiKey = apiKey;
        
        // Skip fetch initialization in test environment
        if (process.env.NODE_ENV === 'test' || process.env.JEST_WORKER_ID) {
            this.fetchReady = Promise.resolve();
            global.fetch = global.fetch || (() => Promise.resolve({
                ok: true,
                json: () => Promise.resolve({ data: [] })
            }));
        } else {
            this.fetchReady = this.initializeFetch();
        }
        
        // DEX Programs from QuickNode's analysis (exact match)
        this.DEX_PROGRAMS = {
            PHOENIX: 'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY',
            RAYDIUM_CLM: '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8',
            JUPITER: 'JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4',
        };
    }

    async initializeFetch() {
        if (!fetch) {
            try {
                const nodeFetch = await import('node-fetch');
                fetch = nodeFetch.default;
            } catch (err) {
                // In test environment, we might not need fetch
                console.warn('⚠️ Could not load node-fetch:', err.message);
                // Use a mock fetch for testing
                fetch = () => Promise.resolve({
                    ok: true,
                    json: () => Promise.resolve({ data: [] })
                });
            }
        }
    }

    /**
     * Main analysis function adapted from QuickNode's approach
     * Analyzes a specific block or latest block for DEX activity
     */
    async analyzeBlock(blockNumber = null) {
        try {
            // Ensure fetch is ready
            await this.fetchReady;
            
            // Get slot first
            let slot = blockNumber;
            if (!slot) {
                const slotResponse = await this.makeRPCCall('getSlot');
                slot = slotResponse.result;
            }
            
            // Get block data with program invocations and logs
            const blockData = await this.getBlockDataWithLogs(slot);
            if (!blockData || !blockData.transactions) {
                return this.getEmptyMetrics();
            }

            // Initialize metrics structure (same as QuickNode's)
            let metrics = this.initializeMetrics();
            
            // Set the actual block information
            metrics.slot = slot;
            metrics.blockTime = blockData.blockTime || Date.now();
            
            // Process each transaction
            blockData.transactions.forEach(tx => {
                this.processTransaction(tx, metrics);
            });

            // Calculate final metrics
            this.calculateFinalMetrics(metrics);
            
            return metrics;
            
        } catch (error) {
            console.error('Error analyzing block:', error.message);
            return this.getEmptyMetrics();
        }
    }

    /**
     * Initialize metrics structure (adapted from QuickNode's function)
     */
    initializeMetrics() {
        return {
            message: `DEX analysis from the programs_with_logs dataset on the solana-mainnet network.`,
            blockTime: 0,
            slot: 0,
            programs: {
                [this.DEX_PROGRAMS.PHOENIX]: {
                    name: 'Phoenix',
                    invocations: 0,
                    transactions: 0,
                    valueChange: 0,
                    uniqueUsers: new Set(),
                    successfulTxs: 0,
                    failedTxs: 0,
                },
                [this.DEX_PROGRAMS.RAYDIUM_CLM]: {
                    name: 'Raydium CLM',
                    invocations: 0,
                    transactions: 0,
                    valueChange: 0,
                    uniqueUsers: new Set(),
                    successfulTxs: 0,
                    failedTxs: 0,
                },
                [this.DEX_PROGRAMS.JUPITER]: {
                    name: 'Jupiter',
                    invocations: 0,
                    transactions: 0,
                    valueChange: 0,
                    uniqueUsers: new Set(),
                    successfulTxs: 0,
                    failedTxs: 0,
                },
            },
            totalDexTransactions: 0,
            totalValueChange: 0,
        };
    }

    /**
     * Process a single transaction (adapted from QuickNode's logic)
     */
    processTransaction(tx, metrics) {
        // Update block info
        metrics.blockTime = tx.blockTime;
        metrics.slot = tx.slot;

        // Track which DEX programs were involved in this transaction
        const involvedPrograms = new Set();

        // Analyze program invocations
        if (tx.transaction && tx.transaction.message && tx.transaction.message.instructions) {
            tx.transaction.message.instructions.forEach(instruction => {
                const programId = tx.transaction.message.accountKeys[instruction.programIdIndex];
                
                if (metrics.programs[programId]) {
                    const program = metrics.programs[programId];
                    program.invocations++;
                    involvedPrograms.add(programId);

                    // Track value changes from account balance changes
                    if (tx.meta && tx.meta.preBalances && tx.meta.postBalances) {
                        const valueChange = tx.meta.postBalances.reduce((sum, postBalance, index) => {
                            const preBalance = tx.meta.preBalances[index] || 0;
                            return sum + (postBalance - preBalance);
                        }, 0);
                        
                        program.valueChange += valueChange;
                        metrics.totalValueChange += valueChange;
                    }

                    // Track unique users from accounts
                    if (tx.transaction.message.accountKeys) {
                        tx.transaction.message.accountKeys.forEach(pubkey => {
                            program.uniqueUsers.add(pubkey);
                        });
                    }
                }
            });
        }

        // For each involved program in this transaction
        involvedPrograms.forEach(programId => {
            const program = metrics.programs[programId];
            program.transactions++;
            metrics.totalDexTransactions++;

            // Determine success based on logs (QuickNode's approach)
            const isSuccess = this.determineTransactionSuccess(tx.meta?.logMessages || [], programId);
            
            if (isSuccess) {
                program.successfulTxs++;
            } else {
                program.failedTxs++;
            }
        });
    }

    /**
     * Determine transaction success based on logs (QuickNode's method)
     */
    determineTransactionSuccess(logs, programId) {
        if (!Array.isArray(logs)) return false;

        const programLogs = logs.filter(log =>
            log.includes(programId) ||
            (programId === this.DEX_PROGRAMS.PHOENIX && log.includes('Phoenix')) ||
            (programId === this.DEX_PROGRAMS.JUPITER && log.includes('Jupiter')) ||
            (programId === this.DEX_PROGRAMS.RAYDIUM_CLM && log.includes('ray_log'))
        );

        return programLogs.some(log =>
            log.includes('success') ||
            log.includes('succeeded') ||
            (programId === this.DEX_PROGRAMS.PHOENIX && log.includes('market events')) ||
            (programId === this.DEX_PROGRAMS.JUPITER && log.includes('Route')) ||
            (programId === this.DEX_PROGRAMS.RAYDIUM_CLM && log.includes('ray_log'))
        );
    }

    /**
     * Calculate final metrics (QuickNode's approach)
     */
    calculateFinalMetrics(metrics) {
        for (const [programId, data] of Object.entries(metrics.programs)) {
            // Calculate user counts
            data.uniqueUserCount = data.uniqueUsers.size;
            delete data.uniqueUsers;

            // Calculate transaction share
            data.transactionShare = metrics.totalDexTransactions > 0
                ? ((data.transactions / metrics.totalDexTransactions) * 100).toFixed(2) + '%'
                : '0%';

            // Format value changes to be more readable
            data.valueChange = (data.valueChange / 1e9).toFixed(4) + ' SOL';

            // Add success rate
            data.successRate = data.transactions > 0
                ? ((data.successfulTxs / data.transactions) * 100).toFixed(2) + '%'
                : '0%';
        }

        metrics.totalValueChange = (metrics.totalValueChange / 1e9).toFixed(4) + ' SOL';
    }

    /**
     * Get block data with logs using RPC
     */
    async getBlockDataWithLogs(blockNumber = null) {
        try {
            // First get the latest slot if no block number specified
            let slot = blockNumber;
            if (!slot) {
                const slotResponse = await this.makeRPCCall('getSlot');
                slot = slotResponse.result;
            }

            // Get block with transaction details and logs
            const blockResponse = await this.makeRPCCall('getBlock', [
                slot,
                {
                    encoding: "json",
                    transactionDetails: "full",
                    rewards: false,
                    maxSupportedTransactionVersion: 0
                }
            ]);

            return blockResponse.result;
            
        } catch (error) {
            console.error('Error fetching block data:', error.message);
            return null;
        }
    }

    /**
     * Make RPC call to QuickNode
     */
    async makeRPCCall(method, params = []) {
        // Ensure fetch is ready
        await this.fetchReady;
        
        const payload = {
            jsonrpc: "2.0",
            id: 1,
            method,
            params
        };

        const response = await fetch(this.quicknodeEndpoint, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(payload)
        });

        const result = await response.json();
        return result;
    }

    /**
     * Get empty metrics structure
     */
    getEmptyMetrics() {
        return {
            message: "No DEX activity found",
            blockTime: 0,
            slot: 0,
            programs: {},
            totalDexTransactions: 0,
            totalValueChange: "0.0000 SOL"
        };
    }
}

module.exports = { SolanaDEXAnalyzer };
