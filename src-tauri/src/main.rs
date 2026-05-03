use meeting_copilot_core::{DecisionReadiness, DecisionState, DecisionType};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{DragDropEvent, Emitter, Manager, WindowEvent, include_image};

static LIVE_SESSIONS: OnceLock<Mutex<HashMap<String, NativeLiveSession>>> = OnceLock::new();
static NATIVE_TRANSCRIBERS: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
static PREP_DICTATION: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static DROP_READ_GRANTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
const SCHEMA_SQL: &str = include_str!("../../src/storage/schema.sql");
const TRAY_ICON: Image<'_> = include_image!("./icons/32x32.png");
const NATIVE_SPEECH_HELPER: &str = "meeting-copilot-native-speech";

#[derive(Debug, Clone, Serialize)]
struct DesktopShellPlan {
    platform: &'static str,
    status_surface: &'static str,
    audio_capture: &'static str,
    suggestion_surface: &'static str,
}

#[cfg(target_os = "macos")]
fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "macos",
        status_surface: "macos_status_item",
        audio_capture: "coreaudio+screencapturekit",
        suggestion_surface: "popover",
    }
}

#[cfg(target_os = "windows")]
fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "windows",
        status_surface: "windows_system_tray",
        audio_capture: "wasapi_capture+wasapi_loopback",
        suggestion_surface: "flyout",
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "unsupported",
        status_surface: "none",
        audio_capture: "none",
        suggestion_surface: "none",
    }
}

#[derive(Debug)]
struct NativeLiveSession {
    brief: MeetingBrief,
    events: Vec<TranscriptEvent>,
    shown_suggestion_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeetingBrief {
    session_id: String,
    project_id: Option<String>,
    meeting_type: String,
    title: Option<String>,
    goal: String,
    must_confirm: Vec<String>,
    risks: Vec<String>,
    constraints: Vec<String>,
    known_participants: Vec<serde_json::Value>,
    preferred_tone: String,
    started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartSessionRequest {
    brief: Option<MeetingBrief>,
    text_provider_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartSessionResponse {
    session_id: String,
    brief: MeetingBrief,
    db_path: String,
    platform: DesktopShellPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptInput {
    id: Option<String>,
    text: String,
    source: Option<String>,
    speaker: Option<String>,
    speaker_confidence: Option<f64>,
    started_at_ms: Option<i64>,
    ended_at_ms: Option<i64>,
    is_final: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptEvent {
    id: String,
    session_id: String,
    source: String,
    speaker: Option<String>,
    speaker_confidence: f64,
    language: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    text: String,
    is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeSuggestion {
    id: String,
    session_id: String,
    shown_at: String,
    kind: String,
    text: String,
    reason: String,
    confidence: f64,
    priority: String,
    evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeReadiness {
    score: f64,
    safe_to_decide: bool,
    blockers: Vec<String>,
    evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeDecisionState {
    session_id: String,
    current_decision: Option<String>,
    decision_type: String,
    meeting_items: Vec<serde_json::Value>,
    options: Vec<serde_json::Value>,
    risks: Vec<serde_json::Value>,
    missing_inputs: Vec<serde_json::Value>,
    readiness: NativeReadiness,
    evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IngestTranscriptResponse {
    event: TranscriptEvent,
    suggestions: Vec<NativeSuggestion>,
    decision_state: NativeDecisionState,
    persisted: PersistedSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSummary {
    transcript_events: usize,
    new_suggestions: usize,
    decision_snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTranscriptionRequest {
    language: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeTranscriptionStartResponse {
    session_id: String,
    provider_id: String,
    source: String,
    language: String,
    helper_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepDictationStartResponse {
    provider_id: String,
    language: String,
    helper_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeTranscriberHealth {
    provider_id: String,
    kind: String,
    ready: bool,
    supports_streaming: bool,
    supports_diarization: bool,
    supports_source_hints: bool,
    platform: DesktopShellPlan,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextProviderStatus {
    provider_id: String,
    kind: String,
    authenticated: bool,
    can_refresh_token: bool,
    supports_structured_output: bool,
    supports_streaming: bool,
    active: bool,
    status_label: String,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiTranscriptLine {
    id: String,
    text: String,
    source: String,
    language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiSummarySections {
    key_points: Vec<String>,
    decisions_and_open_questions: Vec<String>,
    suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiSummaryRequest {
    title: String,
    session_id: String,
    generated_at: String,
    prep_context: String,
    local_summary: AiSummarySections,
    transcript: Vec<AiTranscriptLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiSummaryResponse {
    provider_id: String,
    model: String,
    summary: AiSummarySections,
    raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepSummaryRequest {
    context: String,
    file_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepSummaryResponse {
    provider_id: String,
    model: String,
    key_points: Vec<String>,
    raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptCleanupRequest {
    text: String,
    context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptCleanupResponse {
    provider_id: String,
    model: String,
    text: String,
    raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppErrorLogInput {
    session_id: Option<String>,
    stage: String,
    source: String,
    severity: String,
    message: String,
    detail_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppErrorLogRecord {
    id: String,
    session_id: Option<String>,
    stage: String,
    source: String,
    severity: String,
    message: String,
    detail_json: serde_json::Value,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveStatePatchEnvelope {
    meeting_state_patch: LiveMeetingStatePatch,
    decision_state_patch: LiveDecisionStatePatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveMeetingStatePatch {
    add_items: Vec<serde_json::Value>,
    update_items: Vec<serde_json::Value>,
    resolve_item_ids: Vec<String>,
    phase_change: Option<String>,
    evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveDecisionStatePatch {
    current_decision: Option<String>,
    add_options: Vec<serde_json::Value>,
    update_options: Vec<serde_json::Value>,
    add_risks: Vec<serde_json::Value>,
    add_missing_inputs: Vec<serde_json::Value>,
    readiness_patch: Option<LiveReadinessPatch>,
    evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveReadinessPatch {
    score: Option<f64>,
    safe_to_decide: Option<bool>,
    blockers: Option<Vec<String>>,
    evidence_transcript_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DroppedContextFile {
    name: String,
    text: String,
    truncated: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperTranscriptLine {
    kind: String,
    text: String,
    is_final: bool,
    confidence: f64,
    language: String,
    source: String,
    started_at_ms: i64,
    ended_at_ms: i64,
}

#[tauri::command]
fn desktop_shell_plan_command() -> DesktopShellPlan {
    desktop_shell_plan()
}

#[tauri::command]
fn start_session(request: Option<StartSessionRequest>) -> Result<StartSessionResponse, String> {
    let text_provider_enabled = request
        .as_ref()
        .and_then(|body| body.text_provider_enabled)
        .unwrap_or(false);
    let mut brief = request
        .and_then(|body| body.brief)
        .unwrap_or_else(default_brief);
    if brief.session_id.is_empty() {
        brief.session_id = format!("native_{}", now_ms());
    }
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    insert_session(&conn, &brief, text_provider_enabled)?;
    LIVE_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .insert(
            brief.session_id.clone(),
            NativeLiveSession {
                brief: brief.clone(),
                events: vec![],
                shown_suggestion_ids: HashSet::new(),
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
fn ingest_transcript(
    session_id: String,
    input: TranscriptInput,
) -> Result<IngestTranscriptResponse, String> {
    ingest_transcript_inner(session_id, input)
}

#[tauri::command]
fn native_transcriber_health() -> NativeTranscriberHealth {
    match native_speech_helper_path() {
        Ok(path) => NativeTranscriberHealth {
            provider_id: native_speech_provider_id().to_string(),
            kind: "stt".to_string(),
            ready: true,
            supports_streaming: true,
            supports_diarization: false,
            supports_source_hints: true,
            platform: desktop_shell_plan(),
            last_error: Some(format!("helper available at {}", path.display())),
        },
        Err(error) => NativeTranscriberHealth {
            provider_id: native_speech_provider_id().to_string(),
            kind: "stt".to_string(),
            ready: false,
            supports_streaming: true,
            supports_diarization: false,
            supports_source_hints: true,
            platform: desktop_shell_plan(),
            last_error: Some(error),
        },
    }
}

#[tauri::command]
fn text_provider_status() -> TextProviderStatus {
    subscription_oauth_status()
}

#[tauri::command]
fn start_text_provider_login() -> Result<(), String> {
    start_subscription_oauth_login()
}

#[tauri::command]
fn generate_ai_summary_oauth(request: AiSummaryRequest) -> Result<AiSummaryResponse, String> {
    let status = subscription_oauth_status();
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let prompt = build_ai_summary_prompt(&request)?;
    let started = now_ms();
    let raw_output = match run_codex_oauth_prompt(&prompt) {
        Ok(output) => output,
        Err(error) => {
            let _ = log_provider_failure(
                &request.session_id,
                "generate_ai_summary",
                "generate_ai_summary.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let summary = match parse_ai_summary_sections(&raw_output) {
        Ok(summary) => summary,
        Err(error) => {
            let _ = log_provider_failure(
                &request.session_id,
                "generate_ai_summary",
                "generate_ai_summary.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage(
        &request.session_id,
        "generate_ai_summary",
        "generate_ai_summary.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(AiSummaryResponse {
        provider_id: "codex-chatgpt-oauth".to_string(),
        model: "subscription_oauth".to_string(),
        summary,
        raw_output_ref: stable_id(&raw_output),
    })
}

#[tauri::command]
fn generate_prep_summary_oauth(request: PrepSummaryRequest) -> Result<PrepSummaryResponse, String> {
    let status = subscription_oauth_status();
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    if request.context.trim().is_empty() {
        return Ok(PrepSummaryResponse {
            provider_id: "codex-chatgpt-oauth".to_string(),
            model: "subscription_oauth".to_string(),
            key_points: vec![],
            raw_output_ref: stable_id("empty-prep-summary"),
        });
    }
    let prompt = build_prep_summary_prompt(&request)?;
    let audit_id = stable_id(&format!("prep:{}:{}", request.file_count, request.context));
    let started = now_ms();
    let raw_output = match run_codex_oauth_prompt_with_timeout(&prompt, 25_000) {
        Ok(output) => output,
        Err(error) => {
            let _ = log_provider_failure(
                &audit_id,
                "generate_prep_summary",
                "generate_prep_summary.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let key_points = match parse_prep_summary_points(&raw_output) {
        Ok(points) => points,
        Err(error) => {
            let _ = log_provider_failure(
                &audit_id,
                "generate_prep_summary",
                "generate_prep_summary.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage(
        &audit_id,
        "generate_prep_summary",
        "generate_prep_summary.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(PrepSummaryResponse {
        provider_id: "codex-chatgpt-oauth".to_string(),
        model: "subscription_oauth".to_string(),
        key_points,
        raw_output_ref: stable_id(&raw_output),
    })
}

#[tauri::command]
fn cleanup_transcript_text_oauth(
    request: TranscriptCleanupRequest,
) -> Result<TranscriptCleanupResponse, String> {
    let cleaned =
        cleanup_transcript_text_oauth_inner(&request.text, &request.context, Some("ui_cleanup"))?;
    Ok(TranscriptCleanupResponse {
        provider_id: "codex-chatgpt-oauth".to_string(),
        model: "subscription_oauth".to_string(),
        raw_output_ref: stable_id(&cleaned),
        text: cleaned,
    })
}

#[tauri::command]
fn log_app_error(input: AppErrorLogInput) -> Result<String, String> {
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
fn export_app_error_logs(session_id: Option<String>) -> Result<Vec<AppErrorLogRecord>, String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    list_app_error_logs(&conn, session_id.as_deref())
}

#[tauri::command]
fn extract_live_state_patch_oauth(session_id: String) -> Result<IngestTranscriptResponse, String> {
    let status = subscription_oauth_status();
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
    let last_event = events
        .last()
        .cloned()
        .ok_or_else(|| "no transcript available for live AI extraction".to_string())?;
    let local_state = derive_decision_state(&session_id, &events);
    let prompt = build_live_state_patch_prompt(&brief, &events, &local_state)?;
    let started = now_ms();
    let raw_output = match run_codex_oauth_prompt_with_timeout(&prompt, 25_000) {
        Ok(output) => output,
        Err(error) => {
            log_extraction_failure(&session_id, "timeout_or_api_error", &error)?;
            return Err(format!(
                "subscription OAuth live extraction failed: {error}"
            ));
        }
    };
    let patch = match parse_live_state_patch(&raw_output) {
        Ok(patch) => patch,
        Err((failure_kind, error)) => {
            log_extraction_failure(
                &session_id,
                failure_kind,
                &format!("{error}: {}", stable_id(&raw_output)),
            )?;
            return Err(format!(
                "subscription OAuth live extraction rejected: {error}"
            ));
        }
    };
    let decision_state = apply_live_state_patch(local_state, &patch, &events);
    let mut suggestions = derive_suggestions(&brief, &events, &decision_state);
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
        &session_id,
        "extract_state_patch",
        "codex-chatgpt-oauth",
        "subscription_oauth",
        "extract_state_patch.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    )?;

    Ok(IngestTranscriptResponse {
        event: last_event,
        suggestions: suggestions.clone(),
        decision_state,
        persisted: PersistedSummary {
            transcript_events: events.len(),
            new_suggestions: suggestions.len(),
            decision_snapshot_id: snapshot_id,
        },
    })
}

#[tauri::command]
fn read_dropped_context_files(paths: Vec<String>) -> Vec<DroppedContextFile> {
    paths
        .into_iter()
        .take(8)
        .map(|path| read_granted_dropped_context_file(PathBuf::from(path)))
        .collect()
}

fn register_drop_read_grants(paths: &[PathBuf]) {
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

fn read_granted_dropped_context_file(path: PathBuf) -> DroppedContextFile {
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
fn set_window_opacity(app: tauri::AppHandle, percent: u8) -> Result<u8, String> {
    let clamped = percent.clamp(10, 100);
    let opacity = clamped as f64 / 100.0;
    set_native_window_opacity(&app, opacity)?;
    Ok(clamped)
}

#[tauri::command]
fn start_prep_dictation(app: tauri::AppHandle) -> Result<PrepDictationStartResponse, String> {
    stop_prep_dictation()?;
    let language = "zh-TW".to_string();
    let helper_path = native_speech_helper_path()?;
    let mut child = Command::new(&helper_path)
        .arg("--language")
        .arg(&language)
        .arg("--source")
        .arg("mic")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start prep dictation helper: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "prep dictation helper stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "prep dictation helper stderr unavailable".to_string())?;
    PREP_DICTATION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|error| error.to_string())?
        .replace(child);

    let app_for_stdout = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let parsed: Result<HelperTranscriptLine, _> = serde_json::from_str(&line);
            match parsed {
                Ok(helper_line) if helper_line.kind == "transcript" && helper_line.is_final => {
                    let cleaned_text = match cleanup_transcript_text_oauth_inner(
                        &helper_line.text,
                        "prep_dictation",
                        Some("prep_dictation_cleanup"),
                    ) {
                        Ok(cleaned_text) => cleaned_text,
                        Err(error) => {
                            let _ = log_app_error_inner(
                                None,
                                "prep_dictation.cleanup_fallback",
                                "native",
                                "warning",
                                &error,
                                serde_json::json!({
                                    "fallback": "raw_transcript_text",
                                    "inputHash": stable_id(&helper_line.text)
                                }),
                            );
                            helper_line.text.clone()
                        }
                    };
                    let _ = app_for_stdout.emit("prep_dictation_text", cleaned_text);
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = log_app_error_inner(
                        None,
                        "prep_dictation.parse_line",
                        "native",
                        "error",
                        &error.to_string(),
                        serde_json::json!({"rawLineHash": stable_id(&line)}),
                    );
                    let _ = app_for_stdout.emit(
                        "prep_dictation_error",
                        format!("failed to parse prep dictation line: {error}"),
                    );
                }
            }
        }
    });

    let app_for_stderr = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = log_app_error_inner(
                None,
                "prep_dictation.stderr",
                "native_speech_helper",
                "error",
                &line,
                serde_json::json!({}),
            );
            let _ = app_for_stderr.emit("prep_dictation_error", line);
        }
    });

    Ok(PrepDictationStartResponse {
        provider_id: native_speech_provider_id().to_string(),
        language,
        helper_path: helper_path.display().to_string(),
    })
}

#[tauri::command]
fn stop_prep_dictation() -> Result<(), String> {
    if let Some(dictation) = PREP_DICTATION.get() {
        if let Some(mut child) = dictation.lock().map_err(|error| error.to_string())?.take() {
            if let Err(error) = child.kill() {
                let _ = log_app_error_inner(
                    None,
                    "prep_dictation.stop.kill",
                    "native",
                    "warning",
                    &error.to_string(),
                    serde_json::json!({}),
                );
            }
            if let Err(error) = child.wait() {
                let _ = log_app_error_inner(
                    None,
                    "prep_dictation.stop.wait",
                    "native",
                    "warning",
                    &error.to_string(),
                    serde_json::json!({}),
                );
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn start_native_transcription(
    app: tauri::AppHandle,
    session_id: String,
    request: Option<NativeTranscriptionRequest>,
) -> Result<NativeTranscriptionStartResponse, String> {
    let request = request.unwrap_or(NativeTranscriptionRequest {
        language: None,
        source: None,
    });
    let language = request.language.unwrap_or_else(|| "zh-TW".to_string());
    let source = request.source.unwrap_or_else(|| "mic".to_string());
    if source != "mic" && source != "system" {
        return Err("native live transcription source must be mic or system".to_string());
    }
    ensure_session_exists(&session_id)?;
    let helper_path = native_speech_helper_path()?;
    let mut child = Command::new(&helper_path)
        .arg("--language")
        .arg(&language)
        .arg("--source")
        .arg(&source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start native speech helper: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "native speech helper stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "native speech helper stderr unavailable".to_string())?;

    NATIVE_TRANSCRIBERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .insert(session_id.clone(), child);
    set_listening_window_mode(&app, true);
    show_main_window(&app);

    let event_session_id = session_id.clone();
    let app_for_stdout = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let parsed: Result<HelperTranscriptLine, _> = serde_json::from_str(&line);
            match parsed {
                Ok(helper_line) if helper_line.kind == "transcript" && !helper_line.is_final => {
                    let _ = app_for_stdout.emit("native_transcript_preview", helper_line);
                }
                Ok(helper_line) if helper_line.kind == "transcript" && helper_line.is_final => {
                    let cleaned_text = match cleanup_transcript_text_oauth_inner(
                        &helper_line.text,
                        "live_transcript",
                        Some(&event_session_id),
                    ) {
                        Ok(cleaned_text) => cleaned_text,
                        Err(error) => {
                            let _ = log_app_error_inner(
                                Some(&event_session_id),
                                "native_transcription.cleanup_fallback",
                                "native",
                                "warning",
                                &error,
                                serde_json::json!({
                                    "fallback": "raw_transcript_text",
                                    "inputHash": stable_id(&helper_line.text)
                                }),
                            );
                            helper_line.text.clone()
                        }
                    };
                    let event_id = stable_id(&format!(
                        "native:{}:{}:{}",
                        event_session_id, helper_line.ended_at_ms, cleaned_text
                    ));
                    let input = TranscriptInput {
                        id: Some(event_id.clone()),
                        text: cleaned_text,
                        source: Some(helper_line.source),
                        speaker: None,
                        speaker_confidence: Some(helper_line.confidence),
                        started_at_ms: Some(helper_line.started_at_ms),
                        ended_at_ms: Some(helper_line.ended_at_ms),
                        is_final: Some(true),
                    };
                    match ingest_transcript_inner(event_session_id.clone(), input) {
                        Ok(payload) => {
                            let _ = app_for_stdout.emit("native_transcript_ingested", payload);
                        }
                        Err(error) => {
                            let _ = log_app_error_inner(
                                Some(&event_session_id),
                                "native_transcription.ingest",
                                "native",
                                "error",
                                &error,
                                serde_json::json!({"eventId": event_id}),
                            );
                            let _ = app_for_stdout.emit("native_transcription_error", error);
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = log_app_error_inner(
                        Some(&event_session_id),
                        "native_transcription.parse_line",
                        "native",
                        "error",
                        &error.to_string(),
                        serde_json::json!({"rawLineHash": stable_id(&line)}),
                    );
                    let _ = app_for_stdout.emit(
                        "native_transcription_error",
                        format!("failed to parse native transcript line: {error}"),
                    );
                }
            }
        }
    });

    let app_for_stderr = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = log_app_error_inner(
                None,
                "native_transcription.stderr",
                "native_speech_helper",
                "error",
                &line,
                serde_json::json!({}),
            );
            let _ = app_for_stderr.emit("native_transcription_error", line);
        }
    });

    Ok(NativeTranscriptionStartResponse {
        session_id,
        provider_id: native_speech_provider_id().to_string(),
        source,
        language,
        helper_path: helper_path.display().to_string(),
    })
}

#[tauri::command]
fn stop_native_transcription(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get() {
        if let Some(mut child) = transcribers
            .lock()
            .map_err(|error| error.to_string())?
            .remove(&session_id)
        {
            if let Err(error) = child.kill() {
                let _ = log_app_error_inner(
                    Some(&session_id),
                    "native_transcription.stop.kill",
                    "native",
                    "warning",
                    &error.to_string(),
                    serde_json::json!({}),
                );
            }
            if let Err(error) = child.wait() {
                let _ = log_app_error_inner(
                    Some(&session_id),
                    "native_transcription.stop.wait",
                    "native",
                    "warning",
                    &error.to_string(),
                    serde_json::json!({}),
                );
            }
        }
    }
    set_listening_window_mode(&app, false);
    Ok(())
}

fn stop_all_native_transcribers(app: &tauri::AppHandle) {
    let _ = stop_prep_dictation();
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get() {
        if let Ok(mut transcribers) = transcribers.lock() {
            for (session_id, mut child) in transcribers.drain() {
                if let Err(error) = child.kill() {
                    let _ = log_app_error_inner(
                        Some(&session_id),
                        "native_transcription.stop_all.kill",
                        "native",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({}),
                    );
                }
                if let Err(error) = child.wait() {
                    let _ = log_app_error_inner(
                        Some(&session_id),
                        "native_transcription.stop_all.wait",
                        "native",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({}),
                    );
                }
            }
        }
    }
    set_listening_window_mode(app, false);
}

fn ingest_transcript_inner(
    session_id: String,
    input: TranscriptInput,
) -> Result<IngestTranscriptResponse, String> {
    let mut sessions = LIVE_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| "session not found".to_string())?;

    let event_index = session.events.len() + 1;
    let event = TranscriptEvent {
        id: input
            .id
            .unwrap_or_else(|| format!("native_{}_{}", session_id, event_index)),
        session_id: session_id.clone(),
        source: input.source.unwrap_or_else(|| "mic".to_string()),
        speaker: input.speaker,
        speaker_confidence: input.speaker_confidence.unwrap_or(0.35),
        language: detect_language(&input.text),
        started_at_ms: input
            .started_at_ms
            .unwrap_or(((event_index - 1) * 5000) as i64),
        ended_at_ms: input.ended_at_ms,
        text: input.text,
        is_final: input.is_final.unwrap_or(true),
    };
    session.events.push(event.clone());

    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    insert_transcript_event(&conn, &event)?;

    let decision_state = derive_decision_state(&session_id, &session.events);
    let mut suggestions = derive_suggestions(&session.brief, &session.events, &decision_state);
    suggestions.retain(|suggestion| session.shown_suggestion_ids.insert(suggestion.id.clone()));

    for suggestion in &suggestions {
        insert_suggestion(&conn, suggestion)?;
    }

    let snapshot_id = stable_id(&format!(
        "{}:{}:{}",
        session_id,
        now_ms(),
        serde_json::to_string(&decision_state).map_err(|error| error.to_string())?
    ));
    insert_decision_snapshot(&conn, &snapshot_id, &session_id, &decision_state)?;

    Ok(IngestTranscriptResponse {
        event,
        suggestions: suggestions.clone(),
        decision_state,
        persisted: PersistedSummary {
            transcript_events: session.events.len(),
            new_suggestions: suggestions.len(),
            decision_snapshot_id: snapshot_id,
        },
    })
}

#[tauri::command]
fn stop_session(session_id: String) -> Result<(), String> {
    if let Some(sessions) = LIVE_SESSIONS.get() {
        sessions
            .lock()
            .map_err(|error| error.to_string())?
            .remove(&session_id);
    }
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    conn.execute(
        "UPDATE meeting_sessions SET ended_at = ?1 WHERE id = ?2",
        params![now_iso(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

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
            read_dropped_context_files,
            text_provider_status,
            start_text_provider_login,
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

fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Meeting Copilot", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop Listening", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &stop, &quit])?;
    TrayIconBuilder::with_id("meeting-copilot")
        .icon(TRAY_ICON)
        .tooltip("Meeting Copilot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "stop" => {
                stop_all_native_transcribers(app);
                show_main_window(app);
                let _ = app.emit(
                    "native_transcription_error",
                    "Native transcription stopped from tray",
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn set_listening_window_mode(app: &tauri::AppHandle, enabled: bool) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(enabled);
    }
}

#[cfg(target_os = "macos")]
fn set_native_window_opacity(app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let ns_window = window.ns_window().map_err(|error| error.to_string())?;
    unsafe {
        let ns_window = &*(ns_window.cast::<objc2_app_kit::NSWindow>());
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
        ns_window.setAlphaValue(opacity.clamp(0.1, 1.0));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_native_window_opacity(app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongW,
        WS_EX_LAYERED,
    };

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let alpha = (opacity.clamp(0.1, 1.0) * 255.0).round() as u8;
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as i32);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn set_native_window_opacity(_app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let _ = opacity;
    Err("native window opacity is not implemented for this platform yet".to_string())
}

fn open_db(db_path: &PathBuf) -> Result<Connection, String> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let conn = Connection::open(db_path).map_err(|error| error.to_string())?;
    conn.execute_batch(SCHEMA_SQL)
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn app_db_path() -> Result<PathBuf, String> {
    let base = app_data_dir()?;
    Ok(base.join("meeting-copilot-native.db"))
}

#[cfg(target_os = "macos")]
fn app_data_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Meeting Copilot"))
}

fn read_dropped_context_file(path: PathBuf) -> DroppedContextFile {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dropped-file")
        .to_string();
    let allowed = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt" | "md" | "markdown" | "csv" | "json" | "log" | "srt" | "vtt"
            )
        })
        .unwrap_or(false);
    if !allowed {
        return DroppedContextFile {
            name,
            text: String::new(),
            truncated: false,
            error: Some("只支援文字檔：txt、md、csv、json、log、srt、vtt".to_string()),
        };
    }
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    if !metadata.is_file() {
        return DroppedContextFile {
            name,
            text: String::new(),
            truncated: false,
            error: Some("拖拉項目不是檔案".to_string()),
        };
    }
    const MAX_CONTEXT_BYTES: usize = 256 * 1024;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    let truncated = bytes.len() > MAX_CONTEXT_BYTES;
    let bytes = if truncated {
        &bytes[..MAX_CONTEXT_BYTES]
    } else {
        bytes.as_slice()
    };
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => DroppedContextFile {
            name,
            text,
            truncated,
            error: None,
        },
        Err(error) => DroppedContextFile {
            name,
            text: String::new(),
            truncated,
            error: Some(format!("不是 UTF-8 文字檔：{error}")),
        },
    }
}

fn subscription_oauth_status() -> TextProviderStatus {
    let codex = codex_command_path();
    let mut command = Command::new(&codex);
    configure_codex_oauth_env(&mut command);
    match command.arg("login").arg("status").output() {
        Ok(output) if output.status.success() => {
            let status_text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let authenticated = parse_subscription_oauth_authenticated(&status_text);
            TextProviderStatus {
                provider_id: "codex-chatgpt-oauth".to_string(),
                kind: "subscription_oauth".to_string(),
                authenticated,
                can_refresh_token: authenticated,
                supports_structured_output: true,
                supports_streaming: true,
                active: authenticated,
                status_label: if authenticated {
                    "已登入 ChatGPT 訂閱 OAuth".to_string()
                } else {
                    "尚未登入 ChatGPT 訂閱 OAuth".to_string()
                },
                last_error: if authenticated {
                    None
                } else {
                    Some(status_text.trim().to_string())
                },
            }
        }
        Ok(output) => TextProviderStatus {
            provider_id: "codex-chatgpt-oauth".to_string(),
            kind: "subscription_oauth".to_string(),
            authenticated: false,
            can_refresh_token: false,
            supports_structured_output: true,
            supports_streaming: true,
            active: false,
            status_label: "無法確認 ChatGPT 訂閱 OAuth".to_string(),
            last_error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        },
        Err(error) => TextProviderStatus {
            provider_id: "codex-chatgpt-oauth".to_string(),
            kind: "subscription_oauth".to_string(),
            authenticated: false,
            can_refresh_token: false,
            supports_structured_output: true,
            supports_streaming: true,
            active: false,
            status_label: "找不到 Codex 訂閱 OAuth connector".to_string(),
            last_error: Some(format!("{}: {error}", codex.display())),
        },
    }
}

fn parse_subscription_oauth_authenticated(status_text: &str) -> bool {
    let normalized = status_text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.trim().is_empty() {
        return false;
    }
    for negative in [
        "not logged in",
        "not authenticated",
        "not signed in",
        "login required",
        "please log in",
        "please login",
        "no chatgpt login",
        "unauthenticated",
    ] {
        if normalized.contains(negative) {
            return false;
        }
    }
    [
        "logged in",
        "authenticated",
        "signed in",
        "chatgpt subscription",
        "chatgpt account",
    ]
    .iter()
    .any(|positive| normalized.contains(positive))
}

fn start_subscription_oauth_login() -> Result<(), String> {
    let codex = codex_command_path();
    if !codex.exists() && codex.to_string_lossy() != "codex" {
        return Err(format!("找不到 Codex connector：{}", codex.display()));
    }
    #[cfg(target_os = "macos")]
    {
        let script_path =
            std::env::temp_dir().join(format!("meeting-copilot-codex-login-{}.command", now_ms()));
        let script = format!(
            r#"#!/bin/zsh
export HOME="${{HOME:-/Users/$USER}}"
export CODEX_HOME="${{CODEX_HOME:-$HOME/.codex}}"
echo "Meeting Copilot ChatGPT subscription OAuth login"
echo "This window was opened by Meeting Copilot. Complete the browser login, then return to the app; Meeting Copilot will refresh the status automatically."
{} login
echo
echo "Login flow finished. You can close this window."
"#,
            shell_quote(&codex.display().to_string())
        );
        fs::write(&script_path, script).map_err(|error| error.to_string())?;
        let _ = Command::new("chmod").arg("+x").arg(&script_path).status();
        Command::new("open")
            .arg(&script_path)
            .spawn()
            .map_err(|error| format!("failed to open login terminal: {error}"))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(codex)
            .arg("login")
            .spawn()
            .map_err(|error| format!("failed to open login terminal: {error}"))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("subscription OAuth login launcher is not implemented for this platform".to_string())
    }
}

fn build_ai_summary_prompt(request: &AiSummaryRequest) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's subscription OAuth text decision provider.
Return ONLY a JSON object with this exact shape:
{{"keyPoints":["..."],"decisionsAndOpenQuestions":["..."],"suggestedActions":["..."]}}

Rules:
- Write Traditional Chinese.
- Use the transcript, prepContext, and localSummary only.
- Do not invent decisions, owners, dates, or commitments.
- If evidence is insufficient, say that explicitly.
- Keep each array to 3-6 concise items.

Meeting payload:
{payload}
"#
    ))
}

fn build_prep_summary_prompt(request: &PrepSummaryRequest) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's preparation summarizer.
Return ONLY a JSON object with this exact shape:
{{"keyPoints":["..."]}}

Rules:
- Write Traditional Chinese.
- Use only the user's prep context and dropped file context.
- Extract what the user must protect in the meeting: decision goal, constraints, risks, missing owner/deadline/acceptance criteria, and questions to ask.
- Do not invent owners, dates, decisions, or commitments.
- Keep 3-6 concise bullet points.

Prep payload:
{payload}
"#
    ))
}

fn cleanup_transcript_text_oauth_inner(
    text: &str,
    context: &str,
    audit_session_id: Option<&str>,
) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let status = subscription_oauth_status();
    if !status.authenticated {
        return Err(status
            .last_error
            .unwrap_or_else(|| "subscription OAuth provider is not authenticated".to_string()));
    }
    let request = TranscriptCleanupRequest {
        text: trimmed.to_string(),
        context: context.to_string(),
    };
    let prompt = build_transcript_cleanup_prompt(&request)?;
    let audit_id = audit_session_id
        .map(|value| stable_id(&format!("cleanup:{value}:{}", request.text)))
        .unwrap_or_else(|| stable_id(&format!("cleanup:{}", request.text)));
    let started = now_ms();
    let raw_output = match run_codex_oauth_prompt_with_timeout(&prompt, 12_000) {
        Ok(output) => output,
        Err(error) => {
            let _ = log_provider_failure(
                &audit_id,
                "cleanup_transcript_text",
                "cleanup_transcript_text.oauth.v1",
                "timeout_or_api_error",
                &error,
            );
            return Err(error);
        }
    };
    let cleaned = match parse_transcript_cleanup_text(&raw_output) {
        Ok(value) => value,
        Err(error) => {
            let _ = log_provider_failure(
                &audit_id,
                "cleanup_transcript_text",
                "cleanup_transcript_text.oauth.v1",
                "schema_validation",
                &format!("{error}: {}", stable_id(&raw_output)),
            );
            return Err(error);
        }
    };
    let _ = log_provider_usage(
        &audit_id,
        "cleanup_transcript_text",
        "cleanup_transcript_text.oauth.v1",
        &prompt,
        &raw_output,
        now_ms().saturating_sub(started) as i64,
    );
    Ok(cleaned)
}

fn build_transcript_cleanup_prompt(request: &TranscriptCleanupRequest) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's transcript cleanup provider.
Return ONLY a JSON object with this exact shape:
{{"text":"..."}}

Rules:
- Preserve the original meaning. Do not summarize.
- Remove only obvious stutters, repeated starts, filler words, and speech disfluencies.
- Keep names, numbers, dates, owners, deadlines, scope, technical terms, and mixed English terms.
- Do not add facts, conclusions, speakers, or punctuation that changes meaning.
- Write Traditional Chinese when the source is Chinese. Preserve English terms that appear in the source.
- If cleanup is unsafe or unnecessary, return the original text.

Transcript payload:
{payload}
"#
    ))
}

fn build_live_state_patch_prompt(
    brief: &MeetingBrief,
    events: &[TranscriptEvent],
    local_state: &NativeDecisionState,
) -> Result<String, String> {
    let recent_events: Vec<&TranscriptEvent> = events
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let payload = serde_json::json!({
        "brief": brief,
        "priorDecisionState": local_state,
        "recentTranscriptEvents": recent_events,
        "allowedEvidenceTranscriptIds": events.iter().map(|event| event.id.clone()).collect::<Vec<_>>()
    });
    let payload = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's live Layer 3 state extraction provider.
Return ONLY a JSON object. You MUST return a PATCH, never a full state rewrite.

Exact shape:
{{
  "meetingStatePatch": {{
    "addItems": [],
    "updateItems": [],
    "resolveItemIds": [],
    "phaseChange": null,
    "evidenceTranscriptIds": []
  }},
  "decisionStatePatch": {{
    "currentDecision": null,
    "addOptions": [],
    "updateOptions": [],
    "addRisks": [],
    "addMissingInputs": [],
    "readinessPatch": {{"score": null, "safeToDecide": null, "blockers": [], "evidenceTranscriptIds": []}},
    "evidenceTranscriptIds": []
  }}
}}

Rules:
- Write Traditional Chinese in text fields.
- Do not invent owners, dates, commitments, decisions, or speakers.
- Use only allowedEvidenceTranscriptIds for evidence.
- If unsure, return empty arrays and null fields.
- addMissingInputs item shape: {{"kind":"owner|deadline|acceptance_criteria|rollback_plan|other","text":"...","blocksDecision":true}}
- addRisks item shape: {{"text":"...","severity":"low|medium|high","evidenceTranscriptIds":["..."]}}
- Do not include fields named meetingState, decisionState, fullState, replacementState, transcriptEvents, or suggestions.

Meeting payload:
{payload}
"#
    ))
}

fn run_codex_oauth_prompt(prompt: &str) -> Result<String, String> {
    run_codex_oauth_prompt_with_timeout(prompt, 60_000)
}

fn run_codex_oauth_prompt_with_timeout(prompt: &str, timeout_ms: u64) -> Result<String, String> {
    let output_path =
        std::env::temp_dir().join(format!("meeting-copilot-ai-summary-{}.txt", now_ms()));
    let mut command = Command::new(codex_command_path());
    configure_codex_oauth_env(&mut command);
    let mut child = command
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--ephemeral")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--output-last-message")
        .arg(&output_path)
        .arg("-")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start subscription OAuth provider: {error}"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "subscription OAuth provider stderr unavailable".to_string())?;
    let stderr_reader = thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr.read_to_string(&mut buffer);
        buffer
    });
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "subscription OAuth provider stdin unavailable".to_string())?;
    stdin
        .write_all(prompt.as_bytes())
        .map_err(|error| error.to_string())?;
    drop(stdin);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                if let Err(error) = child.kill() {
                    let _ = log_app_error_inner(
                        None,
                        "text_provider.timeout.kill",
                        "text_provider",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({"timeoutMs": timeout_ms}),
                    );
                }
                if let Err(error) = child.wait() {
                    let _ = log_app_error_inner(
                        None,
                        "text_provider.timeout.wait",
                        "text_provider",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({"timeoutMs": timeout_ms}),
                    );
                }
                let stderr = stderr_reader.join().unwrap_or_default();
                let _ = fs::remove_file(&output_path);
                let detail = truncate_for_diagnostic(&stderr, 800);
                if detail.is_empty() {
                    return Err("provider timeout".to_string());
                }
                return Err(format!("provider timeout: {detail}"));
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    };
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let _ = fs::remove_file(&output_path);
        let detail = truncate_for_diagnostic(&stderr, 1200);
        return Err(if detail.is_empty() {
            format!("subscription OAuth provider exited with {status}")
        } else {
            format!("subscription OAuth provider exited with {status}: {detail}")
        });
    }
    let output = fs::read_to_string(&output_path)
        .map_err(|error| format!("subscription OAuth provider output unavailable: {error}"));
    let _ = fs::remove_file(&output_path);
    output
}

fn truncate_for_diagnostic(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect::<String>()
}

fn parse_live_state_patch(
    raw_output: &str,
) -> Result<LiveStatePatchEnvelope, (&'static str, String)> {
    let value = parse_json_object_value(raw_output).map_err(|error| ("malformed_json", error))?;
    validate_live_state_patch_value(&value).map_err(|error| ("schema_validation", error))?;
    serde_json::from_value(value).map_err(|error| ("schema_validation", error.to_string()))
}

fn parse_json_object_value(raw_output: &str) -> Result<serde_json::Value, String> {
    let trimmed = raw_output.trim();
    match serde_json::from_str(trimmed) {
        Ok(value) => Ok(value),
        Err(_) => {
            let start = trimmed
                .find('{')
                .ok_or_else(|| "missing JSON object".to_string())?;
            let end = trimmed
                .rfind('}')
                .ok_or_else(|| "missing JSON object".to_string())?;
            serde_json::from_str(&trimmed[start..=end]).map_err(|error| error.to_string())
        }
    }
}

fn validate_live_state_patch_value(value: &serde_json::Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "patch output must be an object".to_string())?;
    for forbidden in [
        "meetingState",
        "decisionState",
        "fullState",
        "replacementState",
        "transcriptEvents",
        "suggestions",
    ] {
        if object.contains_key(forbidden) {
            return Err(format!(
                "provider attempted full rewrite field: {forbidden}"
            ));
        }
    }
    let meeting = object
        .get("meetingStatePatch")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "meetingStatePatch is required".to_string())?;
    let decision = object
        .get("decisionStatePatch")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "decisionStatePatch is required".to_string())?;
    for field in [
        "addItems",
        "updateItems",
        "resolveItemIds",
        "evidenceTranscriptIds",
    ] {
        if !meeting.get(field).is_some_and(|value| value.is_array()) {
            return Err(format!("meetingStatePatch.{field} must be an array"));
        }
    }
    for field in [
        "addOptions",
        "updateOptions",
        "addRisks",
        "addMissingInputs",
        "evidenceTranscriptIds",
    ] {
        if !decision.get(field).is_some_and(|value| value.is_array()) {
            return Err(format!("decisionStatePatch.{field} must be an array"));
        }
    }
    reject_nested_full_rewrite(value, "")?;
    Ok(())
}

fn reject_nested_full_rewrite(value: &serde_json::Value, path: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Object(object) => {
            for (key, nested) in object {
                if matches!(
                    key.as_str(),
                    "meetingState"
                        | "decisionState"
                        | "fullState"
                        | "replacementState"
                        | "transcriptEvents"
                        | "suggestions"
                ) {
                    return Err(format!(
                        "provider attempted nested full rewrite field: {path}{key}"
                    ));
                }
                reject_nested_full_rewrite(nested, &format!("{path}{key}."))?;
            }
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                reject_nested_full_rewrite(nested, path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn codex_command_path() -> PathBuf {
    for candidate in ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("codex")
}

fn configure_codex_oauth_env(command: &mut Command) {
    if let Ok(home) = std::env::var("HOME") {
        command.env("HOME", &home);
        let codex_home = PathBuf::from(&home).join(".codex");
        if codex_home.exists() {
            command.env("CODEX_HOME", codex_home);
        }
        return;
    }
    if let Ok(user) = std::env::var("USER") {
        let home = PathBuf::from("/Users").join(user);
        if home.exists() {
            command.env("HOME", &home);
            let codex_home = home.join(".codex");
            if codex_home.exists() {
                command.env("CODEX_HOME", codex_home);
            }
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_ai_summary_sections(raw_output: &str) -> Result<AiSummarySections, String> {
    let trimmed = raw_output.trim();
    serde_json::from_str(trimmed)
        .or_else(|_| {
            let start = trimmed.find('{').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("missing JSON object"))
            })?;
            let end = trimmed.rfind('}').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("missing JSON object"))
            })?;
            serde_json::from_str(&trimmed[start..=end])
        })
        .map_err(|error| {
            format!("subscription OAuth provider returned invalid summary JSON: {error}")
        })
}

fn parse_prep_summary_points(raw_output: &str) -> Result<Vec<String>, String> {
    let value = parse_json_object_value(raw_output).map_err(|error| {
        format!("subscription OAuth provider returned invalid prep JSON: {error}")
    })?;
    let points = value
        .get("keyPoints")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "subscription OAuth provider prep summary missing keyPoints".to_string())?;
    Ok(points
        .iter()
        .filter_map(|value| value.as_str())
        .map(|value| value.trim().chars().take(160).collect::<String>())
        .filter(|value| !value.is_empty())
        .take(6)
        .collect())
}

fn parse_transcript_cleanup_text(raw_output: &str) -> Result<String, String> {
    let value = parse_json_object_value(raw_output).map_err(|error| {
        format!("subscription OAuth provider returned invalid cleanup JSON: {error}")
    })?;
    let text = value
        .get("text")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "subscription OAuth provider cleanup output missing text".to_string())?
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("subscription OAuth provider cleanup text must not be empty".to_string());
    }
    Ok(text)
}

#[cfg(target_os = "windows")]
fn app_data_dir() -> Result<PathBuf, String> {
    let app_data = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(app_data).join("Meeting Copilot"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn app_data_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("meeting-copilot"))
}

fn ensure_session_exists(session_id: &str) -> Result<(), String> {
    let sessions = LIVE_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    if sessions
        .lock()
        .map_err(|error| error.to_string())?
        .contains_key(session_id)
    {
        Ok(())
    } else {
        Err("session not found".to_string())
    }
}

fn native_speech_provider_id() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos-speech-native"
    }
    #[cfg(target_os = "windows")]
    {
        "windows-speech-native"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "unsupported-speech-native"
    }
}

fn native_speech_helper_path() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let host = rust_host_triple();
    let dev_binary = if cfg!(target_os = "windows") {
        format!("{NATIVE_SPEECH_HELPER}-{host}.exe")
    } else {
        format!("{NATIVE_SPEECH_HELPER}-{host}")
    };
    let runtime_binary = if cfg!(target_os = "windows") {
        format!("{NATIVE_SPEECH_HELPER}.exe")
    } else {
        NATIVE_SPEECH_HELPER.to_string()
    };
    let dev_path = cwd.join("src-tauri").join("binaries").join(dev_binary);
    if dev_path.exists() {
        return Ok(dev_path);
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(parent) = exe.parent() {
        let sibling = parent.join(&runtime_binary);
        if sibling.exists() {
            return Ok(sibling);
        }
        let bundled = parent.join("../Resources").join(&runtime_binary);
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    Err(format!("native speech helper not found: {runtime_binary}"))
}

fn rust_host_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "aarch64-pc-windows-msvc"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64")
    )))]
    {
        "unknown"
    }
}

fn insert_session(
    conn: &Connection,
    brief: &MeetingBrief,
    text_provider_enabled: bool,
) -> Result<(), String> {
    let disclosure = serde_json::json!({
        "sttProvider": native_speech_provider_id(),
        "llmProvider": if text_provider_enabled { "codex-chatgpt-oauth" } else { "disabled" },
        "sentAudioToCloud": false,
        "sentTranscriptToCloud": text_provider_enabled,
        "sentMemoryToCloud": text_provider_enabled,
        "textProviderKind": if text_provider_enabled { "subscription_oauth" } else { "none" }
    });
    conn.execute(
        "INSERT OR REPLACE INTO meeting_sessions
        (id, project_id, title, meeting_type, started_at, ended_at, brief_json, processing_disclosure_json)
        VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
        params![
            brief.session_id,
            brief.project_id,
            brief.title,
            brief.meeting_type,
            brief.started_at,
            serde_json::to_string(brief).map_err(|error| error.to_string())?,
            disclosure.to_string()
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_transcript_event(conn: &Connection, event: &TranscriptEvent) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO transcript_events
        (id, session_id, source, speaker, speaker_confidence, language, language_segments_json,
         started_at_ms, ended_at_ms, text, is_final)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10)",
        params![
            event.id,
            event.session_id,
            event.source,
            event.speaker,
            event.speaker_confidence,
            event.language,
            event.started_at_ms,
            event.ended_at_ms,
            event.text,
            if event.is_final { 1 } else { 0 }
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_suggestion(conn: &Connection, suggestion: &NativeSuggestion) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO suggestions
        (id, session_id, shown_at, text, reason, trigger_rule_id, confidence, priority, evidence_transcript_ids_json, feedback)
        VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, NULL)",
        params![
            suggestion.id,
            suggestion.session_id,
            suggestion.shown_at,
            suggestion.text,
            suggestion.reason,
            suggestion.confidence,
            suggestion.priority,
            serde_json::to_string(&suggestion.evidence_transcript_ids)
                .map_err(|error| error.to_string())?
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_decision_snapshot(
    conn: &Connection,
    snapshot_id: &str,
    session_id: &str,
    decision_state: &NativeDecisionState,
) -> Result<(), String> {
    insert_decision_snapshot_with_source(conn, snapshot_id, session_id, decision_state, None)
}

fn insert_decision_snapshot_with_source(
    conn: &Connection,
    snapshot_id: &str,
    session_id: &str,
    decision_state: &NativeDecisionState,
    source_extraction_id: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO decision_state_snapshots
        (id, session_id, created_at_ms, decision_state_json, source_extraction_id)
        VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            snapshot_id,
            session_id,
            now_ms() as i64,
            serde_json::to_string(decision_state).map_err(|error| error.to_string())?,
            source_extraction_id
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_llm_usage_log(
    conn: &Connection,
    session_id: &str,
    call_type: &str,
    provider: &str,
    model: &str,
    prompt_version: &str,
    prompt: &str,
    output: &str,
    latency_ms: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO llm_usage_logs
        (id, session_id, call_type, provider, model, prompt_version, prompt_hash, input_tokens, cached_input_tokens, output_tokens, audio_input_tokens, estimated_cost_usd, latency_ms, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, NULL, NULL, ?10, ?11)",
        params![
            stable_id(&format!("usage:{session_id}:{call_type}:{provider}:{}", now_ms())),
            session_id,
            call_type,
            provider,
            model,
            prompt_version,
            stable_id(prompt),
            estimate_tokens(prompt),
            estimate_tokens(output),
            latency_ms,
            now_iso()
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_app_error_log(conn: &Connection, record: &AppErrorLogRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_error_logs
        (id, session_id, stage, source, severity, message, detail_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.id,
            record.session_id,
            record.stage,
            record.source,
            record.severity,
            record.message,
            serde_json::to_string(&record.detail_json).map_err(|error| error.to_string())?,
            record.created_at
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn list_app_error_logs(
    conn: &Connection,
    session_id: Option<&str>,
) -> Result<Vec<AppErrorLogRecord>, String> {
    let mut records = vec![];
    let push_row = |row: &rusqlite::Row<'_>| -> Result<AppErrorLogRecord, rusqlite::Error> {
        let detail_text: String = row.get(6)?;
        let detail_json = serde_json::from_str(&detail_text).unwrap_or_else(|_| {
            serde_json::json!({
                "parseError": "stored detail_json was malformed",
                "raw": detail_text
            })
        });
        Ok(AppErrorLogRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            stage: row.get(2)?,
            source: row.get(3)?,
            severity: row.get(4)?,
            message: row.get(5)?,
            detail_json,
            created_at: row.get(7)?,
        })
    };
    if let Some(session_id) = session_id {
        let mut statement = conn
            .prepare(
                "SELECT id, session_id, stage, source, severity, message, detail_json, created_at
                FROM app_error_logs
                WHERE session_id = ?1 OR session_id IS NULL
                ORDER BY CAST(created_at AS INTEGER) ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![session_id], |row| push_row(row))
            .map_err(|error| error.to_string())?;
        for row in rows {
            records.push(row.map_err(|error| error.to_string())?);
        }
    } else {
        let mut statement = conn
            .prepare(
                "SELECT id, session_id, stage, source, severity, message, detail_json, created_at
                FROM app_error_logs
                ORDER BY CAST(created_at AS INTEGER) ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| push_row(row))
            .map_err(|error| error.to_string())?;
        for row in rows {
            records.push(row.map_err(|error| error.to_string())?);
        }
    }
    Ok(records)
}

fn log_app_error_inner(
    session_id: Option<&str>,
    stage: &str,
    source: &str,
    severity: &str,
    message: &str,
    detail_json: serde_json::Value,
) -> Result<String, String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    let record = AppErrorLogRecord {
        id: stable_id(&format!(
            "app-error:{}:{}:{}:{}:{}",
            session_id.unwrap_or("global"),
            stage,
            source,
            message,
            now_ms()
        )),
        session_id: session_id.map(ToString::to_string),
        stage: stage.to_string(),
        source: source.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
        detail_json,
        created_at: now_iso(),
    };
    insert_app_error_log(&conn, &record)?;
    Ok(record.id)
}

fn log_provider_usage(
    audit_id: &str,
    call_type: &str,
    prompt_version: &str,
    prompt: &str,
    output: &str,
    latency_ms: i64,
) -> Result<(), String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    insert_llm_usage_log(
        &conn,
        audit_id,
        call_type,
        "codex-chatgpt-oauth",
        "subscription_oauth",
        prompt_version,
        prompt,
        output,
        latency_ms,
    )
}

fn log_provider_failure(
    audit_id: &str,
    call_type: &str,
    prompt_version: &str,
    failure_kind: &str,
    raw_output_ref: &str,
) -> Result<(), String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    conn.execute(
        "INSERT INTO ai_provider_failure_logs
        (id, audit_id, call_type, prompt_version, provider, failure_kind, raw_output_ref, created_at)
        VALUES (?1, ?2, ?3, ?4, 'codex-chatgpt-oauth', ?5, ?6, ?7)",
        params![
            stable_id(&format!("provider-failure:{audit_id}:{call_type}:{failure_kind}:{raw_output_ref}:{}", now_ms())),
            audit_id,
            call_type,
            prompt_version,
            failure_kind,
            stable_id(raw_output_ref),
            now_iso()
        ],
    )
    .map_err(|error| error.to_string())?;
    let _ = log_app_error_inner(
        Some(audit_id),
        call_type,
        "text_provider",
        "error",
        failure_kind,
        serde_json::json!({
            "promptVersion": prompt_version,
            "provider": "codex-chatgpt-oauth",
            "rawOutputRef": stable_id(raw_output_ref)
        }),
    );
    Ok(())
}

fn log_extraction_failure(
    session_id: &str,
    failure_kind: &str,
    raw_output_ref: &str,
) -> Result<(), String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    conn.execute(
        "INSERT INTO extraction_failure_logs
        (id, session_id, call_type, prompt_version, provider, failure_kind, raw_output_ref, created_at)
        VALUES (?1, ?2, 'extract_state_patch', 'extract_state_patch.oauth.v1', 'codex-chatgpt-oauth', ?3, ?4, ?5)",
        params![
            stable_id(&format!("failure:{session_id}:{failure_kind}:{raw_output_ref}:{}", now_ms())),
            session_id,
            failure_kind,
            raw_output_ref,
            now_iso()
        ],
    )
    .map_err(|error| error.to_string())?;
    let _ = log_app_error_inner(
        Some(session_id),
        "extract_state_patch",
        "state_extraction_engine",
        "error",
        failure_kind,
        serde_json::json!({
            "promptVersion": "extract_state_patch.oauth.v1",
            "provider": "codex-chatgpt-oauth",
            "rawOutputRef": raw_output_ref
        }),
    );
    Ok(())
}

fn estimate_tokens(text: &str) -> i64 {
    ((text.chars().count() as f64) / 4.0).ceil() as i64
}

fn derive_decision_state(session_id: &str, events: &[TranscriptEvent]) -> NativeDecisionState {
    let texts: Vec<String> = events.iter().map(|event| event.text.clone()).collect();
    let joined = texts.join(" ").to_lowercase();
    let mut blockers = vec![];
    let mut missing_inputs = vec![];
    if contains_any(&joined, &["owner", "負責", "誰"])
        && contains_any(&joined, &["沒", "未", "還", "不清楚"])
    {
        blockers.push("還沒有明確 owner".to_string());
        missing_inputs.push(
            serde_json::json!({"kind":"owner","text":"還沒有明確 owner","blocksDecision":true}),
        );
    }
    if contains_any(&joined, &["deadline", "時程", "什麼時候"])
        && contains_any(&joined, &["沒", "未", "還", "不要"])
    {
        blockers.push("deadline 還沒有明確承諾".to_string());
        missing_inputs.push(serde_json::json!({"kind":"deadline","text":"deadline 還沒有明確承諾","blocksDecision":true}));
    }
    if contains_any(&joined, &["驗收", "acceptance", "成功標準"])
        && contains_any(&joined, &["沒", "未", "還", "不清楚"])
    {
        blockers.push("驗收標準還沒定".to_string());
        missing_inputs.push(serde_json::json!({"kind":"acceptance_criteria","text":"驗收標準還沒定","blocksDecision":true}));
    }
    let has_decision = contains_any(&joined, &["決定", "commit", "先這樣", "scope", "v1"]);
    let score = if has_decision {
        (1.0_f64 - blockers.len() as f64 * 0.22).max(0.0)
    } else {
        0.0
    };
    NativeDecisionState {
        session_id: session_id.to_string(),
        current_decision: events.last().map(|event| event.text.clone()),
        decision_type: if joined.contains("scope") || joined.contains("範圍") {
            "scope".to_string()
        } else {
            "unknown".to_string()
        },
        meeting_items: vec![],
        options: vec![],
        risks: vec![],
        missing_inputs,
        readiness: NativeReadiness {
            score,
            safe_to_decide: has_decision && blockers.is_empty() && score >= 0.72,
            blockers,
            evidence_transcript_ids: events.iter().map(|event| event.id.clone()).collect(),
        },
        evidence_transcript_ids: events.iter().map(|event| event.id.clone()).collect(),
    }
}

fn apply_live_state_patch(
    mut state: NativeDecisionState,
    patch: &LiveStatePatchEnvelope,
    events: &[TranscriptEvent],
) -> NativeDecisionState {
    let allowed_ids: HashSet<String> = events.iter().map(|event| event.id.clone()).collect();
    for item in &patch.meeting_state_patch.add_items {
        push_unique_state_value(&mut state.meeting_items, item, 220);
    }
    for item in &patch.meeting_state_patch.update_items {
        push_unique_state_value(&mut state.meeting_items, item, 220);
    }
    if let Some(phase) = patch
        .meeting_state_patch
        .phase_change
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        push_unique_state_value(
            &mut state.meeting_items,
            &serde_json::json!({"kind": "phase", "text": format!("會議階段：{phase}")}),
            220,
        );
    }
    for option in &patch.decision_state_patch.add_options {
        push_unique_state_value(&mut state.options, option, 180);
    }
    for option in &patch.decision_state_patch.update_options {
        push_unique_state_value(&mut state.options, option, 180);
    }
    let meeting_evidence = sanitize_evidence(
        &patch.meeting_state_patch.evidence_transcript_ids,
        &allowed_ids,
    );
    if !meeting_evidence.is_empty() {
        state.evidence_transcript_ids =
            merge_strings(&state.evidence_transcript_ids, &meeting_evidence);
        state.readiness.evidence_transcript_ids =
            merge_strings(&state.readiness.evidence_transcript_ids, &meeting_evidence);
    }
    let patch_evidence = sanitize_evidence(
        &patch.decision_state_patch.evidence_transcript_ids,
        &allowed_ids,
    );
    if !patch_evidence.is_empty() {
        state.evidence_transcript_ids =
            merge_strings(&state.evidence_transcript_ids, &patch_evidence);
        state.readiness.evidence_transcript_ids =
            merge_strings(&state.readiness.evidence_transcript_ids, &patch_evidence);
    }

    if let Some(current_decision) = patch
        .decision_state_patch
        .current_decision
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        state.current_decision = Some(current_decision.chars().take(180).collect());
    }

    for item in &patch.decision_state_patch.add_missing_inputs {
        let text = value_string(item, "text")
            .chars()
            .take(160)
            .collect::<String>();
        if text.is_empty()
            || state
                .missing_inputs
                .iter()
                .any(|existing| value_string(existing, "text") == text)
        {
            continue;
        }
        let kind = value_string(item, "kind");
        let blocks_decision = item
            .get("blocksDecision")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        state.missing_inputs.push(
            serde_json::json!({"kind": kind, "text": text, "blocksDecision": blocks_decision}),
        );
        if blocks_decision && !state.readiness.blockers.contains(&text) {
            state.readiness.blockers.push(text);
        }
    }

    for risk in &patch.decision_state_patch.add_risks {
        let text = value_string(risk, "text")
            .chars()
            .take(160)
            .collect::<String>();
        if text.is_empty() {
            continue;
        }
        let severity = value_string(risk, "severity");
        push_unique_state_value(
            &mut state.risks,
            &serde_json::json!({"severity": severity.clone(), "text": text}),
            160,
        );
        if matches!(severity.as_str(), "medium" | "high") {
            let blocker = format!("風險：{text}");
            if !state.readiness.blockers.contains(&blocker) {
                state.readiness.blockers.push(blocker);
            }
        }
    }

    if let Some(readiness) = &patch.decision_state_patch.readiness_patch {
        if let Some(score) = readiness.score {
            state.readiness.score = score.clamp(0.0, 1.0);
        }
        if let Some(blockers) = &readiness.blockers {
            for blocker in blockers
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                let blocker = blocker.chars().take(160).collect::<String>();
                if !state.readiness.blockers.contains(&blocker) {
                    state.readiness.blockers.push(blocker);
                }
            }
        }
        if let Some(evidence) = &readiness.evidence_transcript_ids {
            let evidence = sanitize_evidence(evidence, &allowed_ids);
            state.readiness.evidence_transcript_ids =
                merge_strings(&state.readiness.evidence_transcript_ids, &evidence);
        }
        if let Some(safe_to_decide) = readiness.safe_to_decide {
            state.readiness.safe_to_decide = safe_to_decide
                && state.readiness.blockers.is_empty()
                && state.readiness.score >= 0.72;
        }
    }

    if !state.readiness.blockers.is_empty() {
        state.readiness.safe_to_decide = false;
        state.readiness.score = state.readiness.score.min(0.68);
    }
    state
}

fn push_unique_state_value(
    target: &mut Vec<serde_json::Value>,
    value: &serde_json::Value,
    max_chars: usize,
) {
    let text = value_string(value, "text")
        .chars()
        .take(max_chars)
        .collect::<String>();
    if text.trim().is_empty() {
        return;
    }
    if target
        .iter()
        .any(|existing| value_string(existing, "text") == text)
    {
        return;
    }
    let mut normalized = value.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.insert("text".to_string(), serde_json::Value::String(text));
    } else {
        normalized = serde_json::json!({ "text": text });
    }
    target.push(normalized);
}

fn sanitize_evidence(values: &[String], allowed_ids: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|id| allowed_ids.contains(*id))
        .cloned()
        .collect()
}

fn merge_strings(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = vec![];
    for item in left.iter().chain(right.iter()) {
        if !merged.contains(item) {
            merged.push(item.clone());
        }
    }
    merged
}

fn value_string(value: &serde_json::Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn derive_suggestions(
    _brief: &MeetingBrief,
    events: &[TranscriptEvent],
    decision_state: &NativeDecisionState,
) -> Vec<NativeSuggestion> {
    if decision_state.current_decision.is_none()
        || decision_state.readiness.safe_to_decide
        || decision_state.readiness.blockers.is_empty()
    {
        return vec![];
    }
    let text = "先不要定案，這裡還缺 owner、deadline 或驗收標準。建議先補問清楚再承諾 scope。";
    let evidence: Vec<String> = events.iter().map(|event| event.id.clone()).collect();
    vec![NativeSuggestion {
        id: stable_id(&format!(
            "identify_missing_input:{}:{}",
            decision_state.session_id,
            evidence.join(",")
        )),
        session_id: decision_state.session_id.clone(),
        shown_at: now_iso(),
        kind: "identify_missing_input".to_string(),
        text: text.to_string(),
        reason: format!(
            "Decision readiness score {:.2}; blockers: {}",
            decision_state.readiness.score,
            decision_state.readiness.blockers.join(", ")
        ),
        confidence: 0.86,
        priority: "high".to_string(),
        evidence_transcript_ids: evidence,
    }]
}

fn default_brief() -> MeetingBrief {
    MeetingBrief {
        session_id: format!("native_{}", now_ms()),
        project_id: Some("native_default_project".to_string()),
        meeting_type: "requirement_scoping".to_string(),
        title: Some("即時會議".to_string()),
        goal: "即時監聽會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope".to_string(),
        must_confirm: vec![
            "owner".to_string(),
            "deadline".to_string(),
            "驗收標準".to_string(),
            "rollback plan".to_string(),
        ],
        risks: vec![
            "未定義 owner/deadline 就做承諾".to_string(),
            "demo scope 和正式版 scope 混在一起".to_string(),
        ],
        constraints: vec!["先確認決策條件再承諾交付".to_string()],
        known_participants: vec![],
        preferred_tone: "direct".to_string(),
        started_at: now_iso(),
    }
}

fn detect_language(text: &str) -> String {
    let has_chinese = text
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    let has_english = text.chars().any(|ch| ch.is_ascii_alphabetic());
    match (has_chinese, has_english) {
        (true, true) => "mixed",
        (true, false) => "zh-TW",
        (false, true) => "en",
        _ => "unknown",
    }
    .to_string()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn now_iso() -> String {
    // Stable enough for local audit rows without adding a time crate.
    format!("{}", now_ms())
}

fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_context_file_reads_utf8_text() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-drop-{}.md", now_ms()));
        fs::write(&path, "今天只確認 demo scope").expect("write temp context file");
        let file = read_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(file.name.starts_with("meeting-copilot-drop-"));
        assert!(file.text.contains("demo scope"));
        assert!(!file.truncated);
        assert!(file.error.is_none());
    }

    #[test]
    fn dropped_context_file_rejects_unsupported_extension() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-drop-{}.png", now_ms()));
        fs::write(&path, "not actually an image").expect("write temp unsupported file");
        let file = read_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(file.text.is_empty());
        assert!(file.error.unwrap_or_default().contains("只支援文字檔"));
    }

    #[test]
    fn oauth_status_parser_does_not_accept_negative_logged_in_text() {
        assert!(!parse_subscription_oauth_authenticated(
            "Not logged in. Run codex login to use ChatGPT."
        ));
        assert!(!parse_subscription_oauth_authenticated(
            "ChatGPT login required"
        ));
        assert!(parse_subscription_oauth_authenticated(
            "Logged in to ChatGPT account"
        ));
    }

    #[test]
    fn transcript_cleanup_parser_accepts_text_only_shape() {
        let cleaned =
            parse_transcript_cleanup_text(r#"{"text":"這場只確認 demo 範圍，不承諾正式版時程。"}"#)
                .expect("parse cleanup text");
        assert_eq!(cleaned, "這場只確認 demo 範圍，不承諾正式版時程。");
    }

    #[test]
    fn transcript_cleanup_prompt_preserves_meaning_boundary() {
        let prompt = build_transcript_cleanup_prompt(&TranscriptCleanupRequest {
            text: "呃我們就是先確認 demo scope 然後不承諾 deadline".to_string(),
            context: "test".to_string(),
        })
        .expect("build cleanup prompt");
        assert!(prompt.contains("Do not summarize"));
        assert!(prompt.contains("Preserve the original meaning"));
        assert!(prompt.contains("names, numbers, dates, owners, deadlines, scope"));
    }

    #[test]
    fn app_error_log_round_trips_diagnostic_detail() {
        let db_path =
            std::env::temp_dir().join(format!("meeting-copilot-error-log-{}.db", now_ms()));
        let conn = open_db(&db_path).expect("open temp db");
        let record = AppErrorLogRecord {
            id: "error-log-test".to_string(),
            session_id: Some("session-test".to_string()),
            stage: "native_transcription.stderr".to_string(),
            source: "native_speech_helper".to_string(),
            severity: "error".to_string(),
            message: "helper failed".to_string(),
            detail_json: serde_json::json!({"platform": "test"}),
            created_at: now_iso(),
        };
        insert_app_error_log(&conn, &record).expect("insert app error log");
        let records =
            list_app_error_logs(&conn, Some("session-test")).expect("list app error logs");
        let _ = fs::remove_file(db_path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].stage, "native_transcription.stderr");
        assert_eq!(records[0].detail_json["platform"], "test");
    }

    #[test]
    fn dropped_context_requires_native_drop_grant() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-ungranted-{}.md", now_ms()));
        fs::write(&path, "private meeting notes").expect("write temp context file");
        let file = read_granted_dropped_context_file(path.clone());
        assert!(file.text.is_empty());
        assert!(file.error.unwrap_or_default().contains("未經本次拖拉授權"));

        register_drop_read_grants(&[path.clone()]);
        let granted = read_granted_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(granted.error.is_none());
        assert!(granted.text.contains("private meeting notes"));
    }

    #[test]
    fn live_patch_applies_meeting_items_options_and_risks() {
        let events = vec![TranscriptEvent {
            id: "e1".to_string(),
            session_id: "s1".to_string(),
            source: "mic".to_string(),
            speaker: Some("A".to_string()),
            speaker_confidence: 0.7,
            language: "mixed".to_string(),
            started_at_ms: 0,
            ended_at_ms: Some(1),
            text: "先比較方案 A 和 B，但風險還沒釐清".to_string(),
            is_final: true,
        }];
        let state = derive_decision_state("s1", &events);
        let patch = LiveStatePatchEnvelope {
            meeting_state_patch: LiveMeetingStatePatch {
                add_items: vec![
                    serde_json::json!({"kind": "context", "text": "客戶只要 demo scope"}),
                ],
                update_items: vec![],
                resolve_item_ids: vec![],
                phase_change: Some("對齊方案".to_string()),
                evidence_transcript_ids: vec!["e1".to_string()],
            },
            decision_state_patch: LiveDecisionStatePatch {
                current_decision: Some("先做 demo scope".to_string()),
                add_options: vec![serde_json::json!({"text": "方案 A"})],
                update_options: vec![],
                add_risks: vec![
                    serde_json::json!({"severity": "high", "text": "正式版 scope 被偷渡"}),
                ],
                add_missing_inputs: vec![],
                readiness_patch: None,
                evidence_transcript_ids: vec!["e1".to_string()],
            },
        };

        let patched = apply_live_state_patch(state, &patch, &events);
        assert!(
            patched
                .meeting_items
                .iter()
                .any(|item| value_string(item, "text").contains("demo scope"))
        );
        assert!(
            patched
                .options
                .iter()
                .any(|item| value_string(item, "text") == "方案 A")
        );
        assert!(
            patched
                .risks
                .iter()
                .any(|item| value_string(item, "text").contains("偷渡"))
        );
        assert!(
            patched
                .readiness
                .blockers
                .iter()
                .any(|item| item.contains("風險"))
        );
    }

    #[test]
    fn live_patch_rejects_nested_full_rewrite_attempt() {
        let raw = serde_json::json!({
            "meetingStatePatch": {
                "addItems": [{"text": "bad", "decisionState": {"currentDecision": "rewrite"}}],
                "updateItems": [],
                "resolveItemIds": [],
                "evidenceTranscriptIds": []
            },
            "decisionStatePatch": {
                "addOptions": [],
                "updateOptions": [],
                "addRisks": [],
                "addMissingInputs": [],
                "evidenceTranscriptIds": []
            }
        });
        assert!(
            validate_live_state_patch_value(&raw)
                .unwrap_err()
                .contains("nested full rewrite")
        );
    }
}
