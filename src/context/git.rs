//! Git context collection
//!
//! Collects git repository information including branch, status,
//! commit history, and file changes.

use anyhow::Result;
use std::process::{Command, Output};

use super::GitContext;

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

/// Get the git status as a short summary
fn get_status_summary() -> String {
    let output = git_command(&["status", "--porcelain"][..]).ok();

    if output.as_ref().map(|o| !o.status.success()).unwrap_or(true) {
        return "unknown".to_string();
    }

    let stdout = output.unwrap().stdout;
    let status_lines = String::from_utf8_lossy(&stdout);

    let mut modified = 0;
    let mut staged = 0;
    let mut untracked = 0;

    for line in status_lines.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse status format: XY filename
        if line.len() >= 3 {
            let status = &line[..2];
            let rest = &line[2..].trim_start();

            // Skip submodules and renames
            if rest.starts_with("..") || rest.starts_with("R") {
                continue;
            }

            // Check first character (staged)
            match status.chars().nth(0) {
                Some('A') | Some('M') | Some('D') | Some('R') | Some('C') | Some('U') => staged += 1,
                _ => {}
            }

            // Check second character (working tree)
            match status.chars().nth(1) {
                Some('M') => modified += 1,
                Some('D') => modified += 1,
                Some('?') => untracked += 1,
                Some('U') => modified += 1,
                _ => {}
            }
        }
    }

    let mut summary = String::new();
    if staged > 0 {
        summary.push_str(&format!("{} staged, ", staged));
    }
    if modified > 0 {
        summary.push_str(&format!("{} modified, ", modified));
    }
    if untracked > 0 {
        summary.push_str(&format!("{} untracked", untracked));
    }

    // Remove trailing comma and space
    if summary.ends_with(", ") {
        summary.truncate(summary.len() - 2);
    }

    if summary.is_empty() {
        summary = "clean".to_string();
    }

    summary
}

/// Get git diff statistics for staged changes
fn get_staged_diff_stats() -> Option<(usize, usize)> {
    let output = git_command(&["diff", "--cached", "--stat"][..]).ok()?;
    if !output.status.success() {
        return None;
    }

    let stats = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stats.split_whitespace().collect();

    // Format is typically: "X files changed, Y insertions(+), Z deletions(-)"
    if parts.len() >= 3 {
        if let Ok(files) = parts[0].parse::<usize>() {
            return Some((files, 0));
        }
    }

    None
}

/// Get diff statistics for working tree changes
fn get_working_diff_stats() -> Option<(usize, usize)> {
    let output = git_command(&["diff", "--stat"][..]).ok()?;
    if !output.status.success() {
        return None;
    }

    let stats = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stats.split_whitespace().collect();

    if parts.len() >= 3 {
        if let Ok(files) = parts[0].parse::<usize>() {
            return Some((files, 0));
        }
    }

    None
}

/// Collect all git context information
pub async fn collect_git_context() -> Result<GitContext> {
    // Run git commands in parallel using blocking in async context
    let is_repo = tokio::task::spawn_blocking(is_git_repo)
        .await
        .unwrap_or(false);

    if !is_repo {
        return Ok(GitContext {
            is_repo: false,
            ..Default::default()
        });
    }

    // Collect all info in parallel
    let (branch, commit, commit_message, status_summary) = tokio::join!(
        tokio::task::spawn_blocking(get_branch_name),
        tokio::task::spawn_blocking(get_commit_hash),
        tokio::task::spawn_blocking(get_commit_message),
        tokio::task::spawn_blocking(get_status_summary)
    );

    Ok(GitContext {
        is_repo: true,
        branch: branch.ok().flatten(),
        commit: commit.ok().flatten(),
        commit_message: commit_message.ok().flatten(),
        status_summary: Some(status_summary),
        untracked_count: None,
        modified_count: None,
        staged_count: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_repo_in_git_dir() {
        // This test assumes we're running in a git repo
        let in_repo = is_git_repo();
        // We should be in a git repo for these tests
        assert!(in_repo, "Tests should run in a git repository");
    }

    #[test]
    fn test_get_branch_name() {
        let branch = get_branch_name();
        assert!(branch.is_some());
        assert!(!branch.unwrap().is_empty());
    }

    #[test]
    fn test_get_commit_hash() {
        let hash = get_commit_hash();
        assert!(hash.is_some());
        // SHA-1 hash should be 40 characters
        let h = hash.unwrap();
        assert_eq!(h.len(), 40);
    }

    #[test]
    fn test_get_status_summary() {
        let summary = get_status_summary();
        assert!(!summary.is_empty());
    }
}
