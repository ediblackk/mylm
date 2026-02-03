use super::*;
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

// NOTE: Some tests mutate process-wide environment variables and/or the
// current working directory. Guard those changes to avoid flakiness when
// the test runner executes in parallel.
lazy_static! {
    static ref ENV_LOCK: Mutex<()> = Mutex::new(());
}

#[test]
fn test_default_config() {
    let config = ConfigV2::default();
    assert_eq!(config.profile, "fast");
    assert!(config.profiles.is_empty());
}

#[test]
fn test_default_endpoint() {
    let endpoint = EndpointConfig::default();
    assert_eq!(endpoint.provider, Provider::Openai);
    assert_eq!(endpoint.model, "default-model");
    assert_eq!(endpoint.timeout_secs, 30);
    assert!(endpoint.base_url.is_none());
    assert!(endpoint.api_key.is_none());
}

#[test]
fn test_profile_override() {
    let profile = Profile {
        endpoint: Some(EndpointOverride {
            model: Some("test-model-override".to_string()),
            api_key: None,
        }),
        agent: Some(AgentOverride {
            max_iterations: Some(5),
            main_model: None,
            worker_model: Some("test-model-worker".to_string()),
            ..Default::default()
        }),
    };

    assert_eq!(profile.endpoint.as_ref().unwrap().model.as_ref().unwrap(), "test-model-override");
    assert_eq!(profile.agent.as_ref().unwrap().max_iterations, Some(5));
}

#[test]
fn test_features_default() {
    let features = FeaturesConfig::default();
    assert!(!features.web_search.enabled);
    assert!(features.memory.enabled);
    assert!(features.memory.auto_record);
    assert!(features.memory.auto_context);
}

#[test]
fn test_provider_serialization() {
    // Test that providers serialize to snake_case
    let providers = vec![
        (Provider::Openai, "openai"),
        (Provider::Google, "google"),
        (Provider::Ollama, "ollama"),
        (Provider::Openrouter, "openrouter"),
        (Provider::Kimi, "kimi"),
        (Provider::Custom, "custom"),
    ];

    for (provider, expected) in providers {
        let json = serde_json::to_string(&provider).unwrap();
        assert!(json.contains(expected), "Provider {:?} should serialize to {}", provider, expected);
    }
}

#[test]
fn test_load_default_config() {
    // When no config files exist, load() should return default config.
    //
    // IMPORTANT: `load()` checks *real* filesystem locations (cwd and
    // ~/.config/mylm/mylm.toml). On developer machines, that user config
    // may exist, which would make this test flaky unless we isolate it.
    let _guard = ENV_LOCK.lock().unwrap();

    let original_dir = std::env::current_dir().unwrap();
    let original_home = std::env::var_os("HOME");

    // Create an empty, isolated HOME + cwd.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_home = std::env::temp_dir().join(format!("mylm_test_home_{nanos}"));
    fs::create_dir_all(&temp_home).unwrap();
    std::env::set_var("HOME", &temp_home);
    std::env::set_current_dir(&temp_home).unwrap();

    // Sanity: ensure no local config exists.
    assert!(!Path::new("mylm.toml").exists());

    let result = ConfigV2::load();
    assert!(result.is_ok());
    let config = result.unwrap();

    assert_eq!(config.profile, "fast");
    assert_eq!(config.endpoint.provider, Provider::Openai);
    assert_eq!(config.endpoint.model, "default-model");

    // Restore global process state.
    std::env::set_current_dir(&original_dir).unwrap();
    match original_home {
        Some(val) => std::env::set_var("HOME", val),
        None => std::env::remove_var("HOME"),
    }

    let _ = fs::remove_dir_all(&temp_home);
}

#[test]
fn test_save_and_load_roundtrip() {
    // Create a temporary directory for the test
    let temp_dir = std::env::temp_dir().join("mylm_test_config");
    fs::create_dir_all(&temp_dir).unwrap();
    
    // Create a config with custom values
    let config = ConfigV2 {
        profile: "test".to_string(),
        endpoint: EndpointConfig {
            provider: Provider::Google,
            model: "gemini-pro".to_string(),
            api_key: Some("test-api-key".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    
    // Save the config
    let config_path = temp_dir.join("test_mylm.toml");
    config.save(Some(&config_path)).unwrap();
    
    // Read the file and verify it contains expected content
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("profile = \"test\""));
    assert!(content.contains("provider = \"google\""));
    assert!(content.contains("model = \"gemini-pro\""));
    
    // Clean up
    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_toml_parsing() {
    let toml_str = r#"
profile = "thorough"

[endpoint]
provider = "kimi"
model = "moonshot-v1-128k"
base_url = "https://api.moonshot.cn/v1"

[profiles.thorough]
endpoint = { model = "moonshot-v1-32k" }
agent = { max_iterations = 20 }
"#;

    let config: ConfigV2 = toml::from_str(toml_str).unwrap();
    assert_eq!(config.profile, "thorough");
    assert_eq!(config.endpoint.provider, Provider::Kimi);
    assert_eq!(config.endpoint.model, "moonshot-v1-128k");
    assert_eq!(config.endpoint.base_url, Some("https://api.moonshot.cn/v1".to_string()));
    
    let thorough_profile = config.profiles.get("thorough").unwrap();
    assert_eq!(thorough_profile.endpoint.as_ref().unwrap().model, Some("moonshot-v1-32k".to_string()));
    assert_eq!(thorough_profile.agent.as_ref().unwrap().max_iterations, Some(20));
}

#[test]
fn test_environment_variable_overrides() {
    // Set environment variables
    env::set_var("MYLM_PROFILE", "custom_profile");
    env::set_var("MYLM_PROVIDER", "ollama");
    env::set_var("MYLM_MODEL", "llama2");
    env::set_var("MYLM_API_KEY", "secret-key");
    env::set_var("MYLM_BASE_URL", "http://localhost:11434");
    env::set_var("MYLM_MAX_ITERATIONS", "25");

    let mut config = ConfigV2::default();
    config.apply_env_overrides();

    assert_eq!(config.profile, "custom_profile");
    assert_eq!(config.endpoint.provider, Provider::Ollama);
    assert_eq!(config.endpoint.model, "llama2");
    assert_eq!(config.endpoint.api_key, Some("secret-key".to_string()));
    assert_eq!(config.endpoint.base_url, Some("http://localhost:11434".to_string()));
    
    // Check that max_iterations was set in the profile
    let profile = config.profiles.get("custom_profile").unwrap();
    assert_eq!(profile.agent.as_ref().unwrap().max_iterations, Some(25));

    // Clean up
    env::remove_var("MYLM_PROFILE");
    env::remove_var("MYLM_PROVIDER");
    env::remove_var("MYLM_MODEL");
    env::remove_var("MYLM_API_KEY");
    env::remove_var("MYLM_BASE_URL");
    env::remove_var("MYLM_MAX_ITERATIONS");
}

#[test]
fn test_invalid_provider_env_var() {
    env::set_var("MYLM_PROVIDER", "invalid_provider");

    let mut config = ConfigV2::default();
    let original_provider = config.endpoint.provider.clone();
    
    // Should not panic, just print warning
    config.apply_env_overrides();
    
    // Provider should remain unchanged
    assert_eq!(config.endpoint.provider, original_provider);

    env::remove_var("MYLM_PROVIDER");
}

#[test]
fn test_invalid_max_iterations_env_var() {
    env::set_var("MYLM_MAX_ITERATIONS", "not_a_number");

    let mut config = ConfigV2::default();
    
    // Should not panic, just print warning
    config.apply_env_overrides();

    env::remove_var("MYLM_MAX_ITERATIONS");
}

#[test]
fn test_profile_resolution_no_profile() {
    let config = ConfigV2::default();
    let resolved = config.resolve_profile();

    assert_eq!(resolved.provider, Provider::Openai);
    assert_eq!(resolved.model, "default-model");
    assert_eq!(resolved.agent.main_model, "default-model");
    assert_eq!(resolved.agent.worker_model, "default-model");
    assert_eq!(resolved.agent.max_iterations, 10);
}

#[test]
fn test_profile_resolution_with_overrides() {
    let mut profiles = HashMap::new();
    profiles.insert("fast".to_string(), Profile {
        endpoint: Some(EndpointOverride {
            model: Some("test-model-fast".to_string()),
            api_key: Some("profile-key".to_string()),
        }),
        agent: Some(AgentOverride {
            max_iterations: Some(5),
            main_model: Some("test-model-main".to_string()),
            worker_model: Some("test-model-fast".to_string()),
            ..Default::default()
        }),
    });

    let config = ConfigV2 {
        profile: "fast".to_string(),
        profiles,
        ..Default::default()
    };

    let resolved = config.resolve_profile();

    assert_eq!(resolved.model, "test-model-fast");
    assert_eq!(resolved.api_key, Some("profile-key".to_string()));
    assert_eq!(resolved.agent.max_iterations, 5);
    assert_eq!(resolved.agent.main_model, "test-model-main");
    assert_eq!(resolved.agent.worker_model, "test-model-fast");
}

#[test]
fn test_profile_resolution_partial_agent_override() {
    let mut profiles = HashMap::new();
    profiles.insert("custom".to_string(), Profile {
        endpoint: None,
        agent: Some(AgentOverride {
            max_iterations: Some(15),
            main_model: None,
            worker_model: Some("claude-3-haiku".to_string()),
            ..Default::default()
        }),
    });

    let config = ConfigV2 {
        profile: "custom".to_string(),
        endpoint: EndpointConfig {
            model: "claude-3-opus".to_string(),
            ..Default::default()
        },
        profiles,
        ..Default::default()
    };

    let resolved = config.resolve_profile();

    // Model should come from base endpoint
    assert_eq!(resolved.model, "claude-3-opus");
    // Agent should use endpoint model for main (not specified in override)
    assert_eq!(resolved.agent.main_model, "claude-3-opus");
    // Worker model from override
    assert_eq!(resolved.agent.worker_model, "claude-3-haiku");
    assert_eq!(resolved.agent.max_iterations, 15);
}

#[test]
fn test_profile_resolution_nonexistent_profile() {
    let config = ConfigV2 {
        profile: "nonexistent".to_string(),
        ..Default::default()
    };

    let resolved = config.resolve_profile();

    // Should fall back to base config
    assert_eq!(resolved.provider, Provider::Openai);
    assert_eq!(resolved.model, "default-model");
}

#[test]
fn test_config_error_display() {
    let io_err = ConfigError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file not found"
    ));
    assert!(io_err.to_string().contains("IO error"));

    let profile_err = ConfigError::InvalidProfile("test".to_string());
    assert!(profile_err.to_string().contains("Invalid profile: test"));
}

#[test]
fn test_resolved_config_default() {
    let agent_config = AgentConfig::default();
    assert_eq!(agent_config.max_iterations, 10);
    assert_eq!(agent_config.main_model, "default-model");
    assert_eq!(agent_config.worker_model, "default-worker-model");
}

#[test]
fn test_empty_env_vars_ignored() {
    env::set_var("MYLM_PROFILE", "");
    env::set_var("MYLM_MODEL", "");
    env::set_var("MYLM_API_KEY", "");
    env::set_var("MYLM_BASE_URL", "");

    let mut config = ConfigV2::default();
    let original_profile = config.profile.clone();
    let original_model = config.endpoint.model.clone();
    
    config.apply_env_overrides();

    // Empty env vars should be ignored
    assert_eq!(config.profile, original_profile);
    assert_eq!(config.endpoint.model, original_model);
    assert!(config.endpoint.api_key.is_none());
    assert!(config.endpoint.base_url.is_none());

    env::remove_var("MYLM_PROFILE");
    env::remove_var("MYLM_MODEL");
    env::remove_var("MYLM_API_KEY");
    env::remove_var("MYLM_BASE_URL");
}
