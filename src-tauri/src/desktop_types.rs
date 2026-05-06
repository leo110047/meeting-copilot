use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopShellPlan {
    pub(crate) platform: &'static str,
    pub(crate) status_surface: &'static str,
    pub(crate) audio_capture: &'static str,
    pub(crate) suggestion_surface: &'static str,
}

#[cfg(target_os = "macos")]
pub(crate) fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "macos",
        status_surface: "macos_status_item",
        audio_capture: "coreaudio+screencapturekit",
        suggestion_surface: "popover",
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "windows",
        status_surface: "windows_system_tray",
        audio_capture: "wasapi_capture+wasapi_loopback",
        suggestion_surface: "flyout",
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn desktop_shell_plan() -> DesktopShellPlan {
    DesktopShellPlan {
        platform: "unsupported",
        status_surface: "none",
        audio_capture: "none",
        suggestion_surface: "none",
    }
}

#[derive(Debug)]
pub(crate) struct NativeLiveSession {
    pub(crate) brief: MeetingBrief,
    pub(crate) text_provider_id: Option<String>,
    pub(crate) events: Vec<TranscriptEvent>,
    pub(crate) shown_suggestion_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingBrief {
    pub(crate) session_id: String,
    pub(crate) project_id: Option<String>,
    pub(crate) meeting_type: String,
    pub(crate) title: Option<String>,
    pub(crate) goal: String,
    pub(crate) must_confirm: Vec<String>,
    pub(crate) risks: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) known_participants: Vec<serde_json::Value>,
    pub(crate) preferred_tone: String,
    pub(crate) started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartSessionRequest {
    pub(crate) brief: Option<MeetingBrief>,
    pub(crate) text_provider_enabled: Option<bool>,
    // Provider routing only. Provider IDs are logged in dedicated audit columns, not echoed into prompt payload JSON.
    #[serde(default, skip_serializing)]
    pub(crate) text_provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartSessionResponse {
    pub(crate) session_id: String,
    pub(crate) brief: MeetingBrief,
    pub(crate) db_path: String,
    pub(crate) platform: DesktopShellPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptInput {
    pub(crate) id: Option<String>,
    pub(crate) text: String,
    pub(crate) source: Option<String>,
    pub(crate) speaker: Option<String>,
    pub(crate) speaker_confidence: Option<f64>,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) ended_at_ms: Option<i64>,
    pub(crate) is_final: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptEvent {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) source: String,
    pub(crate) speaker: Option<String>,
    pub(crate) speaker_confidence: f64,
    pub(crate) language: String,
    pub(crate) started_at_ms: i64,
    pub(crate) ended_at_ms: Option<i64>,
    pub(crate) text: String,
    pub(crate) is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeSuggestion {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) shown_at: String,
    pub(crate) kind: String,
    pub(crate) title: Option<String>,
    pub(crate) text: String,
    pub(crate) suggested_move: Option<String>,
    pub(crate) watch_out: Option<String>,
    pub(crate) reason: String,
    pub(crate) confidence: f64,
    pub(crate) priority: String,
    pub(crate) evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeReadiness {
    pub(crate) score: f64,
    pub(crate) safe_to_decide: bool,
    pub(crate) blockers: Vec<String>,
    pub(crate) evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeDecisionState {
    pub(crate) session_id: String,
    pub(crate) current_decision: Option<String>,
    pub(crate) decision_type: String,
    pub(crate) meeting_items: Vec<serde_json::Value>,
    pub(crate) options: Vec<serde_json::Value>,
    pub(crate) risks: Vec<serde_json::Value>,
    pub(crate) missing_inputs: Vec<serde_json::Value>,
    pub(crate) readiness: NativeReadiness,
    pub(crate) evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IngestTranscriptResponse {
    pub(crate) event: TranscriptEvent,
    pub(crate) suggestions: Vec<NativeSuggestion>,
    pub(crate) decision_state: NativeDecisionState,
    pub(crate) persisted: PersistedSummary,
    pub(crate) coaching_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedSummary {
    pub(crate) transcript_events: usize,
    pub(crate) new_suggestions: usize,
    pub(crate) decision_snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeTranscriptionRequest {
    pub(crate) language: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) stt_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeTranscriberHealthRequest {
    pub(crate) source: Option<String>,
    pub(crate) stt_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeTranscriptionStartResponse {
    pub(crate) session_id: String,
    pub(crate) provider_id: String,
    pub(crate) source: String,
    pub(crate) language: String,
    pub(crate) helper_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrepDictationStartResponse {
    pub(crate) provider_id: String,
    pub(crate) language: String,
    pub(crate) helper_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeTranscriberHealth {
    pub(crate) provider_id: String,
    pub(crate) kind: String,
    pub(crate) ready: bool,
    pub(crate) supports_streaming: bool,
    pub(crate) supports_diarization: bool,
    pub(crate) supports_source_hints: bool,
    pub(crate) platform: DesktopShellPlan,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalSttProfile {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) detail: &'static str,
    pub(crate) engine: &'static str,
    pub(crate) model_file: Option<&'static str>,
    pub(crate) model_size_mb: Option<u32>,
    pub(crate) model_sha256: Option<&'static str>,
    pub(crate) model_download_url: Option<&'static str>,
    pub(crate) recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalSttStatus {
    pub(crate) selected_profile_id: String,
    pub(crate) profiles: Vec<LocalSttProfile>,
    pub(crate) provider_id: String,
    pub(crate) ready: bool,
    pub(crate) engine_ready: bool,
    pub(crate) model_ready: bool,
    pub(crate) model_path: Option<String>,
    pub(crate) model_directory: String,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalSttModelDownloadProgress {
    pub(crate) profile_id: String,
    pub(crate) model_file: String,
    pub(crate) state: String,
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) percent: Option<f64>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HelperHealthLine {
    pub(crate) provider_id: String,
    pub(crate) ready: bool,
    pub(crate) supports_streaming: bool,
    pub(crate) supports_diarization: bool,
    pub(crate) supports_source_hints: bool,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextProviderStatus {
    pub(crate) provider_id: String,
    pub(crate) kind: String,
    pub(crate) connector_installed: bool,
    pub(crate) connector_label: String,
    pub(crate) authenticated: bool,
    pub(crate) can_refresh_token: bool,
    pub(crate) supports_structured_output: bool,
    pub(crate) supports_streaming: bool,
    pub(crate) active: bool,
    pub(crate) status_label: String,
    pub(crate) install_command: Option<String>,
    pub(crate) install_url: Option<String>,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiTranscriptLine {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) speaker: Option<String>,
    pub(crate) source: String,
    pub(crate) language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) editable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSummarySections {
    pub(crate) key_points: Vec<String>,
    pub(crate) decisions_and_open_questions: Vec<String>,
    pub(crate) suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSummaryRequest {
    // Provider routing only. Provider IDs are logged in dedicated audit columns, not echoed into prompt payload JSON.
    #[serde(default, skip_serializing)]
    pub(crate) text_provider_id: Option<String>,
    pub(crate) title: String,
    pub(crate) session_id: String,
    pub(crate) generated_at: String,
    pub(crate) prep_context: String,
    pub(crate) local_summary: AiSummarySections,
    pub(crate) transcript: Vec<AiTranscriptLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSummaryResponse {
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) summary: AiSummarySections,
    pub(crate) raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrepSummaryRequest {
    // Provider routing only. Provider IDs are logged in dedicated audit columns, not echoed into prompt payload JSON.
    #[serde(default, skip_serializing)]
    pub(crate) text_provider_id: Option<String>,
    pub(crate) context: String,
    pub(crate) file_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrepSummaryResponse {
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) key_points: Vec<String>,
    pub(crate) raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptCleanupRequest {
    // Provider routing only. Provider IDs are logged in dedicated audit columns, not echoed into prompt payload JSON.
    #[serde(default, skip_serializing)]
    pub(crate) text_provider_id: Option<String>,
    pub(crate) text: String,
    pub(crate) context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptCleanupResponse {
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) text: String,
    pub(crate) raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptRevisionRequest {
    pub(crate) session_id: String,
    // Provider routing only. Provider IDs are logged in dedicated audit columns, not echoed into prompt payload JSON.
    #[serde(default, skip_serializing)]
    pub(crate) text_provider_id: Option<String>,
    pub(crate) transcript: Vec<AiTranscriptLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevisedTranscriptLine {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) speaker: String,
    pub(crate) source: String,
    pub(crate) language: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptRevisionResponse {
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) transcript: Vec<RevisedTranscriptLine>,
    pub(crate) raw_output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppErrorLogInput {
    pub(crate) session_id: Option<String>,
    pub(crate) stage: String,
    pub(crate) source: String,
    pub(crate) severity: String,
    pub(crate) message: String,
    pub(crate) detail_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppErrorLogRecord {
    pub(crate) id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) stage: String,
    pub(crate) source: String,
    pub(crate) severity: String,
    pub(crate) message: String,
    pub(crate) detail_json: serde_json::Value,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveStatePatchEnvelope {
    pub(crate) meeting_state_patch: LiveMeetingStatePatch,
    pub(crate) decision_state_patch: LiveDecisionStatePatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveMeetingStatePatch {
    pub(crate) add_items: Vec<serde_json::Value>,
    pub(crate) update_items: Vec<serde_json::Value>,
    pub(crate) resolve_item_ids: Vec<String>,
    pub(crate) phase_change: Option<String>,
    pub(crate) evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveDecisionStatePatch {
    pub(crate) current_decision: Option<String>,
    pub(crate) add_options: Vec<serde_json::Value>,
    pub(crate) update_options: Vec<serde_json::Value>,
    pub(crate) add_risks: Vec<serde_json::Value>,
    pub(crate) add_missing_inputs: Vec<serde_json::Value>,
    pub(crate) readiness_patch: Option<LiveReadinessPatch>,
    pub(crate) evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveReadinessPatch {
    pub(crate) score: Option<f64>,
    pub(crate) safe_to_decide: Option<bool>,
    pub(crate) blockers: Option<Vec<String>>,
    pub(crate) evidence_transcript_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DroppedContextFile {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) truncated: bool,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HelperTranscriptLine {
    pub(crate) kind: String,
    pub(crate) text: String,
    pub(crate) is_final: bool,
    pub(crate) confidence: f64,
    pub(crate) language: String,
    pub(crate) source: String,
    pub(crate) started_at_ms: i64,
    pub(crate) ended_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeTranscriptionErrorEvent {
    pub(crate) message: String,
    pub(crate) source: String,
    pub(crate) code: String,
}
