use crate::agent_old::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use std::error::Error as StdError;
use tokio::time::{sleep, Duration};

/// A tool for pausing agent execution.
/// Useful for waiting for background tasks to progress.
pub struct WaitTool;

#[async_trait]
impl Tool for WaitTool {
    fn name(&self) -> &str {
        "wait"
    }

    fn description(&self) -> &str {
        "Wait for a specified number of seconds. Use this when monitoring background tasks to allow them to progress before checking again."
    }

    fn usage(&self) -> &str {
        "Pass the number of seconds to wait. Example: '5'."
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let seconds: u64 = args.trim().parse().unwrap_or(2);
        let seconds = seconds.clamp(1, 60); // Safety limit

        sleep(Duration::from_secs(seconds)).await;

        Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
            "Waited for {} seconds.",
            seconds
        ))))
    }
}
