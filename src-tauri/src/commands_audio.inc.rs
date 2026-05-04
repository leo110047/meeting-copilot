#[tauri::command]
#[cfg(target_os = "macos")]
fn start_prep_dictation(app: tauri::AppHandle) -> Result<PrepDictationStartResponse, String> {
    stop_prep_dictation()?;
    let language = "zh-TW".to_string();
    start_macos_prep_dictation_bridge(app, &language)?;
    Ok(PrepDictationStartResponse {
        provider_id: native_speech_provider_id().to_string(),
        language,
        helper_path: "in-process-macos-speech-bridge".to_string(),
    })
}

#[tauri::command]
#[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    {
        stop_macos_prep_dictation_bridge()?;
    }
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
    if source != "mic" && source != "system" && source != "mixed" {
        return Err("native live transcription source must be mic, system, or mixed".to_string());
    }
    ensure_session_exists(&session_id)?;
    let helper_path = native_speech_helper_path()?;
    let requested_sources = if source == "mixed" {
        vec!["mic".to_string(), "system".to_string()]
    } else {
        vec![source.clone()]
    };
    let mut processes = vec![];
    let mut bridge_sources = vec![];
    for helper_source in requested_sources {
        #[cfg(target_os = "macos")]
        {
            if helper_source == "mic" || helper_source == "system" {
                match start_macos_speech_bridge(
                    app.clone(),
                    &session_id,
                    &helper_source,
                    &language,
                ) {
                    Ok(()) => {
                        bridge_sources.push(helper_source);
                        continue;
                    }
                    Err(error) => {
                        stop_spawned_native_processes(processes);
                        for source in bridge_sources {
                            let _ = stop_macos_speech_bridge(&session_id, Some(&source));
                        }
                        return Err(error);
                    }
                }
            }
        }
        match spawn_native_speech_helper(&helper_path, &language, &helper_source) {
            Ok(process) => processes.push(process),
            Err(error) => {
                stop_spawned_native_processes(processes);
                #[cfg(target_os = "macos")]
                {
                    for source in bridge_sources {
                        let _ = stop_macos_speech_bridge(&session_id, Some(&source));
                    }
                }
                return Err(error);
            }
        }
    }

    {
        let mut transcribers = NATIVE_TRANSCRIBERS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        for process in processes.iter_mut() {
            let key = native_transcriber_key(&session_id, &process.source);
            if let Some(mut stale_child) = transcribers.remove(&key) {
                let _ = stale_child.kill();
                let _ = stale_child.wait();
            }
            transcribers.insert(key, process.child.take().ok_or_else(|| {
                format!("native speech helper child missing for {}", process.source)
            })?);
        }
    }
    set_listening_window_mode(&app, true);
    show_main_window(&app);
    for process in processes {
        install_native_transcriber_io(app.clone(), session_id.clone(), process);
    }

    Ok(NativeTranscriptionStartResponse {
        session_id,
        provider_id: native_speech_provider_id().to_string(),
        source,
        language,
        helper_path: helper_path.display().to_string(),
    })
}

struct NativeTranscriberProcess {
    source: String,
    child: Option<Child>,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
}

fn spawn_native_speech_helper(
    helper_path: &PathBuf,
    language: &str,
    source: &str,
) -> Result<NativeTranscriberProcess, String> {
    let mut child = Command::new(helper_path)
        .arg("--language")
        .arg(language)
        .arg("--source")
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start {source} native speech helper: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{source} native speech helper stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{source} native speech helper stderr unavailable"))?;
    Ok(NativeTranscriberProcess {
        source: source.to_string(),
        child: Some(child),
        stdout: Some(stdout),
        stderr: Some(stderr),
    })
}

fn stop_spawned_native_processes(processes: Vec<NativeTranscriberProcess>) {
    for mut process in processes {
        if let Some(mut child) = process.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn install_native_transcriber_io(
    app: tauri::AppHandle,
    session_id: String,
    mut process: NativeTranscriberProcess,
) {
    monitor_native_transcriber_exit(app.clone(), session_id.clone(), process.source.clone());

    let event_session_id = session_id.clone();
    let stdout_source = process.source.clone();
    let Some(stdout) = process.stdout.take() else {
        let _ = log_app_error_inner(
            Some(&session_id),
            "native_transcription.io_missing_stdout",
            "native",
            "error",
            "native speech helper stdout missing before IO install",
            serde_json::json!({"source": stdout_source}),
        );
        return;
    };
    let app_for_stdout = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let parsed: Result<HelperTranscriptLine, _> = serde_json::from_str(&line);
            match parsed {
                Ok(helper_line) if helper_line.kind == "transcript" && !helper_line.is_final => {
                    let _ = app_for_stdout.emit("native_transcript_preview", helper_line);
                }
                Ok(helper_line) if helper_line.kind == "transcript" && helper_line.is_final => {
                    handle_native_transcript_line(&app_for_stdout, &event_session_id, helper_line);
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = log_app_error_inner(
                        Some(&event_session_id),
                        "native_transcription.parse_line",
                        "native",
                        "error",
                        &error.to_string(),
                        serde_json::json!({"rawLineHash": stable_id(&line), "source": stdout_source}),
                    );
                    let _ = app_for_stdout.emit(
                        "native_transcription_error",
                        format!("failed to parse native transcript line: {error}"),
                    );
                }
            }
        }
    });

    let stderr_source = process.source;
    let Some(stderr) = process.stderr.take() else {
        let _ = log_app_error_inner(
            Some(&session_id),
            "native_transcription.io_missing_stderr",
            "native",
            "error",
            "native speech helper stderr missing before IO install",
            serde_json::json!({"source": stderr_source}),
        );
        return;
    };
    let app_for_stderr = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = log_app_error_inner(
                Some(&session_id),
                "native_transcription.stderr",
                "native_speech_helper",
                "error",
                &line,
                serde_json::json!({"source": stderr_source}),
            );
            let _ = app_for_stderr.emit("native_transcription_error", line);
        }
    });
}

fn handle_native_transcript_line(
    app: &tauri::AppHandle,
    session_id: &str,
    helper_line: HelperTranscriptLine,
) {
    let cleaned_text = match cleanup_transcript_text_oauth_inner(
        &helper_line.text,
        "live_transcript",
        Some(session_id),
    ) {
        Ok(cleaned_text) => cleaned_text,
        Err(error) => {
            let _ = log_app_error_inner(
                Some(session_id),
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
        "native:{}:{}:{}:{}",
        session_id, helper_line.source, helper_line.ended_at_ms, cleaned_text
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
    match ingest_transcript_inner(session_id.to_string(), input) {
        Ok(payload) => {
            let _ = app.emit("native_transcript_ingested", payload);
        }
        Err(error) => {
            let _ = log_app_error_inner(
                Some(session_id),
                "native_transcription.ingest",
                "native",
                "error",
                &error,
                serde_json::json!({"eventId": event_id}),
            );
            let _ = app.emit("native_transcription_error", error);
        }
    }
}

fn native_transcriber_key(session_id: &str, source: &str) -> String {
    format!("{session_id}::{source}")
}

fn monitor_native_transcriber_exit(app: tauri::AppHandle, session_id: String, source: String) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));
            let key = native_transcriber_key(&session_id, &source);
            let exit_result = {
                let Some(transcribers) = NATIVE_TRANSCRIBERS.get() else {
                    return;
                };
                let mut transcribers = match transcribers.lock() {
                    Ok(transcribers) => transcribers,
                    Err(error) => {
                        let message = format!("native speech helper monitor lock failed: {error}");
                        return emit_native_transcriber_exit(&app, &session_id, &message, "error");
                    }
                };
                let Some(child) = transcribers.get_mut(&key) else {
                    return;
                };
                match child.try_wait() {
                    Ok(Some(status)) => {
                        transcribers.remove(&key);
                        Some(Ok(status.to_string()))
                    }
                    Ok(None) => None,
                    Err(error) => {
                        transcribers.remove(&key);
                        Some(Err(error.to_string()))
                    }
                }
            };
            match exit_result {
                Some(Ok(status)) => {
                    let message = format!(
                        "{source} native speech helper exited before Stop Listening: {status}"
                    );
                    emit_native_transcriber_exit(&app, &session_id, &message, "warning");
                    return;
                }
                Some(Err(error)) => {
                    let message = format!("{source} native speech helper monitor failed: {error}");
                    emit_native_transcriber_exit(&app, &session_id, &message, "error");
                    return;
                }
                None => {}
            }
        }
    });
}

fn emit_native_transcriber_exit(
    app: &tauri::AppHandle,
    session_id: &str,
    message: &str,
    severity: &str,
) {
    if !has_active_native_transcribers(session_id) {
        set_listening_window_mode(app, false);
    }
    let _ = log_app_error_inner(
        Some(session_id),
        "native_transcription.process_exit",
        "native_speech_helper",
        severity,
        message,
        serde_json::json!({}),
    );
    let _ = app.emit("native_transcription_error", message);
}

fn has_active_native_transcribers(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Some(bridges) = MACOS_SPEECH_BRIDGES.get() {
            if let Ok(bridges) = bridges.lock() {
                let prefix = format!("{session_id}::");
                if bridges.keys().any(|key| key.starts_with(&prefix)) {
                    return true;
                }
            }
        }
    }
    let Some(transcribers) = NATIVE_TRANSCRIBERS.get() else {
        return false;
    };
    let Ok(transcribers) = transcribers.lock() else {
        return false;
    };
    let prefix = format!("{session_id}::");
    transcribers.keys().any(|key| key.starts_with(&prefix))
}

#[tauri::command]
fn stop_native_transcription(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    stop_macos_speech_bridge(&session_id, None)?;
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get() {
        let mut transcribers = transcribers
            .lock()
            .map_err(|error| error.to_string())?;
        let prefix = format!("{session_id}::");
        let keys = transcribers
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        let stopped = keys
            .into_iter()
            .filter_map(|key| transcribers.remove(&key).map(|child| (key, child)))
            .collect::<Vec<_>>();
        drop(transcribers);
        for (_key, mut child) in stopped {
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
    #[cfg(target_os = "macos")]
    {
        if let Some(bridges) = MACOS_SPEECH_BRIDGES.get() {
            if let Ok(bridges) = bridges.lock() {
                let session_ids = bridges
                    .keys()
                    .filter_map(|key| key.split_once("::").map(|(session_id, _)| session_id.to_string()))
                    .collect::<Vec<_>>();
                drop(bridges);
                for session_id in session_ids {
                    let _ = stop_macos_speech_bridge(&session_id, None);
                }
            }
        }
    }
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get() {
        if let Ok(mut transcribers) = transcribers.lock() {
            for (key, mut child) in transcribers.drain() {
                let session_id = key.split_once("::").map(|(session_id, _)| session_id).unwrap_or(&key);
                if let Err(error) = child.kill() {
                    let _ = log_app_error_inner(
                        Some(session_id),
                        "native_transcription.stop_all.kill",
                        "native",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({}),
                    );
                }
                if let Err(error) = child.wait() {
                    let _ = log_app_error_inner(
                        Some(session_id),
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
    let (event, brief, events_snapshot) = {
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
        (event, session.brief.clone(), session.events.clone())
    };

    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    insert_transcript_event(&conn, &event)?;

    let decision_state = derive_decision_state(&session_id, &events_snapshot);
    let mut suggestions = derive_suggestions(&brief, &events_snapshot, &decision_state);
    let transcript_events = {
        let mut sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session not found".to_string())?;
        suggestions.retain(|suggestion| session.shown_suggestion_ids.insert(suggestion.id.clone()));
        session.events.len()
    };

    for suggestion in &suggestions {
        insert_suggestion(&conn, suggestion)?;
    }

    let snapshot_id = stable_id(&format!(
        "{}:{}:{}:{}",
        session_id,
        event.id,
        now_ms(),
        serde_json::to_string(&decision_state).map_err(|error| error.to_string())?
    ));
    insert_decision_snapshot(&conn, &snapshot_id, &session_id, &decision_state)?;

    Ok(IngestTranscriptResponse {
        event,
        suggestions: suggestions.clone(),
        decision_state,
        persisted: PersistedSummary {
            transcript_events,
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
        params![now_ms_string(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}
