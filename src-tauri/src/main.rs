use meeting_copilot_core::{DecisionReadiness, DecisionState, DecisionType};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use tauri::image::Image;
use tauri::{DragDropEvent, WindowEvent, include_image};

mod commands_audio;
mod commands_core;
mod decision_logic;
mod desktop_types;
mod macos_speech_bridge;
mod native_storage;
mod oauth_provider;
mod shell_storage;

#[cfg(test)]
mod tests;

pub(crate) static LIVE_SESSIONS: OnceLock<
    Mutex<HashMap<String, desktop_types::NativeLiveSession>>,
> = OnceLock::new();
pub(crate) static NATIVE_TRANSCRIBERS: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
pub(crate) static PREP_DICTATION: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
pub(crate) static DROP_READ_GRANTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
pub(crate) const SCHEMA_SQL: &str = include_str!("../../src/storage/schema.sql");
pub(crate) const TRAY_ICON: Image<'_> = include_image!("./icons/32x32.png");
pub(crate) const NATIVE_SPEECH_HELPER: &str = "meeting-copilot-native-speech";

fn main() {
    let _state = DecisionState {
        session_id: "desktop-shell".to_string(),
        current_decision: None,
        decision_type: DecisionType::Unknown,
        missing_inputs: vec![],
        readiness: DecisionReadiness {
            score: 0.0,
            safe_to_decide: false,
            blockers: vec![],
            evidence_transcript_ids: vec![],
        },
        evidence_transcript_ids: vec![],
    };
    let plan = desktop_types::desktop_shell_plan();
    println!(
        "Meeting Copilot desktop shell skeleton: platform={} status_surface={} audio_capture={} suggestion_surface={}",
        plan.platform, plan.status_surface, plan.audio_capture, plan.suggestion_surface
    );
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Regular)?;
            shell_storage::install_tray(app.handle())?;
            shell_storage::show_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                commands_core::register_drop_read_grants(paths);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands_core::desktop_shell_plan_command,
            commands_core::start_session,
            commands_core::ingest_transcript,
            commands_core::stop_session,
            commands_core::native_transcriber_health,
            commands_core::request_native_audio_permissions,
            commands_core::read_dropped_context_files,
            commands_core::text_provider_status,
            commands_core::start_text_provider_login,
            commands_core::request_screen_recording_permission,
            commands_core::generate_ai_summary_oauth,
            commands_core::generate_prep_summary_oauth,
            commands_core::cleanup_transcript_text_oauth,
            commands_core::log_app_error,
            commands_core::export_app_error_logs,
            commands_core::extract_live_state_patch_oauth,
            commands_core::set_window_opacity,
            commands_audio::start_prep_dictation,
            commands_audio::stop_prep_dictation,
            commands_audio::start_native_transcription,
            commands_audio::stop_native_transcription
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Meeting Copilot native app")
        .run(|_app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => shell_storage::show_main_window(_app),
            _ => {}
        });
}
