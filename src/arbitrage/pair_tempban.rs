use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Holds (pair tuple → ban_until timestamp, epoch seconds); released when expired.
pub struct PairTempBan {
    map: HashMap<(String, String), u64>,
    ttl_secs: u64,
}

impl PairTempBan {
    /// Create with a TTL duration in seconds (e.g. 24h = 86400).
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            map: HashMap::new(),
            ttl_secs,
        }
    }

    /// Optionally load old bans from disk.
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

    /// Ban a pair until now + ttl, optionally also persist to disk.
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

    /// Check if pair is currently temp-banned; flushes expired before check.
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
            if until > now {
                return true;
            }
        }
        false
    }

    /// Remove expired entries from the list (call periodically or before most checks).
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
    #[test]
    fn test_tempban_expiry() {
        let mut tban = PairTempBan::new(1);
        tban.ban("SOL", "DOGE", None);
        assert!(tban.is_banned("doge", "sol"));
        std::thread::sleep(Duration::from_secs(2));
        assert!(!tban.is_banned("SoL", "dOgE"));
    }
}
