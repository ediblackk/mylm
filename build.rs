use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // Always rerun if build.rs itself changes
    println!("cargo:rerun-if-changed=build.rs");

    let git_dir = Path::new(".git");
    if git_dir.exists() {
        // Watch HEAD for branch changes or direct commits
        println!("cargo:rerun-if-changed=.git/HEAD");

        // If HEAD is a symbolic ref, watch the actual ref file for commits
        if let Ok(head_content) = fs::read_to_string(".git/HEAD") {
            if head_content.starts_with("ref: ") {
                let ref_path = head_content.trim_start_matches("ref: ").trim();
                let full_ref_path = format!(".git/{}", ref_path);
                if Path::new(&full_ref_path).exists() {
                    println!("cargo:rerun-if-changed={}", full_ref_path);
                }
            }
        }
    }

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
