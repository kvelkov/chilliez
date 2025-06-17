// src/streams/solana_stream_filter.rs
//! Solana Stream Filter for QuickNode Streams
//! Filters transactions and account changes for arbitrage opportunities

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::str::FromStr;
use anyhow::Result;

/// Known DEX program IDs to monitor
pub const DEX_PROGRAMS: &[&str] = &[
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", // Orca Whirlpools
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium AMM
    "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB",  // Jupiter
    "MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky",  // Meteora
    "2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c", // Lifinity
];

/// Important token mints to monitor
pub const MONITORED_TOKENS: &[&str] = &[
    "So11111111111111111111111111111111111111112", // SOL
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", // USDC
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", // USDT
    "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",  // mSOL
    "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj", // stSOL
];

/// Stream data structure
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamData {
    pub timestamp: Option<u64>,
    pub slot: Option<u64>,
    pub transaction: Option<Value>,
    pub transactions: Option<Vec<Value>>,
    pub account: Option<Value>,
    pub accounts: Option<Vec<Value>>,
    pub block: Option<Value>,
}

/// Filtered stream result
#[derive(Debug, Serialize, Deserialize)]
pub struct FilteredStreamResult {
    pub timestamp: u64,
    pub slot: Option<u64>,
    pub matching_transactions: Vec<Value>,
    pub matching_account_changes: Vec<Value>,
    pub matching_instructions: Vec<InstructionMatch>,
    pub metadata: FilterMetadata,
}

/// Instruction match details
#[derive(Debug, Serialize, Deserialize)]
pub struct InstructionMatch {
    pub index: String,
    pub program_id: String,
    pub accounts: Option<Vec<u8>>,
    pub data: Option<String>,
    pub is_inner: bool,
    pub transaction_signature: Option<String>,
    pub instruction_type: Option<String>,
}

/// Filter metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct FilterMetadata {
    pub total_transactions: usize,
    pub total_account_changes: usize,
    pub total_instructions: usize,
    pub dex_interactions: usize,
    pub token_transfers: usize,
}

/// Solana stream filter
pub struct SolanaStreamFilter {
    watched_addresses: HashSet<String>,
    dex_programs: HashSet<String>,
    monitored_tokens: HashSet<String>,
}

impl SolanaStreamFilter {
    /// Create a new stream filter with default configuration
    pub fn new() -> Self {
        let mut watched_addresses = HashSet::new();
        let mut dex_programs = HashSet::new();
        let mut monitored_tokens = HashSet::new();

        // Add DEX programs
        for program in DEX_PROGRAMS {
            dex_programs.insert(program.to_lowercase());
            watched_addresses.insert(program.to_lowercase());
        }

        // Add monitored tokens
        for token in MONITORED_TOKENS {
            monitored_tokens.insert(token.to_lowercase());
            watched_addresses.insert(token.to_lowercase());
        }

        Self {
            watched_addresses,
            dex_programs,
            monitored_tokens,
        }
    }

    /// Add custom addresses to monitor
    pub fn add_watched_address(&mut self, address: &str) -> Result<()> {
        // Validate the address format
        Pubkey::from_str(address)?;
        self.watched_addresses.insert(address.to_lowercase());
        Ok(())
    }

    /// Add custom program to monitor
    pub fn add_dex_program(&mut self, program_id: &str) -> Result<()> {
        // Validate the program ID format
        Pubkey::from_str(program_id)?;
        self.dex_programs.insert(program_id.to_lowercase());
        self.watched_addresses.insert(program_id.to_lowercase());
        Ok(())
    }

    /// Filter stream data for relevant transactions and account changes
    pub fn filter_stream(&self, data: StreamData) -> Result<Option<FilteredStreamResult>> {
        let mut matching_transactions = Vec::new();
        let mut matching_account_changes = Vec::new();
        let mut matching_instructions = Vec::new();
        let mut dex_interactions = 0;
        let mut token_transfers = 0;

        // Process single transaction
        if let Some(transaction) = &data.transaction {
            let result = self.process_transaction(transaction)?;
            if result.matches {
                matching_transactions.push(transaction.clone());
                matching_instructions.extend(result.instructions);
                if result.is_dex_interaction {
                    dex_interactions += 1;
                }
                if result.has_token_transfer {
                    token_transfers += 1;
                }
            }
        }

        // Process multiple transactions
        if let Some(transactions) = &data.transactions {
            for transaction in transactions {
                let result = self.process_transaction(transaction)?;
                if result.matches {
                    matching_transactions.push(transaction.clone());
                    matching_instructions.extend(result.instructions);
                    if result.is_dex_interaction {
                        dex_interactions += 1;
                    }
                    if result.has_token_transfer {
                        token_transfers += 1;
                    }
                }
            }
        }

        // Process single account change
        if let Some(account) = &data.account {
            let result = self.process_account_change(account)?;
            if result.matches {
                matching_account_changes.push(account.clone());
            }
        }

        // Process multiple account changes
        if let Some(accounts) = &data.accounts {
            for account in accounts {
                let result = self.process_account_change(account)?;
                if result.matches {
                    matching_account_changes.push(account.clone());
                }
            }
        }

        // Check if we have any matches
        if matching_transactions.is_empty() 
            && matching_account_changes.is_empty() 
            && matching_instructions.is_empty() {
            return Ok(None);
        }

        // Calculate lengths before moving the vectors
        let total_transactions = matching_transactions.len();
        let total_account_changes = matching_account_changes.len();
        let total_instructions = matching_instructions.len();

        Ok(Some(FilteredStreamResult {
            timestamp: chrono::Utc::now().timestamp() as u64,
            slot: data.slot,
            matching_transactions,
            matching_account_changes,
            matching_instructions,
            metadata: FilterMetadata {
                total_transactions,
                total_account_changes,
                total_instructions,
                dex_interactions,
                token_transfers,
            },
        }))
    }

    /// Process a single transaction for matches
    fn process_transaction(&self, transaction: &Value) -> Result<TransactionProcessResult> {
        let mut matches = false;
        let mut matching_instructions = Vec::new();
        let mut is_dex_interaction = false;
        let mut has_token_transfer = false;

        // Extract transaction data
        if let Some(tx_data) = transaction.get("transaction") {
            if let Some(message) = tx_data.get("message") {
                // Check account keys
                if let Some(account_keys) = message.get("accountKeys").and_then(|v| v.as_array()) {
                    for account in account_keys {
                        let account_str = if let Some(s) = account.as_str() {
                            s
                        } else if let Some(obj) = account.as_object() {
                            obj.get("pubkey").and_then(|v| v.as_str()).unwrap_or("")
                        } else {
                            ""
                        };

                        if !account_str.is_empty() && self.watched_addresses.contains(&account_str.to_lowercase()) {
                            matches = true;
                            if self.dex_programs.contains(&account_str.to_lowercase()) {
                                is_dex_interaction = true;
                            }
                            if self.monitored_tokens.contains(&account_str.to_lowercase()) {
                                has_token_transfer = true;
                            }
                        }
                    }
                }

                // Check instructions
                if let Some(instructions) = message.get("instructions").and_then(|v| v.as_array()) {
                    for (index, instruction) in instructions.iter().enumerate() {
                        if let Some(program_id_index) = instruction.get("programIdIndex").and_then(|v| v.as_u64()) {
                            if let Some(account_keys) = message.get("accountKeys").and_then(|v| v.as_array()) {
                                if let Some(program_account) = account_keys.get(program_id_index as usize) {
                                    let program_id = if let Some(s) = program_account.as_str() {
                                        s
                                    } else if let Some(obj) = program_account.as_object() {
                                        obj.get("pubkey").and_then(|v| v.as_str()).unwrap_or("")
                                    } else {
                                        ""
                                    };

                                    if !program_id.is_empty() && self.dex_programs.contains(&program_id.to_lowercase()) {
                                        matches = true;
                                        is_dex_interaction = true;

                                        matching_instructions.push(InstructionMatch {
                                            index: index.to_string(),
                                            program_id: program_id.to_string(),
                                            accounts: instruction.get("accounts").and_then(|v| v.as_array()).map(|arr| {
                                                arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect()
                                            }),
                                            data: instruction.get("data").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                            is_inner: false,
                                            transaction_signature: tx_data.get("signatures")
                                                .and_then(|v| v.as_array())
                                                .and_then(|arr| arr.get(0))
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            instruction_type: self.identify_instruction_type(program_id, instruction),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check meta for inner instructions
            if let Some(meta) = transaction.get("meta") {
                if let Some(inner_instructions) = meta.get("innerInstructions").and_then(|v| v.as_array()) {
                    for inner_group in inner_instructions {
                        if let Some(group_index) = inner_group.get("index").and_then(|v| v.as_u64()) {
                            if let Some(instructions) = inner_group.get("instructions").and_then(|v| v.as_array()) {
                                for (inner_index, instruction) in instructions.iter().enumerate() {
                                    if let Some(program_id_index) = instruction.get("programIdIndex").and_then(|v| v.as_u64()) {
                                        if let Some(message) = tx_data.get("message") {
                                            if let Some(account_keys) = message.get("accountKeys").and_then(|v| v.as_array()) {
                                                if let Some(program_account) = account_keys.get(program_id_index as usize) {
                                                    let program_id = if let Some(s) = program_account.as_str() {
                                                        s
                                                    } else if let Some(obj) = program_account.as_object() {
                                                        obj.get("pubkey").and_then(|v| v.as_str()).unwrap_or("")
                                                    } else {
                                                        ""
                                                    };

                                                    if !program_id.is_empty() && self.dex_programs.contains(&program_id.to_lowercase()) {
                                                        matches = true;
                                                        is_dex_interaction = true;

                                                        matching_instructions.push(InstructionMatch {
                                                            index: format!("inner_{}_{}", group_index, inner_index),
                                                            program_id: program_id.to_string(),
                                                            accounts: instruction.get("accounts").and_then(|v| v.as_array()).map(|arr| {
                                                                arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect()
                                                            }),
                                                            data: instruction.get("data").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                                            is_inner: true,
                                                            transaction_signature: tx_data.get("signatures")
                                                                .and_then(|v| v.as_array())
                                                                .and_then(|arr| arr.get(0))
                                                                .and_then(|v| v.as_str())
                                                                .map(|s| s.to_string()),
                                                            instruction_type: self.identify_instruction_type(program_id, instruction),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Check token balances
                if let Some(pre_balances) = meta.get("preTokenBalances").and_then(|v| v.as_array()) {
                    for balance in pre_balances {
                        if let Some(mint) = balance.get("mint").and_then(|v| v.as_str()) {
                            if self.monitored_tokens.contains(&mint.to_lowercase()) {
                                matches = true;
                                has_token_transfer = true;
                            }
                        }
                    }
                }

                if let Some(post_balances) = meta.get("postTokenBalances").and_then(|v| v.as_array()) {
                    for balance in post_balances {
                        if let Some(mint) = balance.get("mint").and_then(|v| v.as_str()) {
                            if self.monitored_tokens.contains(&mint.to_lowercase()) {
                                matches = true;
                                has_token_transfer = true;
                            }
                        }
                    }
                }
            }
        }

        Ok(TransactionProcessResult {
            matches,
            instructions: matching_instructions,
            is_dex_interaction,
            has_token_transfer,
        })
    }

    /// Process account change for matches
    fn process_account_change(&self, account: &Value) -> Result<AccountProcessResult> {
        let mut matches = false;

        // Check account pubkey
        if let Some(pubkey) = account.get("pubkey").and_then(|v| v.as_str()) {
            if self.watched_addresses.contains(&pubkey.to_lowercase()) {
                matches = true;
            }
        }

        // Check account owner
        if let Some(account_data) = account.get("account") {
            if let Some(owner) = account_data.get("owner").and_then(|v| v.as_str()) {
                if self.dex_programs.contains(&owner.to_lowercase()) {
                    matches = true;
                }
            }
        }

        Ok(AccountProcessResult { matches })
    }

    /// Identify the type of instruction based on program and data
    fn identify_instruction_type(&self, program_id: &str, _instruction: &Value) -> Option<String> {
        match program_id {
            "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM" => Some("Orca Whirlpool".to_string()),
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => Some("Raydium AMM".to_string()),
            "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB" => Some("Jupiter Swap".to_string()),
            "MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky" => Some("Meteora".to_string()),
            "2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c" => Some("Lifinity".to_string()),
            _ => None,
        }
    }
}

impl Default for SolanaStreamFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Transaction processing result
struct TransactionProcessResult {
    matches: bool,
    instructions: Vec<InstructionMatch>,
    is_dex_interaction: bool,
    has_token_transfer: bool,
}

/// Account processing result
struct AccountProcessResult {
    matches: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_filter_creation() {
        let filter = SolanaStreamFilter::new();
        assert!(!filter.watched_addresses.is_empty());
        assert!(!filter.dex_programs.is_empty());
        assert!(!filter.monitored_tokens.is_empty());
    }

    #[test]
    fn test_add_watched_address() {
        let mut filter = SolanaStreamFilter::new();
        let test_address = "11111111111111111111111111111111";
        
        assert!(filter.add_watched_address(test_address).is_ok());
        assert!(filter.watched_addresses.contains(&test_address.to_lowercase()));
    }

    #[test]
    fn test_filter_dex_transaction() {
        let filter = SolanaStreamFilter::new();
        
        let mock_transaction = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM" // Orca Whirlpools
                    ],
                    "instructions": []
                },
                "signatures": ["test_signature"]
            },
            "meta": {}
        });

        let stream_data = StreamData {
            timestamp: None,
            slot: None,
            transaction: Some(mock_transaction),
            transactions: None,
            account: None,
            accounts: None,
            block: None,
        };

        let result = filter.filter_stream(stream_data).unwrap();
        assert!(result.is_some());
        
        let filtered = result.unwrap();
        assert_eq!(filtered.matching_transactions.len(), 1);
        assert_eq!(filtered.metadata.dex_interactions, 1);
    }
}
