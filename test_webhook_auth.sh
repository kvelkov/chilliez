#!/bin/bash

# Webhook Authentication Integration Test
# This script tests the webhook server authentication with the mainnet paper trading setup

set -e

echo "🧪 Starting Webhook Authentication Integration Test"
echo "=================================================="

# Set environment for paper trading
if [ -f .env.paper-trading ]; then
    set -a
    source .env.paper-trading
    set +a
else
    echo "❌ .env.paper-trading file not found"
    exit 1
fi

# Build the project
echo "🔨 Building project..."
cargo build --release

echo ""
echo "🚀 Starting webhook server in background..."

# Start the webhook server in background
cargo run --example mainnet_paper_trading_demo &
SERVER_PID=$!

# Wait for server to start
sleep 5

echo ""
echo "🔍 Testing webhook authentication..."

# Test 1: Valid authHeader (Helius format)
echo "Test 1: Valid authHeader (Helius format)"
RESPONSE1=$(curl -s -w "%{http_code}" -o /tmp/test1.json \
  -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -H "authheader: heligo567" \
  -d '{"signature": "test_auth_header", "accounts": ["9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"], "timestamp": 1234567890}')

if [ "$RESPONSE1" = "200" ]; then
    echo "✅ Test 1 PASSED: authHeader authentication successful"
else
    echo "❌ Test 1 FAILED: Expected 200, got $RESPONSE1"
fi

# Test 2: Valid Authorization Bearer
echo "Test 2: Valid Authorization Bearer"
RESPONSE2=$(curl -s -w "%{http_code}" -o /tmp/test2.json \
  -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer heligo567" \
  -d '{"signature": "test_auth_bearer", "accounts": ["9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"], "timestamp": 1234567890}')

if [ "$RESPONSE2" = "200" ]; then
    echo "✅ Test 2 PASSED: Authorization Bearer authentication successful"
else
    echo "❌ Test 2 FAILED: Expected 200, got $RESPONSE2"
fi

# Test 3: Invalid authentication
echo "Test 3: Invalid authentication"
RESPONSE3=$(curl -s -w "%{http_code}" -o /tmp/test3.json \
  -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -H "authheader: wrong_password" \
  -d '{"signature": "test_invalid", "accounts": [], "timestamp": 1234567890}')

if [ "$RESPONSE3" = "401" ]; then
    echo "✅ Test 3 PASSED: Invalid authentication correctly rejected"
else
    echo "❌ Test 3 FAILED: Expected 401, got $RESPONSE3"
fi

# Test 4: Missing authentication
echo "Test 4: Missing authentication header"
RESPONSE4=$(curl -s -w "%{http_code}" -o /tmp/test4.json \
  -X POST http://localhost:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{"signature": "test_no_auth", "accounts": [], "timestamp": 1234567890}')

if [ "$RESPONSE4" = "401" ]; then
    echo "✅ Test 4 PASSED: Missing authentication correctly rejected"
else
    echo "❌ Test 4 FAILED: Expected 401, got $RESPONSE4"
fi

# Test 5: Health check (no auth required)
echo "Test 5: Health check endpoint"
RESPONSE5=$(curl -s -w "%{http_code}" -o /tmp/test5.json \
  -X POST http://localhost:8080/health \
  -H "Content-Type: application/json")

if [ "$RESPONSE5" = "200" ]; then
    echo "✅ Test 5 PASSED: Health check accessible without authentication"
else
    echo "❌ Test 5 FAILED: Expected 200, got $RESPONSE5"
fi

echo ""
echo "🛑 Stopping webhook server..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "📊 Test Summary:"
echo "==============="
echo "Test 1 (authHeader): $([ "$RESPONSE1" = "200" ] && echo "✅ PASS" || echo "❌ FAIL")"
echo "Test 2 (Bearer): $([ "$RESPONSE2" = "200" ] && echo "✅ PASS" || echo "❌ FAIL")"
echo "Test 3 (Invalid): $([ "$RESPONSE3" = "401" ] && echo "✅ PASS" || echo "❌ FAIL")"
echo "Test 4 (Missing): $([ "$RESPONSE4" = "401" ] && echo "✅ PASS" || echo "❌ FAIL")"
echo "Test 5 (Health): $([ "$RESPONSE5" = "200" ] && echo "✅ PASS" || echo "❌ FAIL")"

# Cleanup
rm -f /tmp/test*.json

TOTAL_PASSED=$(echo "$RESPONSE1 $RESPONSE2 $RESPONSE3 $RESPONSE4 $RESPONSE5" | tr ' ' '\n' | grep -c "200\|401")
if [ "$TOTAL_PASSED" -eq 5 ]; then
    echo ""
    echo "🎉 ALL TESTS PASSED! Webhook authentication is working correctly."
    exit 0
else
    echo ""
    echo "❌ Some tests failed. Check the webhook server configuration."
    exit 1
fi
