# Jito Bundle Integration for Atomic Arbitrage Execution

This document outlines the integration of Jito Labs' bundle functionality for atomic and MEV-prioritized execution of arbitrage opportunities. This feature replaces the previous, slower method of sending individual transactions.

## Key Features

1.  **Atomic Execution**: Multi-hop arbitrage trades are bundled into a single atomic unit. This ensures that either all transactions in the sequence succeed or none do, eliminating the risk of partial execution and asset loss.

2.  **MEV Protection & Priority**: By using Jito's bundle endpoint, trades are sent directly to validators, bypassing the public mempool. This provides protection against front-running and other MEV-related risks. Bundles are also given priority execution.

3.  **Dynamic Tip Mechanism**: To incentivize validators to include the bundle, a tip is included.
    *   A random tip account is selected from a configurable list of fee accounts.
    *   The tip amount is dynamically calculated as a percentage of the estimated profit from the arbitrage opportunity. This ensures that tips are proportional to the potential gains.
    *   A `system_instruction::transfer` is added to the bundle to pay the tip.

4.  **Bundle Status Polling**:
    *   After a bundle is submitted, the system polls Jito's `getBundleStatuses` endpoint to track its execution status.
    *   Polling continues at a configurable interval until the bundle is confirmed on-chain or a timeout is reached.
    *   This provides real-time feedback on whether a trade was successful.

5.  **Configuration**: The Jito integration is controlled by the following settings in `config/paper-trading.toml` (or environment variables):
    *   `enable_jito_bundle`: Master switch to enable/disable the feature.
    *   `jito_quicknode_url`: The QuickNode RPC endpoint for Jito.
    *   `jito_tip_lamports`: A base tip amount (can be augmented by the dynamic tip).
    *   `jito_region`: The Jito region for the block engine.
    *   `jito_tip_accounts`: A list of public keys for the tip accounts.
    *   `jito_dynamic_tip_percentage`: The percentage of profit to use as a dynamic tip.
    *   `jito_bundle_status_poll_interval_ms`: How often to poll for bundle status.
    *   `jito_bundle_status_timeout_secs`: How long to wait for bundle confirmation before timing out.

## Workflow

1.  **Opportunity Detection**: The arbitrage bot identifies a profitable multi-hop opportunity.
2.  **Instruction Grouping**: The individual transactions for the trade are grouped into a sequence.
3.  **Dynamic Tip Calculation**: A tip is calculated based on the opportunity's profit.
4.  **Bundle Creation**: The trade instructions and the tip instruction are packaged into a Jito bundle.
5.  **Bundle Submission**: The bundle is sent to the Jito endpoint via `sendBundle`.
6.  **Status Polling**: The system begins polling for the bundle's status.
7.  **Outcome**:
    *   **Success**: If the bundle is confirmed, the trade is complete.
    *   **Failure/Timeout**: If the bundle fails or times out, the system logs the error and can fall back to standard transaction execution if configured to do so.
