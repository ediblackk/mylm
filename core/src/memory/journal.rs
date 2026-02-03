use std::fs::{self, OpenOptions};
use std::io::{Write, BufRead, BufReader};
use std::path::PathBuf;
use chrono::Utc;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InteractionType {
    Thought,
    Tool,
    Output,
    Chat,
}

impl std::fmt::Display for InteractionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InteractionType::Thought => write!(f, "Thought"),
            InteractionType::Tool => write!(f, "Tool"),
            InteractionType::Output => write!(f, "Output"),
            InteractionType::Chat => write!(f, "Chat"),
        }
    }
}

impl From<&str> for InteractionType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "thought" => InteractionType::Thought,
            "tool" | "action" => InteractionType::Tool,
            "output" | "observation" => InteractionType::Output,
            _ => InteractionType::Chat,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub timestamp: String,
    pub entry_type: InteractionType,
    pub content: String,
}

pub struct Journal {
    path: PathBuf,
    entries: Vec<JournalEntry>,
}

impl Journal {
    pub fn new() -> Result<Self> {
        let data_dir = dirs::data_dir()
            .context("Could not find data directory")?
            .join("mylm")
            .join("journals");
        fs::create_dir_all(&data_dir)?;

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let path = data_dir.join(format!("{}.md", today));

        let mut journal = Self {
            path,
            entries: Vec::new(),
        };

        if !journal.path.exists() {
            let mut file = fs::File::create(&journal.path)?;
            writeln!(file, "# Journal - {}\n", today)?;
        } else {
            journal.load_today()?;
        }

        Ok(journal)
    }

    /// Create a journal at a specific path (for incognito mode or custom locations)
    pub fn with_path(path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Create file with header if it doesn't exist
        if !path.exists() {
            let mut file = fs::File::create(&path)?;
            let today = Utc::now().format("%Y-%m-%d").to_string();
            writeln!(file, "# Journal - {}\n", today)?;
        }

        let mut journal = Self {
            path,
            entries: Vec::new(),
        };
        journal.load_today()?;

        Ok(journal)
    }

    pub fn load_today(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let file = fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        
        let mut current_timestamp = String::new();
        let mut current_type = InteractionType::Chat;
        let mut current_content = Vec::new();
        let mut in_entry = false;

        for line_res in reader.lines() {
            let line = line_res?;
            if line.starts_with("### [") {
                // Save previous entry
                if in_entry {
                    entries.push(JournalEntry {
                        timestamp: current_timestamp.clone(),
                        entry_type: current_type.clone(),
                        content: current_content.join("\n").trim().to_string(),
                    });
                    current_content.clear();
                }

                // Parse new header: ### [HH:MM:SS] Type
                if let Some(close_bracket) = line.find(']') {
                    current_timestamp = line[5..close_bracket].to_string();
                    let type_str = line[close_bracket + 2..].trim();
                    current_type = InteractionType::from(type_str);
                    in_entry = true;
                }
            } else if line == "---" {
                if in_entry {
                    entries.push(JournalEntry {
                        timestamp: current_timestamp.clone(),
                        entry_type: current_type.clone(),
                        content: current_content.join("\n").trim().to_string(),
                    });
                    current_content.clear();
                    in_entry = false;
                }
            } else if in_entry {
                current_content.push(line);
            }
        }

        // Catch last one if no trailing ---
        if in_entry && !current_content.is_empty() {
             entries.push(JournalEntry {
                timestamp: current_timestamp,
                entry_type: current_type,
                content: current_content.join("\n").trim().to_string(),
            });
        }

        self.entries = entries;
        Ok(())
    }

    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    pub fn log(&mut self, entry_type: InteractionType, content: &str) -> Result<()> {
        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        
        writeln!(file, "### [{}] {}\n{}\n---\n", timestamp, entry_type, content)?;
        
        self.entries.push(JournalEntry {
            timestamp,
            entry_type,
            content: content.to_string(),
        });

        Ok(())
    }

    pub fn log_interaction(&mut self, thought: Option<&str>, tool: Option<&str>, args: Option<&str>, output: Option<&str>) -> Result<()> {
        if let Some(t) = thought {
            self.log(InteractionType::Thought, t)?;
        }

        if let Some(tool_name) = tool {
            let content = if let Some(a) = args {
                format!("Action: `{}`\nArguments:\n```json\n{}\n```", tool_name, a)
            } else {
                format!("Action: `{}`", tool_name)
            };
            self.log(InteractionType::Tool, &content)?;
        }

        if let Some(obs) = output {
            self.log(InteractionType::Output, obs)?;
        }

        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        // Truncate the file but keep the header
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut file = fs::File::create(&self.path)?;
        writeln!(file, "# Journal - {}\n", today)?;
        Ok(())
    }
}
