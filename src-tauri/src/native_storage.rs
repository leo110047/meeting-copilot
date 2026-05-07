use crate::decision_logic::{now_ms, now_ms_string, stable_id};
use crate::desktop_types::desktop_shell_plan;
use crate::desktop_types::{
    AppErrorLogRecord, HelperHealthLine, MeetingBrief, MeetingSeriesOption, NativeDecisionState,
    NativeSuggestion, NativeTranscriberHealth, SaveMeetingHistoryRequest,
    SaveMeetingHistoryResponse, TranscriptEvent,
};
use crate::shell_storage::{app_db_path, open_db};
use crate::{LIVE_SESSIONS, NATIVE_SPEECH_HELPER};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn ensure_session_exists(session_id: &str) -> Result<(), String> {
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

pub(crate) fn native_speech_provider_id() -> &'static str {
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

pub(crate) fn native_speech_helper_path() -> Result<PathBuf, String> {
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

#[cfg(target_os = "macos")]
pub(crate) fn macos_speech_bridge_path() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let dev_bridge = cwd
        .join("src-tauri")
        .join("binaries")
        .join("libmeeting_copilot_speech_bridge.dylib");
    if dev_bridge.exists() {
        return Ok(dev_bridge);
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(parent) = exe.parent() {
        let bundled_bridge = parent
            .join("../Frameworks")
            .join("libmeeting_copilot_speech_bridge.dylib");
        if bundled_bridge.exists() {
            return Ok(bundled_bridge);
        }
        let resource_bridge = parent
            .join("../Resources")
            .join("libmeeting_copilot_speech_bridge.dylib");
        if resource_bridge.exists() {
            return Ok(resource_bridge);
        }
    }
    Err("macOS speech bridge not found".to_string())
}

pub(crate) fn run_native_transcriber_health_check(
    helper_path: &PathBuf,
    source: &str,
) -> Result<NativeTranscriberHealth, String> {
    let mut child = Command::new(helper_path)
        .arg("--health")
        .arg("--language")
        .arg("zh-TW")
        .arg("--source")
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start native speech health check: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "native speech health stdout unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "native speech health stderr unavailable".to_string())?;

    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("native speech health check failed: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("native speech health check timed out".to_string());
        }
        thread::sleep(Duration::from_millis(50));
    };

    let mut stdout_text = String::new();
    let mut stderr_text = String::new();
    let _ = stdout.read_to_string(&mut stdout_text);
    let _ = stderr.read_to_string(&mut stderr_text);
    let health_line = stdout_text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| {
            format!(
                "native speech health check did not return JSON: {}",
                stderr_text.trim()
            )
        })?;
    let helper_health: HelperHealthLine = serde_json::from_str(health_line).map_err(|error| {
        format!(
            "native speech health check returned invalid JSON: {error}; stderr={}",
            stderr_text.trim()
        )
    })?;
    let stderr_error = stderr_text.trim();
    let last_error = helper_health.last_error.or_else(|| {
        if stderr_error.is_empty() {
            None
        } else {
            Some(stderr_error.to_string())
        }
    });

    Ok(NativeTranscriberHealth {
        provider_id: helper_health.provider_id,
        kind: "stt".to_string(),
        ready: status.success() && helper_health.ready,
        supports_streaming: helper_health.supports_streaming,
        supports_diarization: helper_health.supports_diarization,
        supports_source_hints: helper_health.supports_source_hints,
        platform: desktop_shell_plan(),
        last_error,
    })
}

pub(crate) fn rust_host_triple() -> &'static str {
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

pub(crate) fn insert_session(
    conn: &Connection,
    brief: &MeetingBrief,
    text_provider_enabled: bool,
    text_provider_id: Option<&str>,
) -> Result<(), String> {
    let current_provider = text_provider_id.unwrap_or("codex-chatgpt-oauth");
    let disclosure = serde_json::json!({
        "sttProvider": native_speech_provider_id(),
        "llmProvider": if text_provider_enabled { current_provider } else { "disabled" },
        "llmProviders": if text_provider_enabled {
            serde_json::json!([current_provider])
        } else {
            serde_json::json!([])
        },
        "providerChanges": if text_provider_enabled {
            serde_json::json!([{ "provider": current_provider, "changedAt": brief.started_at }])
        } else {
            serde_json::json!([])
        },
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

pub(crate) fn record_session_text_provider(
    conn: &Connection,
    session_id: &str,
    provider_id: &str,
) -> Result<(), String> {
    let (disclosure_text, started_at): (String, String) = conn
        .query_row(
            "SELECT processing_disclosure_json, started_at FROM meeting_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let mut disclosure: serde_json::Value =
        serde_json::from_str(&disclosure_text).unwrap_or_else(|_| serde_json::json!({}));
    let previous_provider = disclosure
        .get("llmProvider")
        .and_then(|value| value.as_str())
        .filter(|value| *value != "disabled")
        .map(str::to_string);
    disclosure["llmProvider"] = serde_json::Value::String(provider_id.to_string());
    disclosure["textProviderKind"] = serde_json::Value::String("subscription_oauth".to_string());
    disclosure["sentTranscriptToCloud"] = serde_json::Value::Bool(true);
    disclosure["sentMemoryToCloud"] = serde_json::Value::Bool(true);
    let mut providers = disclosure
        .get("llmProviders")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_else(|| {
            previous_provider
                .as_ref()
                .map(|value| vec![serde_json::Value::String(value.to_string())])
                .unwrap_or_default()
        });
    if !providers
        .iter()
        .any(|value| value.as_str() == Some(provider_id))
    {
        providers.push(serde_json::Value::String(provider_id.to_string()));
    }
    disclosure["llmProviders"] = serde_json::Value::Array(providers);
    let change = serde_json::json!({
        "provider": provider_id,
        "changedAt": now_ms_string(),
    });
    let mut changes = disclosure
        .get("providerChanges")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if changes.is_empty()
        && let Some(previous) = previous_provider.as_deref()
        && previous != provider_id
    {
        changes.push(serde_json::json!({
            "provider": previous,
            "changedAt": started_at
        }));
    }
    changes.push(change);
    disclosure["providerChanges"] = serde_json::Value::Array(changes);
    conn.execute(
        "UPDATE meeting_sessions SET processing_disclosure_json = ?1 WHERE id = ?2",
        params![disclosure.to_string(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn list_meeting_series(conn: &Connection) -> Result<Vec<MeetingSeriesOption>, String> {
    let mut statement = conn
        .prepare(
            "SELECT
                s.id,
                s.title,
                s.summary,
                s.latest_context_json,
                s.updated_at,
                COUNT(h.id) AS history_count
            FROM meeting_series s
            LEFT JOIN meeting_history_entries h ON h.series_id = s.id
            WHERE s.archived_at IS NULL
            GROUP BY s.id
            ORDER BY s.updated_at DESC, s.title ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let latest_context_text: String = row.get(3)?;
            let latest_context = serde_json::from_str(&latest_context_text)
                .unwrap_or_else(|_| serde_json::json!({}));
            Ok(MeetingSeriesOption {
                id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                latest_context,
                last_saved_at: row.get(4)?,
                history_count: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut series = vec![];
    for row in rows {
        series.push(row.map_err(|error| error.to_string())?);
    }
    Ok(series)
}

fn read_meeting_series_by_id(
    conn: &Connection,
    series_id: &str,
) -> Result<MeetingSeriesOption, String> {
    conn.query_row(
        "SELECT
            s.id,
            s.title,
            s.summary,
            s.latest_context_json,
            s.updated_at,
            (SELECT COUNT(*) FROM meeting_history_entries h WHERE h.series_id = s.id) AS history_count
        FROM meeting_series s
        WHERE s.id = ?1 AND s.archived_at IS NULL",
        params![series_id],
        |row| {
            let latest_context_text: String = row.get(3)?;
            let latest_context = serde_json::from_str(&latest_context_text)
                .unwrap_or_else(|_| serde_json::json!({}));
            Ok(MeetingSeriesOption {
                id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                latest_context,
                last_saved_at: row.get(4)?,
                history_count: row.get(5)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn save_meeting_history(
    conn: &Connection,
    request: SaveMeetingHistoryRequest,
) -> Result<SaveMeetingHistoryResponse, String> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| error.to_string())?;
    let result = save_meeting_history_in_transaction(conn, request);
    match result {
        Ok(response) => {
            if let Err(error) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error.to_string())
            } else {
                Ok(response)
            }
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn save_meeting_history_in_transaction(
    conn: &Connection,
    request: SaveMeetingHistoryRequest,
) -> Result<SaveMeetingHistoryResponse, String> {
    let saved_at = now_iso_string();
    let title = normalize_history_title(request.series_title.as_deref().or_else(|| {
        request
            .artifact
            .get("title")
            .and_then(|value| value.as_str())
    }));
    let (series_id, series_title, series_exists) =
        resolve_meeting_series_identity(conn, request.series_id.as_deref(), &title)?;
    let entry_id = stable_id(&format!(
        "meeting-history:{}:{}:{}",
        series_id,
        request.session_id.as_deref().unwrap_or("local"),
        saved_at
    ));
    let latest_context = if request.allow_ai_context {
        build_latest_meeting_context(&entry_id, &series_title, &saved_at, &request)
    } else {
        serde_json::json!({})
    };
    let summary = latest_context
        .get("summaryText")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if series_exists {
        conn.execute(
            "UPDATE meeting_series
            SET
                summary = CASE WHEN ?1 = 1 THEN ?2 ELSE summary END,
                latest_context_json = CASE WHEN ?1 = 1 THEN ?3 ELSE latest_context_json END,
                updated_at = ?4,
                archived_at = NULL
            WHERE id = ?5",
            params![
                if request.allow_ai_context { 1 } else { 0 },
                &summary,
                latest_context.to_string(),
                &saved_at,
                &series_id
            ],
        )
        .map_err(|error| error.to_string())?;
    } else {
        conn.execute(
            "INSERT INTO meeting_series (id, title, summary, latest_context_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &series_id,
                &series_title,
                &summary,
                latest_context.to_string(),
                &saved_at,
                &saved_at
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    conn.execute(
        "INSERT OR REPLACE INTO meeting_history_entries
        (id, series_id, session_id, title, artifact_json, allow_ai_context, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &entry_id,
            &series_id,
            &request.session_id,
            &series_title,
            request.artifact.to_string(),
            if request.allow_ai_context { 1 } else { 0 },
            &saved_at
        ],
    )
    .map_err(|error| error.to_string())?;
    let series = read_meeting_series_by_id(conn, &series_id)?;
    Ok(SaveMeetingHistoryResponse {
        entry_id,
        series,
        saved_at,
    })
}

fn resolve_meeting_series_identity(
    conn: &Connection,
    requested_series_id: Option<&str>,
    requested_title: &str,
) -> Result<(String, String, bool), String> {
    if let Some(series_id) = requested_series_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some((id, title)) = conn
            .query_row(
                "SELECT id, title FROM meeting_series WHERE id = ?1",
                params![series_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            return Ok((id, title, true));
        }
    }
    if let Some((id, title)) = conn
        .query_row(
            "SELECT id, title FROM meeting_series WHERE title = ?1",
            params![requested_title],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
    {
        return Ok((id, title, true));
    }
    let series_id = requested_series_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| stable_id(&format!("meeting-series:{requested_title}")));
    Ok((series_id, requested_title.to_string(), false))
}

fn normalize_history_title(value: Option<&str>) -> String {
    let title = value.unwrap_or("").trim();
    if title.is_empty() {
        "未命名會議".to_string()
    } else {
        title.chars().take(120).collect()
    }
}

fn now_iso_string() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = duration.as_secs() as i64;
    let millis = duration.subsec_millis();
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn build_latest_meeting_context(
    entry_id: &str,
    title: &str,
    saved_at: &str,
    request: &SaveMeetingHistoryRequest,
) -> serde_json::Value {
    let summary = request
        .artifact
        .get("summary")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let key_points = limited_string_array(summary.get("keyPoints"), 6);
    let unresolved = limited_string_array(summary.get("decisionsAndOpenQuestions"), 6);
    let suggested_actions = limited_string_array(summary.get("suggestedActions"), 6);
    let transcript_preview = request
        .artifact
        .get("transcript")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .filter_map(|line| {
                    let text =
                        normalize_context_text(line.get("text").and_then(|value| value.as_str()));
                    if text.is_empty() {
                        return None;
                    }
                    let speaker = normalize_context_text(
                        line.get("speaker").and_then(|value| value.as_str()),
                    );
                    Some(serde_json::json!({
                        "speaker": if speaker.is_empty() { "未標記來源".to_string() } else { speaker },
                        "text": text
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::json!({
        "entryId": entry_id,
        "sessionId": request.session_id,
        "title": title,
        "updatedAt": saved_at,
        "summaryText": key_points.iter().take(2).cloned().collect::<Vec<_>>().join("；"),
        "keyPoints": key_points,
        "unresolved": unresolved,
        "suggestedActions": suggested_actions,
        "transcriptPreview": transcript_preview
    })
}

fn limited_string_array(value: Option<&serde_json::Value>, limit: usize) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| normalize_context_text(Some(item)))
                .filter(|item| !item.is_empty())
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_context_text(value: Option<&str>) -> String {
    value
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn insert_transcript_event(
    conn: &Connection,
    event: &TranscriptEvent,
) -> Result<(), String> {
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

pub(crate) fn insert_suggestion(
    conn: &Connection,
    suggestion: &NativeSuggestion,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO suggestions
        (id, session_id, shown_at, text, reason, trigger_rule_id, confidence, priority, evidence_transcript_ids_json, suggestion_json, feedback)
        VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, NULL)",
        params![
            suggestion.id,
            suggestion.session_id,
            suggestion.shown_at,
            suggestion.text,
            suggestion.reason,
            suggestion.confidence,
            suggestion.priority,
            serde_json::to_string(&suggestion.evidence_transcript_ids)
                .map_err(|error| error.to_string())?,
            serde_json::to_string(suggestion).map_err(|error| error.to_string())?
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn insert_decision_snapshot(
    conn: &Connection,
    snapshot_id: &str,
    session_id: &str,
    decision_state: &NativeDecisionState,
) -> Result<(), String> {
    insert_decision_snapshot_with_source(conn, snapshot_id, session_id, decision_state, None)
}

pub(crate) fn insert_decision_snapshot_with_source(
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

pub(crate) struct LlmUsageLogInput<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) call_type: &'a str,
    pub(crate) provider: &'a str,
    pub(crate) model: &'a str,
    pub(crate) prompt_version: &'a str,
    pub(crate) prompt: &'a str,
    pub(crate) output: &'a str,
    pub(crate) latency_ms: i64,
}

pub(crate) fn insert_llm_usage_log(
    conn: &Connection,
    input: LlmUsageLogInput<'_>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO llm_usage_logs
        (id, session_id, call_type, provider, model, prompt_version, prompt_hash, input_tokens, cached_input_tokens, output_tokens, audio_input_tokens, estimated_cost_usd, latency_ms, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, NULL, NULL, ?10, ?11)",
        params![
            stable_id(&format!(
                "usage:{}:{}:{}:{}",
                input.session_id,
                input.call_type,
                input.provider,
                now_ms()
            )),
            input.session_id,
            input.call_type,
            input.provider,
            input.model,
            input.prompt_version,
            stable_id(input.prompt),
            estimate_tokens(input.prompt),
            estimate_tokens(input.output),
            input.latency_ms,
            now_ms_string()
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn insert_app_error_log(
    conn: &Connection,
    record: &AppErrorLogRecord,
) -> Result<(), String> {
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

pub(crate) fn list_app_error_logs(
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

pub(crate) fn log_app_error_inner(
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
        created_at: now_ms_string(),
    };
    insert_app_error_log(&conn, &record)?;
    Ok(record.id)
}

pub(crate) fn log_provider_usage_for(
    provider: &str,
    model: &str,
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
        LlmUsageLogInput {
            session_id: audit_id,
            call_type,
            provider,
            model,
            prompt_version,
            prompt,
            output,
            latency_ms,
        },
    )
}

pub(crate) fn log_provider_failure_for(
    provider: &str,
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
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            stable_id(&format!("provider-failure:{audit_id}:{call_type}:{failure_kind}:{raw_output_ref}:{}", now_ms())),
            audit_id,
            call_type,
            prompt_version,
            provider,
            failure_kind,
            stable_id(raw_output_ref),
            now_ms_string()
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
            "provider": provider,
            "rawOutputRef": stable_id(raw_output_ref)
        }),
    );
    Ok(())
}

pub(crate) fn log_extraction_failure_for(
    session_id: &str,
    provider: &str,
    failure_kind: &str,
    raw_output_ref: &str,
) -> Result<(), String> {
    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    conn.execute(
        "INSERT INTO extraction_failure_logs
        (id, session_id, call_type, prompt_version, provider, failure_kind, raw_output_ref, created_at)
        VALUES (?1, ?2, 'extract_state_patch', 'extract_state_patch.oauth.v1', ?3, ?4, ?5, ?6)",
        params![
            stable_id(&format!("failure:{session_id}:{failure_kind}:{raw_output_ref}:{}", now_ms())),
            session_id,
            provider,
            failure_kind,
            raw_output_ref,
            now_ms_string()
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
            "provider": provider,
            "rawOutputRef": raw_output_ref
        }),
    );
    Ok(())
}

pub(crate) fn estimate_tokens(text: &str) -> i64 {
    ((text.chars().count() as f64) / 4.0).ceil() as i64
}
