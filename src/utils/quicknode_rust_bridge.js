/**
 * 🔗 JAVASCRIPT-RUST BRIDGE
 * QuickNode DEX Analysis Integration
 * 
 * This module provides the bridge between our JavaScript QuickNode analyzer
 * and the Rust arbitrage analysis engine for real-time paper trading.
 */

const ffi = require('ffi-napi');
const ref = require('ref-napi');
const path = require('path');
const os = require('os');

class QuickNodeRustBridge {
    constructor() {
        this.lib = null;
        this.isLoaded = false;
        this.initializeLibrary();
    }

    /**
     * Initialize the Rust library with FFI bindings
     */
    initializeLibrary() {
        try {
            // Determine the library file name based on the platform
            const libExtension = this.getLibraryExtension();
            const libName = `libsolana_arb_bot${libExtension}`;
            
            // Try different possible paths for the library
            const possiblePaths = [
                path.join(__dirname, '../../target/release/', libName),
                path.join(__dirname, '../../target/debug/', libName),
                path.join(process.cwd(), 'target/release/', libName),
                path.join(process.cwd(), 'target/debug/', libName),
            ];

            let libPath = null;
            for (const testPath of possiblePaths) {
                try {
                    require('fs').accessSync(testPath);
                    libPath = testPath;
                    break;
                } catch (e) {
                    // Continue to next path
                }
            }

            if (!libPath) {
                console.warn('⚠️  Rust library not found. Please build with: cargo build --release');
                return;
            }

            // Define the FFI interface
            this.lib = ffi.Library(libPath, {
                'process_quicknode_dex_analysis_for_paper_trading': ['pointer', ['string']],
                'free_string': ['void', ['pointer']],
                'get_library_info': ['pointer', []],
                'health_check': ['pointer', []],
                // New arbitrage engine functions
                'initialize_arbitrage_engine': ['pointer', []],
                'detect_arbitrage_opportunities_ffi': ['pointer', ['string']],
                'execute_paper_arbitrage_ffi': ['pointer', ['string']]
            });

            this.isLoaded = true;
            console.log('✅ Rust bridge loaded successfully');
            
            // Test the connection
            this.testConnection();
            
        } catch (error) {
            console.error('❌ Failed to load Rust library:', error.message);
            console.log('💡 Make sure to build the Rust library first: cargo build --release');
        }
    }

    /**
     * Get the appropriate library extension for the current platform
     */
    getLibraryExtension() {
        const platform = os.platform();
        switch (platform) {
            case 'win32':
                return '.dll';
            case 'darwin':
                return '.dylib';
            default:
                return '.so';
        }
    }

    /**
     * Test the connection to the Rust library
     */
    testConnection() {
        if (!this.isLoaded) return false;

        try {
            const infoPtr = this.lib.get_library_info();
            const infoStr = this.pointerToString(infoPtr);
            this.lib.free_string(infoPtr);
            
            const info = JSON.parse(infoStr);
            console.log(`🦀 Rust library info:`, info);
            
            const healthPtr = this.lib.health_check();
            const healthStr = this.pointerToString(healthPtr);
            this.lib.free_string(healthPtr);
            
            const health = JSON.parse(healthStr);
            console.log(`💊 Health check:`, health);
            
            return true;
        } catch (error) {
            console.error('❌ Bridge test failed:', error.message);
            return false;
        }
    }

    /**
     * Convert a C string pointer to a JavaScript string
     */
    pointerToString(ptr) {
        if (ptr.isNull()) {
            return null;
        }
        return ref.readCString(ptr);
    }

    /**
     * 🧪 PAPER TRADING: Process QuickNode DEX analysis through Rust
     * 
     * @param {Object} dexAnalysis - DEX analysis data from JavaScript analyzer
     * @returns {Array} Array of trade recommendations from Rust analysis
     */
    async processQuickNodeAnalysis(dexAnalysis) {
        if (!this.isLoaded) {
            console.warn('⚠️  Rust bridge not loaded, falling back to JavaScript-only mode');
            return this.fallbackJavaScriptAnalysis(dexAnalysis);
        }

        try {
            // Convert the analysis to JSON string
            const analysisJson = JSON.stringify(dexAnalysis);
            
            // Call the Rust function
            const resultPtr = this.lib.process_quicknode_dex_analysis_for_paper_trading(analysisJson);
            
            if (resultPtr.isNull()) {
                throw new Error('Rust function returned null');
            }
            
            // Convert result back to JavaScript
            const resultStr = this.pointerToString(resultPtr);
            this.lib.free_string(resultPtr);
            
            if (!resultStr) {
                throw new Error('Failed to read result from Rust');
            }
            
            const result = JSON.parse(resultStr);
            
            // Check for errors in the result
            if (result.error) {
                throw new Error(result.error);
            }
            
            console.log(`🦀 Rust analysis complete: ${result.length} recommendations generated`);
            return result;
            
        } catch (error) {
            console.error('❌ Rust analysis failed:', error.message);
            console.log('⬇️  Falling back to JavaScript analysis...');
            return this.fallbackJavaScriptAnalysis(dexAnalysis);
        }
    }

    /**
     * 🧪 FALLBACK: JavaScript-only analysis when Rust bridge fails
     * 
     * @param {Object} dexAnalysis - DEX analysis data
     * @returns {Array} Simplified trade recommendations
     */
    fallbackJavaScriptAnalysis(dexAnalysis) {
        const recommendations = [];
        
        // Simple JavaScript fallback logic
        for (const [programId, metrics] of Object.entries(dexAnalysis.programs)) {
            const successRate = parseFloat(metrics.successRate.replace('%', ''));
            const transactionCount = metrics.transactions;
            
            // Basic opportunity detection
            if (successRate >= 90 && transactionCount >= 5) {
                const valueChange = parseFloat(metrics.valueChange.replace(' SOL', ''));
                const direction = valueChange > 0 ? 'LONG' : 'SHORT';
                
                recommendations.push({
                    dexName: metrics.name,
                    direction: direction,
                    positionSizeSol: Math.min(10 * (successRate / 100), 50),
                    confidence: successRate / 100,
                    expectedProfitSol: Math.abs(valueChange) * 0.01,
                    riskLevel: successRate >= 95 ? 'LOW' : 'MEDIUM',
                    entryReason: `${successRate}% success rate, ${transactionCount} transactions (JS fallback)`
                });
            }
        }
        
        console.log(`📊 JavaScript fallback analysis: ${recommendations.length} recommendations`);
        return recommendations;
    }

    /**
     * 🚀 Initialize the real arbitrage engine
     */
    async initializeArbitrageEngine() {
        if (!this.isLoaded) {
            throw new Error('Rust library not loaded');
        }
        
        try {
            console.log('🚀 Initializing real arbitrage engine...');
            
            const resultPtr = this.lib.initialize_arbitrage_engine();
            
            if (resultPtr.isNull()) {
                throw new Error('Failed to initialize arbitrage engine');
            }
            
            const resultStr = this.pointerToString(resultPtr);
            this.lib.free_string(resultPtr);
            
            const result = JSON.parse(resultStr);
            
            if (result.error) {
                throw new Error(result.error);
            }
            
            console.log('✅ Arbitrage engine initialized:', result);
            return result;
            
        } catch (error) {
            console.error('❌ Failed to initialize arbitrage engine:', error.message);
            throw error;
        }
    }

    /**
     * 🎯 Detect real arbitrage opportunities using the Rust engine
     */
    async detectArbitrageOpportunities(poolData = []) {
        if (!this.isLoaded) {
            throw new Error('Rust library not loaded');
        }
        
        try {
            console.log('🎯 Detecting arbitrage opportunities...');
            
            // Send pool data directly as array for the updated FFI
            const inputData = JSON.stringify(poolData);
            const resultPtr = this.lib.detect_arbitrage_opportunities_ffi(inputData);
            
            if (resultPtr.isNull()) {
                throw new Error('Failed to detect arbitrage opportunities');
            }
            
            const resultStr = this.pointerToString(resultPtr);
            this.lib.free_string(resultPtr);
            
            const result = JSON.parse(resultStr);
            
            if (result.error) {
                throw new Error(result.error);
            }
            
            console.log(`🎯 Found ${result.opportunities ? result.opportunities.length : 0} arbitrage opportunities`);
            return result;
            
        } catch (error) {
            console.error('❌ Failed to detect arbitrage opportunities:', error.message);
            throw error;
        }
    }

    /**
     * 📊 Execute simulated arbitrage trade (paper trading)
     */
    async executePaperArbitrage(opportunity) {
        if (!this.isLoaded) {
            throw new Error('Rust library not loaded');
        }
        
        try {
            console.log('📊 Executing paper arbitrage trade...');
            
            const inputData = JSON.stringify(opportunity);
            const resultPtr = this.lib.execute_paper_arbitrage_ffi(inputData);
            
            if (resultPtr.isNull()) {
                throw new Error('Failed to execute paper arbitrage');
            }
            
            const resultStr = this.pointerToString(resultPtr);
            this.lib.free_string(resultPtr);
            
            const result = JSON.parse(resultStr);
            
            if (result.error) {
                throw new Error(result.error);
            }
            
            console.log('📊 Paper arbitrage executed:', result);
            return result;
            
        } catch (error) {
            console.error('❌ Failed to execute paper arbitrage:', error.message);
            throw error;
        }
    }

    /**
     * Check if the Rust bridge is available and working
     */
    isAvailable() {
        return this.isLoaded && this.testConnection();
    }

    /**
     * Get bridge status information
     */
    getStatus() {
        return {
            loaded: this.isLoaded,
            available: this.isAvailable(),
            mode: this.isLoaded ? 'rust-accelerated' : 'javascript-fallback'
        };
    }
}

// Export singleton instance
const bridge = new QuickNodeRustBridge();

module.exports = {
    QuickNodeRustBridge,
    bridge,
    
    // Convenience functions
    processQuickNodeAnalysis: (analysis) => bridge.processQuickNodeAnalysis(analysis),
    isRustAvailable: () => bridge.isAvailable(),
    getBridgeStatus: () => bridge.getStatus(),
    
    // New arbitrage engine functions
    initializeArbitrageEngine: () => bridge.initializeArbitrageEngine(),
    detectArbitrageOpportunities: (poolData) => bridge.detectArbitrageOpportunities(poolData),
    executePaperArbitrage: (opportunity) => bridge.executePaperArbitrage(opportunity)
};
