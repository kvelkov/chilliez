//! QuickNode stream integration

use super::solana_stream_filter::{
    current_timestamp, FilteredStreamResult, SolanaStreamFilter, StreamData,
};
use serde_json::json;
use serde_json::Value;

pub fn create_mock_orca_transaction() -> Value {
    json!({
        "transaction": {
            "message": {
                "accountKeys": [
                    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
                    "So11111111111111111111111111111111111111112",
                    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "USER_WALLET_ADDRESS_PLACEHOLDER",
                    "WHIRLPOOL_ADDRESS_PLACEHOLDER"
                ],
                "instructions": [
                    {
                        "programIdIndex": 0,
                        "accounts": [1, 2, 3, 4],
                        "data": "base64_encoded_instruction_data"
                    }
                ]
            },
            "signatures": ["mock_signature_hash_123456789"]
        },
        "meta": {
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "So11111111111111111111111111111111111111112",
                    "uiTokenAmount": {
                        "amount": "1000000000",
                        "decimals": 9,
                        "uiAmount": 1.0
                    }
                }
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 2,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "uiTokenAmount": {
                        "amount": "150000000",
                        "decimals": 6,
                        "uiAmount": 150.0
                    }
                }
            ],
            "innerInstructions": [],
            "err": null
        }
    })
}

pub fn create_mock_jupiter_transaction() -> Value {
    json!({
        "transaction": {
            "message": {
                "accountKeys": [
                    "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB",
                    "So11111111111111111111111111111111111111112",
                    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
                    "USER_WALLET_ADDRESS_PLACEHOLDER"
                ],
                "instructions": [
                    {
                        "programIdIndex": 0,
                        "accounts": [1, 2, 3],
                        "data": "jupiter_swap_instruction_data"
                    }
                ]
            },
            "signatures": ["jupiter_signature_hash_987654321"]
        },
        "meta": {
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "So11111111111111111111111111111111111111112",
                    "uiTokenAmount": {
                        "amount": "500000000",
                        "decimals": 9,
                        "uiAmount": 0.5
                    }
                }
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 2,
                    "mint": "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
                    "uiTokenAmount": {
                        "amount": "75000000",
                        "decimals": 6,
                        "uiAmount": 75.0
                    }
                }
            ],
            "innerInstructions": [
                {
                    "index": 0,
                    "instructions": [
                        {
                            "programIdIndex": 0,
                            "accounts": [1, 2],
                            "data": "inner_instruction_data"
                        }
                    ]
                }
            ],
            "err": null
        }
    })
}

pub fn create_mock_pool_account_change() -> Value {
    json!({
        "pubkey": "MOCK_POOL_ADDRESS_123456789",
        "account": {
            "owner": "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
            "lamports": 1000000000,
            "data": "base64_encoded_pool_state_data",
            "executable": false,
            "rentEpoch": 350
        }
    })
}

pub fn print_filter_result(result: &FilteredStreamResult) {
    println!("📋 Filter Result:");
    println!("  Timestamp: {}", result.timestamp);
    if let Some(slot) = result.slot {
        println!("  Slot: {}", slot);
    }
    println!("  Transactions: {}", result.metadata.total_transactions);
    println!(
        "  Account Changes: {}",
        result.metadata.total_account_changes
    );
    println!("  Instructions: {}", result.metadata.total_instructions);
    println!("  DEX Interactions: {}", result.metadata.dex_interactions);
    println!("  Token Transfers: {}", result.metadata.token_transfers);
    if !result.matching_instructions.is_empty() {
        println!("  🔍 Matching Instructions:");
        for instruction in &result.matching_instructions {
            println!(
                "    • {} ({}): {}",
                instruction.index,
                instruction.program_id,
                instruction
                    .instruction_type
                    .as_ref()
                    .unwrap_or(&"Unknown".to_string())
            );
            if instruction.is_inner {
                println!("      └─ Inner instruction");
            }
        }
    }
}
