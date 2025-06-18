/**
 * 🧪 PAPER TRADING: JavaScript-Rust Bridge
 * Connects our JavaScript QuickNode analyzer with Rust arbitrage engine
 */

const path = require('path');

class RustArbitrageBridge {
    constructor() {
        this.isAvailable = false;
        this.rustLib = null;
        this.initializeRustBridge();
    }

    /**
     * Initialize the Rust library bridge
     */
    initializeRustBridge() {
        try {
            // Path to compiled Rust library
            const libPath = this.findRustLibrary();
            
            if (libPath) {
                console.log(`🔗 Loading Rust library from: ${libPath}`);
                
                // Define the Rust function signatures
                this.rustLib = ffi.Library(libPath, {
                    'health_check': ['string', []],
                    'get_library_info': ['string', []],
                    'process_quicknode_dex_analysis_for_paper_trading': ['string', ['string']],
                    'free_string': ['void', ['string']],
                });
                
                // Test the connection
                const healthCheck = this.rustLib.health_check();
                console.log('🔧 Rust bridge health check:', healthCheck);
                
                this.isAvailable = true;
                console.log('✅ Rust arbitrage bridge initialized successfully');
            } else {
                console.warn('⚠️  Rust library not found - falling back to JavaScript-only mode');
            }
        } catch (error) {
            console.warn('⚠️  Failed to initialize Rust bridge:', error.message);
            console.warn('   Falling back to JavaScript-only mode');
        }
    }

    /**
     * Find the compiled Rust library
     */
    findRustLibrary() {
        const possiblePaths = [
            path.join(__dirname, '../../../target/release/libsolana_arb_bot.dylib'), // macOS release (prefer)
            path.join(__dirname, '../../../target/release/libsolana_arb_bot.so'),    // Linux release
            path.join(__dirname, '../../../target/release/solana_arb_bot.dll'),      // Windows release
            path.join(__dirname, '../../../target/debug/libsolana_arb_bot.dylib'), // macOS debug
            path.join(__dirname, '../../../target/debug/libsolana_arb_bot.so'),    // Linux debug
            path.join(__dirname, '../../../target/debug/solana_arb_bot.dll'),      // Windows debug
        ];

        const fs = require('fs');
        for (const libPath of possiblePaths) {
            if (fs.existsSync(libPath)) {
                return libPath;
            }
        }
        return null;
    }

    /**
     * 🧪 PAPER TRADING: Process DEX analysis data using Rust engine
     * @param {Object} dexAnalysis - QuickNode DEX analysis data
     * @returns {Array} Array of trade recommendations
     */
    async processWithRust(dexAnalysis) {
        if (!this.isAvailable) {
            throw new Error('Rust bridge not available');
        }

        try {
            // Convert to JSON string for Rust
            const analysisJson = JSON.stringify(dexAnalysis);
            
            // Call Rust function
            const resultJson = this.rustLib.process_quicknode_dex_analysis_for_paper_trading(analysisJson);
            
            // Parse the result
            const recommendations = JSON.parse(resultJson);
            
            console.log(`🦀 Rust engine processed ${recommendations.length} trade recommendations`);
            return recommendations;
            
        } catch (error) {
            console.error('❌ Rust processing failed:', error.message);
            throw error;
        }
    }

    /**
     * 🧪 PAPER TRADING: Fallback JavaScript analysis
     * Simple JavaScript-based opportunity detection when Rust is unavailable
     */
    processWithJavaScript(dexAnalysis) {
        const opportunities = [];
        
        // Simple JavaScript fallback logic
        for (const [programId, metrics] of Object.entries(dexAnalysis.programs)) {
            const successRate = parseFloat(metrics.successRate.replace('%', ''));
            const transactions = metrics.transactions;
            
            // Basic opportunity detection
            if (successRate >= 90 && transactions >= 5) {
                opportunities.push({
                    dex_name: metrics.name,
                    direction: 'LONG', // Simplified
                    position_size_sol: 10.0,
                    confidence: successRate / 100.0,
                    expected_profit_sol: 0.01,
                    risk_level: 'MEDIUM',
                    entry_reason: `${successRate}% success rate, ${transactions} transactions`,
                });
            }
        }
        
        console.log(`📊 JavaScript fallback processed ${opportunities.length} opportunities`);
        return opportunities;
    }

    /**
     * 🧪 PAPER TRADING: Main processing function
     * Tries Rust first, falls back to JavaScript if needed
     */
    async processAnalysis(dexAnalysis) {
        try {
            if (this.isAvailable) {
                return await this.processWithRust(dexAnalysis);
            } else {
                return this.processWithJavaScript(dexAnalysis);
            }
        } catch (error) {
            console.warn('⚠️  Rust processing failed, falling back to JavaScript:', error.message);
            return this.processWithJavaScript(dexAnalysis);
        }
    }

    /**
     * Get bridge status information
     */
    getStatus() {
        return {
            rustAvailable: this.isAvailable,
            mode: this.isAvailable ? 'Rust + JavaScript' : 'JavaScript Only',
            libraryPath: this.rustLib ? 'Found' : 'Not Found',
        };
    }
}

module.exports = RustArbitrageBridge;
