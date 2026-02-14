//! JSONL (JSON Lines) file utilities for PaCoRe.
//!
//! Async read/write operations for JSON Lines format, commonly used
//! for storing and loading batch datasets for LLM experiments.
//!
//! # Functions
//! - `load_jsonl`: Read a JSONL file into a vector of typed records
//! - `save_jsonl`: Write a slice of records to a JSONL file

use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::{de::DeserializeOwned, Serialize};
use crate::pacore::error::Error;

pub async fn load_jsonl<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<Vec<T>, Error> {
    let file = File::open(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut results = Vec::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str(&line)?;
        results.push(item);
    }

    Ok(results)
}

pub async fn save_jsonl<T: Serialize>(path: impl AsRef<Path>, items: &[T]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .await?;

    for item in items {
        let json = serde_json::to_string(item)?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }

    file.flush().await?;
    Ok(())
}
