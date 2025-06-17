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
