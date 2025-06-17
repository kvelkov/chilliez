// src/streams/quicknode.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct QuickNodeEvent {
    pub data: Vec<BlockData>,
}

#[derive(Debug, Deserialize)]
pub struct BlockData {
    #[serde(rename = "blockHeight")]
    pub block_height: u64,
    #[serde(rename = "blockTime")]
    pub block_time: i64,
    pub blockhash: String,
    #[serde(rename = "parentSlot")]
    pub parent_slot: u64,
    #[serde(rename = "previousBlockhash")]
    pub previous_blockhash: String,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub meta: Option<TransactionMeta>,
    pub transaction: Option<TransactionData>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionMeta {
    #[serde(rename = "computeUnitsConsumed")]
    pub compute_units_consumed: Option<u64>,
    pub err: Option<serde_json::Value>,
    pub fee: u64,
    #[serde(rename = "innerInstructions")]
    pub inner_instructions: Option<Vec<InnerInstruction>>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
    #[serde(rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(rename = "postBalances")]
    pub post_balances: Vec<u64>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionData {
    pub message: Message,
    pub signatures: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[serde(rename = "accountKeys")]
    pub account_keys: Vec<String>,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Deserialize)]
pub struct Instruction {
    pub accounts: Vec<u8>,
    pub data: String,
    #[serde(rename = "programIdIndex")]
    pub program_id_index: u8,
    #[serde(rename = "stackHeight")]
    pub stack_height: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct InnerInstruction {
    pub index: u8,
    pub instructions: Vec<ParsedInstruction>,
}

#[derive(Debug, Deserialize)]
pub struct ParsedInstruction {
    pub parsed: Option<serde_json::Value>,
    pub program: Option<String>,
    #[serde(rename = "programId")]
    pub program_id: String,
    #[serde(rename = "stackHeight")]
    pub stack_height: Option<u8>,
    pub accounts: Option<Vec<String>>,
    pub data: Option<String>,
}

// DEX-specific data structures for your arbitrage bot
#[derive(Debug, Clone)]
pub struct DexSwapEvent {
    pub signature: String,
    pub block_slot: u64,
    pub block_time: i64,
    pub dex_program: String,
    pub swap_accounts: Vec<String>,
    pub token_amounts: TokenAmounts,
    pub fee_lamports: u64,
}

#[derive(Debug, Clone)]
pub struct TokenAmounts {
    pub token_a_before: u64,
    pub token_a_after: u64,
    pub token_b_before: u64,
    pub token_b_after: u64,
}

impl QuickNodeEvent {
    /// Extract DEX swap events from QuickNode webhook data
    pub fn extract_dex_swaps(&self) -> Vec<DexSwapEvent> {
        let mut swaps = Vec::new();
        
        // DEX program IDs to monitor
        let dex_programs = [
            "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin", // Orca
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc", // Whirlpool
            "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB", // Meteora
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium CLMM
        ];

        for block in &self.data {
            for tx in &block.transactions {
                // Skip failed transactions
                if let Some(meta) = &tx.meta {
                    if meta.err.is_some() {
                        continue;
                    }
                }

                if let Some(tx_data) = &tx.transaction {
                    // Check if transaction involves DEX programs
                    for instruction in &tx_data.message.instructions {
                        let program_id_index = instruction.program_id_index as usize;
                        if program_id_index < tx_data.message.account_keys.len() {
                            let program_id = &tx_data.message.account_keys[program_id_index];
                            
                            if dex_programs.contains(&program_id.as_str()) {
                                // Extract swap data
                                if let Some(swap) = self.parse_dex_swap(
                                    block,
                                    tx,
                                    tx_data,
                                    program_id,
                                    instruction
                                ) {
                                    swaps.push(swap);
                                }
                            }
                        }
                    }
                }
            }
        }

        swaps
    }

    fn parse_dex_swap(
        &self,
        block: &BlockData,
        tx: &Transaction,
        tx_data: &TransactionData,
        program_id: &str,
        instruction: &Instruction,
    ) -> Option<DexSwapEvent> {
        let meta = tx.meta.as_ref()?;
        let signature = tx_data.signatures.first()?.clone();

        // Extract account addresses involved in the swap
        let mut swap_accounts = Vec::new();
        for &account_index in &instruction.accounts {
            if (account_index as usize) < tx_data.message.account_keys.len() {
                swap_accounts.push(tx_data.message.account_keys[account_index as usize].clone());
            }
        }

        // Calculate token amounts from balance changes
        let token_amounts = self.calculate_token_amounts(&meta.pre_balances, &meta.post_balances);

        Some(DexSwapEvent {
            signature,
            block_slot: block.parent_slot, // Use parent slot as the actual slot
            block_time: block.block_time,
            dex_program: program_id.to_string(),
            swap_accounts,
            token_amounts,
            fee_lamports: meta.fee,
        })
    }

    fn calculate_token_amounts(&self, pre_balances: &[u64], post_balances: &[u64]) -> TokenAmounts {
        // Simplified token amount calculation
        // In a real implementation, you'd parse the instruction data and log messages
        // to get exact token amounts
        
        let mut token_a_before = 0;
        let mut token_a_after = 0;
        let mut token_b_before = 0;
        let mut token_b_after = 0;

        // Find the accounts with balance changes (simplified)
        for i in 0..pre_balances.len().min(post_balances.len()) {
            if pre_balances[i] != post_balances[i] {
                if token_a_before == 0 {
                    token_a_before = pre_balances[i];
                    token_a_after = post_balances[i];
                } else if token_b_before == 0 {
                    token_b_before = pre_balances[i];
                    token_b_after = post_balances[i];
                    break;
                }
            }
        }

        TokenAmounts {
            token_a_before,
            token_a_after,
            token_b_before,
            token_b_after,
        }
    }
}

// Integration with your existing arbitrage system
impl DexSwapEvent {
    /// Convert to your existing PoolInfo format for arbitrage detection
    pub fn to_pool_update(&self) -> Option<crate::utils::PoolInfo> {
        // This would integrate with your existing pool info structure
        // Implementation depends on your current PoolInfo format
        None // Placeholder
    }

    /// Check if this swap represents a significant price movement
    pub fn is_significant_price_change(&self, threshold_bps: u64) -> bool {
        let token_a_change = if self.token_amounts.token_a_after > self.token_amounts.token_a_before {
            self.token_amounts.token_a_after - self.token_amounts.token_a_before
        } else {
            self.token_amounts.token_a_before - self.token_amounts.token_a_after
        };

        let token_a_change_bps = if self.token_amounts.token_a_before > 0 {
            (token_a_change * 10_000) / self.token_amounts.token_a_before
        } else {
            0
        };

        token_a_change_bps >= threshold_bps
    }
}
