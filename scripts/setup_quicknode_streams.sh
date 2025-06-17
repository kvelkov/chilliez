#!/bin/bash
# QuickNode Stream Setup Script
# This script will help you set up and test QuickNode streams

set -e

echo "🚀 QuickNode Stream Setup for Solana Arbitrage Bot"
echo "================================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check if required environment variables are set
echo -e "${BLUE}📋 Checking environment configuration...${NC}"

if [ -z "$RPC_URL" ]; then
    echo -e "${RED}❌ RPC_URL not set. Please run: source .env.paper-trading${NC}"
    exit 1
fi

if [ -z "$WS_URL" ]; then
    echo -e "${RED}❌ WS_URL not set. Please run: source .env.paper-trading${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Environment variables configured${NC}"
echo "   RPC URL: $RPC_URL"
echo "   WS URL: $WS_URL"

# Test RPC connection
echo -e "${BLUE}🌐 Testing RPC connection...${NC}"
RPC_TEST=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getHealth"
  }' || echo "FAILED")

if echo "$RPC_TEST" | grep -q "ok"; then
    echo -e "${GREEN}✅ RPC connection successful${NC}"
else
    echo -e "${RED}❌ RPC connection failed${NC}"
    echo "Response: $RPC_TEST"
    exit 1
fi

# Create stream configuration directory
echo -e "${BLUE}📁 Creating stream configuration...${NC}"
mkdir -p streams
mkdir -p logs/streams

# Create a simple WebSocket test script
cat > streams/test_websocket.js << 'EOF'
const WebSocket = require('ws');

// QuickNode WebSocket URL from environment
const WS_URL = process.env.WS_URL;

if (!WS_URL) {
    console.error('❌ WS_URL environment variable not set');
    process.exit(1);
}

console.log('🔌 Connecting to QuickNode WebSocket...');
console.log('URL:', WS_URL);

const ws = new WebSocket(WS_URL);

ws.on('open', function open() {
    console.log('✅ WebSocket connected successfully!');
    
    // Subscribe to slot updates (simplest subscription)
    const slotSubscribe = {
        jsonrpc: "2.0",
        id: 1,
        method: "slotSubscribe"
    };
    
    console.log('📡 Subscribing to slot updates...');
    ws.send(JSON.stringify(slotSubscribe));
});

ws.on('message', function message(data) {
    try {
        const parsed = JSON.parse(data);
        console.log('📨 Received message:', JSON.stringify(parsed, null, 2));
        
        // Close after receiving a few messages for testing
        if (parsed.method === 'slotNotification') {
            console.log('🎯 Slot notification received - WebSocket is working!');
            console.log('Slot number:', parsed.params?.result?.slot);
            
            // Close after first slot notification
            setTimeout(() => {
                console.log('✅ Test completed successfully');
                ws.close();
            }, 1000);
        }
    } catch (error) {
        console.error('❌ Error parsing message:', error);
        console.log('Raw data:', data.toString());
    }
});

ws.on('error', function error(err) {
    console.error('❌ WebSocket error:', err);
});

ws.on('close', function close() {
    console.log('🔌 WebSocket connection closed');
    process.exit(0);
});

// Timeout after 30 seconds
setTimeout(() => {
    console.log('⏰ Test timeout - closing connection');
    ws.close();
}, 30000);
EOF

# Create DEX monitoring script
cat > streams/monitor_dex.js << 'EOF'
const WebSocket = require('ws');

const WS_URL = process.env.WS_URL;
if (!WS_URL) {
    console.error('❌ WS_URL environment variable not set');
    process.exit(1);
}

// DEX Program IDs to monitor
const DEX_PROGRAMS = [
    '9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools
    '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM
    'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter
];

console.log('🎯 Starting DEX activity monitor...');
console.log('Monitoring programs:', DEX_PROGRAMS);

const ws = new WebSocket(WS_URL);

ws.on('open', function open() {
    console.log('✅ Connected to QuickNode');
    
    // Subscribe to logs for each DEX program
    DEX_PROGRAMS.forEach((programId, index) => {
        const subscription = {
            jsonrpc: "2.0",
            id: index + 1,
            method: "logsSubscribe",
            params: [
                { mentions: [programId] },
                { commitment: "finalized" }
            ]
        };
        
        console.log(`📡 Subscribing to ${programId} logs...`);
        ws.send(JSON.stringify(subscription));
    });
});

ws.on('message', function message(data) {
    try {
        const parsed = JSON.parse(data);
        
        if (parsed.method === 'logsNotification') {
            const logs = parsed.params?.result?.value?.logs || [];
            const signature = parsed.params?.result?.value?.signature;
            
            console.log('\n🔥 DEX Activity Detected!');
            console.log('Signature:', signature);
            console.log('Logs preview:', logs.slice(0, 3).join('\n'));
            console.log('Total log lines:', logs.length);
            
            // Look for common DEX patterns
            const logText = logs.join(' ').toLowerCase();
            if (logText.includes('swap')) {
                console.log('💱 SWAP detected!');
            }
            if (logText.includes('liquidity')) {
                console.log('💧 LIQUIDITY operation detected!');
            }
        }
    } catch (error) {
        console.error('❌ Error processing message:', error);
    }
});

ws.on('error', function error(err) {
    console.error('❌ WebSocket error:', err);
});

ws.on('close', function close() {
    console.log('🔌 DEX monitor disconnected');
});

console.log('🏃 DEX monitor running... Press Ctrl+C to stop');
EOF

# Create account monitoring script for specific pools
cat > streams/monitor_pools.js << 'EOF'
const WebSocket = require('ws');

const WS_URL = process.env.WS_URL;
if (!WS_URL) {
    console.error('❌ WS_URL environment variable not set');
    process.exit(1);
}

// Example pool addresses (you can add your specific pools here)
const POOL_ADDRESSES = [
    // Add specific pool addresses you want to monitor
    // Example: 'PoolAddressHere123456789'
];

console.log('🏊 Starting pool monitor...');

if (POOL_ADDRESSES.length === 0) {
    console.log('⚠️  No specific pools configured. Add pool addresses to POOL_ADDRESSES array.');
    console.log('💡 You can find pool addresses from DEX websites or by analyzing transactions.');
    process.exit(0);
}

const ws = new WebSocket(WS_URL);

ws.on('open', function open() {
    console.log('✅ Connected to QuickNode');
    
    // Subscribe to account changes for each pool
    POOL_ADDRESSES.forEach((poolAddress, index) => {
        const subscription = {
            jsonrpc: "2.0",
            id: index + 1,
            method: "accountSubscribe",
            params: [
                poolAddress,
                {
                    encoding: "base64",
                    commitment: "finalized"
                }
            ]
        };
        
        console.log(`📡 Subscribing to pool ${poolAddress}...`);
        ws.send(JSON.stringify(subscription));
    });
});

ws.on('message', function message(data) {
    try {
        const parsed = JSON.parse(data);
        
        if (parsed.method === 'accountNotification') {
            const account = parsed.params?.result?.value;
            console.log('\n💧 Pool State Change Detected!');
            console.log('Account:', account?.pubkey);
            console.log('Owner:', account?.owner);
            console.log('Lamports:', account?.lamports);
            console.log('Data length:', account?.data?.length);
        }
    } catch (error) {
        console.error('❌ Error processing message:', error);
    }
});

ws.on('error', function error(err) {
    console.error('❌ WebSocket error:', err);
});

ws.on('close', function close() {
    console.log('🔌 Pool monitor disconnected');
});

console.log('🏃 Pool monitor running... Press Ctrl+C to stop');
EOF

# Make scripts executable
chmod +x streams/*.js

echo -e "${GREEN}✅ Stream configuration created${NC}"
echo ""
echo -e "${YELLOW}📋 Next Steps:${NC}"
echo "1. Test basic WebSocket connection:"
echo "   ${BLUE}cd streams && node test_websocket.js${NC}"
echo ""
echo "2. Monitor DEX activity:"
echo "   ${BLUE}cd streams && node monitor_dex.js${NC}"
echo ""
echo "3. Monitor specific pools (after adding addresses):"
echo "   ${BLUE}cd streams && node monitor_pools.js${NC}"
echo ""
echo -e "${YELLOW}💡 Tips:${NC}"
echo "- Start with test_websocket.js to verify your connection"
echo "- DEX monitor will show you real arbitrage opportunities"
echo "- Add specific pool addresses to monitor_pools.js for targeted monitoring"
echo ""
echo -e "${GREEN}🎯 Files created in ./streams/ directory${NC}"
ls -la streams/
