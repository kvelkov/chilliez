// src/dex/banned_pairs.rs

use crate::dex::quote::DexClient;
use anyhow::{Context, Result};
use async_trait::async_trait;
use csv::{ReaderBuilder, WriterBuilder as CsvWriterBuilder};
use log;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

/// A canonical key for a token pair, ensuring (A,B) and (B,A) are treated the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BannedPairKey {
    // Lexicographically smaller token mint string
    token1: String,
    // Lexicographically larger token mint string
    token2: String,
}

impl BannedPairKey {
    fn new(token_a: &str, token_b: &str) -> Self {
        if token_a <= token_b {
            BannedPairKey {
                token1: token_a.to_string(),
                token2: token_b.to_string(),
            }
        } else {
            BannedPairKey {
                token1: token_b.to_string(),
                token2: token_a.to_string(),
            }
        }
    }
}

/// Manages the set of banned trading pairs.
#[derive(Debug)]
pub struct BannedPairsManager {
    banned_pairs: HashSet<BannedPairKey>,
    csv_file_path: Box<Path>, // Store the path for persisting new bans
}

impl BannedPairsManager {
    /// Creates a new BannedPairsManager by loading banned pairs from a CSV file.
    /// The CSV file is expected to have headers "TokenA" and "TokenB".
    pub fn new(csv_file_path: &Path) -> Result<Self> {
        log::info!("Loading banned pairs from: {:?}", csv_file_path);
        let file = File::open(csv_file_path)
            .with_context(|| format!("Failed to open banned pairs CSV file: {:?}", csv_file_path))?;
        let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(BufReader::new(file));

        let mut banned_pairs_set = HashSet::new();
        let headers = rdr.headers()?.clone();
        let token_a_idx = headers.iter().position(|h| h.trim().eq_ignore_ascii_case("TokenA")).context("CSV missing 'TokenA' header")?;
        let token_b_idx = headers.iter().position(|h| h.trim().eq_ignore_ascii_case("TokenB")).context("CSV missing 'TokenB' header")?;

        for result in rdr.records() {
            let record = result.with_context(|| format!("Failed to read record from banned pairs CSV: {:?}", csv_file_path))?;
            let token_a = record.get(token_a_idx).context("Missing TokenA value in record")?.trim();
            let token_b = record.get(token_b_idx).context("Missing TokenB value in record")?.trim();
            if !token_a.is_empty() && !token_b.is_empty() {
                banned_pairs_set.insert(BannedPairKey::new(token_a, token_b));
            } else {
                log::warn!("Skipping empty token mint in banned pairs log: {:?}", record);
            }
        }
        log::info!("Loaded {} unique banned pairs.", banned_pairs_set.len());
        Ok(Self {
            banned_pairs: banned_pairs_set,
            csv_file_path: csv_file_path.into(),
        })
    }

    /// Checks if a given token pair is banned.
    pub fn is_banned(&self, token_a: &str, token_b: &str) -> bool {
        let key = BannedPairKey::new(token_a, token_b);
        self.banned_pairs.contains(&key)
    }

    /// Adds a pair to the banned list and persists it to the CSV file.
    /// Returns `Ok(true)` if the pair was newly banned, `Ok(false)` if it was already banned.
    pub fn ban_pair_and_persist(
        &mut self,
        token_a: &str,
        token_b: &str,
        ban_type: &str,
        details: &str,
    ) -> Result<bool> {
        let key = BannedPairKey::new(token_a, token_b);
        if self.banned_pairs.contains(&key) {
            log::debug!("Pair {}/{} is already banned in memory. No CSV update needed.", token_a, token_b);
            return Ok(false); // Already banned
        }

        // Add to in-memory set
        self.banned_pairs.insert(key);

        // Append to CSV
        // Check if file exists to determine if headers are needed
        let file_exists_and_not_empty = self.csv_file_path.exists() && self.csv_file_path.metadata().map(|m| m.len() > 0).unwrap_or(false);

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.csv_file_path)
            .with_context(|| format!("Failed to open/create banned pairs CSV for appending: {:?}", self.csv_file_path))?;

        let mut wtr = CsvWriterBuilder::new()
            .has_headers(!file_exists_and_not_empty) // Write headers only if file is new/empty
            .from_writer(file);

        if !file_exists_and_not_empty {
            wtr.write_record(&["TokenA", "TokenB", "BanType", "Details"])
                .with_context(|| format!("Failed to write headers to banned pairs CSV: {:?}", self.csv_file_path))?;
        }

        wtr.write_record(&[token_a, token_b, ban_type, details])
            .with_context(|| format!("Failed to write record to banned pairs CSV: {:?}", self.csv_file_path))?;
        wtr.flush().with_context(|| format!("Failed to flush banned pairs CSV writer for: {:?}", self.csv_file_path))?;

        log::info!("Newly banned pair {}/{} and persisted to CSV.", token_a, token_b);
        Ok(true) // Newly banned
    }

    /// Returns a vector of all banned pairs.
    pub fn all_banned_pairs(&self) -> Vec<(String, String)> {
        self.banned_pairs.iter().map(|k| (k.token1.clone(), k.token2.clone())).collect()
    }

    /// Clears all banned pairs (and the CSV file).
    pub fn clear_all(&mut self) -> Result<()> {
        self.banned_pairs.clear();
        // Truncate the CSV file
        std::fs::write(&self.csv_file_path, "TokenA,TokenB,BanType,Details\n")?;
        Ok(())
    }
}

pub struct BannedPairFilteringDexClientDecorator {
    pub inner_client: Box<dyn DexClient>,
    #[allow(dead_code)] // Remove unused field warning for banned_pairs_manager by marking as #[allow(dead_code)]
    pub banned_pairs_manager: Arc<tokio::sync::RwLock<BannedPairsManager>>,
}

impl BannedPairFilteringDexClientDecorator {
    pub fn new(
        inner_client: Box<dyn DexClient>,
        banned_pairs_manager: Arc<tokio::sync::RwLock<BannedPairsManager>>,
    ) -> Self {
        Self { inner_client, banned_pairs_manager }
    }
}

#[async_trait]
impl DexClient for BannedPairFilteringDexClientDecorator {
    fn get_name(&self) -> &str {
        self.inner_client.get_name()
    }
    fn calculate_onchain_quote(
        &self,
        pool: &crate::utils::PoolInfo,
        input_amount: u64,
    ) -> anyhow::Result<crate::dex::quote::Quote> {
        // Filtering logic can be added here if needed
        self.inner_client.calculate_onchain_quote(pool, input_amount)
    }
    fn get_swap_instruction(
        &self,
        swap_info: &crate::dex::quote::SwapInfo,
    ) -> anyhow::Result<solana_sdk::instruction::Instruction> {
        self.inner_client.get_swap_instruction(swap_info)
    }
}

// Public dummy DexClient for integration example
pub struct DummyDexClient;
impl DexClient for DummyDexClient {
    fn get_name(&self) -> &str { "Dummy" }
    fn calculate_onchain_quote(&self, _pool: &crate::utils::PoolInfo, _input_amount: u64) -> anyhow::Result<crate::dex::quote::Quote> {
        Err(anyhow::anyhow!("not implemented"))
    }
    fn get_swap_instruction(&self, _swap_info: &crate::dex::quote::SwapInfo) -> anyhow::Result<solana_sdk::instruction::Instruction> {
        Err(anyhow::anyhow!("not implemented"))
    }
}

// Example integration: create and use the banned pairs manager and decorator in a real function
#[allow(dead_code)]
pub fn example_banned_pairs_usage() -> Result<()> {
    let csv_path = std::path::Path::new("banned_pairs_log.csv");
    let mut manager = BannedPairsManager::new(csv_path)?;
    manager.ban_pair_and_persist("USDC", "SOL", "manual", "test reason")?;
    let _banned = manager.is_banned("USDC", "SOL");
    let _all = manager.all_banned_pairs();
    manager.clear_all()?;
    let arc_manager = std::sync::Arc::new(tokio::sync::RwLock::new(manager));
    let dummy = Box::new(DummyDexClient);
    let _decorator = BannedPairFilteringDexClientDecorator::new(dummy, arc_manager);
    Ok(())
}

/// Integrate banned pairs system in a real (non-test) context, to be called from main.rs
pub fn integrate_banned_pairs_system() -> Result<()> {
    let csv_path = std::path::Path::new("banned_pairs_log.csv");
    let mut manager = BannedPairsManager::new(csv_path)?;
    // Ban a pair and persist
    manager.ban_pair_and_persist("USDC", "SOL", "manual", "integration usage")?;
    // Check if a pair is banned
    let _banned = manager.is_banned("USDC", "SOL");
    // List all banned pairs
    let _all = manager.all_banned_pairs();
    // Clear all bans
    manager.clear_all()?;
    // Wrap in Arc<RwLock> for decorator usage
    let arc_manager = std::sync::Arc::new(tokio::sync::RwLock::new(manager));
    let dummy = Box::new(DummyDexClient);
    let _decorator = BannedPairFilteringDexClientDecorator::new(dummy, arc_manager);
    Ok(())
}
