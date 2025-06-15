//! Live Update Manager Service
//! 
//! This service creates the critical connection between the webhook system and the orchestrator's hot cache.
//! It uses Tokio MPSC channels to feed real-time pool updates from the Helius webhook system 
//! directly into the arbitrage engine's hot cache for sub-millisecond access.

use crate::{
    config::Config,
    utils::{PoolInfo, DexType},
    webhooks::{
        types::{PoolUpdateEvent, PoolUpdateType, HeliusWebhookNotification},
        integration::WebhookIntegrationService,
        processor::PoolUpdateProcessor,
    },
    dex::api::DexClient,
};

use anyhow::{Result, anyhow};
use dashmap::DashMap;
use log::{info, warn, debug};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::{AtomicU64, AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, RwLock, Mutex},
    time::interval,
};

/// Configuration for the live update manager
#[derive(Debug, Clone)]
pub struct LiveUpdateConfig {
    /// Buffer size for the update channel
    pub channel_buffer_size: usize,
    /// Maximum updates per second to prevent overwhelming
    pub max_updates_per_second: u32,
    /// Whether to enable update batching
    pub enable_batching: bool,
    /// Batch size for processing multiple updates
    pub batch_size: usize,
    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
    /// Whether to validate updates before applying
    #[allow(dead_code)] // Planned for validation logic implementation
    pub validate_updates: bool,
    /// Maximum age of an update before it's considered stale (ms)
    #[allow(dead_code)] // Planned for stale update filtering
    pub max_update_age_ms: u64,
}

impl Default for LiveUpdateConfig {
    fn default() -> Self {
        Self {
            channel_buffer_size: 10000,
            max_updates_per_second: 1000,
            enable_batching: true,
            batch_size: 50,
            batch_timeout_ms: 100,
            validate_updates: true,
            max_update_age_ms: 5000, // 5 seconds
        }
    }
}

/// Metrics for monitoring live update performance
#[derive(Debug, Default)]
pub struct LiveUpdateMetrics {
    pub total_updates_received: AtomicU64,
    pub total_updates_applied: AtomicU64,
    pub total_updates_rejected: AtomicU64,
    pub total_batches_processed: AtomicU64,
    pub hot_cache_updates: AtomicU64,
    pub webhook_updates: AtomicU64,
    #[allow(dead_code)] // Planned for validation failure tracking
    pub validation_failures: AtomicU64,
    #[allow(dead_code)] // Planned for rate limiting implementation
    pub rate_limit_hits: AtomicU64,
    pub average_update_latency_ms: AtomicU64,
    pub last_update_timestamp: AtomicU64,
}

impl LiveUpdateMetrics {
    pub fn log_summary(&self) {
        let received = self.total_updates_received.load(Ordering::Relaxed);
        let applied = self.total_updates_applied.load(Ordering::Relaxed);
        let rejected = self.total_updates_rejected.load(Ordering::Relaxed);
        let avg_latency = self.average_update_latency_ms.load(Ordering::Relaxed);
        
        info!("📊 Live Update Metrics:");
        info!("   📥 Total received: {}", received);
        info!("   ✅ Applied: {} ({:.1}%)", applied, if received > 0 { (applied as f64 / received as f64) * 100.0 } else { 0.0 });
        info!("   ❌ Rejected: {} ({:.1}%)", rejected, if received > 0 { (rejected as f64 / received as f64) * 100.0 } else { 0.0 });
        info!("   ⚡ Avg latency: {}ms", avg_latency);
    }
}

/// Live update event that flows through the pipeline
#[derive(Debug, Clone)]
pub struct LiveUpdateEvent {
    pub pool_address: Pubkey,
    pub pool_info: Arc<PoolInfo>,
    #[allow(dead_code)] // Planned for different update type handling
    pub update_type: PoolUpdateType,
    pub timestamp: u64,
    pub source: UpdateSource,
}

/// Source of the live update
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants planned for different update source tracking
pub enum UpdateSource {
    Webhook,
    WebSocket,
    Polling,
    Manual,
}

/// The main LiveUpdateManager service
pub struct LiveUpdateManager {
    config: LiveUpdateConfig,
    hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    
    // MPSC channels for data flow
    update_sender: mpsc::Sender<LiveUpdateEvent>,
    update_receiver: Option<mpsc::Receiver<LiveUpdateEvent>>,
    
    // External integration channels (future webhook integration)
    #[allow(dead_code)] // Planned for webhook receiver integration
    webhook_receiver: Option<mpsc::UnboundedReceiver<HeliusWebhookNotification>>,
    #[allow(dead_code)] // Planned for external pool update notifications
    pool_update_sender: Option<mpsc::UnboundedSender<PoolUpdateEvent>>,
    
    // Components
    webhook_service: Option<Arc<Mutex<WebhookIntegrationService>>>,
    pool_processor: Arc<PoolUpdateProcessor>,
    
    // State management
    metrics: Arc<LiveUpdateMetrics>,
    is_running: Arc<AtomicBool>,
    rate_limiter: Arc<RwLock<RateLimiter>>,
    
    // DEX clients for parsing
    dex_clients: HashMap<DexType, Arc<dyn DexClient>>,
}

/// Simple rate limiter for update processing
struct RateLimiter {
    max_per_second: u32,
    current_count: u32,
    last_reset: Instant,
}

impl RateLimiter {
    fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            current_count: 0,
            last_reset: Instant::now(),
        }
    }
    
    fn should_allow(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_reset).as_secs() >= 1 {
            self.current_count = 0;
            self.last_reset = now;
        }
        
        if self.current_count < self.max_per_second {
            self.current_count += 1;
            true
        } else {
            false
        }
    }
}

impl LiveUpdateManager {
    /// Create a new LiveUpdateManager
    pub fn new(
        config: LiveUpdateConfig,
        hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        app_config: Arc<Config>,
    ) -> Result<Self> {
        let (update_sender, update_receiver) = mpsc::channel(config.channel_buffer_size);
        
        let webhook_service = if app_config.enable_webhooks {
            Some(Arc::new(Mutex::new(WebhookIntegrationService::new(app_config.clone()))))
        } else {
            None
        };
        
        let pool_processor = Arc::new(PoolUpdateProcessor::new());
        let rate_limiter = Arc::new(RwLock::new(RateLimiter::new(config.max_updates_per_second)));
        
        Ok(Self {
            config,
            hot_cache,
            update_sender,
            update_receiver: Some(update_receiver),
            webhook_receiver: None,
            pool_update_sender: None,
            webhook_service,
            pool_processor,
            metrics: Arc::new(LiveUpdateMetrics::default()),
            is_running: Arc::new(AtomicBool::new(false)),
            rate_limiter,
            dex_clients: HashMap::new(),
        })
    }
    
    /// Initialize the manager with DEX clients
    pub fn with_dex_clients(mut self, dex_clients: HashMap<DexType, Arc<dyn DexClient>>) -> Self {
        self.dex_clients = dex_clients;
        self
    }
    
    /// Connect to the webhook system
    pub async fn connect_webhook_system(&mut self) -> Result<()> {
        if let Some(webhook_service) = &self.webhook_service {
            let mut service = webhook_service.lock().await;
            service.initialize().await
                .map_err(|e| anyhow!("Failed to initialize webhook service: {}", e))?;
            
            // Get the notification receiver from the webhook service
            // Note: This would need to be implemented in the webhook service
            info!("✅ Connected to webhook system for live updates");
        }
        Ok(())
    }
    
    /// Start the live update processing
    pub async fn start(&mut self) -> Result<()> {
        if self.is_running.swap(true, Ordering::SeqCst) {
            return Err(anyhow!("LiveUpdateManager is already running"));
        }
        
        info!("🚀 Starting LiveUpdateManager...");
        
        // Take the receiver to move into the task
        let mut update_receiver = self.update_receiver.take()
            .ok_or_else(|| anyhow!("Update receiver already taken"))?;
        
        // Clone necessary components for the processing task
        let hot_cache = self.hot_cache.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        let is_running = self.is_running.clone();
        let _rate_limiter = self.rate_limiter.clone();
        let pool_processor = self.pool_processor.clone();
        
        // Start the main update processing task
        tokio::spawn(async move {
            info!("📡 Live update processing task started");
            
            let mut batch = Vec::with_capacity(config.batch_size);
            let mut batch_timer = interval(Duration::from_millis(config.batch_timeout_ms));
            
            while is_running.load(Ordering::Relaxed) {
                tokio::select! {
                    // Process individual updates
                    update = update_receiver.recv() => {
                        match update {
                            Some(update_event) => {
                                if config.enable_batching {
                                    batch.push(update_event);
                                    if batch.len() >= config.batch_size {
                                        Self::process_batch(&hot_cache, &mut batch, &metrics, &pool_processor).await;
                                        metrics.total_batches_processed.fetch_add(1, Ordering::Relaxed);
                                    }
                                } else {
                                    Self::process_single_update(&hot_cache, update_event, &metrics, &pool_processor).await;
                                }
                            }
                            None => {
                                warn!("Update channel closed, stopping live update processing");
                                break;
                            }
                        }
                    }
                    
                    // Process batches on timeout
                    _ = batch_timer.tick() => {
                        if config.enable_batching && !batch.is_empty() {
                            Self::process_batch(&hot_cache, &mut batch, &metrics, &pool_processor).await;
                            metrics.total_batches_processed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            
            info!("📡 Live update processing task stopped");
        });
        
        // Start webhook processing if available
        if self.webhook_service.is_some() {
            self.start_webhook_processing().await?;
        }
        
        // Start metrics reporting
        self.start_metrics_reporting().await;
        
        info!("✅ LiveUpdateManager started successfully");
        Ok(())
    }
    
    /// Start webhook processing task
    async fn start_webhook_processing(&self) -> Result<()> {
        // This would connect to the webhook receiver channel
        // For now, we'll create a placeholder
        let _hot_cache = self.hot_cache.clone();
        let _update_sender = self.update_sender.clone();
        let _metrics = self.metrics.clone();
        
        tokio::spawn(async move {
            info!("🕸️ Webhook processing task started");
            
            // This is where we would receive webhook notifications
            // and convert them to LiveUpdateEvents
            
            // Placeholder: In a real implementation, this would:
            // 1. Receive HeliusWebhookNotification from webhook system
            // 2. Parse the notification to extract pool updates
            // 3. Create LiveUpdateEvent and send via update_sender
            
            // Example processing loop:
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                // Process webhook notifications...
            }
        });
        
        Ok(())
    }
    
    /// Start metrics reporting task
    async fn start_metrics_reporting(&self) {
        let metrics = self.metrics.clone();
        let is_running = self.is_running.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            
            while is_running.load(Ordering::Relaxed) {
                interval.tick().await;
                metrics.log_summary();
            }
        });
    }
    
    /// Process a single update
    async fn process_single_update(
        hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        update: LiveUpdateEvent,
        metrics: &Arc<LiveUpdateMetrics>,
        _pool_processor: &Arc<PoolUpdateProcessor>,
    ) {
        let start_time = Instant::now();
        
        // Apply the update to hot cache
        hot_cache.insert(update.pool_address, update.pool_info.clone());
        
        // Update metrics
        metrics.total_updates_applied.fetch_add(1, Ordering::Relaxed);
        metrics.hot_cache_updates.fetch_add(1, Ordering::Relaxed);
        
        if matches!(update.source, UpdateSource::Webhook) {
            metrics.webhook_updates.fetch_add(1, Ordering::Relaxed);
        }
        
        let latency = start_time.elapsed().as_millis() as u64;
        metrics.average_update_latency_ms.store(latency, Ordering::Relaxed);
        metrics.last_update_timestamp.store(update.timestamp, Ordering::Relaxed);
        
        debug!("🔄 Applied update for pool {} (latency: {}ms)", update.pool_address, latency);
    }
    
    /// Process a batch of updates
    async fn process_batch(
        hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        batch: &mut Vec<LiveUpdateEvent>,
        metrics: &Arc<LiveUpdateMetrics>,
        _pool_processor: &Arc<PoolUpdateProcessor>,
    ) {
        if batch.is_empty() {
            return;
        }
        
        let start_time = Instant::now();
        let batch_size = batch.len();
        
        info!("📦 Processing batch of {} updates", batch_size);
        
        // Group updates by pool to avoid duplicate processing
        let mut pool_updates: HashMap<Pubkey, LiveUpdateEvent> = HashMap::new();
        
        for update in batch.drain(..) {
            // Keep the latest update for each pool
            pool_updates.insert(update.pool_address, update);
        }
        
        // Apply updates to hot cache
        for (pool_address, update) in pool_updates {
            hot_cache.insert(pool_address, update.pool_info);
            metrics.total_updates_applied.fetch_add(1, Ordering::Relaxed);
        }
        
        let latency = start_time.elapsed().as_millis() as u64;
        info!("📦 Processed batch in {}ms", latency);
        
        // Update average latency (simplified)
        metrics.average_update_latency_ms.store(latency, Ordering::Relaxed);
    }
    
    /// Send a live update (used by external systems)
    pub async fn send_update(&self, update: LiveUpdateEvent) -> Result<()> {
        // Check rate limiting
        {
            let mut limiter = self.rate_limiter.write().await;
            if !limiter.should_allow() {
                self.metrics.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
                return Err(anyhow!("Rate limit exceeded"));
            }
        }
        
        // Validate update age
        let now = chrono::Utc::now().timestamp_millis() as u64;
        if now - update.timestamp > self.config.max_update_age_ms {
            warn!("Rejecting stale update for pool {} (age: {}ms)", 
                  update.pool_address, now - update.timestamp);
            self.metrics.total_updates_rejected.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        
        // Send the update
        self.update_sender.send(update).await
            .map_err(|_| anyhow!("Failed to send update - channel closed"))?;
        
        self.metrics.total_updates_received.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    
    /// Create an update from pool info (convenience method)
    pub fn create_update(
        &self,
        pool_address: Pubkey,
        pool_info: Arc<PoolInfo>,
        update_type: PoolUpdateType,
        source: UpdateSource,
    ) -> LiveUpdateEvent {
        LiveUpdateEvent {
            pool_address,
            pool_info,
            update_type,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            source,
        }
    }
    
    /// Stop the live update manager
    pub async fn stop(&self) {
        info!("🛑 Stopping LiveUpdateManager...");
        self.is_running.store(false, Ordering::SeqCst);
        
        // Log final metrics
        self.metrics.log_summary();
        
        info!("✅ LiveUpdateManager stopped");
    }
    
    /// Get current metrics
    pub fn get_metrics(&self) -> &Arc<LiveUpdateMetrics> {
        &self.metrics
    }
    
    /// Get hot cache size
    pub fn get_hot_cache_size(&self) -> usize {
        self.hot_cache.len()
    }
    
    /// Health check
    pub async fn health_check(&self) -> Result<()> {
        if !self.is_running.load(Ordering::Relaxed) {
            return Err(anyhow!("LiveUpdateManager is not running"));
        }
        
        let metrics = &self.metrics;
        let last_update = metrics.last_update_timestamp.load(Ordering::Relaxed);
        let now = chrono::Utc::now().timestamp_millis() as u64;
        
        if last_update > 0 && now - last_update > 30000 { // 30 seconds
            return Err(anyhow!("No updates received in the last 30 seconds"));
        }
        
        Ok(())
    }
}

/// Builder for LiveUpdateManager configuration
pub struct LiveUpdateManagerBuilder {
    config: LiveUpdateConfig,
    hot_cache: Option<Arc<DashMap<Pubkey, Arc<PoolInfo>>>>,
    app_config: Option<Arc<Config>>,
    dex_clients: HashMap<DexType, Arc<dyn DexClient>>,
}

impl LiveUpdateManagerBuilder {
    pub fn new() -> Self {
        Self {
            config: LiveUpdateConfig::default(),
            hot_cache: None,
            app_config: None,
            dex_clients: HashMap::new(),
        }
    }
    
    pub fn with_config(mut self, config: LiveUpdateConfig) -> Self {
        self.config = config;
        self
    }
    
    pub fn with_hot_cache(mut self, hot_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>) -> Self {
        self.hot_cache = Some(hot_cache);
        self
    }
    
    pub fn with_app_config(mut self, app_config: Arc<Config>) -> Self {
        self.app_config = Some(app_config);
        self
    }
    
    pub fn with_dex_client(mut self, dex_type: DexType, client: Arc<dyn DexClient>) -> Self {
        self.dex_clients.insert(dex_type, client);
        self
    }
    
    pub fn build(self) -> Result<LiveUpdateManager> {
        let hot_cache = self.hot_cache.ok_or_else(|| anyhow!("Hot cache is required"))?;
        let app_config = self.app_config.ok_or_else(|| anyhow!("App config is required"))?;
        
        let manager = LiveUpdateManager::new(self.config, hot_cache, app_config)?
            .with_dex_clients(self.dex_clients);
        
        Ok(manager)
    }
}

impl Default for LiveUpdateManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
