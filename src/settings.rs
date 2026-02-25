//! Settings Menu Handlers
//!
//! All settings UI flows consolidated here to keep main.rs clean.
//! Main.rs just calls these functions from the hub menu loop.

use anyhow::Result;
use mylm_core::config::Config;
use std::io::Write;

use crate::hub;
use crate::hub::{SettingsMenuChoice, MainLLMSettingsChoice, WorkerLLMSettingsChoice};
use crate::hub::{ContextSettingsChoice, AgenticSettingsChoice, PaCoReSubSettingsChoice};
use crate::hub::{ProviderMenuChoice, ApplicationSettingsChoice, MemoryManagementChoice};

/// ============================================================================
/// SETTINGS DASHBOARD - Main entry point
/// ============================================================================

pub async fn run_settings_dashboard(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_settings_dashboard(config)? {
            SettingsMenuChoice::ManageProviders => {
                run_provider_menu(config).await?;
            }
            SettingsMenuChoice::MainLLMSettings => {
                run_main_llm_settings(config).await?;
            }
            SettingsMenuChoice::WorkerLLMSettings => {
                run_worker_llm_settings(config).await?;
            }
            SettingsMenuChoice::TestMainConnection => {
                hub::test_profile_connection(config, &config.active_profile.clone()).await?;
            }
            SettingsMenuChoice::TestWorkerConnection => {
                hub::test_profile_connection(config, "worker").await?;
            }
            SettingsMenuChoice::WebSearchSettings => {
                run_web_search_menu(config).await?;
            }
            SettingsMenuChoice::ApplicationSettings => {
                run_application_settings(config).await?;
            }
            SettingsMenuChoice::MemoryManagement => {
                run_memory_management(config).await?;
            }
            SettingsMenuChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// MAIN LLM SETTINGS
/// ============================================================================

pub async fn run_main_llm_settings(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_main_llm_settings_menu(config)? {
            MainLLMSettingsChoice::SelectModel => {
                hub::handle_select_main_model(config).await?;
            }
            MainLLMSettingsChoice::ContextSettings => {
                loop {
                    match hub::show_context_settings_menu(true)? {
                        ContextSettingsChoice::SetMaxTokens => {
                            hub::set_max_tokens(config, true)?;
                        }
                        ContextSettingsChoice::SetCondenseThreshold => {
                            hub::set_condense_threshold(config, true)?;
                        }
                        ContextSettingsChoice::SetInputPrice => {
                            hub::set_input_price(config, true)?;
                        }
                        ContextSettingsChoice::SetOutputPrice => {
                            hub::set_output_price(config, true)?;
                        }
                        ContextSettingsChoice::SetRateLimit => {
                            hub::set_rate_limit_rpm(config, true)?;
                        }
                        ContextSettingsChoice::Back => break,
                    }
                }
            }
            MainLLMSettingsChoice::AgenticSettings => {
                loop {
                    match hub::show_agentic_settings_menu(true)? {
                        AgenticSettingsChoice::SetAllowedCommands => {
                            hub::set_allowed_commands(config, true)?;
                        }
                        AgenticSettingsChoice::SetRestrictedCommands => {
                            hub::set_restricted_commands(config, true)?;
                        }
                        AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                            hub::set_max_actions_before_stall(config, true)?;
                        }
                        AgenticSettingsChoice::PaCoReSettings => {
                            loop {
                                match hub::show_pacore_sub_settings_menu()? {
                                    PaCoReSubSettingsChoice::ToggleEnabled => {
                                        hub::toggle_pacore_enabled(config, true)?;
                                    }
                                    PaCoReSubSettingsChoice::SetRounds => {
                                        hub::set_pacore_rounds(config, true)?;
                                    }
                                    PaCoReSubSettingsChoice::Back => break,
                                }
                            }
                        }
                        AgenticSettingsChoice::Back => break,
                    }
                }
            }
            MainLLMSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// WORKER LLM SETTINGS
/// ============================================================================

pub async fn run_worker_llm_settings(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_worker_llm_settings_menu(config)? {
            WorkerLLMSettingsChoice::SelectModel => {
                hub::handle_select_worker_model(config).await?;
            }
            WorkerLLMSettingsChoice::ContextSettings => {
                loop {
                    match hub::show_context_settings_menu(false)? {
                        ContextSettingsChoice::SetMaxTokens => {
                            hub::set_max_tokens(config, false)?;
                        }
                        ContextSettingsChoice::SetCondenseThreshold => {
                            hub::set_condense_threshold(config, false)?;
                        }
                        ContextSettingsChoice::SetInputPrice => {
                            hub::set_input_price(config, false)?;
                        }
                        ContextSettingsChoice::SetOutputPrice => {
                            hub::set_output_price(config, false)?;
                        }
                        ContextSettingsChoice::SetRateLimit => {
                            hub::set_rate_limit_rpm(config, false)?;
                        }
                        ContextSettingsChoice::Back => break,
                    }
                }
            }
            WorkerLLMSettingsChoice::AgenticSettings => {
                loop {
                    match hub::show_agentic_settings_menu(false)? {
                        AgenticSettingsChoice::SetAllowedCommands => {
                            hub::set_allowed_commands(config, false)?;
                        }
                        AgenticSettingsChoice::SetRestrictedCommands => {
                            hub::set_restricted_commands(config, false)?;
                        }
                        AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                            hub::set_max_actions_before_stall(config, false)?;
                        }
                        AgenticSettingsChoice::PaCoReSettings => {
                            loop {
                                match hub::show_pacore_sub_settings_menu()? {
                                    PaCoReSubSettingsChoice::ToggleEnabled => {
                                        hub::toggle_pacore_enabled(config, false)?;
                                    }
                                    PaCoReSubSettingsChoice::SetRounds => {
                                        hub::set_pacore_rounds(config, false)?;
                                    }
                                    PaCoReSubSettingsChoice::Back => break,
                                }
                            }
                        }
                        AgenticSettingsChoice::Back => break,
                    }
                }
            }
            WorkerLLMSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// PROVIDER MENU
/// ============================================================================

pub async fn run_provider_menu(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_provider_menu()? {
            ProviderMenuChoice::AddProvider => {
                hub::handle_add_provider(config).await?;
            }
            ProviderMenuChoice::EditProvider => {
                hub::handle_edit_provider(config).await?;
            }
            ProviderMenuChoice::RemoveProvider => {
                hub::handle_remove_provider(config)?;
            }
            ProviderMenuChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// WEB SEARCH MENU
/// ============================================================================

pub async fn run_web_search_menu(config: &mut Config) -> Result<()> {
    // Sync features.web_search with profile.web_search.enabled on entry
    let profile_enabled = config.active_profile().web_search.enabled;
    if config.features.web_search != profile_enabled {
        log::debug!("[CONFIG] Syncing web_search: features={} profile={}", 
            config.features.web_search, profile_enabled);
        config.features.web_search = profile_enabled;
    }
    
    // Use the unified web search settings handler
    hub::handle_web_search_settings(config).await?;
    Ok(())
}

/// ============================================================================
/// APPLICATION SETTINGS
/// ============================================================================

pub async fn run_application_settings(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_application_settings_menu(config)? {
            ApplicationSettingsChoice::ToggleTmuxAutostart => {
                config.app.tmux_enabled = !config.app.tmux_enabled;
                config.save_default()?;
                println!("\n✅ Tmux autostart {}", 
                    if config.app.tmux_enabled { "enabled" } else { "disabled" });
            }
            ApplicationSettingsChoice::SetPreferredAlias => {
                let alias: String = dialoguer::Input::new()
                    .with_prompt("Enter preferred alias")
                    .default(config.active_profile.clone())
                    .interact()?;
                println!("\n[STUB] Set preferred alias to: {}\n", alias);
            }
            ApplicationSettingsChoice::SetSandboxDirectory => {
                if let Err(e) = hub::set_sandbox_directory() {
                    eprintln!("\n❌ Error setting sandbox: {}", e);
                }
            }
            ApplicationSettingsChoice::ToggleSandboxForMain => {
                if let Err(e) = hub::toggle_sandbox_for_main() {
                    eprintln!("\n❌ Error toggling sandbox: {}", e);
                }
            }
            ApplicationSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// MEMORY MANAGEMENT
/// ============================================================================

pub async fn run_memory_management(config: &mut Config) -> Result<()> {
    loop {
        match hub::show_memory_management_menu()? {
            MemoryManagementChoice::ViewMemoryStats => {
                show_memory_stats().await;
            }
            MemoryManagementChoice::ExportArchive => {
                if let Err(e) = export_memories().await {
                    eprintln!("\n❌ Export failed: {}", e);
                }
            }
            MemoryManagementChoice::DeleteAll => {
                if let Err(e) = delete_all_memories().await {
                    eprintln!("\n❌ Delete failed: {}", e);
                }
            }
            MemoryManagementChoice::ImportMemories => {
                if let Err(e) = import_memories().await {
                    eprintln!("\n❌ Import failed: {}", e);
                }
            }
            MemoryManagementChoice::Back => break,
        }
    }
    Ok(())
}

/// Show memory statistics
async fn show_memory_stats() {
    use mylm_core::config::agent::MemoryConfig;
    use mylm_core::agent::memory::AgentMemoryManager;
    
    println!("\n📊 Memory Statistics");
    println!("{}", "─".repeat(40));
    
    let memory_path = dirs::data_dir()
        .map(|d| d.join("mylm").join("memory"))
        .unwrap_or_else(|| std::path::PathBuf::from("unknown"));
    
    println!("Storage path: {}", memory_path.display());
    
    // Try to get actual count from manager
    let memory_config = MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    };
    
    match AgentMemoryManager::new(memory_config).await {
        Ok(manager) => {
            match manager.stats().await {
                Ok(stats) => {
                    println!("Total memories: {}", stats.total_memories);
                    println!("Context window: {}", stats.recent_memories);
                    println!("Mode: {:?}", stats.mode);
                    println!("Enabled: {}", stats.enabled);
                }
                Err(e) => {
                    println!("Could not load stats: {}", e);
                }
            }
        }
        Err(e) => {
            println!("Could not connect to memory store: {}", e);
        }
    }
    
    // Check directory size
    if let Ok(metadata) = std::fs::metadata(&memory_path) {
        if metadata.is_dir() {
            let size = calculate_dir_size(&memory_path);
            println!("Storage size: ~{} MB", size / 1024 / 1024);
        }
    }
    
    println!();
    dialoguer::Input::<String>::new()
        .with_prompt("Press Enter to continue")
        .allow_empty(true)
        .interact()
        .ok();
}

/// Calculate directory size in bytes
fn calculate_dir_size(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let path = e.path();
                    if path.is_file() {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    } else if path.is_dir() {
                        calculate_dir_size(&path)
                    } else {
                        0
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Export all memories to JSON file
async fn export_memories() -> Result<()> {
    use mylm_core::config::agent::MemoryConfig;
    use mylm_core::agent::memory::AgentMemoryManager;
    use std::io::Write;
    
    println!("\n💾 Exporting Memories");
    println!("{}", "─".repeat(40));
    
    // Get output path
    let default_name = format!("mylm_memory_backup_{}.json", 
        chrono::Local::now().format("%Y%m%d_%H%M%S"));
    
    let export_path: String = dialoguer::Input::new()
        .with_prompt("Export file path")
        .default(default_name)
        .interact()?;
    
    let export_path = std::path::PathBuf::from(export_path);
    
    // Confirm if file exists
    if export_path.exists() {
        let overwrite = dialoguer::Confirm::new()
            .with_prompt("File exists. Overwrite?")
            .default(false)
            .interact()?;
        if !overwrite {
            println!("Export cancelled.");
            return Ok(());
        }
    }
    
    // Connect to memory
    let memory_config = MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    };
    
    let manager = AgentMemoryManager::new(memory_config).await?;
    
    // Get all memories (load with large limit)
    println!("Loading memories...");
    let memories = manager.get_recent_memories(10000).await?;
    
    println!("Found {} memories to export", memories.len());
    
    // Serialize to JSON
    let json = serde_json::to_string_pretty(&memories)?;
    
    // Write to file
    let mut file = std::fs::File::create(&export_path)?;
    file.write_all(json.as_bytes())?;
    
    println!("✅ Exported {} memories to {}", memories.len(), export_path.display());
    
    // Show file size
    if let Ok(metadata) = std::fs::metadata(&export_path) {
        let size_kb = metadata.len() / 1024;
        println!("   File size: {} KB", size_kb);
    }
    
    Ok(())
}

/// Delete all memories with confirmation
async fn delete_all_memories() -> Result<()> {
    use mylm_core::config::agent::MemoryConfig;
    use mylm_core::agent::memory::AgentMemoryManager;
    
    println!("\n🗑️  Delete All Memories");
    println!("{}", "─".repeat(40));
    
    // Get memory path
    let memory_path = dirs::data_dir()
        .map(|d| d.join("mylm").join("memory"))
        .unwrap_or_else(|| std::path::PathBuf::from("unknown"));
    
    // Get count first
    let memory_config = MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    };
    
    let manager = AgentMemoryManager::new(memory_config).await?;
    let stats = manager.stats().await?;
    
    println!("⚠️  WARNING: This will delete ALL {} memories!", stats.total_memories);
    println!("   Storage location: {}", memory_path.display());
    println!();
    
    // Double confirmation
    let confirm1 = dialoguer::Confirm::new()
        .with_prompt("Are you sure you want to delete ALL memories?")
        .default(false)
        .interact()?;
    
    if !confirm1 {
        println!("Delete cancelled.");
        return Ok(());
    }
    
    // Type confirmation
    let typed: String = dialoguer::Input::new()
        .with_prompt("Type 'DELETE' to confirm")
        .interact()?;
    
    if typed != "DELETE" {
        println!("Delete cancelled.");
        return Ok(());
    }
    
    // Close manager connection first
    drop(manager);
    
    // Delete the entire memory directory
    println!("Deleting memory storage...");
    if memory_path.exists() {
        std::fs::remove_dir_all(&memory_path)?;
    }
    
    // Recreate empty directory
    std::fs::create_dir_all(&memory_path)?;
    
    println!("✅ All memories deleted. Storage reset.");
    println!("   New memories will be created on next use.");
    
    Ok(())
}

/// Import memories from JSON file
async fn import_memories() -> Result<()> {
    use mylm_core::config::agent::MemoryConfig;
    use mylm_core::agent::memory::AgentMemoryManager;
    use mylm_core::memory::store::MemoryType;
    
    println!("\n📥 Import Memories");
    println!("{}", "─".repeat(40));
    
    // Get import path
    let import_path: String = dialoguer::Input::new()
        .with_prompt("Import file path")
        .interact()?;
    
    let import_path = std::path::PathBuf::from(import_path);
    
    if !import_path.exists() {
        eprintln!("❌ File not found: {}", import_path.display());
        return Ok(());
    }
    
    // Read JSON
    println!("Reading file...");
    let json = std::fs::read_to_string(&import_path)?;
    
    // Parse memories
    println!("Parsing memories...");
    let memories: Vec<mylm_core::memory::store::Memory> = serde_json::from_str(&json)?;
    
    println!("Found {} memories to import", memories.len());
    
    if memories.is_empty() {
        println!("No memories to import.");
        return Ok(());
    }
    
    // Confirm
    let proceed = dialoguer::Confirm::new()
        .with_prompt("Proceed with import?")
        .default(true)
        .interact()?;
    
    if !proceed {
        println!("Import cancelled.");
        return Ok(());
    }
    
    // Connect to memory
    let memory_config = MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    };
    
    let manager = AgentMemoryManager::new(memory_config).await?;
    
    // Import each memory
    println!("Importing...");
    let mut imported = 0;
    let mut failed = 0;
    
    for memory in memories {
        match manager.add_memory_full(
            &memory.content,
            memory.r#type,
            memory.session_id.clone(),
            memory.metadata.clone(),
            memory.category_id.clone(),
            memory.summary.clone(),
        ).await {
            Ok(_) => imported += 1,
            Err(_) => failed += 1,
        }
        
        // Progress every 100
        if imported % 100 == 0 {
            print!("\r   Progress: {} imported", imported);
            std::io::stdout().flush().ok();
        }
    }
    
    println!("\r✅ Import complete: {} imported, {} failed", imported, failed);
    
    Ok(())
}
