//! Jito Bundle Integration for Atomic Arbitrage Execution
//!
//! This module provides functions to submit bundles of signed transactions to QuickNode's Jito endpoint
//! for atomic, MEV-prioritized execution on Solana. It also includes logic for dynamic tip calculation
//! and bundle status polling.

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use solana_program::{instruction::Instruction, pubkey::Pubkey, system_instruction};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JitoBundleStatus {
    Processed,
    Confirmed,
    Dropped,
    Pending,
    Unknown,
}

/// Sends a bundle of signed transactions to QuickNode Jito endpoint for atomic execution.
/// Returns the bundle_id if accepted.
pub async fn send_jito_bundle(
    quicknode_url: &str,
    signed_txs_base64: Vec<String>,
    region: Option<&str>,
) -> Result<String> {
    let client = Client::new();
    let params = if let Some(region) = region {
        json!([signed_txs_base64, region])
    } else {
        json!([signed_txs_base64])
    };
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": params
    });
    let resp = client.post(quicknode_url).json(&req).send().await?;
    let resp_json: serde_json::Value = resp.json().await?;
    if let Some(bundle_id) = resp_json.get("result").and_then(|v| v.as_str()) {
        Ok(bundle_id.to_string())
    } else {
        Err(anyhow!("sendBundle failed: {:?}", resp_json))
    }
}

/// Checks the status of a bundle by bundle_id.
pub async fn get_jito_bundle_status(
    quicknode_url: &str,
    bundle_id: &str,
) -> Result<serde_json::Value> {
    let client = Client::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBundleStatuses",
        "params": [[bundle_id]]
    });
    let resp = client.post(quicknode_url).json(&req).send().await?;
    let resp_json: serde_json::Value = resp.json().await?;
    Ok(resp_json)
}

/// Polls for the status of a Jito bundle until it is processed or a timeout is reached.
pub async fn poll_bundle_status(
    quicknode_url: &str,
    bundle_id: &str,
    poll_interval_ms: u64,
    timeout_secs: u64,
) -> Result<serde_json::Value> {
    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(timeout_secs);

    loop {
        if start_time.elapsed() > timeout_duration {
            return Err(anyhow!(
                "Timeout waiting for bundle {} to be processed",
                bundle_id
            ));
        }

        match get_jito_bundle_status(quicknode_url, bundle_id).await {
            Ok(status_response) => {
                // Assuming the response has a 'result' field with a 'value' array
                if let Some(result) = status_response.get("result") {
                    if let Some(value) = result.get("value").and_then(|v| v.as_array()) {
                        if !value.is_empty() {
                            // A non-empty value array indicates the bundle has been processed.
                            return Ok(status_response);
                        }
                    }
                }
                // If the status is not final, wait before polling again.
                sleep(Duration::from_millis(poll_interval_ms)).await;
            }
            Err(e) => {
                // Don't immediately fail on a single failed request, could be transient.
                log::warn!(
                    "Failed to get bundle status for {}: {}. Retrying...",
                    bundle_id,
                    e
                );
                sleep(Duration::from_millis(poll_interval_ms)).await;
            }
        }
    }
}

/// Selects a random tip account from the provided list.
pub fn select_random_tip_account(tip_accounts: &[String]) -> Result<Pubkey> {
    let mut rng = rand::thread_rng();
    let chosen_account_str = tip_accounts
        .choose(&mut rng)
        .ok_or_else(|| anyhow!("No tip accounts available to choose from."))?;
    Pubkey::from_str(chosen_account_str)
        .map_err(|e| anyhow!("Failed to parse tip account pubkey: {}", e))
}

/// Creates a system transfer instruction to tip the Jito validator.
pub fn create_tip_instruction(
    from_pubkey: &Pubkey,
    tip_lamports: u64,
    tip_account: &Pubkey,
) -> Instruction {
    system_instruction::transfer(from_pubkey, tip_account, tip_lamports)
}
