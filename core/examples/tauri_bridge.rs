//! Example Tauri bridge layer for mylm-core
//! 
//! This shows how a GUI application (like your Javi Tauri app) would
//! integrate with mylm-core. This code lives in your closed-source repo.

use mylm_core::prelude::*;
use tokio::sync::Mutex;
use std::sync::Arc;

/// Application state shared across Tauri commands
pub struct AppState {
    /// The active session (if any)
    session: Arc<Mutex<Option<AgencySession<Planner, ContractRuntime, InMemoryTransport>>>>,
    /// Session output receiver
    output_rx: Arc<Mutex<Option<tokio::sync::broadcast::Receiver<OutputEvent>>>>,
}

/// Start a new MyLM session
/// 
/// Tauri command that creates a session, optionally restoring from previous.
#[tauri::command]
pub async fn mylm_start_session(
    state: tauri::State<'_, AppState>,
    resume: bool,
) -> Result<String, String> {
    // Load configuration
    let config = Config::load_or_default();
    
    // Create the factory
    let factory = AgentSessionFactory::new(config);
    
    // Create session (with optional resume)
    let (session, session_data) = if resume {
        factory.create_resumable_session().await
            .map_err(|e| e.to_string())?
    } else {
        let session = factory.create_default_session().await
            .map_err(|e| e.to_string())?;
        (session, None)
    };
    
    // Subscribe to output events BEFORE starting the session
    let output_rx = session.subscribe_output();
    
    // Store in state
    *state.session.lock().await = Some(session);
    *state.output_rx.lock().await = Some(output_rx);
    
    // Return session info
    let info = if let Some(data) = session_data {
        format!("Session restored with {} messages", data.history.len())
    } else {
        "New session started".to_string()
    };
    
    Ok(info)
}

/// Send a user message to the active session
#[tauri::command]
pub async fn mylm_send_user_message(
    state: tauri::State<'_, AppState>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    
    if let Some(ref session) = *session_guard {
        session.submit_input(UserInput::Message(text))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("No active session".to_string())
    }
}

/// Approve or deny a pending tool execution
#[tauri::command]
pub async fn mylm_approve_action(
    state: tauri::State<'_, AppState>,
    session_id: String,
    approval_id: String,
    decision: String,
) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    
    if let Some(ref session) = *session_guard {
        let approved = decision == "approve";
        // Note: You'd need to add an approve method to the Session trait
        // or handle approval through the input channel
        // session.approve(approval_id, approved).await.map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("No active session".to_string())
    }
}

/// Example of spawning an output event forwarder
/// 
/// This would run in a background task and forward events to the frontend
/// via Tauri's event system.
pub async fn forward_output_events(
    window: tauri::Window,
    mut output_rx: tokio::sync::broadcast::Receiver<OutputEvent>,
) {
    while let Ok(event) = output_rx.recv().await {
        // Serialize event to JSON and emit to frontend
        let event_json = serde_json::to_string(&event).unwrap();
        let _ = window.emit("mylm-event", event_json);
    }
}

/// Example frontend usage (React/TypeScript):
/// 
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
/// import { listen } from '@tauri-apps/api/event';
/// 
/// // Start session
/// await invoke('mylm_start_session', { resume: true });
/// 
/// // Listen for events
/// listen('mylm-event', (event) => {
///   const outputEvent = JSON.parse(event.payload);
///   switch (outputEvent.type) {
///     case 'ResponseChunk':
///       appendToChat(outputEvent.content);
///       break;
///     case 'ToolExecuting':
///       showToolStatus(outputEvent.tool);
///       break;
///     // ... handle other events
///   }
/// });
/// 
/// // Send message
/// await invoke('mylm_send_user_message', { 
///   sessionId: 'current',
///   text: 'Hello MyLM' 
/// });
/// ```