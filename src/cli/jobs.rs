use anyhow::Result;
use inquire::Select as InquireSelect;
use mylm_core::config::Config;

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

pub async fn handle_jobs_dashboard(_config: &mut Config) -> Result<()> {
    loop {
        match show_jobs_hub()? {
            JobsHubChoice::ListJobs => handle_list_jobs().await?,
            JobsHubChoice::CreateJob => handle_create_job().await?,
            JobsHubChoice::Back => break,
        }
    }

    Ok(())
}

pub async fn handle_list_jobs() -> Result<()> {
    println!("Not implemented");
    Ok(())
}

pub async fn handle_create_job() -> Result<()> {
    println!("Not implemented");
    Ok(())
}
