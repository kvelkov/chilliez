use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// =====================================
///  Permanent Blacklist for Trading Pairs
/// =====================================

/// Blacklist for trading pairs by mint strings (sorted, lowercase).
#[derive(Clone)]
pub struct PairBlacklist {
    set: HashSet<(String, String)>,
}

impl PairBlacklist {
    /// Create new, optionally loading from path.
    pub fn load_from_file(path: Option<&str>) -> Self {
        let mut set = HashSet::new();
        if let Some(file_path) = path {
            if let Ok(file) = File::open(file_path) {
                for line in BufReader::new(file).lines().flatten() {
                    let parts: Vec<_> = line.trim().split(',').collect();
                    if parts.len() == 2 {
                        let mut a = parts[0].to_lowercase();
                        let mut b = parts[1].to_lowercase();
                        if a > b {
                            std::mem::swap(&mut a, &mut b);
                        }
                        set.insert((a, b));
                    }
                }
            }
        }
        Self { set }
    }

    /// New, empty.
    pub fn new() -> Self {
        Self {
            set: HashSet::new(),
        }
    }

    /// Check if pair is blacklisted (unordered, case-insensitive).
    pub fn contains(&self, a: &str, b: &str) -> bool {
        let (mut a, mut b) = (a.to_lowercase(), b.to_lowercase());
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        self.set.contains(&(a, b))
    }

    /// Add pair and optionally persist to disk.
    pub fn insert(&mut self, a: &str, b: &str, path: Option<&str>) {
        let (mut a, mut b) = (a.to_lowercase(), b.to_lowercase());
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        self.set.insert((a.clone(), b.clone()));
        if let Some(file_path) = path {
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(file_path) {
                let _ = writeln!(file, "{},{}", a, b);
            }
        }
    }
}

/// =========================================
///  Temporary Ban List (auto-release after TTL)
/// =========================================
#[derive(Clone)]
pub struct PairTempBan {
    map: HashMap<(String, String), u64>, // (pair) => ban_until (epoch seconds)
    ttl_secs: u64,
}

impl PairTempBan {
    /// Create with TTL window (seconds).
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            map: HashMap::new(),
            ttl_secs,
        }
    }

    /// Optionally load from file. Each record: a,b,until_epoch_secs
    pub fn load_from_file(path: Option<&str>, ttl_secs: u64) -> Self {
        let mut map = HashMap::new();
        if let Some(file_path) = path {
            if let Ok(file) = File::open(file_path) {
                for line in BufReader::new(file).lines().flatten() {
                    let parts: Vec<_> = line.trim().split(',').collect();
                    if parts.len() == 3 {
                        let mut a = parts[0].to_lowercase();
                        let mut b = parts[1].to_lowercase();
                        let until = parts[2].parse::<u64>().unwrap_or(0);
                        if a > b {
                            std::mem::swap(&mut a, &mut b);
                        }
                        map.insert((a, b), until);
                    }
                }
            }
        }
        Self { map, ttl_secs }
    }

    /// Ban a pair until "now + ttl", optionally persist. Idempotent if called again.
    pub fn ban(&mut self, a: &str, b: &str, path: Option<&str>) {
        let (mut a, mut b) = (a.to_lowercase(), b.to_lowercase());
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let until = now + self.ttl_secs;
        self.map.insert((a.clone(), b.clone()), until);
        if let Some(file_path) = path {
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(file_path) {
                let _ = writeln!(file, "{},{},{}", a, b, until);
            }
        }
    }

    /// Check if pair is currently temp-banned (auto release expired).
    pub fn is_banned(&mut self, a: &str, b: &str) -> bool {
        self.release_expired();
        let (mut a, mut b) = (a.to_lowercase(), b.to_lowercase());
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if let Some(&until) = self.map.get(&(a.clone(), b.clone())) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            return until > now;
        }
        false
    }

    /// Purge expired bans. Call before checking or periodically.
    pub fn release_expired(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.map.retain(|_, &mut until| until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_blacklist_basic() {
        let mut bl = PairBlacklist::new();
        assert!(!bl.contains("SOL", "MOON"));
        bl.insert("sol", "moon", None);
        assert!(bl.contains("moon", "sol"));
    }

    #[test]
    fn test_tempban_expiry() {
        let mut tban = PairTempBan::new(1);
        tban.ban("SOL", "DOGE", None);
        assert!(tban.is_banned("doge", "sol"));
        sleep(Duration::from_secs(2));
        assert!(!tban.is_banned("SoL", "dOgE"));
    }
}
