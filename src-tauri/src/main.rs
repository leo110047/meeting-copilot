use meeting_copilot_core::{DecisionReadiness, DecisionState, DecisionType};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{include_image, DragDropEvent, Emitter, Manager, WindowEvent};

static LIVE_SESSIONS: OnceLock<Mutex<HashMap<String, NativeLiveSession>>> = OnceLock::new();
static NATIVE_TRANSCRIBERS: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
static PREP_DICTATION: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static DROP_READ_GRANTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
const SCHEMA_SQL: &str = include_str!("../../src/storage/schema.sql");
const TRAY_ICON: Image<'_> = include_image!("./icons/32x32.png");
const NATIVE_SPEECH_HELPER: &str = "meeting-copilot-native-speech";

include!("desktop_types.inc.rs");
include!("commands_core.inc.rs");
include!("commands_audio.inc.rs");
include!("macos_speech_bridge.inc.rs");
include!("shell_storage.inc.rs");
include!("oauth_provider.inc.rs");
include!("native_storage.inc.rs");
include!("decision_logic.inc.rs");
include!("tests.inc.rs");

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
    let plan = desktop_shell_plan();
    println!(
        "Meeting Copilot desktop shell skeleton: platform={} status_surface={} audio_capture={} suggestion_surface={}",
        plan.platform, plan.status_surface, plan.audio_capture, plan.suggestion_surface
    );
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Regular)?;
            install_tray(app.handle())?;
            show_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                register_drop_read_grants(paths);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            desktop_shell_plan_command,
            start_session,
            ingest_transcript,
            stop_session,
            native_transcriber_health,
            request_native_audio_permissions,
            read_dropped_context_files,
            text_provider_status,
            start_text_provider_login,
            request_screen_recording_permission,
            generate_ai_summary_oauth,
            generate_prep_summary_oauth,
            cleanup_transcript_text_oauth,
            log_app_error,
            export_app_error_logs,
            extract_live_state_patch_oauth,
            set_window_opacity,
            start_prep_dictation,
            stop_prep_dictation,
            start_native_transcription,
            stop_native_transcription
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Meeting Copilot native app")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_main_window(app),
            _ => {}
        });
}
