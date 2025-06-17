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
