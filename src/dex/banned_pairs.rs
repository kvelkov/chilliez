// src/dex/banned_pairs.rs

use crate::dex::quote::{DexClient, Quote};
use anyhow::{anyhow, Context, Result};
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
    _banned_pairs: HashSet<BannedPairKey>,
    _csv_file_path: Box<Path>, // Store the path for persisting new bans
}

impl BannedPairsManager {
    /// Creates a new BannedPairsManager by loading banned pairs from a CSV file.
    /// The CSV file is expected to have headers "TokenA" and "TokenB".
    pub fn _new(csv_file_path: &Path) -> Result<Self> {
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
            _banned_pairs: banned_pairs_set,
            _csv_file_path: csv_file_path.into(),
        })
    }

    /// Checks if a given token pair is banned.
    pub fn is_banned(&self, token_a: &str, token_b: &str) -> bool {
        let key = BannedPairKey::new(token_a, token_b);
        self._banned_pairs.contains(&key)
    }

    /// Adds a pair to the banned list and persists it to the CSV file.
    /// Returns `Ok(true)` if the pair was newly banned, `Ok(false)` if it was already banned.
    pub fn _ban_pair_and_persist(
        &mut self,
        token_a: &str,
        token_b: &str,
        ban_type: &str,
        details: &str,
    ) -> Result<bool> {
        let _key = BannedPairKey::new(token_a, token_b);
        if self._banned_pairs.contains(&_key) {
            log::debug!("Pair {}/{} is already banned in memory. No CSV update needed.", token_a, token_b);
            return Ok(false); // Already banned
        }

        // Add to in-memory set
        self._banned_pairs.insert(_key);

        // Append to CSV
        // Check if file exists to determine if headers are needed
        let file_exists_and_not_empty = self._csv_file_path.exists() && self._csv_file_path.metadata().map(|m| m.len() > 0).unwrap_or(false);

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self._csv_file_path)
            .with_context(|| format!("Failed to open/create banned pairs CSV for appending: {:?}", self._csv_file_path))?;

        let mut wtr = CsvWriterBuilder::new()
            .has_headers(!file_exists_and_not_empty) // Write headers only if file is new/empty
            .from_writer(file);

        if !file_exists_and_not_empty {
            wtr.write_record(&["TokenA", "TokenB", "BanType", "Details"])
                .with_context(|| format!("Failed to write headers to banned pairs CSV: {:?}", self._csv_file_path))?;
        }

        wtr.write_record(&[token_a, token_b, ban_type, details])
            .with_context(|| format!("Failed to write record to banned pairs CSV: {:?}", self._csv_file_path))?;
        wtr.flush().with_context(|| format!("Failed to flush banned pairs CSV writer for: {:?}", self._csv_file_path))?;

        log::info!("Newly banned pair {}/{} and persisted to CSV.", token_a, token_b);
        Ok(true) // Newly banned
    }
}

/// A decorator for `DexClient` that filters out banned pairs.
pub struct BannedPairFilteringDexClientDecorator {
    inner_client: Box<dyn DexClient>,
    banned_pairs_manager: Arc<tokio::sync::RwLock<BannedPairsManager>>, // Changed to RwLock
}

impl BannedPairFilteringDexClientDecorator {
    pub fn _new(
        inner_client: Box<dyn DexClient>,
        banned_pairs_manager: Arc<tokio::sync::RwLock<BannedPairsManager>>,
    ) -> Self {
        Self { inner_client, banned_pairs_manager }
    }
}

#[async_trait]
impl DexClient for BannedPairFilteringDexClientDecorator {
    async fn get_best_swap_quote(
        &self,
        input_token_mint: &str,
        output_token_mint: &str,
        amount_in_atomic_units: u64,
    ) -> Result<Quote> {
        // Acquire read lock to check
        if self.banned_pairs_manager.read().await.is_banned(input_token_mint, output_token_mint) {
            log::debug!(
                "Pair {}/{} is banned. Skipping quote fetch for DEX: {}.",
                input_token_mint, output_token_mint, self.inner_client.get_name()
            );
            return Err(anyhow!(
                "Pair {}/{} is banned and will not be quoted by {}",
                input_token_mint, output_token_mint, self.inner_client.get_name()
            ));
        }
        self.inner_client.get_best_swap_quote(input_token_mint, output_token_mint, amount_in_atomic_units).await
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        // Could also filter these, but the primary check is in get_best_swap_quote
        self.inner_client.get_supported_pairs()
    }

    fn get_name(&self) -> &str {
        self.inner_client.get_name() // Or you could prepend "Filtered-" to the name
    }
}
