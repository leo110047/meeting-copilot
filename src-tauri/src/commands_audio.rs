use crate::decision_logic::{derive_decision_state, detect_language, now_ms, stable_id};
use crate::desktop_types::{
    HelperTranscriptLine, IngestTranscriptResponse, NativeTranscriptionErrorEvent,
    NativeTranscriptionRequest, NativeTranscriptionStartResponse, PersistedSummary,
    PrepDictationStartResponse, TranscriptEvent, TranscriptInput,
};
use crate::local_stt::{
    is_local_whisper_profile, local_whisper_runtime, normalize_local_stt_profile_id,
};
#[cfg(not(target_os = "macos"))]
use crate::native_storage::native_speech_helper_path;
use crate::native_storage::{
    ensure_session_exists, insert_decision_snapshot, insert_transcript_event, log_app_error_inner,
    native_speech_provider_id,
};
use crate::shell_storage::{app_db_path, open_db, set_listening_window_mode, show_main_window};
use crate::{LIVE_SESSIONS, ManagedNativeTranscriber, NATIVE_TRANSCRIBERS, PREP_DICTATION};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::Emitter;

#[cfg(target_os = "macos")]
use crate::macos_speech_bridge::{
    MACOS_SPEECH_BRIDGES, start_macos_prep_dictation_bridge, start_macos_speech_bridge,
    start_macos_whisper_bridge, stop_macos_prep_dictation_bridge, stop_macos_speech_bridge,
};

#[tauri::command]
#[cfg(target_os = "macos")]
pub(crate) fn start_prep_dictation(
    app: tauri::AppHandle,
    provider_id: Option<String>,
) -> Result<PrepDictationStartResponse, String> {
    stop_prep_dictation()?;
    let language = "zh-TW".to_string();
    start_macos_prep_dictation_bridge(app, &language, provider_id)?;
    Ok(PrepDictationStartResponse {
        provider_id: native_speech_provider_id().to_string(),
        language,
        helper_path: "in-process-macos-speech-bridge".to_string(),
    })
}

#[tauri::command]
#[cfg(not(target_os = "macos"))]
pub(crate) fn start_prep_dictation(
    app: tauri::AppHandle,
    provider_id: Option<String>,
) -> Result<PrepDictationStartResponse, String> {
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
                    let cleaned_text =
                        match crate::oauth_provider::cleanup_transcript_text_with_provider_inner(
                            provider_id.as_deref(),
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
pub(crate) fn stop_prep_dictation() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        stop_macos_prep_dictation_bridge()?;
    }
    if let Some(dictation) = PREP_DICTATION.get()
        && let Some(mut child) = dictation.lock().map_err(|error| error.to_string())?.take()
    {
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
    Ok(())
}

#[tauri::command]
pub(crate) fn start_native_transcription(
    app: tauri::AppHandle,
    session_id: String,
    request: Option<NativeTranscriptionRequest>,
) -> Result<NativeTranscriptionStartResponse, String> {
    let request = request.unwrap_or(NativeTranscriptionRequest {
        language: None,
        source: None,
        stt_profile_id: None,
    });
    let language = request.language.unwrap_or_else(|| "zh-TW".to_string());
    let source = request.source.unwrap_or_else(|| "mic".to_string());
    let stt_profile_id = normalize_local_stt_profile_id(request.stt_profile_id.as_deref());
    if source != "mic" && source != "system" && source != "mixed" {
        return Err("native live transcription source must be mic, system, or mixed".to_string());
    }
    ensure_session_exists(&session_id)?;
    let whisper_runtime = if is_local_whisper_profile(stt_profile_id) {
        Some(local_whisper_runtime(stt_profile_id)?)
    } else {
        None
    };
    #[cfg(target_os = "macos")]
    let helper_path: Option<PathBuf> = None;
    #[cfg(not(target_os = "macos"))]
    let helper_path: Option<PathBuf> = Some(native_speech_helper_path()?);
    // Platform speech starts separate helpers for mixed capture. Whisper helpers
    // receive "mixed" and expand mic/system lanes inside one process so one
    // persistent runner/model instance can serve both sources.
    let requested_sources = if source == "mixed" && whisper_runtime.is_none() {
        vec!["mic".to_string(), "system".to_string()]
    } else {
        vec![source.clone()]
    };
    let mut processes = vec![];
    #[cfg(target_os = "macos")]
    let mut bridge_sources: Vec<String> = vec![];
    for helper_source in requested_sources {
        #[cfg(target_os = "macos")]
        {
            if helper_source == "mic"
                || helper_source == "system"
                || (helper_source == "mixed" && whisper_runtime.is_some())
            {
                let bridge_result = if let Some(runtime) = whisper_runtime.as_ref() {
                    start_macos_whisper_bridge(
                        app.clone(),
                        &session_id,
                        &helper_source,
                        &language,
                        runtime,
                    )
                } else {
                    start_macos_speech_bridge(app.clone(), &session_id, &helper_source, &language)
                };
                match bridge_result {
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
        match spawn_native_speech_helper(
            helper_path
                .as_ref()
                .ok_or_else(|| "native speech helper path is unavailable".to_string())?,
            &language,
            &helper_source,
            whisper_runtime.as_ref(),
        ) {
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
            if let Some(stale_child) = transcribers.remove(&key) {
                stop_managed_native_transcriber_background(
                    session_id.clone(),
                    stale_child,
                    true,
                    "native_transcription.stop_stale",
                );
            }
            transcribers.insert(
                key,
                ManagedNativeTranscriber {
                    child: process.child.take().ok_or_else(|| {
                        format!("native speech helper child missing for {}", process.source)
                    })?,
                    stdin: process.stdin.take(),
                    stop_file: process.stop_file.take(),
                    may_defer_post_meeting: process.source == "mixed" && whisper_runtime.is_some(),
                },
            );
        }
    }
    set_listening_window_mode(&app, true);
    show_main_window(&app);
    for process in processes {
        install_native_transcriber_io(app.clone(), session_id.clone(), process);
    }

    Ok(NativeTranscriptionStartResponse {
        session_id,
        provider_id: whisper_runtime
            .as_ref()
            .map(|runtime| runtime.provider_id.to_string())
            .unwrap_or_else(|| native_speech_provider_id().to_string()),
        source,
        language,
        helper_path: helper_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| {
                if whisper_runtime.is_some() {
                    "in-process-macos-whisper-bridge".to_string()
                } else {
                    "in-process-macos-speech-bridge".to_string()
                }
            }),
    })
}

pub(crate) struct NativeTranscriberProcess {
    source: String,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stop_file: Option<PathBuf>,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
}

pub(crate) fn spawn_native_speech_helper(
    helper_path: &PathBuf,
    language: &str,
    source: &str,
    whisper_runtime: Option<&crate::local_stt::LocalWhisperRuntime>,
) -> Result<NativeTranscriberProcess, String> {
    let stop_file = whisper_runtime.map(|_| whisper_helper_stop_file_path(source));
    let mut command = Command::new(helper_path);
    command
        .arg("--language")
        .arg(language)
        .arg("--source")
        .arg(source);
    if let Some(runtime) = whisper_runtime {
        if let Some(stop_file) = stop_file.as_ref() {
            let _ = std::fs::remove_file(stop_file);
        }
        command
            .stdin(Stdio::piped())
            .arg("--engine")
            .arg("whisper")
            .arg("--whisper-runner")
            .arg(&runtime.runner_path)
            .arg("--whisper-model")
            .arg(&runtime.model_path);
        if let Some(stop_file) = stop_file.as_ref() {
            command.arg("--stop-file").arg(stop_file);
        }
    }
    let mut child = command
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
        stdin: child.stdin.take(),
        stop_file,
        child: Some(child),
        stdout: Some(stdout),
        stderr: Some(stderr),
    })
}

pub(crate) fn stop_spawned_native_processes(processes: Vec<NativeTranscriberProcess>) {
    for mut process in processes {
        if let Some(child) = process.child.take() {
            let managed = ManagedNativeTranscriber {
                child,
                stdin: process.stdin.take(),
                stop_file: process.stop_file.take(),
                may_defer_post_meeting: process.source == "mixed" && process.stop_file.is_some(),
            };
            stop_managed_native_transcriber_background(
                "startup".to_string(),
                managed,
                true,
                "native_transcription.stop_startup",
            );
        }
    }
}

fn signal_managed_native_transcriber(
    transcriber: &mut ManagedNativeTranscriber,
) -> Result<(), String> {
    if let Some(stop_file) = transcriber.stop_file.as_ref() {
        std::fs::write(stop_file, b"stop")
            .map_err(|error| format!("failed to signal native speech helper stop file: {error}"))?;
    }
    // The Whisper runner speaks JSON-lines. Closing stdin is the stop signal;
    // writing a sentinel would become an invalid job and surface as a false error.
    drop(transcriber.stdin.take());
    Ok(())
}

fn stop_managed_native_transcriber_background(
    session_id: String,
    mut transcriber: ManagedNativeTranscriber,
    force_on_timeout: bool,
    log_target: &'static str,
) {
    if let Err(error) = signal_managed_native_transcriber(&mut transcriber) {
        let _ = log_app_error_inner(
            Some(&session_id),
            log_target,
            "native",
            "warning",
            &error,
            serde_json::json!({"stage": "signal"}),
        );
    }
    thread::spawn(move || {
        if let Err(error) =
            finish_managed_native_transcriber(&session_id, &mut transcriber, force_on_timeout)
        {
            let _ = log_app_error_inner(
                Some(&session_id),
                log_target,
                "native",
                "warning",
                &error,
                serde_json::json!({"stage": "wait"}),
            );
        }
    });
}

fn finish_managed_native_transcriber(
    session_id: &str,
    transcriber: &mut ManagedNativeTranscriber,
    force_on_timeout: bool,
) -> Result<(), String> {
    let stop_timeout = if transcriber.may_defer_post_meeting {
        // Mixed Whisper helpers defer mic transcription after Stop. The helper
        // enforces its own count-based 30-minute cap; this outer reap timeout
        // only gives it a small margin to close pipes and exit.
        Duration::from_secs(31 * 60)
    } else {
        Duration::from_secs(35)
    };
    let deadline = std::time::Instant::now() + stop_timeout;
    loop {
        match transcriber.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                if force_on_timeout {
                    transcriber
                        .child
                        .kill()
                        .map_err(|error| format!("failed to kill native speech helper: {error}"))?;
                }
                break;
            }
            Err(error) => return Err(format!("failed to wait native speech helper: {error}")),
        }
    }
    transcriber
        .child
        .wait()
        .map_err(|error| format!("failed to reap native speech helper: {error}"))?;
    if let Some(stop_file) = transcriber.stop_file.as_ref() {
        let _ = std::fs::remove_file(stop_file);
    }
    let _ = log_app_error_inner(
        Some(session_id),
        "native_transcription.stop.graceful",
        "native",
        "info",
        "native speech helper stopped",
        serde_json::json!({"graceful": true}),
    );
    Ok(())
}

fn whisper_helper_stop_file_path(source: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "meeting-copilot-whisper-stop-{}-{source}-{}",
        std::process::id(),
        now_ms()
    ))
}

pub(crate) fn install_native_transcriber_io(
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
                    emit_native_transcription_error(
                        &app_for_stdout,
                        format!("failed to parse native transcript line: {error}"),
                        &stdout_source,
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
            let is_diagnostic = is_native_transcription_diagnostic(&line);
            let _ = log_app_error_inner(
                Some(&session_id),
                "native_transcription.stderr",
                "native_speech_helper",
                if is_diagnostic { "info" } else { "error" },
                &line,
                serde_json::json!({"source": stderr_source}),
            );
            if !is_diagnostic {
                emit_native_transcription_error(&app_for_stderr, line, &stderr_source);
            }
        }
    });
}

pub(crate) fn is_native_transcription_diagnostic(message: &str) -> bool {
    let trimmed = message.trim();
    trimmed.starts_with("whisper_init_")
        || trimmed.starts_with("whisper_model_load:")
        || trimmed.starts_with("whisper_backend_init_gpu: no GPU found")
        || trimmed.starts_with("Windows WASAPI Whisper capture started:")
}

pub(crate) fn handle_native_transcript_line(
    app: &tauri::AppHandle,
    session_id: &str,
    helper_line: HelperTranscriptLine,
) {
    let transcript_text = helper_line.text.clone();
    let event_id = stable_id(&format!(
        "native:{}:{}:{}:{}",
        session_id, helper_line.source, helper_line.ended_at_ms, transcript_text
    ));
    let event_source = helper_line.source.clone();
    let input = TranscriptInput {
        id: Some(event_id.clone()),
        text: transcript_text,
        source: Some(event_source.clone()),
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
            emit_native_transcription_error(app, error, &event_source);
        }
    }
}

#[cfg(test)]
pub(crate) fn transcript_cleanup_context(
    session_id: &str,
    helper_line: &HelperTranscriptLine,
) -> String {
    let mut recent_lines = Vec::new();
    if let Some(sessions) = LIVE_SESSIONS.get()
        && let Ok(sessions) = sessions.lock()
        && let Some(session) = sessions.get(session_id)
    {
        recent_lines = session
            .events
            .iter()
            .rev()
            .take(6)
            .map(|event| {
                format!(
                    "[{}] {}",
                    transcript_source_label(&event.source),
                    event.text.replace('\n', " ")
                )
            })
            .collect::<Vec<_>>();
        recent_lines.reverse();
    }
    serde_json::json!({
        "mode": "live_transcript_cleanup",
        "currentSource": helper_line.source,
        "currentSourceLabel": transcript_source_label(&helper_line.source),
        "recentFinalTranscript": recent_lines
    })
    .to_string()
}

#[cfg(test)]
fn transcript_source_label(source: &str) -> &'static str {
    match source {
        "mic" => "我",
        "system" => "系統音訊",
        _ => "未標記來源",
    }
}

pub(crate) fn native_transcriber_key(session_id: &str, source: &str) -> String {
    format!("{session_id}::{source}")
}

pub(crate) fn monitor_native_transcriber_exit(
    app: tauri::AppHandle,
    session_id: String,
    source: String,
) {
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
                        return emit_native_transcriber_exit(
                            &app,
                            &session_id,
                            &source,
                            &message,
                            "error",
                        );
                    }
                };
                let Some(transcriber) = transcribers.get_mut(&key) else {
                    return;
                };
                match transcriber.child.try_wait() {
                    Ok(Some(status)) => {
                        if let Some(transcriber) = transcribers.remove(&key)
                            && let Some(stop_file) = transcriber.stop_file
                        {
                            let _ = std::fs::remove_file(stop_file);
                        }
                        Some(Ok(status.to_string()))
                    }
                    Ok(None) => None,
                    Err(error) => {
                        if let Some(transcriber) = transcribers.remove(&key)
                            && let Some(stop_file) = transcriber.stop_file
                        {
                            let _ = std::fs::remove_file(stop_file);
                        }
                        Some(Err(error.to_string()))
                    }
                }
            };
            match exit_result {
                Some(Ok(status)) => {
                    let message = format!(
                        "{source} native speech helper exited before Stop Listening: {status}"
                    );
                    emit_native_transcriber_exit(&app, &session_id, &source, &message, "warning");
                    return;
                }
                Some(Err(error)) => {
                    let message = format!("{source} native speech helper monitor failed: {error}");
                    emit_native_transcriber_exit(&app, &session_id, &source, &message, "error");
                    return;
                }
                None => {}
            }
        }
    });
}

pub(crate) fn emit_native_transcriber_exit(
    app: &tauri::AppHandle,
    session_id: &str,
    source: &str,
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
    emit_native_transcription_error(app, message, source);
}

pub(crate) fn emit_native_transcription_error(
    app: &tauri::AppHandle,
    message: impl Into<String>,
    source: &str,
) {
    let message = message.into();
    emit_native_transcription_error_with_code(app, message, source, None);
}

pub(crate) fn emit_native_transcription_error_with_code(
    app: &tauri::AppHandle,
    message: impl Into<String>,
    source: &str,
    code: Option<String>,
) {
    let message = message.into();
    let payload = NativeTranscriptionErrorEvent {
        code: code.unwrap_or_else(|| classify_native_transcription_error(&message)),
        source: source.to_string(),
        message,
    };
    let _ = app.emit("native_transcription_error", payload);
}

pub(crate) fn classify_native_transcription_error(message: &str) -> String {
    let lowered = message.to_lowercase();
    if lowered.contains("no speech detected")
        || message.contains("未偵測到語音")
        || message.contains("未检测到语音")
    {
        return "no_speech_detected".to_string();
    }
    if lowered.contains("recognition request was canceled")
        || lowered.contains("recognition request was cancelled")
        || lowered.contains("recognition request canceled")
        || lowered.contains("recognition request cancelled")
    {
        return "recognition_request_canceled".to_string();
    }
    if lowered.contains("stopped from tray") {
        return "stopped_from_tray".to_string();
    }
    if lowered.contains("screen recording")
        || lowered.contains("screen capture")
        || lowered.contains("screensystemaudiopreflight=false")
        || message.contains("螢幕與系統錄音")
    {
        return "screen_recording_permission".to_string();
    }
    "native_transcription_error".to_string()
}

pub(crate) fn has_active_native_transcribers(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Some(bridges) = MACOS_SPEECH_BRIDGES.get()
            && let Ok(bridges) = bridges.lock()
        {
            let prefix = format!("{session_id}::");
            if bridges.keys().any(|key| key.starts_with(&prefix)) {
                return true;
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
pub(crate) fn stop_native_transcription(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    stop_macos_speech_bridge(&session_id, None)?;
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get() {
        let mut transcribers = transcribers.lock().map_err(|error| error.to_string())?;
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
        for (_key, child) in stopped {
            stop_managed_native_transcriber_background(
                session_id.clone(),
                child,
                true,
                "native_transcription.stop",
            );
        }
    }
    set_listening_window_mode(&app, false);
    Ok(())
}

pub(crate) fn stop_all_native_transcribers(app: &tauri::AppHandle) {
    let _ = stop_prep_dictation();
    #[cfg(target_os = "macos")]
    {
        if let Some(bridges) = MACOS_SPEECH_BRIDGES.get()
            && let Ok(bridges) = bridges.lock()
        {
            let session_ids = bridges
                .keys()
                .filter_map(|key| {
                    key.split_once("::")
                        .map(|(session_id, _)| session_id.to_string())
                })
                .collect::<Vec<_>>();
            drop(bridges);
            for session_id in session_ids {
                let _ = stop_macos_speech_bridge(&session_id, None);
            }
        }
    }
    if let Some(transcribers) = NATIVE_TRANSCRIBERS.get()
        && let Ok(mut transcribers) = transcribers.lock()
    {
        let stopped = transcribers.drain().collect::<Vec<_>>();
        drop(transcribers);
        for (key, child) in stopped {
            let session_id = key
                .split_once("::")
                .map(|(session_id, _)| session_id)
                .unwrap_or(&key);
            stop_managed_native_transcriber_background(
                session_id.to_string(),
                child,
                true,
                "native_transcription.stop_all",
            );
        }
    }
    set_listening_window_mode(app, false);
}

pub(crate) fn ingest_transcript_inner(
    session_id: String,
    input: TranscriptInput,
) -> Result<IngestTranscriptResponse, String> {
    let (event, events_snapshot, should_persist_event) = {
        let mut sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session not found".to_string())?;

        let source = input.source.unwrap_or_else(|| "mic".to_string());
        let text = input.text;
        if let Some(existing) = session
            .events
            .iter()
            .find(|event| event.source == source && transcript_text_matches(&event.text, &text))
        {
            let event = existing.clone();
            let events_snapshot = session.events.clone();
            (event, events_snapshot, false)
        } else {
            let event_index = session.events.len() + 1;
            let event = TranscriptEvent {
                id: input
                    .id
                    .unwrap_or_else(|| format!("native_{}_{}", session_id, event_index)),
                session_id: session_id.clone(),
                source,
                speaker: input.speaker,
                speaker_confidence: input.speaker_confidence.unwrap_or(0.35),
                language: detect_language(&text),
                started_at_ms: input
                    .started_at_ms
                    .unwrap_or(((event_index - 1) * 5000) as i64),
                ended_at_ms: input.ended_at_ms,
                text,
                is_final: input.is_final.unwrap_or(true),
            };
            session.events.push(event.clone());
            (event, session.events.clone(), true)
        }
    };

    let db_path = app_db_path()?;
    let conn = open_db(&db_path)?;
    if should_persist_event {
        insert_transcript_event(&conn, &event)?;
    }

    let decision_state = derive_decision_state(&session_id, &events_snapshot);
    let suggestions = Vec::new();
    let transcript_events = {
        let sessions = LIVE_SESSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| "session not found".to_string())?;
        session.events.len()
    };

    let snapshot_id = stable_id(&format!(
        "{}:{}:{}:{}",
        session_id,
        event.id,
        now_ms(),
        serde_json::to_string(&decision_state).map_err(|error| error.to_string())?
    ));
    insert_decision_snapshot(&conn, &snapshot_id, &session_id, &decision_state)?;

    Ok(IngestTranscriptResponse {
        event: Some(event),
        live_evidence_event: None,
        suggestions: suggestions.clone(),
        decision_state,
        persisted: PersistedSummary {
            transcript_events,
            new_suggestions: suggestions.len(),
            decision_snapshot_id: snapshot_id,
        },
        coaching_error: None,
    })
}

pub(crate) fn transcript_text_matches(left: &str, right: &str) -> bool {
    normalize_transcript_text(left) == normalize_transcript_text(right)
}

pub(crate) fn normalize_transcript_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| ch.is_ascii_punctuation() || "。！？；，、".contains(ch))
        .to_lowercase()
}
