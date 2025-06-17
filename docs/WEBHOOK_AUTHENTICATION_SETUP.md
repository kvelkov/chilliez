# Webhook Authentication Setup Guide

## Overview

The Solana arbitrage bot's webhook receiver has been configured with authentication to ensure only authorized Helius webhook requests are processed. This document explains the setup and security measures in place.

## Authentication Configuration

### Environment Variables

The webhook authentication password is configured in `.env.paper-trading`:

```bash
# Webhook Configuration
ENABLE_WEBHOOKS=true
WEBHOOK_PORT=8080
WEBHOOK_AUTH_PASSWORD=heligo567
```

### Security Measures

The webhook server validates incoming requests using the following methods:

1. **Helius AuthHeader (Primary - Recommended)**
   ```
   authheader: heligo567
   ```
   This is the standard format used by Helius webhooks when configured with `"authHeader"` field.

2. **Authorization Header with Bearer Token**
   ```
   Authorization: Bearer heligo567
   ```

3. **Direct Authorization Header**
   ```
   Authorization: heligo567
   ```

4. **X-Webhook-Password Header (Alternative)**
   ```
   X-Webhook-Password: heligo567
   ```

## Helius Webhook Setup

### 1. Create Webhook with Authentication

When creating the webhook on Helius, you can include the password in the URL or headers:

**Option A: JSON Configuration with authHeader (Recommended)**
```json
{
  "webhookURL": "https://your-domain.com:8080/webhook",
  "transactionTypes": ["Any"],
  "accountAddresses": ["9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"],
  "authHeader": "heligo567"
}
```

**Option B: Authorization Header Configuration**
Set the Authorization header in your Helius webhook configuration:
```
Authorization: Bearer heligo567
```

**Option C: URL Parameter (Less Secure)**
```
https://your-domain.com:8080/webhook?auth=heligo567
```

### 2. Current Configuration

Based on your setup, the webhook is configured as:
- **URL**: `https://your-ngrok-url:8080/webhook`
- **Password**: `heligo567`
- **Event Types**: SWAP, ADD_LIQUIDITY, REMOVE_FROM_POOL, TRANSFER

## Testing Authentication

### Valid Request Examples

**Using Helius authHeader format (Primary):**
```bash
curl -X POST https://your-domain.com:8080/webhook \
  -H "Content-Type: application/json" \
  -H "authheader: heligo567" \
  -d '{"signature": "test", "accounts": [], "timestamp": 1234567890}'
```

**Using Authorization Bearer:**
```bash
curl -X POST https://your-domain.com:8080/webhook \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer heligo567" \
  -d '{"signature": "test", "accounts": [], "timestamp": 1234567890}'
```

### Invalid Request (Should Return 401)

```bash
curl -X POST https://your-domain.com:8080/webhook \
  -H "Content-Type: application/json" \
  -d '{"test": "webhook"}'
```

## Security Features

### 1. Authentication Validation

- The webhook server validates all incoming requests
- Requests without valid authentication return HTTP 401 Unauthorized
- Multiple authentication methods are supported for compatibility

### 2. Logging

- All authentication attempts are logged
- Failed authentication attempts are logged with warnings
- Successful authentications are logged for monitoring

### 3. Environment-Based Configuration

- The password is stored in environment variables, not hardcoded
- If `WEBHOOK_AUTH_PASSWORD` is not set, the server logs a warning but allows requests (development mode)

## Production Security Recommendations

### 1. Strong Password

Replace the current password with a stronger one:
```bash
# Generate a strong password
openssl rand -base64 32

# Update environment file
WEBHOOK_AUTH_PASSWORD=your-strong-password-here
```

### 2. HTTPS Only

Ensure your webhook endpoint uses HTTPS in production:
```bash
WEBHOOK_URL=https://your-secure-domain.com:8080/webhook
```

### 3. IP Whitelisting

Consider implementing IP whitelisting for Helius webhook sources:
- Helius webhook IPs: Check Helius documentation for current IP ranges
- Implement at firewall or reverse proxy level

### 4. Request Signing (Future Enhancement)

For maximum security, consider implementing webhook signature verification:
- Use HMAC signatures with shared secrets
- Verify request integrity and authenticity
- This would be a future enhancement to the current system

## Troubleshooting

### Common Issues

1. **401 Unauthorized Error**
   - Check if `WEBHOOK_AUTH_PASSWORD` is set correctly
   - Verify the password matches between Helius and your environment
   - Ensure the Authorization header is formatted correctly

2. **Webhook Not Receiving Events**
   - Verify the webhook URL is publicly accessible
   - Check if the webhook server is running on the correct port
   - Ensure firewall rules allow incoming connections on port 8080

3. **Authentication Bypassed**
   - If `WEBHOOK_AUTH_PASSWORD` is not set, authentication is disabled
   - Check environment variable loading in your deployment

### Debug Mode

To debug authentication issues, you can temporarily enable debug logging:

```bash
RUST_LOG=debug cargo run --example mainnet_paper_trading_demo
```

This will show detailed authentication attempts and headers.

## Conclusion

The webhook authentication system provides a secure way to receive Helius events while preventing unauthorized access. The password-based authentication ensures that only your configured Helius webhooks can trigger pool updates in the arbitrage bot.

For production deployment, follow the security recommendations and consider implementing additional security measures like IP whitelisting and request signing.
