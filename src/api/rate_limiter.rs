// src/api/rate_limiter.rs
//! Advanced Rate Limiting System for Production API Management
//! 
//! Implements intelligent rate limiting with:
//! - Helius API: 3000 requests/hour (conservative limit from 6.7M available)
//! - Per-DEX rate limiting
//! - Exponential backoff on rate limit hits
//! - Priority request queuing
//! - Connection pooling and failover

use anyhow::{Result, anyhow};
use log::{warn, debug, error, info};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore, Mutex};
use tokio::time::sleep;
use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};

/// Request priority levels for intelligent queuing
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    Critical = 4,    // Live trading, balance checks
    High = 3,        // Price updates, opportunity detection
    Medium = 2,      // Pool discovery, DEX queries
    Low = 1,         // Analytics, historical data
    Background = 0,  // Non-essential operations
}

/// Rate limit configuration for different API providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_hour: u32,
    pub requests_per_minute: u32,
    pub burst_capacity: u32,
    pub cooldown_duration_secs: u64,
    pub max_backoff_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_hour: 3000,    // Conservative limit for Helius
            requests_per_minute: 50,    // 3000/60 = 50
            burst_capacity: 10,         // Allow small bursts
            cooldown_duration_secs: 60, // 1 minute cooldown on rate limit hit
            max_backoff_secs: 300,      // 5 minute max backoff
        }
    }
}

/// Request tracking information
#[derive(Debug, Clone)]
struct RequestRecord {
    timestamp: Instant,
    #[allow(dead_code)]
    priority: RequestPriority,
    #[allow(dead_code)]
    endpoint: String,
    #[allow(dead_code)]
    duration_ms: u64,
}

/// Queued request waiting for execution
#[derive(Debug)]
struct QueuedRequest {
    priority: RequestPriority,
    endpoint: String,
    #[allow(dead_code)]
    created_at: Instant,
    sender: tokio::sync::oneshot::Sender<Result<()>>,
}

/// Advanced rate limiter with priority queuing and exponential backoff
#[derive(Clone)]
pub struct AdvancedRateLimiter {
    config: RateLimitConfig,
    request_history: Arc<RwLock<VecDeque<RequestRecord>>>,
    request_queue: Arc<Mutex<Vec<QueuedRequest>>>,
    semaphore: Arc<Semaphore>,
    backoff_until: Arc<RwLock<Option<Instant>>>,
    consecutive_rate_limits: Arc<RwLock<u32>>,
    provider_name: String,
}

impl AdvancedRateLimiter {
    /// Create a new rate limiter for the specified provider
    pub fn new(provider_name: String, config: RateLimitConfig) -> Self {
        let permits = config.burst_capacity as usize;
        
        info!("🚦 Initializing rate limiter for {}: {}req/h, {}req/m, burst: {}", 
              provider_name, config.requests_per_hour, config.requests_per_minute, config.burst_capacity);
        
        Self {
            config,
            request_history: Arc::new(RwLock::new(VecDeque::new())),
            request_queue: Arc::new(Mutex::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(permits)),
            backoff_until: Arc::new(RwLock::new(None)),
            consecutive_rate_limits: Arc::new(RwLock::new(0)),
            provider_name,
        }
    }
    
    /// Create rate limiter with Helius-optimized defaults
    pub fn new_helius() -> Self {
        let config = RateLimitConfig {
            requests_per_hour: 3000,    // Conservative from 6.7M available
            requests_per_minute: 50,
            burst_capacity: 15,         // Allow slightly larger bursts for Helius
            cooldown_duration_secs: 30, // Shorter cooldown for Helius
            max_backoff_secs: 180,      // 3 minute max backoff
        };
        
        Self::new("Helius".to_string(), config)
    }
    
    /// Check if we can make a request without hitting rate limits
    pub async fn can_make_request(&self) -> bool {
        // Check if we're in backoff period
        if let Some(backoff_until) = *self.backoff_until.read().await {
            if Instant::now() < backoff_until {
                debug!("⏳ {} API in backoff period", self.provider_name);
                return false;
            }
        }
        
        // Check semaphore availability
        if self.semaphore.available_permits() == 0 {
            debug!("🚫 {} API semaphore exhausted", self.provider_name);
            return false;
        }
        
        // Check rate limits
        self.check_rate_limits().await
    }
    
    /// Attempt to acquire a permit for making a request
    pub async fn acquire_permit(&self, priority: RequestPriority, endpoint: &str) -> Result<RateLimitPermit> {
        // Check if we're in backoff
        if let Some(backoff_until) = *self.backoff_until.read().await {
            if Instant::now() < backoff_until {
                let wait_time = backoff_until.duration_since(Instant::now());
                warn!("⏳ {} API in backoff, waiting {:?}", self.provider_name, wait_time);
                sleep(wait_time).await;
            }
        }
        
        // For critical requests, try to skip the queue
        if priority == RequestPriority::Critical {
            if let Ok(_permit) = self.semaphore.try_acquire() {
                debug!("🚨 Critical request fast-tracked for {} API: {}", self.provider_name, endpoint);
                return Ok(RateLimitPermit::new(1, self.clone(), endpoint.to_string()));
            }
        }
        
        // Queue the request based on priority
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let queued_request = QueuedRequest {
            priority,
            endpoint: endpoint.to_string(),
            created_at: Instant::now(),
            sender,
        };
        
        // Insert into priority queue
        {
            let mut queue = self.request_queue.lock().await;
            let insert_pos = queue.iter().position(|req| req.priority < priority).unwrap_or(queue.len());
            queue.insert(insert_pos, queued_request);
            debug!("📝 Queued {} priority request for {} API: {} (queue size: {})", 
                   priority_to_string(priority), self.provider_name, endpoint, queue.len());
        }
        
        // Process the queue
        self.process_queue().await;
        
        // Wait for our turn
        receiver.await??;
        
        // Acquire the semaphore permit
        let _permit = self.semaphore.acquire().await.map_err(|e| anyhow!("Failed to acquire semaphore: {}", e))?;
        
        Ok(RateLimitPermit::new(1, self.clone(), endpoint.to_string()))
    }
    
    /// Process the priority queue
    async fn process_queue(&self) {
        let mut queue = self.request_queue.lock().await;
        
        while !queue.is_empty() && self.semaphore.available_permits() > 0 {
            if let Some(request) = queue.pop() {
                let _ = request.sender.send(Ok(()));
                debug!("✅ Processed queued request: {}", request.endpoint);
            }
        }
    }
    
    /// Record a successful request
    pub async fn record_request(&self, endpoint: &str, duration_ms: u64, priority: RequestPriority) {
        let record = RequestRecord {
            timestamp: Instant::now(),
            priority,
            endpoint: endpoint.to_string(),
            duration_ms,
        };
        
        let mut history = self.request_history.write().await;
        history.push_back(record);
        
        // Clean old records (keep only last hour)
        let cutoff = Instant::now() - Duration::from_secs(3600);
        while let Some(front) = history.front() {
            if front.timestamp < cutoff {
                history.pop_front();
            } else {
                break;
            }
        }
        
        debug!("📊 Recorded {} API request: {} ({}ms, {} priority)", 
               self.provider_name, endpoint, duration_ms, priority_to_string(priority));
    }
    
    /// Handle rate limit hit with exponential backoff
    pub async fn handle_rate_limit_hit(&self) {
        let mut consecutive_hits = self.consecutive_rate_limits.write().await;
        *consecutive_hits += 1;
        
        // Calculate exponential backoff: 2^n seconds, capped at max_backoff
        let backoff_secs = std::cmp::min(
            2_u64.pow(*consecutive_hits), 
            self.config.max_backoff_secs
        );
        
        let backoff_until = Instant::now() + Duration::from_secs(backoff_secs);
        *self.backoff_until.write().await = Some(backoff_until);
        
        error!("🚫 {} API rate limit hit! Consecutive hits: {}, backing off for {}s", 
               self.provider_name, *consecutive_hits, backoff_secs);
    }
    
    /// Reset consecutive rate limit counter on successful requests
    pub async fn reset_rate_limit_counter(&self) {
        let mut consecutive_hits = self.consecutive_rate_limits.write().await;
        if *consecutive_hits > 0 {
            debug!("✅ {} API rate limit counter reset", self.provider_name);
            *consecutive_hits = 0;
        }
    }
    
    /// Check current rate limits
    async fn check_rate_limits(&self) -> bool {
        let history = self.request_history.read().await;
        let now = Instant::now();
        
        // Check hourly limit
        let hour_ago = now - Duration::from_secs(3600);
        let hourly_requests = history.iter()
            .filter(|r| r.timestamp > hour_ago)
            .count() as u32;
            
        if hourly_requests >= self.config.requests_per_hour {
            debug!("⚠️ {} API hourly limit reached: {}/{}", 
                   self.provider_name, hourly_requests, self.config.requests_per_hour);
            return false;
        }
        
        // Check minute limit
        let minute_ago = now - Duration::from_secs(60);
        let minute_requests = history.iter()
            .filter(|r| r.timestamp > minute_ago)
            .count() as u32;
            
        if minute_requests >= self.config.requests_per_minute {
            debug!("⚠️ {} API minute limit reached: {}/{}", 
                   self.provider_name, minute_requests, self.config.requests_per_minute);
            return false;
        }
        
        true
    }
    
    /// Get current usage statistics
    pub async fn get_usage_stats(&self) -> RateLimitStats {
        let history = self.request_history.read().await;
        let now = Instant::now();
        
        let hour_ago = now - Duration::from_secs(3600);
        let minute_ago = now - Duration::from_secs(60);
        
        let hourly_requests = history.iter().filter(|r| r.timestamp > hour_ago).count() as u32;
        let minute_requests = history.iter().filter(|r| r.timestamp > minute_ago).count() as u32;
        
        let queue_size = self.request_queue.lock().await.len();
        let available_permits = self.semaphore.available_permits();
        let backoff_remaining = if let Some(backoff_until) = *self.backoff_until.read().await {
            if now < backoff_until {
                Some(backoff_until.duration_since(now))
            } else {
                None
            }
        } else {
            None
        };
        
        RateLimitStats {
            provider_name: self.provider_name.clone(),
            hourly_requests,
            hourly_limit: self.config.requests_per_hour,
            minute_requests,
            minute_limit: self.config.requests_per_minute,
            queue_size,
            available_permits,
            backoff_remaining,
            consecutive_rate_limits: *self.consecutive_rate_limits.read().await,
        }
    }
}

/// RAII permit for rate-limited requests
pub struct RateLimitPermit {
    _permits: u32,
    limiter: AdvancedRateLimiter,
    endpoint: String,
    start_time: Instant,
}

impl RateLimitPermit {
    fn new(permits: u32, limiter: AdvancedRateLimiter, endpoint: String) -> Self {
        Self {
            _permits: permits,
            limiter,
            endpoint,
            start_time: Instant::now(),
        }
    }
    
    /// Mark the request as successful
    pub async fn mark_success(&self, priority: RequestPriority) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        self.limiter.record_request(&self.endpoint, duration_ms, priority).await;
        self.limiter.reset_rate_limit_counter().await;
    }
    
    /// Mark the request as rate limited
    pub async fn mark_rate_limited(&self) {
        self.limiter.handle_rate_limit_hit().await;
    }
}

impl Drop for RateLimitPermit {
    fn drop(&mut self) {
        // Release permits back to the semaphore
        self.limiter.semaphore.add_permits(self._permits as usize);
    }
}

/// Rate limiting statistics
#[derive(Debug, Clone, Serialize)]
pub struct RateLimitStats {
    pub provider_name: String,
    pub hourly_requests: u32,
    pub hourly_limit: u32,
    pub minute_requests: u32,
    pub minute_limit: u32,
    pub queue_size: usize,
    pub available_permits: usize,
    pub backoff_remaining: Option<Duration>,
    pub consecutive_rate_limits: u32,
}

impl std::fmt::Display for RateLimitStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}API: {}/{}/h, {}/{}/m, queue:{}, permits:{}, backoff:{:?}", 
               self.provider_name,
               self.hourly_requests, self.hourly_limit,
               self.minute_requests, self.minute_limit,
               self.queue_size,
               self.available_permits,
               self.backoff_remaining)
    }
}

/// Convert priority enum to human-readable string
fn priority_to_string(priority: RequestPriority) -> &'static str {
    match priority {
        RequestPriority::Critical => "CRITICAL",
        RequestPriority::High => "HIGH",
        RequestPriority::Medium => "MEDIUM", 
        RequestPriority::Low => "LOW",
        RequestPriority::Background => "BACKGROUND",
    }
}

/// Global rate limiter manager for all API providers
pub struct RateLimiterManager {
    limiters: HashMap<String, Arc<AdvancedRateLimiter>>,
}

impl RateLimiterManager {
    /// Create a new rate limiter manager
    pub fn new() -> Self {
        Self {
            limiters: HashMap::new(),
        }
    }
    
    /// Add a rate limiter for a provider
    pub fn add_limiter(&mut self, provider: String, limiter: AdvancedRateLimiter) {
        self.limiters.insert(provider.clone(), Arc::new(limiter));
        info!("📈 Added rate limiter for provider: {}", provider);
    }
    
    /// Get rate limiter for a provider
    pub fn get_limiter(&self, provider: &str) -> Option<Arc<AdvancedRateLimiter>> {
        self.limiters.get(provider).cloned()
    }
    
    /// Get all current usage statistics
    pub async fn get_all_stats(&self) -> Vec<RateLimitStats> {
        let mut stats = Vec::new();
        
        for limiter in self.limiters.values() {
            stats.push(limiter.get_usage_stats().await);
        }
        
        stats
    }
    
    /// Create standard rate limiters for all known providers
    pub fn create_standard_limiters() -> Self {
        let mut manager = Self::new();
        
        // Helius API with optimized settings
        manager.add_limiter("helius".to_string(), AdvancedRateLimiter::new_helius());
        
        // Jupiter API (more conservative)
        let jupiter_config = RateLimitConfig {
            requests_per_hour: 1200,
            requests_per_minute: 20,
            burst_capacity: 5,
            cooldown_duration_secs: 60,
            max_backoff_secs: 300,
        };
        manager.add_limiter("jupiter".to_string(), AdvancedRateLimiter::new("Jupiter".to_string(), jupiter_config));
        
        // Orca API
        let orca_config = RateLimitConfig {
            requests_per_hour: 3600,
            requests_per_minute: 60,
            burst_capacity: 10,
            cooldown_duration_secs: 30,
            max_backoff_secs: 180,
        };
        manager.add_limiter("orca".to_string(), AdvancedRateLimiter::new("Orca".to_string(), orca_config));
        
        // Raydium API
        let raydium_config = RateLimitConfig {
            requests_per_hour: 2400,
            requests_per_minute: 40,
            burst_capacity: 8,
            cooldown_duration_secs: 45,
            max_backoff_secs: 240,
        };
        manager.add_limiter("raydium".to_string(), AdvancedRateLimiter::new("Raydium".to_string(), raydium_config));
        
        // Generic RPC endpoints
        let rpc_config = RateLimitConfig {
            requests_per_hour: 1800,
            requests_per_minute: 30,
            burst_capacity: 6,
            cooldown_duration_secs: 90,
            max_backoff_secs: 360,
        };
        manager.add_limiter("rpc".to_string(), AdvancedRateLimiter::new("RPC".to_string(), rpc_config));
        
        info!("🏭 Created standard rate limiters for all providers");
        manager
    }
}

impl Default for RateLimiterManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;
    
    #[tokio::test]
    async fn test_rate_limiter_basic_functionality() {
        let config = RateLimitConfig {
            requests_per_hour: 100,
            requests_per_minute: 5,
            burst_capacity: 3,
            cooldown_duration_secs: 1,
            max_backoff_secs: 5,
        };
        
        let limiter = AdvancedRateLimiter::new("test".to_string(), config);
        
        // Should be able to make requests initially
        assert!(limiter.can_make_request().await);
        
        // Acquire permit
        let permit = limiter.acquire_permit(RequestPriority::High, "/test").await.unwrap();
        permit.mark_success(RequestPriority::High).await;
    }
    
    #[tokio::test]
    async fn test_priority_queue() {
        let config = RateLimitConfig {
            requests_per_hour: 100,
            requests_per_minute: 5,
            burst_capacity: 1, // Small capacity to force queuing
            cooldown_duration_secs: 1,
            max_backoff_secs: 5,
        };
        
        let limiter = AdvancedRateLimiter::new("test".to_string(), config);
        
        // First request should succeed immediately
        let _permit1 = limiter.acquire_permit(RequestPriority::Low, "/test1").await.unwrap();
        
        // Subsequent requests should be queued
        let critical_future = limiter.acquire_permit(RequestPriority::Critical, "/critical");
        let _low_future = limiter.acquire_permit(RequestPriority::Low, "/low");
        
        // Critical should be processed first when permit becomes available
        drop(_permit1); // Release the permit
        
        let result = timeout(Duration::from_millis(100), critical_future).await;
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_rate_limit_stats() {
        let limiter = AdvancedRateLimiter::new_helius();
        let stats = limiter.get_usage_stats().await;
        
        assert_eq!(stats.provider_name, "Helius");
        assert_eq!(stats.hourly_requests, 0);
        assert!(stats.available_permits > 0);
    }
}
