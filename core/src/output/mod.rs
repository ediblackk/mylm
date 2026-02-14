//! Output formatting module
//!
//! Handles formatting and display of AI responses, context information,
//! and system status using colored output.
//! test debug
use crate::context::TerminalContext;
// ConfigV2Ext removed - using native V2 API directly
use crate::llm::ChatResponse;
use console::Style;

/// Output formatter for CLI results
pub struct OutputFormatter {
    // Styles
    blue: Style,
    green: Style,
    yellow: Style,
    bold: Style,
}

impl Default for OutputFormatter {
    fn default() -> Self {
        Self {
            blue: Style::new().blue(),
            green: Style::new().green(),
            yellow: Style::new().yellow(),
            bold: Style::new().bold(),
        }
    }
}

impl OutputFormatter {
    /// Create a new formatter
    pub fn new() -> Self {
        Self::default()
    }

    /// Print the AI response
    
    pub fn print_response(&self, response: &ChatResponse) {
        println!();
        println!("{}", self.bold.apply_to("AI Response:"));
        println!("{}", response.content());
        println!();

        if let Some(usage) = &response.usage {
            println!("{}", self.blue.apply_to(format!("Tokens: {} (Prompt: {}, Completion: {})", 
                usage.total_tokens, usage.prompt_tokens, usage.completion_tokens)));
        }
    }

    /// Print the current context
    pub fn print_context(&self, ctx: &TerminalContext) {
        println!();
        println!("{}", self.bold.apply_to("Current Context:"));
        
        // System
        if let Some(os) = &ctx.os {
            println!("OS: {}", self.green.apply_to(os));
        }
        if let Some(cwd) = &ctx.cwd {
            println!("CWD: {}", self.green.apply_to(cwd.display()));
        }

        // Git
        if ctx.git.is_repo {
            println!("Git: {} ({})", 
                self.green.apply_to(ctx.git.branch.as_deref().unwrap_or("unknown")),
                self.yellow.apply_to(ctx.git.status_summary.as_deref().unwrap_or("clean"))
            );
        }

        // System Info
        println!("System: {}", self.blue.apply_to(ctx.system_summary()));
    }

    /// Print command analysis result
    pub fn print_command_analysis(&self, response: &ChatResponse) {
        println!();
        println!("{}", self.bold.apply_to("Command Analysis:"));
        println!("{}", response.content());
    }

    /// Print available endpoints
    pub fn print_endpoints(&self, config: &crate::config::ConfigV2) {
        println!();
        println!("{}", self.bold.apply_to("Current Configuration:"));
        let resolved = config.resolve_profile();
        println!("- Profile: {}", self.green.apply_to(&config.profile));
        println!("- Provider: {:?}", resolved.provider);
        println!("- Model: {}", resolved.model);
        if let Some(base_url) = &resolved.base_url {
            println!("- Base URL: {}", base_url);
        }
    }

    /// Print system information
    pub fn print_system_info(&self, ctx: &TerminalContext, brief: bool) {
        println!();
        println!("{}", self.bold.apply_to("System Information:"));
        
        if brief {
            println!("{}", ctx.system_summary());
        } else {
            // detailed info could be added here
             println!("OS: {}", ctx.os.as_deref().unwrap_or("unknown"));
             if let Some(cpu) = &ctx.system.cpu_count {
                 println!("CPU Cores: {}", cpu);
             }
             if let Some(mem) = &ctx.system.total_memory {
                 println!("Total Memory: {} bytes", mem);
             }
        }
    }
}
