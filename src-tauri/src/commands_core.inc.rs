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
fn native_transcriber_health(request: Option<NativeTranscriberHealthRequest>) -> NativeTranscriberHealth {
    let source = request
        .and_then(|body| body.source)
        .unwrap_or_else(|| "mic".to_string());
    native_transcriber_health_for_source(&source).unwrap_or_else(|error| NativeTranscriberHealth {
            provider_id: native_speech_provider_id().to_string(),
            kind: "stt".to_string(),
            ready: false,
            supports_streaming: true,
            supports_diarization: false,
            supports_source_hints: true,
            platform: desktop_shell_plan(),
            last_error: Some(error),
        })
}

#[tauri::command]
fn request_native_audio_permissions(
    request: Option<NativeTranscriberHealthRequest>,
) -> NativeTranscriberHealth {
    let source = request
        .and_then(|body| body.source)
        .unwrap_or_else(|| "mic".to_string());
    request_native_audio_permissions_for_source(&source).unwrap_or_else(|error| NativeTranscriberHealth {
        provider_id: native_speech_provider_id().to_string(),
        kind: "stt".to_string(),
        ready: false,
        supports_streaming: true,
        supports_diarization: false,
        supports_source_hints: true,
        platform: desktop_shell_plan(),
        last_error: Some(error),
    })
}

fn native_transcriber_health_for_source(source: &str) -> Result<NativeTranscriberHealth, String> {
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
        checks.push(run_native_transcriber_health_check(&helper_path, helper_source)?);
    }
    let mut combined = checks
        .first()
        .cloned()
        .ok_or_else(|| "native transcriber health source is empty".to_string())?;
    combined.ready = checks.iter().all(|check| check.ready);
    let errors: Vec<String> = checks
        .into_iter()
        .filter_map(|check| {
            if check.ready {
                None
            } else {
                check.last_error
            }
        })
        .collect();
    combined.last_error = if combined.ready || errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };
    Ok(combined)
}

fn request_native_audio_permissions_for_source(
    source: &str,
) -> Result<NativeTranscriberHealth, String> {
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
    native_transcriber_health_for_source(source)
}

#[tauri::command]
fn request_screen_recording_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = request_macos_speech_bridge_permissions("system", "zh-TW")?;
        let status = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|error| format!("failed to open Screen Recording settings: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("failed to open Screen Recording settings: {status}"))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Screen Recording settings are only available on macOS".to_string())
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
