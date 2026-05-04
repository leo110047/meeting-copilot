#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTranscriberHealthRequest {
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperHealthLine {
    provider_id: String,
    ready: bool,
    supports_streaming: bool,
    supports_diarization: bool,
    supports_source_hints: bool,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeTranscriptionErrorEvent {
    message: String,
    source: String,
    code: String,
}
