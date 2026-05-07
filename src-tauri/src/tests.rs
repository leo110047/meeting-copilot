use crate::LIVE_SESSIONS;
use crate::commands_audio::{
    classify_native_transcription_error, is_native_transcription_diagnostic,
    transcript_cleanup_context, transcript_text_matches,
};
use crate::commands_core::{
    LIVE_SESSION_STOP_GRACE_MS, cleanup_stopped_live_session, read_granted_dropped_context_file,
    register_drop_read_grants, stop_session_inner,
};
use crate::decision_logic::{
    apply_live_state_patch, derive_decision_state, now_ms, now_ms_string, value_string,
};
use crate::desktop_types::{
    AiTranscriptLine, AppErrorLogRecord, HelperTranscriptLine, LiveDecisionStatePatch,
    LiveMeetingStatePatch, LiveStatePatchEnvelope, NativeLiveSession, TranscriptCleanupRequest,
    TranscriptEvent, TranscriptRevisionRequest,
};
use crate::local_stt::{
    is_local_whisper_profile, local_stt_profiles, normalize_local_stt_profile_id,
};
use crate::macos_speech_bridge::audio_diagnostic_severity;
use crate::native_storage::{
    insert_app_error_log, insert_session, list_app_error_logs, record_session_text_provider,
};
#[cfg(not(target_os = "windows"))]
use crate::oauth_provider::codex_unix_fallback_candidates;
use crate::oauth_provider::{
    CLAUDE_TEXT_PROVIDER_ID, CODEX_TEXT_PROVIDER_ID, LIVE_AI_REMOTE_SOURCE, SubscriptionOAuthParse,
    build_live_state_patch_prompt, build_transcript_cleanup_prompt,
    build_transcript_revision_prompt, cached_text_provider_count_for_tests, claude_command_path,
    claude_status_from_print_probe, codex_command_available, is_live_ai_remote_event,
    live_ai_remote_events, normalize_text_provider_id, parse_claude_auth_status,
    parse_claude_cli_capabilities, parse_claude_print_result, parse_live_coaching_suggestions,
    parse_subscription_oauth_authenticated, parse_transcript_cleanup_text,
    parse_transcript_revision_response, start_subscription_oauth_login, text_provider_summary,
    validate_live_state_patch_value, windows_codex_executable_names,
    windows_executable_extensions_from_pathext,
};
use crate::shell_storage::{open_db, read_dropped_context_file};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
fn transcript_text_match_dedupes_preview_and_final_punctuation() {
    assert!(transcript_text_matches("這是最後一句。", "這是最後一句"));
    assert!(!transcript_text_matches("這是最後一句", "這是另一句"));
}

#[test]
fn native_transcription_error_classifier_handles_no_speech_without_locale_only_matching() {
    assert_eq!(
        classify_native_transcription_error("No speech detected"),
        "no_speech_detected"
    );
    assert_eq!(
        classify_native_transcription_error("未偵測到語音"),
        "no_speech_detected"
    );
    assert_eq!(
        classify_native_transcription_error("Recognition request was canceled"),
        "recognition_request_canceled"
    );
}

#[test]
fn macos_audio_diagnostic_severity_promotes_drop_events() {
    assert_eq!(
        audio_diagnostic_severity("local_whisper_audio_dropped"),
        "warning"
    );
    assert_eq!(
        audio_diagnostic_severity("local_whisper_chunk_dropped"),
        "warning"
    );
    assert_eq!(audio_diagnostic_severity("local_whisper_pipeline"), "info");
    assert_eq!(
        audio_diagnostic_severity("local_whisper_silence_dropped"),
        "info"
    );
}

#[test]
fn local_stt_profiles_require_whisper_as_default() {
    assert_eq!(normalize_local_stt_profile_id(None), "whisper-standard");
    assert_eq!(
        normalize_local_stt_profile_id(Some("unexpected-profile")),
        "whisper-standard"
    );
    assert!(is_local_whisper_profile("whisper-fast"));
    assert!(is_local_whisper_profile("whisper-standard"));
}

#[test]
fn local_stt_profiles_expose_user_facing_quality_modes() {
    let profiles = local_stt_profiles();
    assert!(profiles.iter().any(|profile| profile.id == "whisper-fast"));
    assert!(
        profiles
            .iter()
            .any(|profile| profile.id == "whisper-standard" && profile.recommended)
    );
    assert!(
        profiles
            .iter()
            .any(|profile| profile.id == "whisper-accurate")
    );
}

#[test]
fn codex_command_available_returns_false_for_missing_binary() {
    let missing = PathBuf::from(format!("meeting-copilot-missing-codex-{}", now_ms()));
    assert!(!codex_command_available(&missing));
}

#[test]
fn start_subscription_oauth_login_rejects_missing_override_before_launch() {
    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let key = "MEETING_COPILOT_CODEX";
    let previous = std::env::var_os(key);
    let missing =
        std::env::temp_dir().join(format!("meeting-copilot-missing-codex-login-{}", now_ms()));
    unsafe {
        std::env::set_var(key, &missing);
    }
    let result = start_subscription_oauth_login();
    match previous {
        Some(value) => unsafe {
            std::env::set_var(key, value);
        },
        None => unsafe {
            std::env::remove_var(key);
        },
    }
    let error = result.expect_err("missing Codex override should fail before launching login");
    assert!(error.contains("MEETING_COPILOT_CODEX"));
    assert!(error.contains(&missing.display().to_string()));
}

#[test]
fn stop_session_keeps_live_session_for_late_native_transcripts() {
    let session_id = format!("late_native_{}", now_ms());
    let mut brief = crate::decision_logic::default_brief();
    brief.session_id = session_id.clone();
    let db_path = std::env::temp_dir().join(format!("meeting-copilot-stop-{}.db", now_ms()));
    let conn = open_db(&db_path).expect("open temp db");
    insert_session(&conn, &brief, true, Some(CODEX_TEXT_PROVIDER_ID)).expect("insert session");
    drop(conn);
    LIVE_SESSIONS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("lock live sessions")
        .insert(
            session_id.clone(),
            NativeLiveSession {
                brief,
                text_provider_id: None,
                events: vec![],
                shown_suggestion_ids: std::collections::HashSet::new(),
                stopped_at_ms: None,
            },
        );

    let result = stop_session_inner(session_id.clone(), db_path.clone());
    let retained = LIVE_SESSIONS
        .get()
        .and_then(|sessions| sessions.lock().ok())
        .map(|sessions| {
            sessions
                .get(&session_id)
                .and_then(|session| session.stopped_at_ms)
                .is_some()
        })
        .unwrap_or(false);
    if let Some(sessions) = LIVE_SESSIONS.get()
        && let Ok(mut sessions) = sessions.lock()
    {
        sessions.remove(&session_id);
    }
    let ended_at = open_db(&db_path)
        .expect("reopen temp db")
        .query_row(
            "SELECT ended_at FROM meeting_sessions WHERE id = ?1",
            [&session_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read ended_at");
    let _ = fs::remove_file(db_path);

    result.expect("stop session should update ended_at");
    assert!(ended_at.is_some());
    assert!(
        retained,
        "late native transcript ingest needs the live session after Stop"
    );
}

#[test]
fn stopped_live_session_is_removed_after_late_transcript_grace_window() {
    let session_id = format!("cleanup_stopped_{}", now_ms());
    let stopped_at_ms = i64::try_from(now_ms()).unwrap_or(i64::MAX - LIVE_SESSION_STOP_GRACE_MS);
    LIVE_SESSIONS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("lock live sessions")
        .insert(
            session_id.clone(),
            NativeLiveSession {
                brief: crate::decision_logic::default_brief(),
                text_provider_id: None,
                events: vec![],
                shown_suggestion_ids: std::collections::HashSet::new(),
                stopped_at_ms: Some(stopped_at_ms),
            },
        );

    assert!(!cleanup_stopped_live_session(
        &session_id,
        stopped_at_ms,
        stopped_at_ms + LIVE_SESSION_STOP_GRACE_MS - 1
    ));
    assert!(cleanup_stopped_live_session(
        &session_id,
        stopped_at_ms,
        stopped_at_ms + LIVE_SESSION_STOP_GRACE_MS
    ));
    let still_present = LIVE_SESSIONS
        .get()
        .and_then(|sessions| sessions.lock().ok())
        .map(|sessions| sessions.contains_key(&session_id))
        .unwrap_or(false);
    assert!(!still_present);
}

#[test]
fn native_transcription_diagnostics_do_not_surface_as_ui_errors() {
    assert!(is_native_transcription_diagnostic(
        "whisper_init_state: compute buffer (decode) =   97.28 MB"
    ));
    assert!(is_native_transcription_diagnostic(
        "Windows WASAPI Whisper capture started: eCapture 48000Hz/1ch/32bit/tag65534"
    ));
    assert!(!is_native_transcription_diagnostic(
        "Whisper chunk transcription failed: failed to open wav"
    ));
}

#[test]
fn windows_pathext_order_is_preserved_for_codex_candidates() {
    let extensions =
        windows_executable_extensions_from_pathext(Some(std::ffi::OsStr::new(".BAT;.CMD;.EXE")));
    assert_eq!(extensions, vec![".bat", ".cmd", ".exe"]);
    assert_eq!(
        windows_codex_executable_names(Some(std::ffi::OsStr::new(".BAT;.CMD"))),
        vec!["codex.bat", "codex.cmd"]
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn unix_codex_fallbacks_keep_usr_local_for_linux_compatibility() {
    assert!(codex_unix_fallback_candidates().contains(&"/usr/local/bin/codex"));
}

#[test]
fn oauth_status_parser_does_not_accept_negative_logged_in_text() {
    assert_eq!(
        parse_subscription_oauth_authenticated("Not logged in. Run codex login to use ChatGPT."),
        SubscriptionOAuthParse::Unauthenticated
    );
    assert_eq!(
        parse_subscription_oauth_authenticated("ChatGPT login required"),
        SubscriptionOAuthParse::Unauthenticated
    );
    assert_eq!(
        parse_subscription_oauth_authenticated("Logged in using ChatGPT"),
        SubscriptionOAuthParse::Authenticated
    );
    assert_eq!(
        parse_subscription_oauth_authenticated("Codex CLI status changed its wording"),
        SubscriptionOAuthParse::Unknown
    );
}

#[test]
fn text_provider_id_normalization_defaults_to_codex_and_accepts_claude() {
    assert_eq!(normalize_text_provider_id(None), CODEX_TEXT_PROVIDER_ID);
    assert_eq!(
        normalize_text_provider_id(Some("unknown-provider")),
        CODEX_TEXT_PROVIDER_ID
    );
    assert_eq!(
        normalize_text_provider_id(Some(CLAUDE_TEXT_PROVIDER_ID)),
        CLAUDE_TEXT_PROVIDER_ID
    );
    assert_eq!(
        text_provider_summary(Some(CLAUDE_TEXT_PROVIDER_ID)),
        (CLAUDE_TEXT_PROVIDER_ID, "subscription_oauth")
    );
}

#[test]
fn text_provider_status_cache_is_provider_keyed() {
    crate::oauth_provider::clear_text_provider_status_cache(Some(CODEX_TEXT_PROVIDER_ID));
    crate::oauth_provider::clear_text_provider_status_cache(Some(CLAUDE_TEXT_PROVIDER_ID));
    assert_eq!(cached_text_provider_count_for_tests(), 0);
}

#[test]
fn claude_cli_probe_flags_are_supported_when_binary_exists() {
    let claude = claude_command_path();
    let output = std::process::Command::new(&claude).arg("--help").output();
    let Ok(output) = output else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let help = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for expected in [
        "--tools",
        "--no-session-persistence",
        "--no-chrome",
        "--output-format",
    ] {
        assert!(
            help.contains(expected),
            "Claude Code help is missing expected flag or command: {expected}"
        );
    }
    let capabilities = parse_claude_cli_capabilities(&help);
    assert!(capabilities.supports_print_mode);
    if capabilities.supports_legacy_auth {
        let auth_output = std::process::Command::new(&claude)
            .arg("auth")
            .arg("--help")
            .output()
            .expect("run claude auth --help");
        assert!(auth_output.status.success());
        let auth_help = format!(
            "{}{}",
            String::from_utf8_lossy(&auth_output.stdout),
            String::from_utf8_lossy(&auth_output.stderr)
        );
        assert!(auth_help.contains("login"));
        assert!(auth_help.contains("status"));
    } else {
        assert!(
            capabilities.supports_setup_token,
            "Claude Code help is missing both legacy auth and setup-token commands"
        );
    }

    let flag_probe = std::process::Command::new(&claude)
        .arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--tools")
        .arg("")
        .arg("--no-session-persistence")
        .arg("--no-chrome")
        .arg("--help")
        .output()
        .expect("run claude print flag probe");
    assert!(
        flag_probe.status.success(),
        "Claude Code did not accept Meeting Copilot's print-mode flags: {}{}",
        String::from_utf8_lossy(&flag_probe.stdout),
        String::from_utf8_lossy(&flag_probe.stderr)
    );
}

#[test]
fn claude_print_mode_status_requires_successful_runtime_probe() {
    let capabilities = parse_claude_cli_capabilities(
        r#"
Options:
  -p, --print
  --tools <tools...>
  --no-session-persistence
  --no-chrome
  --output-format <format>
Commands:
  setup-token
"#,
    );
    let failed = claude_status_from_print_probe(
        capabilities,
        Err("not authenticated".to_string()),
        "Claude Code CLI".to_string(),
        Some("npm install -g @anthropic-ai/claude-code".to_string()),
        Some("https://example.invalid".to_string()),
    );
    assert!(!failed.authenticated);
    assert!(!failed.active);
    assert!(failed.can_refresh_token);

    let verified = claude_status_from_print_probe(
        capabilities,
        Ok(()),
        "Claude Code CLI".to_string(),
        None,
        None,
    );
    assert!(verified.authenticated);
    assert!(verified.active);
}

#[test]
fn claude_auth_status_parser_reads_json_status() {
    assert_eq!(
        parse_claude_auth_status(
            r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}"#
        ),
        Some((
            true,
            "已登入 Claude Code（claude.ai/firstParty）".to_string()
        ))
    );
    assert_eq!(
        parse_claude_auth_status(
            r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#
        ),
        Some((false, "尚未登入 Claude Code".to_string()))
    );
}

#[test]
fn claude_print_parser_extracts_result_wrapper() {
    assert_eq!(
        parse_claude_print_result(r#"{"type":"result","is_error":false,"result":"{\"ok\":true}"}"#)
            .expect("parse claude print result"),
        r#"{"ok":true}"#
    );
    assert!(parse_claude_print_result(r#"{"is_error":true,"result":"failed"}"#).is_err());
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
        text_provider_id: None,
        text: "呃我們就是先確認 demo scope 然後不承諾 deadline".to_string(),
        context: "test".to_string(),
    })
    .expect("build cleanup prompt");
    assert!(prompt.contains("Do not summarize"));
    assert!(prompt.contains("Preserve the original meaning"));
    assert!(prompt.contains("Use context only to disambiguate"));
    assert!(prompt.contains("names, numbers, dates, owners, deadlines, scope"));
}

#[test]
fn transcript_cleanup_context_includes_source_and_recent_lines() {
    let session_id = format!("cleanup_context_{}", now_ms());
    LIVE_SESSIONS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("lock live sessions")
        .insert(
            session_id.clone(),
            NativeLiveSession {
                brief: crate::decision_logic::default_brief(),
                text_provider_id: Some("codex-chatgpt-oauth".to_string()),
                events: vec![TranscriptEvent {
                    id: "event_1".to_string(),
                    session_id: session_id.clone(),
                    source: "system".to_string(),
                    speaker: None,
                    speaker_confidence: 0.7,
                    language: "zh-TW".to_string(),
                    started_at_ms: 0,
                    ended_at_ms: Some(1000),
                    text: "剛剛客戶說 demo 範圍先不擴大".to_string(),
                    is_final: true,
                }],
                shown_suggestion_ids: std::collections::HashSet::new(),
                stopped_at_ms: None,
            },
        );
    let context = transcript_cleanup_context(
        &session_id,
        &HelperTranscriptLine {
            kind: "transcript".to_string(),
            text: "所以先照這個走".to_string(),
            is_final: true,
            confidence: 0.8,
            language: "zh-TW".to_string(),
            source: "mic".to_string(),
            started_at_ms: 1000,
            ended_at_ms: 2000,
        },
    );
    assert!(context.contains("\"currentSource\":\"mic\""));
    assert!(context.contains("[系統音訊] 剛剛客戶說 demo 範圍先不擴大"));
}

#[test]
fn transcript_revision_prompt_requires_live_speaker_labels() {
    let request = transcript_revision_request_fixture();
    let prompt = build_transcript_revision_prompt(&request).expect("build revision prompt");
    assert!(prompt.contains("對方 A"));
    assert!(prompt.contains("source=\"mic\", speaker MUST be \"我\""));
    assert!(prompt.contains("Preserve every input line id and order"));
    assert!(prompt.contains("Lines with editable=false are locked context"));
    assert!(prompt.contains("keep that speaker unless nearby context strongly contradicts it"));
}

#[test]
fn transcript_revision_parser_preserves_order_and_labels() {
    let request = transcript_revision_request_fixture();
    let revised = parse_transcript_revision_response(
        r#"{"transcript":[{"id":"l1","speaker":"我","text":"我先確認 demo 範圍。","source":"mic","language":"zh-TW"},{"id":"l2","speaker":"對方 A","text":"先不要擴大。","source":"system","language":"zh-TW"}]}"#,
        &request,
    )
    .expect("parse revised transcript");
    assert_eq!(revised[0].speaker, "我");
    assert_eq!(revised[0].text, "呃我先確認 demo 範圍");
    assert_eq!(revised[1].speaker, "對方 A");
}

#[test]
fn transcript_revision_parser_tolerates_missing_locked_line_speaker() {
    let request = transcript_revision_request_fixture();
    let revised = parse_transcript_revision_response(
        r#"{"transcript":[{"id":"l1","text":"ignored by locked context","source":"mic","language":"zh-TW"},{"id":"l2","speaker":"對方 A","text":"先不要擴大。","source":"system","language":"zh-TW"}]}"#,
        &request,
    )
    .expect("parse revised transcript");
    assert_eq!(revised[0].speaker, "我");
    assert_eq!(revised[0].text, "呃我先確認 demo 範圍");
    assert_eq!(revised[1].speaker, "對方 A");
}

#[test]
fn transcript_revision_parser_sanitizes_unknown_source_speaker() {
    let mut request = transcript_revision_request_fixture();
    request.transcript[1].source = "unknown".to_string();
    request.transcript[1].speaker = Some("未標記來源".to_string());
    let revised = parse_transcript_revision_response(
        r#"{"transcript":[{"id":"l1","speaker":"我","text":"我先確認 demo 範圍。","source":"mic","language":"zh-TW"},{"id":"l2","speaker":"Alice","text":" 先不要擴大 ","source":"unknown","language":"zh-TW"}]}"#,
        &request,
    )
    .expect("parse revised transcript");
    assert_eq!(revised[1].speaker, "未標記來源");
    assert_eq!(revised[1].text, " 先不要擴大 ");
}

fn transcript_revision_request_fixture() -> TranscriptRevisionRequest {
    TranscriptRevisionRequest {
        session_id: "session_1".to_string(),
        text_provider_id: None,
        transcript: vec![
            AiTranscriptLine {
                id: "l1".to_string(),
                text: "呃我先確認 demo 範圍".to_string(),
                speaker: Some("我".to_string()),
                source: "mic".to_string(),
                language: "zh-TW".to_string(),
                editable: Some(false),
                stability: Some("context".to_string()),
                revision_count: Some(1),
            },
            AiTranscriptLine {
                id: "l2".to_string(),
                text: "先不要擴大".to_string(),
                speaker: Some("系統音訊".to_string()),
                source: "system".to_string(),
                language: "zh-TW".to_string(),
                editable: Some(true),
                stability: Some("tentative".to_string()),
                revision_count: Some(0),
            },
        ],
    }
}

#[test]
fn app_error_log_round_trips_diagnostic_detail() {
    let db_path = std::env::temp_dir().join(format!("meeting-copilot-error-log-{}.db", now_ms()));
    let conn = open_db(&db_path).expect("open temp db");
    let record = AppErrorLogRecord {
        id: "error-log-test".to_string(),
        session_id: Some("session-test".to_string()),
        stage: "native_transcription.stderr".to_string(),
        source: "native_speech_helper".to_string(),
        severity: "error".to_string(),
        message: "helper failed".to_string(),
        detail_json: serde_json::json!({"platform": "test"}),
        created_at: now_ms_string(),
    };
    insert_app_error_log(&conn, &record).expect("insert app error log");
    let records = list_app_error_logs(&conn, Some("session-test")).expect("list app error logs");
    let _ = fs::remove_file(db_path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].stage, "native_transcription.stderr");
    assert_eq!(records[0].detail_json["platform"], "test");
}

#[test]
fn session_disclosure_records_provider_switch_history() {
    let db_path = std::env::temp_dir().join(format!(
        "meeting-copilot-provider-disclosure-{}.db",
        now_ms()
    ));
    let conn = open_db(&db_path).expect("open temp db");
    let mut brief = crate::decision_logic::default_brief();
    brief.session_id = format!("provider_disclosure_{}", now_ms());
    insert_session(&conn, &brief, true, Some(CODEX_TEXT_PROVIDER_ID)).expect("insert session");
    record_session_text_provider(&conn, &brief.session_id, CLAUDE_TEXT_PROVIDER_ID)
        .expect("record provider switch");
    let disclosure_text: String = conn
        .query_row(
            "SELECT processing_disclosure_json FROM meeting_sessions WHERE id = ?1",
            rusqlite::params![brief.session_id],
            |row| row.get(0),
        )
        .expect("read disclosure");
    let _ = fs::remove_file(db_path);
    let disclosure: serde_json::Value =
        serde_json::from_str(&disclosure_text).expect("parse disclosure");
    assert_eq!(disclosure["llmProvider"], CLAUDE_TEXT_PROVIDER_ID);
    assert_eq!(disclosure["llmProviders"][0], CODEX_TEXT_PROVIDER_ID);
    assert_eq!(disclosure["llmProviders"][1], CLAUDE_TEXT_PROVIDER_ID);
    assert_eq!(
        disclosure["providerChanges"]
            .as_array()
            .expect("provider changes")
            .len(),
        2
    );
}

#[test]
fn dropped_context_requires_native_drop_grant() {
    let path = std::env::temp_dir().join(format!("meeting-copilot-ungranted-{}.md", now_ms()));
    fs::write(&path, "private meeting notes").expect("write temp context file");
    let file = read_granted_dropped_context_file(path.clone());
    assert!(file.text.is_empty());
    assert!(file.error.unwrap_or_default().contains("未經本次拖拉授權"));

    register_drop_read_grants(std::slice::from_ref(&path));
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
            add_items: vec![serde_json::json!({"kind": "context", "text": "客戶只要 demo scope"})],
            update_items: vec![],
            resolve_item_ids: vec![],
            phase_change: Some("對齊方案".to_string()),
            evidence_transcript_ids: vec!["e1".to_string()],
        },
        decision_state_patch: LiveDecisionStatePatch {
            current_decision: Some("先做 demo scope".to_string()),
            add_options: vec![serde_json::json!({"text": "方案 A"})],
            update_options: vec![],
            add_risks: vec![serde_json::json!({"severity": "high", "text": "正式版 scope 被偷渡"})],
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
fn live_state_prompt_requires_actionable_coaching_cards() {
    let events = vec![TranscriptEvent {
        id: "e1".to_string(),
        session_id: "s1".to_string(),
        source: "system".to_string(),
        speaker: Some("對方 A".to_string()),
        speaker_confidence: 0.7,
        language: "zh-TW".to_string(),
        started_at_ms: 0,
        ended_at_ms: Some(1),
        text: "那下週就直接上正式版吧".to_string(),
        is_final: true,
    }];
    let state = derive_decision_state("s1", &events);
    let event_refs = events.iter().collect::<Vec<_>>();
    let prompt =
        build_live_state_patch_prompt(&crate::decision_logic::default_brief(), &event_refs, &state)
            .expect("build live state prompt");
    assert!(prompt.contains("\"coaching\""));
    assert!(prompt.contains("what the user should say next"));
    assert!(prompt.contains("Return at most one high-value coaching card"));
    assert!(prompt.contains("Use the meeting brief as the user's goals"));
}

#[test]
fn live_ai_remote_events_exclude_user_mic_from_live_prompt() {
    let events = vec![
        TranscriptEvent {
            id: "m1".to_string(),
            session_id: "s1".to_string(),
            source: "mic".to_string(),
            speaker: Some("我".to_string()),
            speaker_confidence: 0.8,
            language: "zh-TW".to_string(),
            started_at_ms: 0,
            ended_at_ms: Some(1000),
            text: "owner 還沒定，我先不要承諾下週交。".to_string(),
            is_final: true,
        },
        TranscriptEvent {
            id: "s1".to_string(),
            session_id: "s1".to_string(),
            source: LIVE_AI_REMOTE_SOURCE.to_string(),
            speaker: Some("對方 A".to_string()),
            speaker_confidence: 0.7,
            language: "zh-TW".to_string(),
            started_at_ms: 1200,
            ended_at_ms: Some(3000),
            text: "那下週就直接上正式版吧".to_string(),
            is_final: true,
        },
    ];
    assert!(!is_live_ai_remote_event(&events[0]));
    assert!(is_live_ai_remote_event(&events[1]));
    let remote_events = live_ai_remote_events(&events);
    assert_eq!(remote_events.len(), 1);
    assert_eq!(remote_events[0].id, "s1");
    let state = derive_decision_state("s1", &remote_events);
    let remote_event_refs = remote_events.iter().collect::<Vec<_>>();
    let prompt = build_live_state_patch_prompt(
        &crate::decision_logic::default_brief(),
        &remote_event_refs,
        &state,
    )
    .expect("build live state prompt");
    assert!(prompt.contains("那下週就直接上正式版吧"));
    assert!(!prompt.contains("owner 還沒定"));
    assert!(!prompt.contains("\"m1\""));

    let full_local_state = derive_decision_state("s1", &events);
    let patch = LiveStatePatchEnvelope {
        meeting_state_patch: LiveMeetingStatePatch {
            add_items: vec![
                serde_json::json!({"kind": "risk", "text": "對方要求下週直接上正式版"}),
            ],
            update_items: vec![],
            resolve_item_ids: vec![],
            phase_change: None,
            evidence_transcript_ids: vec!["s1".to_string()],
        },
        decision_state_patch: LiveDecisionStatePatch {
            current_decision: Some("下週正式版 scope 需要再確認".to_string()),
            add_options: vec![],
            update_options: vec![],
            add_risks: vec![],
            add_missing_inputs: vec![],
            readiness_patch: None,
            evidence_transcript_ids: vec!["s1".to_string()],
        },
    };
    let patched = apply_live_state_patch(full_local_state, &patch, &remote_events);
    assert!(
        patched
            .missing_inputs
            .iter()
            .any(|input| value_string(input, "kind") == "owner")
    );
    assert!(
        patched
            .meeting_items
            .iter()
            .any(|item| value_string(item, "text").contains("下週直接上正式版"))
    );
    assert!(patched.evidence_transcript_ids.contains(&"m1".to_string()));
    assert!(patched.evidence_transcript_ids.contains(&"s1".to_string()));
}

#[test]
fn live_coaching_parser_builds_evidence_backed_suggestion() {
    let events = vec![TranscriptEvent {
        id: "e1".to_string(),
        session_id: "s1".to_string(),
        source: "system".to_string(),
        speaker: Some("對方 A".to_string()),
        speaker_confidence: 0.7,
        language: "zh-TW".to_string(),
        started_at_ms: 0,
        ended_at_ms: Some(1),
        text: "那下週就直接上正式版吧".to_string(),
        is_final: true,
    }];
    let parsed = parse_live_coaching_suggestions(
        r#"{
          "meetingStatePatch":{"addItems":[],"updateItems":[],"resolveItemIds":[],"evidenceTranscriptIds":[]},
          "decisionStatePatch":{"addOptions":[],"updateOptions":[],"addRisks":[],"addMissingInputs":[],"evidenceTranscriptIds":[]},
          "coaching":{"cards":[{
            "kind":"ask_clarifying_question",
            "priority":"high",
            "confidence":0.88,
            "title":"先確認正式版定義",
            "suggestedMove":"你可以問：這裡的正式版是指 demo 可用，還是 production 驗收完成？",
            "watchOut":"對方正在把模糊時程推成正式承諾。",
            "reason":"對方要求下週上正式版，但沒有驗收標準。",
            "evidenceTranscriptIds":["e1","missing"]
          }]}}
        "#,
        "s1",
        &events,
    )
    .expect("parse coaching");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].kind, "ask_clarifying_question");
    assert_eq!(parsed[0].title.as_deref(), Some("先確認正式版定義"));
    assert!(
        parsed[0]
            .suggested_move
            .as_deref()
            .unwrap_or("")
            .contains("你可以問")
    );
    assert!(
        parsed[0]
            .watch_out
            .as_deref()
            .unwrap_or("")
            .contains("模糊時程")
    );
    assert_eq!(parsed[0].evidence_transcript_ids, vec!["e1"]);
}

#[test]
fn live_coaching_parser_selects_best_valid_card_after_invalid_first_card() {
    let events = live_coaching_events_fixture();
    let parsed = parse_live_coaching_suggestions(
        r#"{
          "meetingStatePatch":{"addItems":[],"updateItems":[],"resolveItemIds":[],"evidenceTranscriptIds":[]},
          "decisionStatePatch":{"addOptions":[],"updateOptions":[],"addRisks":[],"addMissingInputs":[],"evidenceTranscriptIds":[]},
          "coaching":{"cards":[
            {
              "kind":"ask_clarifying_question",
              "priority":"high",
              "confidence":0.95,
              "title":"沒有證據",
              "suggestedMove":"這張不應該出現",
              "reason":"缺 evidence",
              "evidenceTranscriptIds":[]
            },
            {
              "kind":"watch_out",
              "priority":"medium",
              "confidence":0.75,
              "title":"注意時程承諾",
              "suggestedMove":"先確認下週上線的驗收定義。",
              "watchOut":"對方正在把時程推成承諾。",
              "reason":"有 evidence。",
              "evidenceTranscriptIds":["e1"]
            }
          ]}}
        "#,
        "s1",
        &events,
    )
    .expect("parse coaching with later valid card");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].kind, "watch_out");
}

#[test]
fn live_coaching_parser_reports_schema_error_when_all_cards_are_invalid() {
    let events = live_coaching_events_fixture();
    let error = parse_live_coaching_suggestions(
        r#"{
          "meetingStatePatch":{"addItems":[],"updateItems":[],"resolveItemIds":[],"evidenceTranscriptIds":[]},
          "decisionStatePatch":{"addOptions":[],"updateOptions":[],"addRisks":[],"addMissingInputs":[],"evidenceTranscriptIds":[]},
          "coaching":{"cards":[{"kind":"invent_new_scope"}]}
        }"#,
        "s1",
        &events,
    )
    .expect_err("invalid coaching card should be visible");
    assert_eq!(error.0, "schema_validation");
    assert!(error.1.contains("kind is not allowed"));
}

#[test]
fn live_coaching_parser_reports_when_all_cards_are_discarded() {
    let events = live_coaching_events_fixture();
    let error = parse_live_coaching_suggestions(
        r#"{
          "meetingStatePatch":{"addItems":[],"updateItems":[],"resolveItemIds":[],"evidenceTranscriptIds":[]},
          "decisionStatePatch":{"addOptions":[],"updateOptions":[],"addRisks":[],"addMissingInputs":[],"evidenceTranscriptIds":[]},
          "coaching":{"cards":[
            {
              "kind":"ask_clarifying_question",
              "priority":"medium",
              "confidence":0.2,
              "title":"低信心",
              "suggestedMove":"不要顯示",
              "reason":"低信心",
              "evidenceTranscriptIds":["e1"]
            },
            {
              "kind":"watch_out",
              "priority":"high",
              "confidence":0.9,
              "title":"缺證據",
              "suggestedMove":"不要顯示",
              "reason":"缺證據",
              "evidenceTranscriptIds":["missing"]
            }
          ]}}
        "#,
        "s1",
        &events,
    )
    .expect_err("discarded coaching cards should be visible");
    assert_eq!(error.0, "coaching_cards_discarded");
    assert!(error.1.contains("confidence_too_low"));
    assert!(
        error
            .1
            .contains("missing_or_invalid_evidence_transcript_ids")
    );
}

fn live_coaching_events_fixture() -> Vec<TranscriptEvent> {
    vec![TranscriptEvent {
        id: "e1".to_string(),
        session_id: "s1".to_string(),
        source: "system".to_string(),
        speaker: Some("對方 A".to_string()),
        speaker_confidence: 0.7,
        language: "zh-TW".to_string(),
        started_at_ms: 0,
        ended_at_ms: Some(1),
        text: "那下週就直接上正式版吧".to_string(),
        is_final: true,
    }]
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
