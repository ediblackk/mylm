//! Agent initialization module using new runtime architecture
//!
//! This module provides helper functions for initializing agent components
//! using the new AgentSessionFactory and contract-based architecture.

use mylm_core::agent::factory::AgentSessionFactory;
use mylm_core::agent::runtime::core::ApprovalCapability;
use mylm_core::agent::runtime::core::terminal::TerminalExecutor;
use mylm_core::config::Config;
use std::sync::Arc;

/// Create AgentSessionFactory from config
/// 
/// Optionally provide custom terminal executor and approval capability.
/// If approval is None, auto-approve is used (suitable for non-interactive use).
pub fn create_session_factory(
    config: &Config,
    terminal: Option<Arc<dyn TerminalExecutor>>,
    approval: Option<Arc<dyn ApprovalCapability>>,
) -> AgentSessionFactory {
    let mut factory = AgentSessionFactory::new(config.clone());
    
    if let Some(terminal) = terminal {
        factory = factory.with_terminal(terminal);
    }
    
    if let Some(approval) = approval {
        factory = factory.with_approval(approval);
    }
    
    factory
}
