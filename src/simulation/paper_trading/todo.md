# Paper Trading with QuickNode Integration Plan

This document outlines the steps to integrate QuickNode for real-environment paper trading.

## 1. Update QuickNode Function Client - DONE

- **File:** `src/quicknode/function_client.rs`
- **Task:** Update the Rust structs (`ArbitrageOpportunity`, `DexSwap`, etc.) to match the JSON output of the `quicknode_arbitrage_filter.js` script. The required structs are provided in `src/quicknode/README.md`.

## 2. Create Webhook Processor Module - DONE

- **File:** `src/quicknode/webhook.rs`
- **Task:** Create a new module to handle incoming webhooks from QuickNode. This module will include:
    - An HTTP server (using `warp` or `axum`).
    - Logic to parse the webhook payload.
    - A function to convert the QuickNode opportunity into the bot's internal `MultiHopArbOpportunity` struct.
    - A channel to send the processed opportunities to the `ArbitrageOrchestrator`.

## 3. Integrate Webhook Processor with Arbitrage Orchestrator - DONE

- **File:** `src/main.rs` (or the bot's entry point)
- **Task:**
    - Create an `mpsc::unbounded_channel`.
    - Pass the receiver half of the channel to the `ArbitrageOrchestrator`.

## 4. Implement the Conversion Logic

- **File:** `src/webhook_processor.rs`
- **Task:** Implement the `convert_qn_opp_to_multihop` function. This is a critical step to map the data from the QuickNode function to the bot's internal data structures.

## 5. Configure and Run

- **File:** `config/paper-trading.toml`
- **Task:** Ensure the `enabled = true` flag is set under the `[paper_trading]` section.
- **Deployment:**
    - Deploy the `quicknode_arbitrage_filter.js` script as a QuickNode function.
    - Run the bot.

