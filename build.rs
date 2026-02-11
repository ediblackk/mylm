//! Build script for mylm - generates version and build metadata.
//!
//! Computes source code hash and increments build numbers on changes.
//! Captures git commit hash for version tracking in the compiled binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use sha2::{Sha256, Digest};
use std::io::{self, Read};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=core/src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=core/Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    
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

    // Compute source hash to determine if we should increment build number
    let current_hash = compute_source_hash().expect("Failed to compute source hash");
    let build_number = get_or_increment_build_number(&current_hash);
    
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=BUILD_NUMBER={}", build_number);
}

fn compute_source_hash() -> io::Result<String> {
    let mut hasher = Sha256::new();
    
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("core/Cargo.toml"),
    ];
    
    collect_files(Path::new("src"), &mut files)?;
    collect_files(Path::new("core/src"), &mut files)?;
    files.sort(); // Ensure deterministic order

    for file_path in files {
        if file_path.exists() && file_path.is_file() {
            let mut file = fs::File::open(file_path)?;
            let mut buffer = [0u8; 8192];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 { break; }
                hasher.update(&buffer[..n]);
            }
        }
    }
    
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, files)?;
            } else {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn get_or_increment_build_number(current_hash: &str) -> u64 {
    let hash_file = Path::new(".build_hash");
    let build_file = Path::new(".build_number");
    
    let last_hash = fs::read_to_string(hash_file).unwrap_or_default();
    
    let current_build = if build_file.exists() {
        fs::read_to_string(build_file)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        0
    };

    if current_hash != last_hash {
        let new_build = current_build + 1;
        let _ = fs::write(build_file, new_build.to_string());
        let _ = fs::write(hash_file, current_hash);
        new_build
    } else {
        current_build
    }
}
