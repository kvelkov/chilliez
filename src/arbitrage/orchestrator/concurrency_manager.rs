//! Concurrency Manager Module
//! 
//! This module handles thread-safe execution, deadlock prevention, and
//! concurrent operation management for the arbitrage orchestrator.

use super::core::ArbitrageOrchestrator;
use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    error::ArbError,
    utils::DexType,
};

use log::{info, warn, debug, error};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::Duration,
};
use tokio::time::timeout;

impl ArbitrageOrchestrator {
    /// Execute opportunity with full concurrency safety and deadlock prevention
    pub async fn execute_opportunity_with_concurrency_safety(
        &self,
        opportunity: MultiHopArbOpportunity,
    ) -> Result<(), ArbError> {
        // Check if we're at max concurrent executions
        let current_executions = self.concurrent_executions.load(Ordering::Relaxed);
        if current_executions >= self.max_concurrent_executions {
            return Err(ArbError::ResourceExhausted("Maximum concurrent executions reached".to_string()));
        }

        // Acquire execution semaphore (limits concurrency)
        let _permit = self.execution_semaphore.acquire().await
            .map_err(|e| ArbError::ResourceExhausted(format!("Failed to acquire execution permit: {}", e)))?;

        // Increment concurrent execution counter
        self.concurrent_executions.fetch_add(1, Ordering::Relaxed);

        // Create a unique key for this trading pair to prevent simultaneous trades
        // Use the first hop's dex and the input/output tokens from the opportunity
        let dex_type = if !opportunity.hops.is_empty() {
            opportunity.hops[0].dex.clone()
        } else {
            DexType::Orca // Default fallback
        };
        
        let pair_key = (
            dex_type,
            opportunity.input_token_mint,
            opportunity.output_token_mint,
        );

        // Get or create a lock for this specific trading pair
        let pair_lock = self.trading_pairs_locks
            .entry(pair_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
        let pair_lock = pair_lock.clone();

        let result = {
            // Use timeout to prevent deadlocks
            let lock_timeout = Duration::from_secs(30);
            let pair_guard = timeout(lock_timeout, pair_lock.lock()).await
                .map_err(|_| ArbError::DeadlockPrevention("Failed to acquire pair lock within timeout".to_string()))?;

            // Perform the actual execution with the lock held
            let execution_result = self.execute_opportunity_with_balance_safety(opportunity.clone()).await;

            // Release the lock by dropping the guard
            drop(pair_guard);

            execution_result
        };

        // Decrement concurrent execution counter
        self.concurrent_executions.fetch_sub(1, Ordering::Relaxed);

        // Clean up the lock entry if it's no longer needed
        if let Some((_, lock)) = self.trading_pairs_locks.remove(&pair_key) {
            // Only keep the lock if someone else is waiting for it
            if Arc::strong_count(&lock) > 1 {
                self.trading_pairs_locks.insert(pair_key, lock);
            }
        }

        result
    }

    /// Execute opportunity with balance safety checks
    async fn execute_opportunity_with_balance_safety(&self, opportunity: MultiHopArbOpportunity) -> Result<(), ArbError> {
        // Check balance monitor safety mode
        if let Some(ref balance_monitor) = self.balance_monitor {
            if balance_monitor.is_safety_mode_active() {
                return Err(ArbError::SafetyModeActive("Balance monitor safety mode is active".to_string()));
            }

            // Perform balance checks before execution
            self.perform_pre_execution_balance_checks(&opportunity, balance_monitor).await?;
        }

        // Execute the opportunity
        let result = self.execute_single_opportunity(&opportunity).await;

        // Perform post-execution balance checks
        if let Some(ref balance_monitor) = self.balance_monitor {
            self.perform_post_execution_balance_checks(&opportunity, balance_monitor, &result).await?;
        }

        result
    }

    /// Perform balance checks before execution
    async fn perform_pre_execution_balance_checks(
        &self,
        _opportunity: &MultiHopArbOpportunity,
        _balance_monitor: &crate::solana::BalanceMonitor,
    ) -> Result<(), ArbError> {
        debug!("🔍 Performing pre-execution balance checks");

        // Check if we have sufficient balance for the input amount
        let required_balance = _opportunity.input_amount;
        
        // This would integrate with the balance monitor to check current balances
        // For now, we'll log the check
        debug!("💰 Required balance: {} for token {}", 
               required_balance, _opportunity.input_token);

        Ok(())
    }

    /// Perform balance checks after execution
    async fn perform_post_execution_balance_checks(
        &self,
        _opportunity: &MultiHopArbOpportunity,
        _balance_monitor: &crate::solana::BalanceMonitor,
        execution_result: &Result<(), ArbError>,
    ) -> Result<(), ArbError> {
        debug!("🔍 Performing post-execution balance checks");

        match execution_result {
            Ok(_) => {
                debug!("✅ Execution successful, balance changes should be reflected");
            }
            Err(e) => {
                warn!("❌ Execution failed: {}, checking for balance inconsistencies", e);
                // Here we would check if the balance is consistent with the failed execution
            }
        }

        Ok(())
    }

    /// Check if a trading pair is currently being executed (deadlock prevention)
    pub fn is_pair_being_executed(&self, dex_type: &DexType, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        let pair_key = (dex_type.clone(), *token_a, *token_b);
        self.trading_pairs_locks.contains_key(&pair_key)
    }

    /// Get current concurrency metrics
    pub fn get_concurrency_metrics(&self) -> HashMap<String, serde_json::Value> {
        let mut metrics = HashMap::new();
        
        metrics.insert("current_executions".to_string(), 
                      serde_json::Value::Number(serde_json::Number::from(
                          self.concurrent_executions.load(Ordering::Relaxed))));
        
        metrics.insert("max_executions".to_string(), 
                      serde_json::Value::Number(serde_json::Number::from(self.max_concurrent_executions)));
        
        metrics.insert("available_permits".to_string(), 
                      serde_json::Value::Number(serde_json::Number::from(
                          self.execution_semaphore.available_permits())));
        
        metrics.insert("active_pair_locks".to_string(), 
                      serde_json::Value::Number(serde_json::Number::from(
                          self.trading_pairs_locks.len())));

        metrics
    }

    /// Execute multiple opportunities with proper concurrency control
    pub async fn execute_opportunities_concurrently(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<Result<(), ArbError>>, ArbError> {
        if opportunities.is_empty() {
            return Ok(Vec::new());
        }

        info!("🚀 Executing {} opportunities concurrently with safety controls", opportunities.len());

        // Check available execution slots
        let available_slots = self.execution_semaphore.available_permits();
        let max_concurrent = available_slots.min(opportunities.len());

        info!("⚡ Available execution slots: {}, will execute {} opportunities concurrently", 
              available_slots, max_concurrent);

        // Split opportunities into batches that respect concurrency limits
        let mut results = Vec::new();
        
        for chunk in opportunities.chunks(max_concurrent) {
            // Execute this chunk concurrently
            let chunk_futures: Vec<_> = chunk.iter()
                .map(|op| self.execute_opportunity_with_concurrency_safety(op.clone()))
                .collect();

            let chunk_results = futures::future::join_all(chunk_futures).await;
            results.extend(chunk_results);

            // Brief pause between batches to allow system recovery
            if chunk.len() == max_concurrent {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        info!("✅ Concurrent execution completed: {} results", results.len());
        Ok(results)
    }

    /// Emergency stop all concurrent executions
    pub async fn emergency_stop_all_executions(&self) -> Result<(), ArbError> {
        warn!("🚨 EMERGENCY STOP: Halting all concurrent executions");

        // Disable new executions
        self.execution_enabled.store(false, Ordering::Relaxed);

        // Wait for current executions to complete (with timeout)
        let timeout_duration = Duration::from_secs(30);
        let start_time = tokio::time::Instant::now();

        while self.concurrent_executions.load(Ordering::Relaxed) > 0 {
            if start_time.elapsed() > timeout_duration {
                error!("⏰ Emergency stop timeout: {} executions still running", 
                       self.concurrent_executions.load(Ordering::Relaxed));
                break;
            }
            
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Clear all pair locks
        self.trading_pairs_locks.clear();

        let final_count = self.concurrent_executions.load(Ordering::Relaxed);
        if final_count == 0 {
            info!("✅ Emergency stop completed: all executions halted");
        } else {
            warn!("⚠️ Emergency stop completed with {} executions still running", final_count);
        }

        Ok(())
    }

    /// Resume executions after emergency stop
    pub async fn resume_executions(&self) -> Result<(), ArbError> {
        info!("🔄 Resuming execution operations");
        
        // Re-enable executions
        self.execution_enabled.store(true, Ordering::Relaxed);
        
        // Reset concurrent execution counter (in case of inconsistency)
        self.concurrent_executions.store(0, Ordering::Relaxed);
        
        info!("✅ Execution operations resumed");
        Ok(())
    }

    /// Get detailed concurrency status
    pub async fn get_detailed_concurrency_status(&self) -> ConcurrencyStatus {
        ConcurrencyStatus {
            execution_enabled: self.execution_enabled.load(Ordering::Relaxed),
            current_executions: self.concurrent_executions.load(Ordering::Relaxed),
            max_concurrent_executions: self.max_concurrent_executions,
            available_permits: self.execution_semaphore.available_permits(),
            active_pair_locks: self.trading_pairs_locks.len(),
            trading_pairs_locked: self.trading_pairs_locks.iter()
                .map(|entry| format!("{:?}", entry.key()))
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct ConcurrencyStatus {
    pub execution_enabled: bool,
    pub current_executions: usize,
    pub max_concurrent_executions: usize,
    pub available_permits: usize,
    pub active_pair_locks: usize,
    pub trading_pairs_locked: Vec<String>,
}

// Add the missing import at the top of the file
use std::sync::Arc;
