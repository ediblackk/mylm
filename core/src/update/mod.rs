//! Update checking module
//!
//! Provides functionality to check if a newer version of mylm is available
//! by comparing the current build hash with the git repository.

pub mod git;

pub use git::{is_git_repo, GitInfo, collect_git_info};
