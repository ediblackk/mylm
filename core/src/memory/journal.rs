use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use chrono::Utc;
use anyhow::{Context, Result};

pub struct Journal {
    path: PathBuf,
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

        if !path.exists() {
            let mut file = fs::File::create(&path)?;
            writeln!(file, "# Journal - {}\n", today)?;
        }

        Ok(Self { path })
    }

    pub fn log(&self, role: &str, content: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        
        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        writeln!(file, "### [{}] {}\n{}\n", timestamp, role, content)?;
        Ok(())
    }

    pub fn log_interaction(&self, thought: Option<&str>, tool: Option<&str>, args: Option<&str>, output: Option<&str>) -> Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        
        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        
        writeln!(file, "## Interaction [{}]\n", timestamp)?;
        
        if let Some(t) = thought {
            writeln!(file, "**Thought**: {}\n", t)?;
        }

        if let Some(tool_name) = tool {
            writeln!(file, "**Action**: `{}`", tool_name)?;
            if let Some(a) = args {
                writeln!(file, "```json\n{}\n```\n", a)?;
            }
        }

        if let Some(obs) = output {
            writeln!(file, "**Observation**:\n```\n{}\n```\n", obs)?;
        }
        
        writeln!(file, "---\n")?;

        Ok(())
    }
}
