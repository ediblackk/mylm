//! Budget Management - Action budget and resource tracking for agents
//!
//! This module provides budget tracking and enforcement for agent execution,
//! preventing runaway loops and resource exhaustion.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Configuration for agent budget management
#[derive(Debug, Clone, Copy)]
pub struct BudgetConfig {
    /// Maximum number of actions/steps allowed
    pub max_actions: usize,
    /// Maximum total tokens allowed
    pub max_tokens: usize,
    /// Maximum cost in USD (if applicable)
    pub max_cost_usd: Option<f64>,
    /// Whether to enforce budget limits strictly
    pub strict: bool,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_actions: 100,
            max_tokens: 1_000_000,
            max_cost_usd: None,
            strict: true,
        }
    }
}

/// Tracks resource usage against a budget
#[derive(Debug)]
pub struct BudgetTracker {
    config: BudgetConfig,
    actions_used: AtomicUsize,
    tokens_used: AtomicUsize,
    cost_usd: AtomicUsize, // Stored as cents to avoid floating point issues
}

impl BudgetTracker {
    /// Create a new budget tracker with the given configuration
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            actions_used: AtomicUsize::new(0),
            tokens_used: AtomicUsize::new(0),
            cost_usd: AtomicUsize::new(0),
        }
    }
    
    /// Record an action being taken
    /// 
    /// Returns `BudgetStatus::Exceeded` if the action would exceed the budget
    /// and strict mode is enabled.
    pub fn record_action(&self, count: usize) -> BudgetStatus {
        let new_total = self.actions_used.fetch_add(count, Ordering::SeqCst) + count;
        
        if new_total > self.config.max_actions {
            if self.config.strict {
                return BudgetStatus::Exceeded {
                    resource: ResourceType::Actions,
                    limit: self.config.max_actions,
                    used: new_total,
                };
            }
        }
        
        BudgetStatus::Ok {
            remaining_actions: self.config.max_actions.saturating_sub(new_total),
            remaining_tokens: self.config.max_tokens.saturating_sub(self.tokens_used.load(Ordering::SeqCst)),
        }
    }
    
    /// Record token usage
    /// 
    /// Returns `BudgetStatus::Exceeded` if the tokens would exceed the budget
    /// and strict mode is enabled.
    pub fn record_tokens(&self, count: usize) -> BudgetStatus {
        let new_total = self.tokens_used.fetch_add(count, Ordering::SeqCst) + count;
        
        if new_total > self.config.max_tokens {
            if self.config.strict {
                return BudgetStatus::Exceeded {
                    resource: ResourceType::Tokens,
                    limit: self.config.max_tokens,
                    used: new_total,
                };
            }
        }
        
        BudgetStatus::Ok {
            remaining_actions: self.config.max_actions.saturating_sub(self.actions_used.load(Ordering::SeqCst)),
            remaining_tokens: self.config.max_tokens.saturating_sub(new_total),
        }
    }
    
    /// Record cost in USD
    /// 
    /// Cost is stored with 2 decimal precision (cents).
    pub fn record_cost(&self, usd: f64) -> BudgetStatus {
        let cents = (usd * 100.0) as usize;
        let new_total = self.cost_usd.fetch_add(cents, Ordering::SeqCst) + cents;
        
        if let Some(max_cost) = self.config.max_cost_usd {
            let max_cents = (max_cost * 100.0) as usize;
            if new_total > max_cents {
                if self.config.strict {
                    return BudgetStatus::Exceeded {
                        resource: ResourceType::Cost,
                        limit: max_cents,
                        used: new_total,
                    };
                }
            }
        }
        
        BudgetStatus::Ok {
            remaining_actions: self.config.max_actions.saturating_sub(self.actions_used.load(Ordering::SeqCst)),
            remaining_tokens: self.config.max_tokens.saturating_sub(self.tokens_used.load(Ordering::SeqCst)),
        }
    }
    
    /// Get current usage statistics
    pub fn usage(&self) -> BudgetUsage {
        BudgetUsage {
            actions: self.actions_used.load(Ordering::SeqCst),
            tokens: self.tokens_used.load(Ordering::SeqCst),
            cost_usd: self.cost_usd.load(Ordering::SeqCst) as f64 / 100.0,
        }
    }
    
    /// Get remaining budget
    pub fn remaining(&self) -> BudgetUsage {
        let actions = self.actions_used.load(Ordering::SeqCst);
        let tokens = self.tokens_used.load(Ordering::SeqCst);
        let cost_cents = self.cost_usd.load(Ordering::SeqCst);
        
        BudgetUsage {
            actions: self.config.max_actions.saturating_sub(actions),
            tokens: self.config.max_tokens.saturating_sub(tokens),
            cost_usd: self.config.max_cost_usd.map(|c| c - (cost_cents as f64 / 100.0)).unwrap_or(f64::INFINITY),
        }
    }
    
    /// Check if budget is exhausted
    pub fn is_exhausted(&self) -> bool {
        let actions = self.actions_used.load(Ordering::SeqCst);
        let tokens = self.tokens_used.load(Ordering::SeqCst);
        let cost_cents = self.cost_usd.load(Ordering::SeqCst);
        
        if actions >= self.config.max_actions {
            return true;
        }
        if tokens >= self.config.max_tokens {
            return true;
        }
        if let Some(max_cost) = self.config.max_cost_usd {
            if (cost_cents as f64 / 100.0) >= max_cost {
                return true;
            }
        }
        false
    }
    
    /// Get the percentage of budget used (0.0 to 1.0)
    pub fn usage_percentage(&self) -> BudgetPercentage {
        let actions = self.actions_used.load(Ordering::SeqCst) as f64 / self.config.max_actions as f64;
        let tokens = self.tokens_used.load(Ordering::SeqCst) as f64 / self.config.max_tokens as f64;
        
        let cost = if let Some(max_cost) = self.config.max_cost_usd {
            let cost_cents = self.cost_usd.load(Ordering::SeqCst) as f64 / 100.0;
            cost_cents / max_cost
        } else {
            0.0
        };
        
        BudgetPercentage {
            actions: actions.min(1.0),
            tokens: tokens.min(1.0),
            cost: if self.config.max_cost_usd.is_some() { cost.min(1.0) } else { 0.0 },
        }
    }
    
    /// Reset all counters to zero
    pub fn reset(&self) {
        self.actions_used.store(0, Ordering::SeqCst);
        self.tokens_used.store(0, Ordering::SeqCst);
        self.cost_usd.store(0, Ordering::SeqCst);
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new(BudgetConfig::default())
    }
}

/// Types of resources that can be budgeted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    /// Number of actions/steps
    Actions,
    /// Token count
    Tokens,
    /// Cost in USD
    Cost,
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceType::Actions => write!(f, "actions"),
            ResourceType::Tokens => write!(f, "tokens"),
            ResourceType::Cost => write!(f, "cost"),
        }
    }
}

/// Status of budget after an operation
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetStatus {
    /// Operation succeeded, remaining budget included
    Ok {
        remaining_actions: usize,
        remaining_tokens: usize,
    },
    /// Budget would be exceeded
    Exceeded {
        resource: ResourceType,
        limit: usize,
        used: usize,
    },
}

impl BudgetStatus {
    /// Check if the budget is okay (not exceeded)
    pub fn is_ok(&self) -> bool {
        matches!(self, BudgetStatus::Ok { .. })
    }
    
    /// Check if the budget was exceeded
    pub fn is_exceeded(&self) -> bool {
        matches!(self, BudgetStatus::Exceeded { .. })
    }
}

/// Usage statistics for a budget
#[derive(Debug, Clone, Copy)]
pub struct BudgetUsage {
    pub actions: usize,
    pub tokens: usize,
    pub cost_usd: f64,
}

impl BudgetUsage {
    /// Create an empty usage (all zeros)
    pub fn zero() -> Self {
        Self {
            actions: 0,
            tokens: 0,
            cost_usd: 0.0,
        }
    }
    
    /// Check if any usage is non-zero
    pub fn has_usage(&self) -> bool {
        self.actions > 0 || self.tokens > 0 || self.cost_usd > 0.0
    }
}

/// Percentage of budget used for each resource type
#[derive(Debug, Clone, Copy)]
pub struct BudgetPercentage {
    pub actions: f64,
    pub tokens: f64,
    pub cost: f64,
}

impl BudgetPercentage {
    /// Get the maximum percentage across all resources
    pub fn max(&self) -> f64 {
        self.actions.max(self.tokens).max(self.cost)
    }
    
    /// Check if any resource is over budget (> 1.0 = 100%)
    pub fn is_over_budget(&self) -> bool {
        self.max() > 1.0
    }
    
    /// Format as a human-readable string
    pub fn format(&self) -> String {
        format!(
            "Actions: {:.1}%, Tokens: {:.1}%, Cost: {:.1}%",
            self.actions * 100.0,
            self.tokens * 100.0,
            self.cost * 100.0
        )
    }
}

/// Shared budget tracker that can be cloned and shared across tasks
#[derive(Debug, Clone)]
pub struct SharedBudget {
    inner: Arc<BudgetTracker>,
}

impl SharedBudget {
    /// Create a new shared budget tracker
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            inner: Arc::new(BudgetTracker::new(config)),
        }
    }
    
    /// Record an action
    pub fn record_action(&self, count: usize) -> BudgetStatus {
        self.inner.record_action(count)
    }
    
    /// Record token usage
    pub fn record_tokens(&self, count: usize) -> BudgetStatus {
        self.inner.record_tokens(count)
    }
    
    /// Record cost
    pub fn record_cost(&self, usd: f64) -> BudgetStatus {
        self.inner.record_cost(usd)
    }
    
    /// Get current usage
    pub fn usage(&self) -> BudgetUsage {
        self.inner.usage()
    }
    
    /// Get remaining budget
    pub fn remaining(&self) -> BudgetUsage {
        self.inner.remaining()
    }
    
    /// Check if budget is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.inner.is_exhausted()
    }
    
    /// Get usage percentage
    pub fn usage_percentage(&self) -> BudgetPercentage {
        self.inner.usage_percentage()
    }
    
    /// Reset counters
    pub fn reset(&self) {
        self.inner.reset();
    }
}

impl Default for SharedBudget {
    fn default() -> Self {
        Self::new(BudgetConfig::default())
    }
}

/// Budget-enforced wrapper for agent loops
/// 
/// This provides a convenient way to check budget before each iteration
/// of an agent loop.
pub struct BudgetEnforcer {
    budget: SharedBudget,
}

impl BudgetEnforcer {
    /// Create a new budget enforcer
    pub fn new(budget: SharedBudget) -> Self {
        Self { budget }
    }
    
    /// Check if the agent can proceed with another step
    /// 
    /// This should be called at the start of each agent loop iteration.
    /// It records one action and checks if the budget is still valid.
    /// 
    /// # Returns
    /// - `Ok(remaining)` if the agent can proceed
    /// - `Err(BudgetExceeded)` if the budget is exhausted
    pub fn check_step(&self) -> Result<BudgetUsage, BudgetExceeded> {
        match self.budget.record_action(1) {
            BudgetStatus::Ok { remaining_actions, remaining_tokens } => {
                Ok(BudgetUsage {
                    actions: remaining_actions,
                    tokens: remaining_tokens,
                    cost_usd: 0.0, // Would need to track separately
                })
            }
            BudgetStatus::Exceeded { resource, limit, used } => {
                Err(BudgetExceeded {
                    resource,
                    limit,
                    used,
                })
            }
        }
    }
    
    /// Record token usage from an LLM call
    pub fn record_tokens(&self, tokens: usize) {
        self.budget.record_tokens(tokens);
    }
    
    /// Get the underlying budget tracker
    pub fn budget(&self) -> &SharedBudget {
        &self.budget
    }
}

/// Error type for budget exhaustion
#[derive(Debug, Clone)]
pub struct BudgetExceeded {
    pub resource: ResourceType,
    pub limit: usize,
    pub used: usize,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Budget exceeded for {}: used {} of {} limit",
            self.resource, self.used, self.limit
        )
    }
}

impl std::error::Error for BudgetExceeded {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_budget_tracking() {
        let budget = BudgetTracker::new(BudgetConfig {
            max_actions: 10,
            max_tokens: 1000,
            max_cost_usd: None,
            strict: true,
        });
        
        // Record some actions
        assert!(budget.record_action(5).is_ok());
        assert_eq!(budget.usage().actions, 5);
        
        // Record some tokens
        assert!(budget.record_tokens(500).is_ok());
        assert_eq!(budget.usage().tokens, 500);
        
        // Check remaining
        let remaining = budget.remaining();
        assert_eq!(remaining.actions, 5);
        assert_eq!(remaining.tokens, 500);
    }
    
    #[test]
    fn test_budget_exceeded() {
        let budget = BudgetTracker::new(BudgetConfig {
            max_actions: 5,
            max_tokens: 100,
            max_cost_usd: None,
            strict: true,
        });
        
        // Should exceed after 6 actions
        assert!(budget.record_action(5).is_ok());
        assert!(budget.record_action(1).is_exceeded());
    }
    
    #[test]
    fn test_budget_not_strict() {
        let budget = BudgetTracker::new(BudgetConfig {
            max_actions: 5,
            max_tokens: 100,
            max_cost_usd: None,
            strict: false, // Not strict
        });
        
        // Should not exceed even over limit
        assert!(budget.record_action(10).is_ok());
    }
    
    #[test]
    fn test_usage_percentage() {
        let budget = BudgetTracker::new(BudgetConfig {
            max_actions: 100,
            max_tokens: 1000,
            max_cost_usd: Some(10.0),
            strict: true,
        });
        
        budget.record_action(50);
        budget.record_tokens(250);
        budget.record_cost(2.50);
        
        let pct = budget.usage_percentage();
        assert_eq!(pct.actions, 0.5);
        assert_eq!(pct.tokens, 0.25);
        assert_eq!(pct.cost, 0.25);
    }
    
    #[test]
    fn test_shared_budget() {
        let budget = SharedBudget::new(BudgetConfig {
            max_actions: 10,
            max_tokens: 100,
            max_cost_usd: None,
            strict: true,
        });
        
        let budget2 = budget.clone();
        
        // Both should see the same state
        budget.record_action(5);
        assert_eq!(budget2.usage().actions, 5);
        
        budget2.record_action(3);
        assert_eq!(budget.usage().actions, 8);
    }
    
    #[test]
    fn test_budget_enforcer() {
        let budget = SharedBudget::new(BudgetConfig {
            max_actions: 3,
            max_tokens: 100,
            max_cost_usd: None,
            strict: true,
        });
        
        let enforcer = BudgetEnforcer::new(budget);
        
        // First 3 steps should succeed
        assert!(enforcer.check_step().is_ok());
        assert!(enforcer.check_step().is_ok());
        assert!(enforcer.check_step().is_ok());
        
        // 4th step should fail
        assert!(enforcer.check_step().is_err());
    }
}
