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
