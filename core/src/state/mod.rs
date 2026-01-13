use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use serde_json::Value;

pub struct StateStore {
    data: HashMap<String, Value>,
    path: PathBuf,
}

impl StateStore {
    pub fn new() -> Result<Self> {
        let path = dirs::data_dir()
            .context("Could not find data directory")?
            .join("mylm")
            .join("state.json");
        
        let mut store = Self {
            data: HashMap::new(),
            path,
        };
        
        store.load()?;
        Ok(store)
    }

    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.path)?;
        if content.trim().is_empty() {
            return Ok(());
        }

        self.data = serde_json::from_str(&content)
            .context("Failed to parse state file")?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, content)
            .context("Failed to write state file")?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }

    pub fn set(&mut self, key: String, value: Value) -> Result<()> {
        self.data.insert(key, value);
        self.save()
    }

    pub fn delete(&mut self, key: &str) -> Result<()> {
        self.data.remove(key);
        self.save()
    }

    pub fn list(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }
}
