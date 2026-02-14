//! Smart Wait Algorithm - Intelligent waiting for background workers
//!
//! This module provides the smart wait logic that prevents the agent from
//! polling the LLM when only background workers are running and no new
//! observations are available.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::sleep;

/// Configuration for smart wait behavior
#[derive(Debug, Clone, Copy)]
pub struct SmartWaitConfig {
    /// Maximum number of wait iterations before returning to idle
    pub max_iterations: usize,
    /// Duration to wait between checks
    pub interval: Duration,
    /// Whether smart wait is enabled
    pub enabled: bool,
}

impl Default for SmartWaitConfig {
    fn default() -> Self {
        Self {
            max_iterations: 5,
            interval: Duration::from_secs(1),
            enabled: true,
        }
    }
}

/// Tracks the state of smart waiting
#[derive(Debug)]
pub struct SmartWaitState {
    config: SmartWaitConfig,
    current_iteration: usize,
    is_waiting: AtomicBool,
}

impl SmartWaitState {
    /// Create a new smart wait state with the given configuration
    pub fn new(config: SmartWaitConfig) -> Self {
        Self {
            config,
            current_iteration: 0,
            is_waiting: AtomicBool::new(false),
        }
    }
    
    /// Check if we should enter smart wait mode
    /// 
    /// Returns true if:
    /// - Smart wait is enabled
    /// - There are no new observations
    /// - There are active workers
    pub fn should_wait(&self, has_observations: bool, active_workers: usize) -> bool {
        if !self.config.enabled {
            return false;
        }
        !has_observations && active_workers > 0
    }
    
    /// Begin a smart wait iteration
    /// 
    /// Returns `SmartWaitResult` indicating what action to take:
    /// - `ContinueWaiting`: Keep waiting, sleep and check again
    /// - `ReturnToIdle`: Max iterations reached, return to idle state
    pub async fn wait_iteration(&mut self) -> SmartWaitResult {
        if !self.is_waiting.load(Ordering::SeqCst) {
            self.is_waiting.store(true, Ordering::SeqCst);
            self.current_iteration = 0;
        }
        
        self.current_iteration += 1;
        
        if self.current_iteration >= self.config.max_iterations {
            self.reset();
            return SmartWaitResult::ReturnToIdle;
        }
        
        sleep(self.config.interval).await;
        SmartWaitResult::ContinueWaiting
    }
    
    /// Reset the wait state (call when progress is made)
    pub fn reset(&mut self) {
        self.current_iteration = 0;
        self.is_waiting.store(false, Ordering::SeqCst);
    }
    
    /// Get the current iteration count
    pub fn current_iteration(&self) -> usize {
        self.current_iteration
    }
    
    /// Check if currently in waiting state
    pub fn is_waiting(&self) -> bool {
        self.is_waiting.load(Ordering::SeqCst)
    }
    
    /// Get remaining iterations until timeout
    pub fn remaining_iterations(&self) -> usize {
        self.config.max_iterations.saturating_sub(self.current_iteration)
    }
}

/// Result of a smart wait iteration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartWaitResult {
    /// Continue waiting, check again after sleep
    ContinueWaiting,
    /// Max iterations reached, return to idle to allow user input
    ReturnToIdle,
}

/// High-level smart wait controller
/// 
/// This provides a simpler API for the common case where you want to
/// smart wait and handle the result.
pub struct SmartWaitController {
    state: SmartWaitState,
}

impl SmartWaitController {
    /// Create a new controller with default configuration
    pub fn new() -> Self {
        Self {
            state: SmartWaitState::new(SmartWaitConfig::default()),
        }
    }
    
    /// Create a new controller with custom configuration
    pub fn with_config(config: SmartWaitConfig) -> Self {
        Self {
            state: SmartWaitState::new(config),
        }
    }
    
    /// Check if smart wait should be triggered and execute if needed
    /// 
    /// This is the main entry point for smart waiting. It will:
    /// 1. Check if conditions are met (no observations + active workers)
    /// 2. If yes, wait and return appropriate result
    /// 3. If no, reset state and return Continue
    /// 
    /// # Returns
    /// - `SmartWaitAction::Waited`: Smart wait was executed, continue loop
    /// - `SmartWaitAction::Timeout`: Max iterations reached, return to idle
    /// - `SmartWaitAction::Continue`: Smart wait not triggered, proceed with agent step
    pub async fn check_and_wait(
        &mut self,
        has_observations: bool,
        active_workers: usize,
    ) -> SmartWaitAction {
        if self.state.should_wait(has_observations, active_workers) {
            match self.state.wait_iteration().await {
                SmartWaitResult::ContinueWaiting => SmartWaitAction::Waited,
                SmartWaitResult::ReturnToIdle => SmartWaitAction::Timeout(active_workers),
            }
        } else {
            self.state.reset();
            SmartWaitAction::Continue
        }
    }
    
    /// Get the current wait state for monitoring
    pub fn state(&self) -> &SmartWaitState {
        &self.state
    }
    
    /// Reset the controller state
    pub fn reset(&mut self) {
        self.state.reset();
    }
}

impl Default for SmartWaitController {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions returned by smart wait check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartWaitAction {
    /// Smart wait was executed, continue the loop
    Waited,
    /// Max iterations reached, should return to idle
    Timeout(usize),
    /// Smart wait not triggered, proceed with agent step
    Continue,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_should_wait_conditions() {
        let state = SmartWaitState::new(SmartWaitConfig {
            enabled: true,
            max_iterations: 5,
            interval: Duration::from_secs(1),
        });
        
        // Should wait when no observations and workers active
        assert!(state.should_wait(false, 1));
        assert!(state.should_wait(false, 5));
        
        // Should not wait when there are observations
        assert!(!state.should_wait(true, 1));
        
        // Should not wait when no workers
        assert!(!state.should_wait(false, 0));
    }
    
    #[test]
    fn test_should_wait_disabled() {
        let state = SmartWaitState::new(SmartWaitConfig {
            enabled: false,
            max_iterations: 5,
            interval: Duration::from_secs(1),
        });
        
        // Should never wait when disabled
        assert!(!state.should_wait(false, 5));
    }
    
    #[tokio::test]
    async fn test_wait_iteration_counts() {
        let mut state = SmartWaitState::new(SmartWaitConfig {
            enabled: true,
            max_iterations: 3,
            interval: Duration::from_millis(10),
        });
        
        // First two iterations should continue waiting
        assert_eq!(state.wait_iteration().await, SmartWaitResult::ContinueWaiting);
        assert_eq!(state.wait_iteration().await, SmartWaitResult::ContinueWaiting);
        
        // Third iteration should return to idle
        assert_eq!(state.wait_iteration().await, SmartWaitResult::ReturnToIdle);
    }
    
    #[test]
    fn test_reset() {
        let mut state = SmartWaitState::new(SmartWaitConfig::default());
        
        state.is_waiting.store(true, Ordering::SeqCst);
        state.current_iteration = 3;
        
        state.reset();
        
        assert_eq!(state.current_iteration, 0);
        assert!(!state.is_waiting.load(Ordering::SeqCst));
    }
}
