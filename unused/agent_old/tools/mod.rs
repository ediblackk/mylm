//! Built-in tool implementations for the agent system.
//!
//! This module aggregates all available tools that agents can use to interact
//! with the system, filesystem, web, and memory stores.
//!
//! # Tool Categories
//! - **System**: Shell, system monitoring, terminal sight
//! - **Filesystem**: File read/write, find
//! - **Git**: Status, log, diff operations
//! - **Web**: Search and crawling capabilities
//! - **Memory**: State management, memory consolidation, scratchpad
//! - **Agent**: Job management, task delegation, wait

pub mod shell;
pub mod worker_shell;
pub mod memory;
pub mod web_search;
pub mod crawl;
pub mod fs;
pub mod git;
pub mod state;
pub mod system;
pub mod terminal_sight;

pub use shell::{ShellTool, ShellToolConfig};
pub use worker_shell::{WorkerShellTool, WorkerShellPermissions, EscalationMode, EscalationRequest, EscalationResponse};
pub use memory::MemoryTool;
pub use web_search::WebSearchTool;
pub use crawl::CrawlTool;
pub use fs::{FileReadTool, FileWriteTool};
pub use git::{GitStatusTool, GitLogTool, GitDiffTool};
pub use state::StateTool;
pub use system::SystemMonitorTool;
pub use terminal_sight::TerminalSightTool;
pub mod wait;
pub use wait::WaitTool;
pub mod jobs;
pub use jobs::ListJobsTool;
pub mod delegate;
pub use delegate::{DelegateTool, DelegateToolConfig};

pub mod scratchpad;
pub use scratchpad::{ScratchpadTool, StructuredScratchpad};
pub mod consolidate;
pub use consolidate::ConsolidateTool;
pub mod find;
pub use find::FindTool;
pub mod list_files;
pub use list_files::ListFilesTool;
pub mod shell_utils;
pub use shell_utils::{TailTool, WordCountTool, GrepTool, DiskUsageTool};
