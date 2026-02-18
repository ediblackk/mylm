//! Git repository checking for update detection
//!
//! Provides functionality to check the mylm git repository
//! for updates by comparing build hash with current HEAD.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::{Command, Output};

/// Git repository information for update checking
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct GitInfo {
    /// Whether we're in a git repository
    pub is_repo: bool,
    /// Current branch name
    pub branch: Option<String>,
    /// Latest commit hash
    pub commit: Option<String>,
    /// Latest commit message
    pub commit_message: Option<String>,
}

/// Execute a git command and return the output
fn git_command(args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute git command: {}", e))?;

    Ok(output)
}

/// Check if we're in a git repository
pub fn is_git_repo() -> bool {
    git_command(&["rev-parse", "--is-inside-work-tree"][..])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current branch name
fn get_branch_name() -> Option<String> {
    let output = git_command(&["rev-parse", "--abbrev-ref", "HEAD"][..]).ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Get the latest commit hash
fn get_commit_hash() -> Option<String> {
    let output = git_command(&["rev-parse", "HEAD"][..]).ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Get the latest commit message
fn get_commit_message() -> Option<String> {
    let output = git_command(&["log", "-1", "--pretty=%B"][..]).ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Collect git info for update checking
pub async fn collect_git_info() -> Result<GitInfo> {
    // Run git commands in parallel using blocking in async context
    let is_repo = tokio::task::spawn_blocking(is_git_repo)
        .await
        .unwrap_or(false);

    if !is_repo {
        return Ok(GitInfo {
            is_repo: false,
            ..Default::default()
        });
    }

    // Collect info in parallel
    let (branch_res, commit_res, msg_res) = tokio::join!(
        tokio::task::spawn_blocking(get_branch_name),
        tokio::task::spawn_blocking(get_commit_hash),
        tokio::task::spawn_blocking(get_commit_message)
    );

    Ok(GitInfo {
        is_repo: true,
        branch: branch_res.unwrap_or(None),
        commit: commit_res.unwrap_or(None),
        commit_message: msg_res.unwrap_or(None),
    })
}
