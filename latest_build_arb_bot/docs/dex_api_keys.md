# DEX API Key Configuration (.env)

Add the following keys to your `.env` file as you on-board/whitelist paid API access for each DEX.

```
# Jupiter DEX (future support; add if you have a paid/private endpoint)
JUPITER_API_KEY=your_jupiter_api_key

# Orca DEX aggregator/private (provide if enabled by the service)
ORCA_API_KEY=your_orca_api_key

# Raydium DEX aggregator/private (provide if enabled by the service)
RAYDIUM_API_KEY=your_raydium_api_key

# CoinAPI (already supported, strongly recommended for arbitrary market data)
COINAPI_KEY=your_coinapi_key
```

- All keys are **optional** except `COINAPI_KEY` (required if using CoinAPI).
- If a DEX does not offer API keys, leave blank and the client falls back to public endpoints or on-chain logic.
- Store your `.env` **outside of version control** and restrict access.
- If your provider uses a header other than `"api-key"` (rare), update the `insert` call in the relevant client builder.

> **Pro tip:** Restart your bot after updating `.env` so all clients pick up the new keys.