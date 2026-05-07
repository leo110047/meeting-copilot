use crate::commands_audio::ingest_transcript_inner;
use crate::decision_logic::{
    apply_live_state_patch_from_refs, default_brief, derive_decision_state,
    derive_decision_state_from_refs, now_ms, stable_id,
};
use crate::desktop_types::desktop_shell_plan;
use crate::desktop_types::{
    AiSummaryRequest, AiSummaryResponse, AppErrorLogInput, AppErrorLogRecord, DesktopShellPlan,
    DroppedContextFile, IngestTranscriptResponse, LocalSttStatus, MeetingSeriesOption,
    NativeLiveSession, NativeTranscriberHealth, NativeTranscriberHealthRequest, PersistedSummary,
    PrepSummaryRequest, PrepSummaryResponse, SaveMeetingHistoryRequest, SaveMeetingHistoryResponse,
    StartSessionRequest, StartSessionResponse, TextProviderStatus, TranscriptCleanupRequest,
    TranscriptCleanupResponse, TranscriptInput, TranscriptRevisionRequest,
    TranscriptRevisionResponse,
};
use crate::local_stt::{
    download_local_stt_model, is_local_whisper_profile, local_stt_model_directory,
    local_stt_status, local_whisper_health, normalize_local_stt_profile_id,
    set_selected_local_stt_profile,
};
use crate::native_storage::{
    LlmUsageLogInput, insert_decision_snapshot_with_source, insert_llm_usage_log, insert_session,
    insert_suggestion, list_app_error_logs, list_meeting_series, log_app_error_inner,
    log_extraction_failure_for, log_provider_failure_for, log_provider_usage_for,
    native_speech_helper_path, native_speech_provider_id, record_session_text_provider,
    run_native_transcriber_health_check, save_meeting_history,
};
use crate::oauth_provider::{
    build_ai_summary_prompt, build_live_state_patch_prompt, build_prep_summary_prompt,
    build_transcript_revision_prompt, cleanup_transcript_text_with_provider_inner,
    live_ai_remote_event_refs, parse_ai_summary_sections,
    parse_live_coaching_suggestions_from_refs, parse_live_state_patch, parse_prep_summary_points,
    parse_transcript_revision_response, run_text_provider_prompt_with_timeout,
    start_text_provider_login_for, text_provider_install_guide_url, text_provider_status_for,
    text_provider_status_for_with_refresh, text_provider_summary,
};
use crate::shell_storage::{
    app_db_path, open_db, read_dropped_context_file, set_native_window_opacity,
};
use crate::{DROP_READ_GRANTS, LIVE_SESSIONS};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub(crate) const LIVE_SESSION_STOP_GRACE_MS: i64 = 60_000;
const LIVE_SESSION_STOP_GRACE: Duration = Duration::from_millis(LIVE_SESSION_STOP_GRACE_MS as u64);
const LIVE_AI_PROVIDER_TIMEOUT_MS: u64 = 15_000;

#[cfg(target_os = "macos")]
use crate::macos_speech_bridge::{
    macos_audio_bridge_ready, macos_audio_bridge_status_error, macos_speech_bridge_health,
    macos_speech_bridge_status, macos_speech_bridge_status_error,
    request_macos_audio_bridge_permissions, request_macos_speech_bridge_permissions,
};

#[tauri::command]
pub(crate) fn desktop_shell_plan_command() -> DesktopShellPlan {
    desktop_shell_plan()
}

#[tauri::command]
pub(crate) fn start_session(
    request: Option<StartSessionRequest>,
) -> Result<StartSessionResponse, String> {
    let text_provider_enabled = request
        .as_ref()
        .and_then(|body| body.text_provider_enabled)
        .unwrap_or(false);
    let text_provider_id = request
        .as_ref()
        .and_then(|body| body.text_provider_id.clone());
    let mut brief = request
        .and_then(|body| body.brief)
        .unwrap_or_else(default_brief);
    if brief.session_id.is_empty() {
        brief.session_id = format!("native_{}", now_ms());
    }
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    let (session_text_provider, _) = text_provider_summary(text_provider_id.as_deref());
    insert_session(
        &conn,
        &brief,
        text_provider_enabled,
        Some(session_text_provider),
    )?;
    LIVE_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .insert(
            brief.session_id.clone(),
            NativeLiveSession {
                brief: brief.clone(),
                text_provider_id,
                events: vec![],
                shown_suggestion_ids: HashSet::new(),
                stopped_at_ms: None,
            },
        );
    Ok(StartSessionResponse {
        session_id: brief.session_id.clone(),
        brief,
        db_path: db_path.display().to_string(),
        platform: desktop_shell_plan(),
    })
}

#[tauri::command]
pub(crate) fn stop_session(session_id: String) -> Result<(), String> {
    stop_session_inner(session_id, app_db_path()?)
}

pub(crate) fn stop_session_inner(session_id: String, db_path: PathBuf) -> Result<(), String> {
    let stopped_at_ms = current_time_ms_i64();
    let conn = open_db(&db_path)?;
    conn.execute(
        "UPDATE meeting_sessions SET ended_at = ?1 WHERE id = ?2",
        params![stopped_at_ms.to_string(), session_id],
    )
    .map_err(|error| error.to_string())?;
    mark_live_session_stopped(&session_id, stopped_at_ms);
    schedule_live_session_cleanup(session_id, stopped_at_ms);
    Ok(())
}

fn current_time_ms_i64() -> i64 {
    i64::try_from(now_ms()).unwrap_or(i64::MAX)
}

fn mark_live_session_stopped(session_id: &str, stopped_at_ms: i64) {
    if let Some(sessions) = LIVE_SESSIONS.get()
        && let Ok(mut sessions) = sessions.lock()
        && let Some(session) = sessions.get_mut(session_id)
    {
        session.stopped_at_ms = Some(stopped_at_ms);
    }
}

fn schedule_live_session_cleanup(session_id: String, stopped_at_ms: i64) {
    thread::spawn(move || {
        thread::sleep(LIVE_SESSION_STOP_GRACE);
        let _ = cleanup_stopped_live_session(&session_id, stopped_at_ms, current_time_ms_i64());
    });
}

pub(crate) fn cleanup_stopped_live_session(
    session_id: &str,
    stopped_at_ms: i64,
    current_ms: i64,
) -> bool {
    if current_ms.saturating_sub(stopped_at_ms) < LIVE_SESSION_STOP_GRACE_MS {
        return false;
    }
    let Some(sessions) = LIVE_SESSIONS.get() else {
        return false;
    };
    let Ok(mut sessions) = sessions.lock() else {
        return false;
    };
    let should_remove = sessions
        .get(session_id)
        .map(|session| session.stopped_at_ms == Some(stopped_at_ms))
        .unwrap_or(false);
    if should_remove {
        sessions.remove(session_id);
    }
    should_remove
}

#[tauri::command]
pub(crate) fn ingest_transcript(
    session_id: String,
    input: TranscriptInput,
) -> Result<IngestTranscriptResponse, String> {
    ingest_transcript_inner(session_id, input)
}

#[tauri::command]
pub(crate) fn native_transcriber_health(
    request: Option<NativeTranscriberHealthRequest>,
) -> NativeTranscriberHealth {
    let request = request.unwrap_or(NativeTranscriberHealthRequest {
        source: None,
        stt_profile_id: None,
    });
    let source = request.source.unwrap_or_else(|| "mic".to_string());
    let profile_id = normalize_local_stt_profile_id(request.stt_profile_id.as_deref());
    native_transcriber_health_for_source(&source, profile_id).unwrap_or_else(|error| {
        NativeTranscriberHealth {
            provider_id: native_speech_provider_id().to_string(),
            kind: "stt".to_string(),
            ready: false,
            supports_streaming: true,
            supports_diarization: false,
            supports_source_hints: true,
            platform: desktop_shell_plan(),
            last_error: Some(error),
        }
    })
}

#[tauri::command]
pub(crate) fn request_native_audio_permissions(
    request: Option<NativeTranscriberHealthRequest>,
) -> NativeTranscriberHealth {
    let request = request.unwrap_or(NativeTranscriberHealthRequest {
        source: None,
        stt_profile_id: None,
    });
    let source = request.source.unwrap_or_else(|| "mic".to_string());
    let profile_id = normalize_local_stt_profile_id(request.stt_profile_id.as_deref());
    request_native_audio_permissions_for_source(&source, profile_id).unwrap_or_else(|error| {
        NativeTranscriberHealth {
            provider_id: native_speech_provider_id().to_string(),
            kind: "stt".to_string(),
            ready: false,
            supports_streaming: true,
            supports_diarization: false,
            supports_source_hints: true,
            platform: desktop_shell_plan(),
            last_error: Some(error),
        }
    })
}

pub(crate) fn native_transcriber_health_for_source(
    source: &str,
    stt_profile_id: &str,
) -> Result<NativeTranscriberHealth, String> {
    if is_local_whisper_profile(stt_profile_id) {
        let mut health = local_whisper_health(stt_profile_id)?;
        if !health.ready {
            return Ok(health);
        }
        #[cfg(target_os = "macos")]
        {
            let sources = if source == "mixed" {
                vec!["mic", "system"]
            } else {
                vec![source]
            };
            let mut errors = Vec::new();
            for helper_source in sources {
                // The shared macOS bridge status includes Speech authorization for
                // Apple Speech. Local Whisper only uses the audio permission subset,
                // so gate readiness through macos_audio_bridge_ready/status_error.
                let status = macos_speech_bridge_status(helper_source, "zh-TW")?;
                if !macos_audio_bridge_ready(helper_source, status) {
                    errors.push(macos_audio_bridge_status_error(helper_source, status));
                }
            }
            if !errors.is_empty() {
                health.ready = false;
                health.last_error = Some(errors.join("; "));
            }
        }
        return Ok(health);
    }
    let sources = if source == "mixed" {
        vec!["mic", "system"]
    } else {
        vec![source]
    };
    let mut checks = Vec::new();
    for helper_source in sources {
        #[cfg(target_os = "macos")]
        {
            if helper_source == "mic" || helper_source == "system" {
                let ready = macos_speech_bridge_health(helper_source, "zh-TW")?;
                let status = macos_speech_bridge_status(helper_source, "zh-TW")?;
                checks.push(NativeTranscriberHealth {
                    provider_id: native_speech_provider_id().to_string(),
                    kind: "stt".to_string(),
                    ready,
                    supports_streaming: true,
                    supports_diarization: false,
                    supports_source_hints: true,
                    platform: desktop_shell_plan(),
                    last_error: if ready {
                        None
                    } else {
                        Some(macos_speech_bridge_status_error(
                            helper_source,
                            "zh-TW",
                            status,
                        ))
                    },
                });
                continue;
            }
        }
        let helper_path = native_speech_helper_path()?;
        checks.push(run_native_transcriber_health_check(
            &helper_path,
            helper_source,
        )?);
    }
    let mut combined = checks
        .first()
        .cloned()
        .ok_or_else(|| "native transcriber health source is empty".to_string())?;
    combined.ready = checks.iter().all(|check| check.ready);
    let errors: Vec<String> = checks
        .into_iter()
        .filter_map(|check| if check.ready { None } else { check.last_error })
        .collect();
    combined.last_error = if combined.ready || errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };
    Ok(combined)
}

pub(crate) fn request_native_audio_permissions_for_source(
    source: &str,
    stt_profile_id: &str,
) -> Result<NativeTranscriberHealth, String> {
    if is_local_whisper_profile(stt_profile_id) {
        let health = local_whisper_health(stt_profile_id)?;
        if !health.ready {
            return Ok(health);
        }
        #[cfg(target_os = "macos")]
        {
            let sources = if source == "mixed" {
                vec!["mic", "system"]
            } else {
                vec![source]
            };
            for helper_source in sources {
                if helper_source == "mic" || helper_source == "system" {
                    let _ = request_macos_audio_bridge_permissions(helper_source, "zh-TW")?;
                }
            }
            return native_transcriber_health_for_source(source, stt_profile_id);
        }
        #[cfg(not(target_os = "macos"))]
        return Ok(health);
    }
    #[cfg(target_os = "macos")]
    {
        let sources = if source == "mixed" {
            vec!["mic", "system"]
        } else {
            vec![source]
        };
        for helper_source in sources {
            if helper_source == "mic" || helper_source == "system" {
                let _ = request_macos_speech_bridge_permissions(helper_source, "zh-TW")?;
            }
        }
    }
    native_transcriber_health_for_source(source, stt_profile_id)
}

#[tauri::command]
pub(crate) fn local_stt_status_command(
    profile_id: Option<String>,
) -> Result<LocalSttStatus, String> {
    local_stt_status(profile_id.as_deref())
}

#[tauri::command]
pub(crate) fn set_local_stt_profile_command(
    profile_id: Option<String>,
) -> Result<LocalSttStatus, String> {
    set_selected_local_stt_profile(profile_id.as_deref())
}

#[tauri::command]
pub(crate) async fn download_local_stt_model_command(
    app: tauri::AppHandle,
    profile_id: Option<String>,
) -> Result<LocalSttStatus, String> {
    download_local_stt_model(app, profile_id.as_deref()).await
}

#[tauri::command]
pub(crate) fn open_local_stt_model_folder() -> Result<(), String> {
    let path = local_stt_model_directory()?;
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&path).status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(&path).status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = std::process::Command::new("xdg-open").arg(&path).status();
    let status =
        status.map_err(|error| format!("failed to open local STT model folder: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("failed to open local STT model folder: {status}"))
    }
}

#[tauri::command]
pub(crate) fn request_screen_recording_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = request_macos_speech_bridge_permissions("system", "zh-TW")?;
        let status = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|error| format!("failed to open Screen Recording settings: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "failed to open Screen Recording settings: {status}"
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Screen Recording settings are only available on macOS".to_string())
    }
}

#[tauri::command]
pub(crate) fn text_provider_status(
    provider_id: Option<String>,
    force_refresh: Option<bool>,
) -> TextProviderStatus {
    text_provider_status_for_with_refresh(provider_id.as_deref(), force_refresh.unwrap_or(false))
}

#[tauri::command]
pub(crate) fn open_text_provider_install_guide(provider_id: Option<String>) -> Result<(), String> {
    let url = text_provider_install_guide_url(provider_id.as_deref());
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(url)
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = std::process::Command::new("xdg-open").arg(url).status();
    let status =
        status.map_err(|error| format!("failed to open AI connector install guide: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to open AI connector install guide: {status}"
        ))
    }
}

#[tauri::command]
pub(crate) fn start_text_provider_login(provider_id: Option<String>) -> Result<(), String> {
    start_text_provider_login_for(provider_id.as_deref())
}

#[tauri::command]
pub(crate) fn set_session_text_provider(
    session_id: String,
    provider_id: Option<String>,
) -> Result<(), String> {
    let mut sessions = LIVE_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| "session not found".to_string())?;
    let normalized = text_provider_summary(provider_id.as_deref()).0.to_string();
    session.text_provider_id = Some(normalized.clone());
    drop(sessions);
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    record_session_text_provider(&conn, &session_id, &normalized)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn generate_ai_summary_oauth(
    request: AiSummaryRequest,
) -> Result<AiSummaryResponse, String> {
    let provider_id = request.text_provider_id.as_deref();
    let status = text_provider_status_for(provider_id);
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let prompt = build_ai_summary_prompt(&request)?;
    let started = now_ms();
    let run = match run_text_provider_prompt_with_timeout(provider_id, &prompt, 60_000) {
        Ok(run) => run,
        Err(error) => {
            let (provider, _) = text_provider_summary(provider_id);
            let _ = log_provider_failure_for(
                provider,
                &request.session_id,
                "generate_ai_summary",
                "generate_ai_summary.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let raw_output = run.output;
    let summary = match parse_ai_summary_sections(&raw_output) {
        Ok(summary) => summary,
        Err(error) => {
            let _ = log_provider_failure_for(
                run.provider_id,
                &request.session_id,
                "generate_ai_summary",
                "generate_ai_summary.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage_for(
        run.provider_id,
        run.model,
        &request.session_id,
        "generate_ai_summary",
        "generate_ai_summary.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(AiSummaryResponse {
        provider_id: run.provider_id.to_string(),
        model: run.model.to_string(),
        summary,
        raw_output_ref: stable_id(&raw_output),
    })
}

#[tauri::command]
pub(crate) fn revise_transcript_oauth(
    request: TranscriptRevisionRequest,
) -> Result<TranscriptRevisionResponse, String> {
    let provider_id = request.text_provider_id.as_deref();
    let (empty_provider, empty_model) = text_provider_summary(provider_id);
    if request.transcript.is_empty() {
        return Ok(TranscriptRevisionResponse {
            provider_id: empty_provider.to_string(),
            model: empty_model.to_string(),
            transcript: vec![],
            raw_output_ref: stable_id("empty-transcript-revision"),
        });
    }
    let status = text_provider_status_for(provider_id);
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let prompt = build_transcript_revision_prompt(&request)?;
    let started = now_ms();
    let run = match run_text_provider_prompt_with_timeout(provider_id, &prompt, 18_000) {
        Ok(run) => run,
        Err(error) => {
            let (provider, _) = text_provider_summary(provider_id);
            let _ = log_provider_failure_for(
                provider,
                &request.session_id,
                "revise_transcript",
                "revise_transcript.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let raw_output = run.output;
    let transcript = match parse_transcript_revision_response(&raw_output, &request) {
        Ok(transcript) => transcript,
        Err(error) => {
            let _ = log_provider_failure_for(
                run.provider_id,
                &request.session_id,
                "revise_transcript",
                "revise_transcript.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage_for(
        run.provider_id,
        run.model,
        &request.session_id,
        "revise_transcript",
        "revise_transcript.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(TranscriptRevisionResponse {
        provider_id: run.provider_id.to_string(),
        model: run.model.to_string(),
        transcript,
        raw_output_ref: stable_id(&raw_output),
    })
}

#[tauri::command]
pub(crate) fn generate_prep_summary_oauth(
    request: PrepSummaryRequest,
) -> Result<PrepSummaryResponse, String> {
    let provider_id = request.text_provider_id.as_deref();
    let status = text_provider_status_for(provider_id);
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let (empty_provider, empty_model) = text_provider_summary(provider_id);
    if request.context.trim().is_empty() {
        return Ok(PrepSummaryResponse {
            provider_id: empty_provider.to_string(),
            model: empty_model.to_string(),
            key_points: vec![],
            raw_output_ref: stable_id("empty-prep-summary"),
        });
    }
    let prompt = build_prep_summary_prompt(&request)?;
    let audit_id = stable_id(&format!("prep:{}:{}", request.file_count, request.context));
    let started = now_ms();
    let run = match run_text_provider_prompt_with_timeout(provider_id, &prompt, 25_000) {
        Ok(run) => run,
        Err(error) => {
            let (provider, _) = text_provider_summary(provider_id);
            let _ = log_provider_failure_for(
                provider,
                &audit_id,
                "generate_prep_summary",
                "generate_prep_summary.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let raw_output = run.output;
    let key_points = match parse_prep_summary_points(&raw_output) {
        Ok(points) => points,
        Err(error) => {
            let _ = log_provider_failure_for(
                run.provider_id,
                &audit_id,
                "generate_prep_summary",
                "generate_prep_summary.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage_for(
        run.provider_id,
        run.model,
        &audit_id,
        "generate_prep_summary",
        "generate_prep_summary.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(PrepSummaryResponse {
        provider_id: run.provider_id.to_string(),
        model: run.model.to_string(),
        key_points,
        raw_output_ref: stable_id(&raw_output),
    })
}

#[tauri::command]
pub(crate) fn cleanup_transcript_text_oauth(
    request: TranscriptCleanupRequest,
) -> Result<TranscriptCleanupResponse, String> {
    let provider_id = request.text_provider_id.as_deref();
    let (provider, model) = text_provider_summary(provider_id);
    let cleaned = cleanup_transcript_text_with_provider_inner(
        provider_id,
        &request.text,
        &request.context,
        Some("ui_cleanup"),
    )?;
    Ok(TranscriptCleanupResponse {
        provider_id: provider.to_string(),
        model: model.to_string(),
        raw_output_ref: stable_id(&cleaned),
        text: cleaned,
    })
}

#[tauri::command]
pub(crate) fn log_app_error(input: AppErrorLogInput) -> Result<String, String> {
    log_app_error_inner(
        input.session_id.as_deref(),
        &input.stage,
        &input.source,
        &input.severity,
        &input.message,
        input.detail_json.unwrap_or_else(|| serde_json::json!({})),
    )
}

#[tauri::command]
pub(crate) fn export_app_error_logs(
    session_id: Option<String>,
) -> Result<Vec<AppErrorLogRecord>, String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    list_app_error_logs(&conn, session_id.as_deref())
}

#[tauri::command]
pub(crate) fn list_meeting_series_command() -> Result<Vec<MeetingSeriesOption>, String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    list_meeting_series(&conn)
}

#[tauri::command]
pub(crate) fn save_meeting_history_command(
    request: SaveMeetingHistoryRequest,
) -> Result<SaveMeetingHistoryResponse, String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    save_meeting_history(&conn, request)
}

#[tauri::command]
pub(crate) fn extract_live_state_patch_oauth(
    session_id: String,
) -> Result<IngestTranscriptResponse, String> {
    let provider_id = {
        let sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        sessions
            .get(&session_id)
            .ok_or_else(|| "session not found".to_string())?
            .text_provider_id
            .clone()
    };
    let provider_id_ref = provider_id.as_deref();
    let status = text_provider_status_for(provider_id_ref);
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let (brief, events) = {
        let sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| "session not found".to_string())?;
        (session.brief.clone(), session.events.clone())
    };
    let live_events = live_ai_remote_event_refs(&events);
    let last_event = live_events
        .last()
        .map(|event| (*event).clone())
        .ok_or_else(|| "no remote transcript available for live AI extraction".to_string())?;
    let local_state = derive_decision_state(&session_id, &events);
    let live_prompt_state = derive_decision_state_from_refs(&session_id, &live_events);
    let prompt = build_live_state_patch_prompt(&brief, &live_events, &live_prompt_state)?;
    let started = now_ms();
    let run = match run_text_provider_prompt_with_timeout(
        provider_id_ref,
        &prompt,
        LIVE_AI_PROVIDER_TIMEOUT_MS,
    ) {
        Ok(run) => run,
        Err(error) => {
            let (provider, _) = text_provider_summary(provider_id_ref);
            log_extraction_failure_for(&session_id, provider, "timeout_or_api_error", &error)?;
            return Err(format!(
                "subscription OAuth live extraction failed: {error}"
            ));
        }
    };
    let raw_output = run.output;
    let patch = match parse_live_state_patch(&raw_output) {
        Ok(patch) => patch,
        Err((failure_kind, error)) => {
            log_extraction_failure_for(
                &session_id,
                run.provider_id,
                failure_kind,
                &format!("{error}: {}", stable_id(&raw_output)),
            )?;
            return Err(format!(
                "subscription OAuth live extraction rejected: {error}"
            ));
        }
    };
    // The provider sees only the remote/system transcript. The returned patch is
    // then applied to the full local state so the app keeps the user's own
    // locally derived context without sending it to the provider.
    let decision_state = apply_live_state_patch_from_refs(local_state, &patch, &live_events);
    let mut coaching_error = None;
    let mut suggestions =
        match parse_live_coaching_suggestions_from_refs(&raw_output, &session_id, &live_events) {
            Ok(suggestions) => suggestions,
            Err(("coaching_cards_discarded", error)) => {
                let _ = log_app_error_inner(
                    Some(&session_id),
                    "live_coaching.cards_discarded",
                    "text_provider",
                    "info",
                    &error,
                    serde_json::json!({
                        "provider": run.provider_id,
                        "promptVersion": "extract_state_patch.oauth.v1",
                        "rawOutputRef": stable_id(&raw_output)
                    }),
                );
                coaching_error = Some("AI 暫時沒有可信的提醒。".to_string());
                vec![]
            }
            Err((failure_kind, error)) => {
                log_extraction_failure_for(
                    &session_id,
                    run.provider_id,
                    failure_kind,
                    &format!(
                        "live coaching rejected: {error}: {}",
                        stable_id(&raw_output)
                    ),
                )?;
                coaching_error = Some("AI 回傳的提醒格式不完整，已保留會議判斷。".to_string());
                vec![]
            }
        };
    {
        let mut sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session not found".to_string())?;
        suggestions.retain(|suggestion| session.shown_suggestion_ids.insert(suggestion.id.clone()));
    }

    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    for suggestion in &suggestions {
        insert_suggestion(&conn, suggestion)?;
    }
    let extraction_id = stable_id(&format!(
        "live-oauth:{}:{}:{}",
        session_id, started, raw_output
    ));
    let snapshot_id = stable_id(&format!(
        "{}:{}:{}",
        session_id,
        extraction_id,
        serde_json::to_string(&decision_state).map_err(|error| error.to_string())?
    ));
    insert_decision_snapshot_with_source(
        &conn,
        &snapshot_id,
        &session_id,
        &decision_state,
        Some(&extraction_id),
    )?;
    insert_llm_usage_log(
        &conn,
        LlmUsageLogInput {
            session_id: &session_id,
            call_type: "extract_state_patch",
            provider: run.provider_id,
            model: run.model,
            prompt_version: "extract_state_patch.oauth.v1",
            prompt: &prompt,
            output: &raw_output,
            latency_ms: now_ms().saturating_sub(started) as i64,
        },
    )?;

    Ok(IngestTranscriptResponse {
        event: None,
        live_evidence_event: Some(last_event),
        suggestions: suggestions.clone(),
        decision_state,
        persisted: PersistedSummary {
            transcript_events: events.len(),
            new_suggestions: suggestions.len(),
            decision_snapshot_id: snapshot_id,
        },
        coaching_error,
    })
}

#[tauri::command]
pub(crate) fn read_dropped_context_files(paths: Vec<String>) -> Vec<DroppedContextFile> {
    paths
        .into_iter()
        .take(8)
        .map(|path| read_granted_dropped_context_file(PathBuf::from(path)))
        .collect()
}

pub(crate) fn register_drop_read_grants(paths: &[PathBuf]) {
    let mut grants = match DROP_READ_GRANTS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        Ok(grants) => grants,
        Err(_) => return,
    };
    grants.clear();
    for path in paths.iter().take(8) {
        if let Ok(canonical) = fs::canonicalize(path) {
            grants.insert(canonical);
        }
    }
}

pub(crate) fn read_granted_dropped_context_file(path: PathBuf) -> DroppedContextFile {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dropped-file")
        .to_string();
    let canonical = match fs::canonicalize(&path) {
        Ok(path) => path,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    let mut grants = match DROP_READ_GRANTS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        Ok(grants) => grants,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(format!("無法確認拖拉授權：{error}")),
            };
        }
    };
    if !grants.remove(&canonical) {
        return DroppedContextFile {
            name,
            text: String::new(),
            truncated: false,
            error: Some("檔案讀取未經本次拖拉授權".to_string()),
        };
    }
    drop(grants);
    read_dropped_context_file(canonical)
}

#[tauri::command]
pub(crate) fn set_window_opacity(app: tauri::AppHandle, percent: u8) -> Result<u8, String> {
    let clamped = percent.clamp(10, 100);
    let opacity = clamped as f64 / 100.0;
    set_native_window_opacity(&app, opacity)?;
    Ok(clamped)
}
