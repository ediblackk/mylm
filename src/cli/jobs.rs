use anyhow::Result;
use inquire::Select as InquireSelect;
use mylm_core::config::Config;
use mylm_core::agent::v2::jobs::{JobRegistry, JobStatus};
use console::Style;

#[derive(Debug, Clone, PartialEq)]
enum JobsHubChoice {
    ListJobs,
    CreateJob,
    Back,
}

impl std::fmt::Display for JobsHubChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobsHubChoice::ListJobs => write!(f, "List Jobs"),
            JobsHubChoice::CreateJob => write!(f, "Create Job"),
            JobsHubChoice::Back => write!(f, "Back"),
        }
    }
}

fn show_jobs_hub() -> Result<JobsHubChoice> {
    let options = vec![JobsHubChoice::ListJobs, JobsHubChoice::CreateJob, JobsHubChoice::Back];
    let ans: Result<JobsHubChoice, _> =
        InquireSelect::new("Background Jobs", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(JobsHubChoice::Back),
    }
}

pub async fn handle_jobs_dashboard(_config: &mut Config, job_registry: &JobRegistry) -> Result<()> {
    loop {
        match show_jobs_hub()? {
            JobsHubChoice::ListJobs => handle_list_jobs(job_registry).await?,
            JobsHubChoice::CreateJob => handle_create_job(job_registry).await?,
            JobsHubChoice::Back => break,
        }
    }

    Ok(())
}

pub async fn handle_list_jobs(job_registry: &JobRegistry) -> Result<()> {
    let jobs = job_registry.list_all_jobs();
    
    if jobs.is_empty() {
        println!("\n📭 No background jobs found.");
        return Ok(());
    }

    let blue = Style::new().blue().bold();
    let dim = Style::new().dim();
    let green = Style::new().green();
    let red = Style::new().red();
    let yellow = Style::new().yellow();

    println!("\n📋 Background Jobs");
    println!("{}", blue.apply_to("-".repeat(100)));
    println!("{:<36} | {:<15} | {:<10} | {:<19} | {:<19}",
        "Job ID", "Tool", "Status", "Started At", "Finished At");
    println!("{}", blue.apply_to("-".repeat(100)));

    for job in jobs {
        let status_str = match job.status {
            JobStatus::Running => yellow.apply_to("Running"),
            JobStatus::Completed => green.apply_to("Completed"),
            JobStatus::Failed => red.apply_to("Failed"),
        };

        let started_at = job.started_at.format("%Y-%m-%d %H:%M:%S").to_string();
        let finished_at = job.finished_at
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());

        println!("{:<36} | {:<15} | {:<10} | {:<19} | {:<19}",
            job.id,
            job.tool_name,
            status_str,
            started_at,
            finished_at
        );
        
        println!("  {} {}", dim.apply_to("Description:"), job.description);
        
        if !job.output.is_empty() {
            let preview = if job.output.len() > 80 {
                format!("{}...", &job.output[..77].replace('\n', " "))
            } else {
                job.output.replace('\n', " ")
            };
            println!("  {} {}", dim.apply_to("Output:"), preview);
        }

        if let Some(err) = job.error {
            println!("  {} {}", red.apply_to("Error:"), err);
        }
        
        println!("{}", dim.apply_to("-".repeat(100)));
    }

    Ok(())
}

pub async fn handle_create_job(job_registry: &JobRegistry) -> Result<()> {
    let tool_name = inquire::Text::new("Tool Name:").with_default("test_tool").prompt()?;
    let description = inquire::Text::new("Description:").with_default("Manual test job").prompt()?;

    if tool_name.trim().is_empty() {
        println!("❌ Tool name cannot be empty.");
        return Ok(());
    }

    let id = job_registry.create_job(&tool_name, &description);
    println!("✅ Job created successfully with ID: {}", Style::new().cyan().apply_to(&id));
    
    // Simulate some activity for the test job
    job_registry.update_job_output(&id, "Job initialized...\n");
    
    if dialoguer::Confirm::new()
        .with_prompt("Mark as completed immediately?")
        .default(true)
        .interact()?
    {
        job_registry.complete_job(&id, serde_json::json!({"status": "success"}));
        println!("✅ Job marked as completed.");
    }

    Ok(())
}
