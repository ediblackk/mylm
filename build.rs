use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    
    let git_dir = Path::new(".git");
    if git_dir.exists() {
        println!("cargo:rerun-if-changed=.git/HEAD");
        
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

    // DON'T watch .build_number - that creates a loop!
    // Remove this line:
    // println!("cargo:rerun-if-changed=.build_number");

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

    // Only increment when git hash actually changes
    let build_number = get_build_number_for_hash(&git_hash);
    
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=BUILD_NUMBER={}", build_number);
}

fn get_build_number_for_hash(git_hash: &str) -> u64 {
    let build_file = Path::new(".build_number");
    let hash_file = Path::new(".build_hash");
    
    // Check if hash has changed
    let last_hash = fs::read_to_string(hash_file).unwrap_or_default();
    
    let current_build = if build_file.exists() {
        fs::read_to_string(build_file)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    
    // Only increment if hash changed
    if last_hash.trim() != git_hash {
        let new_build = current_build + 1;
        let _ = fs::write(build_file, new_build.to_string());
        let _ = fs::write(hash_file, git_hash);
        new_build
    } else {
        current_build
    }
}