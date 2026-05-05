use crate::decision_logic::{now_ms, now_ms_string, stable_id};
use crate::desktop_types::{
    AiSummaryRequest, AiSummarySections, LiveStatePatchEnvelope, MeetingBrief, NativeDecisionState,
    NativeSuggestion, PrepSummaryRequest, RevisedTranscriptLine, TextProviderStatus,
    TranscriptCleanupRequest, TranscriptEvent, TranscriptRevisionRequest,
};
use crate::native_storage::{log_app_error_inner, log_provider_failure, log_provider_usage};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone)]
pub(crate) struct CachedTextProviderStatus {
    status: TextProviderStatus,
    checked_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubscriptionOAuthParse {
    Authenticated,
    Unauthenticated,
    Unknown,
}

pub(crate) static SUBSCRIPTION_OAUTH_STATUS_CACHE: OnceLock<
    Mutex<Option<CachedTextProviderStatus>>,
> = OnceLock::new();

pub(crate) fn subscription_oauth_status() -> TextProviderStatus {
    let cache = SUBSCRIPTION_OAUTH_STATUS_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock()
        && let Some(cached) = guard.as_ref()
        && cached.checked_at.elapsed() < subscription_oauth_status_ttl(&cached.status)
    {
        return cached.status.clone();
    }
    let status = subscription_oauth_status_uncached();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some(CachedTextProviderStatus {
            status: status.clone(),
            checked_at: Instant::now(),
        });
    }
    status
}

pub(crate) fn subscription_oauth_status_ttl(status: &TextProviderStatus) -> Duration {
    if status.authenticated {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(3)
    }
}

pub(crate) fn clear_subscription_oauth_status_cache() {
    if let Some(cache) = SUBSCRIPTION_OAUTH_STATUS_CACHE.get()
        && let Ok(mut guard) = cache.lock()
    {
        *guard = None;
    }
}

pub(crate) fn subscription_oauth_status_uncached() -> TextProviderStatus {
    let codex = codex_command_path();
    let mut command = codex_command_from_path(&codex);
    configure_codex_oauth_env(&mut command);
    command.arg("login").arg("status");
    match run_command_output_with_timeout(&mut command, 5_000) {
        Ok(output) if output.status.success() => {
            let status_text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let parsed = parse_subscription_oauth_authenticated(&status_text);
            let authenticated = parsed == SubscriptionOAuthParse::Authenticated;
            TextProviderStatus {
                provider_id: "codex-chatgpt-oauth".to_string(),
                kind: "subscription_oauth".to_string(),
                authenticated,
                can_refresh_token: authenticated,
                supports_structured_output: true,
                supports_streaming: true,
                active: authenticated,
                status_label: match parsed {
                    SubscriptionOAuthParse::Authenticated => {
                        "已登入 ChatGPT 訂閱 OAuth".to_string()
                    }
                    SubscriptionOAuthParse::Unauthenticated => {
                        "尚未登入 ChatGPT 訂閱 OAuth".to_string()
                    }
                    SubscriptionOAuthParse::Unknown => {
                        "無法解析 ChatGPT 訂閱 OAuth 狀態".to_string()
                    }
                },
                last_error: match parsed {
                    SubscriptionOAuthParse::Authenticated => None,
                    SubscriptionOAuthParse::Unauthenticated | SubscriptionOAuthParse::Unknown => {
                        Some(status_text.trim().to_string())
                    }
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
            status_label: if error.contains("timed out") {
                "Codex CLI 無回應".to_string()
            } else {
                codex_connector_missing_label().to_string()
            },
            last_error: Some(format!(
                "{}：{} ({error})",
                codex_connector_missing_message(),
                codex.display()
            )),
        },
    }
}

pub(crate) fn parse_subscription_oauth_authenticated(status_text: &str) -> SubscriptionOAuthParse {
    let normalized = status_text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.trim().is_empty() {
        return SubscriptionOAuthParse::Unknown;
    }
    for negative in [
        "logged out",
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
            return SubscriptionOAuthParse::Unauthenticated;
        }
    }
    if [
        "logged in",
        "authenticated",
        "signed in",
        "chatgpt subscription",
        "chatgpt account",
    ]
    .iter()
    .any(|positive| normalized.contains(positive))
    {
        SubscriptionOAuthParse::Authenticated
    } else {
        SubscriptionOAuthParse::Unknown
    }
}

pub(crate) fn start_subscription_oauth_login() -> Result<(), String> {
    clear_subscription_oauth_status_cache();
    let codex = codex_command_path();
    if let Err(error) = codex_command_probe(&codex) {
        return Err(format!(
            "{}：{} ({error})",
            codex_connector_missing_message(),
            codex.display()
        ));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;

        let script_dir = create_private_oauth_temp_dir()?;
        let script_path = script_dir.join("login.command");
        let script = format!(
            r#"#!/bin/zsh
cleanup() {{
  rm -f -- "$0"
  rmdir -- "$(dirname "$0")" 2>/dev/null || true
}}
trap cleanup EXIT
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
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let open_result = Command::new("open")
            .arg(&script_path)
            .spawn()
            .map_err(|error| format!("failed to open login terminal: {error}"));
        if open_result.is_err() {
            let _ = fs::remove_file(&script_path);
            let _ = fs::remove_dir(&script_dir);
        }
        open_result?;
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(600));
            let _ = fs::remove_file(&script_path);
            let _ = fs::remove_dir(&script_dir);
        });
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let script_dir = create_private_oauth_temp_dir()?;
        let script_path = script_dir.join("login.cmd");
        let script = format!(
            r#"@echo off
setlocal
echo Meeting Copilot ChatGPT subscription OAuth login
echo This window was opened by Meeting Copilot. Complete the browser login, then return to the app; Meeting Copilot will refresh the status automatically.
echo.
{} login
set "EXIT_CODE=%ERRORLEVEL%"
echo.
if "%EXIT_CODE%"=="0" (
  echo Login flow finished. You can close this window.
) else (
  echo Codex login exited with code %EXIT_CODE%.
)
echo.
pause
del "%~f0" >nul 2>nul
for %%I in ("%~dp0.") do rmdir "%%~fI" >nul 2>nul
exit /b %EXIT_CODE%
"#,
            cmd_batch_quote_path(&codex)
        );
        fs::write(&script_path, script).map_err(|error| error.to_string())?;
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(&script_path)
            .spawn()
            .map_err(|error| format!("failed to open login terminal: {error}"))?;
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(600));
            let _ = fs::remove_file(&script_path);
            let _ = fs::remove_dir(&script_dir);
        });
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("subscription OAuth login launcher is not implemented for this platform".to_string())
    }
}

pub(crate) fn create_private_oauth_temp_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut random_bytes = [0_u8; 16];
        fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random_bytes))
            .map_err(|error| format!("failed to read secure random bytes: {error}"))?;
        let suffix = random_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let dir = std::env::temp_dir().join(format!("meeting-copilot-codex-login-{suffix}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|error| format!("failed to create private login temp dir: {error}"))?;
        Ok(dir)
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        for attempt in 0..32 {
            let dir = base.join(format!(
                "meeting-copilot-codex-login-{}-{pid}-{attempt}",
                now_ms()
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("failed to create private login temp dir: {error}"));
                }
            }
        }
        Err("failed to create private login temp dir after retries".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("private OAuth temp dir is not implemented for this platform".to_string())
    }
}

pub(crate) fn build_ai_summary_prompt(request: &AiSummaryRequest) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's subscription OAuth text decision provider.
Return ONLY a JSON object with this exact shape:
{{"keyPoints":["..."],"decisionsAndOpenQuestions":["..."],"suggestedActions":["..."]}}

Rules:
- Write Traditional Chinese.
- Use the transcript, prepContext, and localSummary only.
- Use transcript speaker labels only to attribute statements or responsibilities that are explicitly supported by the transcript.
- Do not invent decisions, owners, dates, or commitments.
- If evidence is insufficient, say that explicitly.
- Keep each array to 3-6 concise items.

Meeting payload:
{payload}
"#
    ))
}

pub(crate) fn build_prep_summary_prompt(request: &PrepSummaryRequest) -> Result<String, String> {
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

pub(crate) fn cleanup_transcript_text_oauth_inner(
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

pub(crate) fn build_transcript_cleanup_prompt(
    request: &TranscriptCleanupRequest,
) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's transcript cleanup provider.
Return ONLY a JSON object with this exact shape:
{{"text":"..."}}

Rules:
- Preserve the original meaning. Do not summarize.
- Remove only obvious stutters, repeated starts, filler words, and speech disfluencies.
- Use context only to disambiguate punctuation, terminology, and short pronouns. Do not import facts from context into the cleaned text.
- Keep names, numbers, dates, owners, deadlines, scope, technical terms, and mixed English terms.
- Do not add facts, conclusions, speakers, or punctuation that changes meaning.
- Write Traditional Chinese when the source is Chinese. Preserve English terms that appear in the source.
- If cleanup is unsafe or unnecessary, return the original text.

Transcript payload:
{payload}
"#
    ))
}

pub(crate) fn build_transcript_revision_prompt(
    request: &TranscriptRevisionRequest,
) -> Result<String, String> {
    let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"You are Meeting Copilot's live transcript revision provider.
Return ONLY a JSON object with this exact shape:
{{"transcript":[{{"id":"...","speaker":"...","text":"...","source":"...","language":"..."}}]}}

Rules:
- Write Traditional Chinese. Preserve English technical terms that appear in the source.
- Preserve every input line id and order. Do not add, remove, merge, split, or reorder lines.
- Preserve the original meaning. Do not summarize or add facts.
- Clean obvious ASR errors, repeated starts, filler words, and punctuation when safe.
- For source="mic", speaker MUST be "我".
- For source="system", infer remote speaker changes from semantics and conversation flow. Use stable labels "對方 A", "對方 B", "對方 C" only. Reuse the same label when the same remote speaker appears to continue.
- If an input line already has speaker "對方 A", "對方 B", or "對方 C", keep that speaker unless nearby context strongly contradicts it.
- If source is missing or unknown, use speaker "未標記來源"; do not use "未知".
- Keep source and language from the input line.
- If speaker inference is uncertain, prefer the previous system speaker label over inventing a new speaker.

Transcript payload:
{payload}
"#
    ))
}

pub(crate) fn parse_transcript_revision_response(
    raw_output: &str,
    request: &TranscriptRevisionRequest,
) -> Result<Vec<RevisedTranscriptLine>, String> {
    let value = parse_json_object_value(raw_output)?;
    let transcript = value
        .get("transcript")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "transcript must be an array".to_string())?;
    if transcript.len() != request.transcript.len() {
        return Err("revised transcript must preserve line count".to_string());
    }
    let mut lines = Vec::with_capacity(transcript.len());
    for (index, item) in transcript.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("transcript[{index}] must be an object"))?;
        let input = &request.transcript[index];
        let id = required_trimmed_string_field(object, "id")?;
        if id != input.id {
            return Err(format!("transcript[{index}].id must preserve input order"));
        }
        let source = required_trimmed_string_field(object, "source")?;
        if source != input.source {
            return Err(format!(
                "transcript[{index}].source must preserve input source"
            ));
        }
        let language = required_trimmed_string_field(object, "language")?;
        if language != input.language {
            return Err(format!(
                "transcript[{index}].language must preserve input language"
            ));
        }
        let mut speaker = required_trimmed_string_field(object, "speaker")?;
        if source == "mic" {
            speaker = "我".to_string();
        } else if source == "system" && !is_allowed_remote_speaker(&speaker) {
            return Err(format!(
                "transcript[{index}].speaker is not an allowed remote label"
            ));
        } else if speaker == "未知" || (source != "system" && !is_allowed_remote_speaker(&speaker))
        {
            speaker = "未標記來源".to_string();
        }
        let text = required_text_field(object, "text")?;
        if text.trim().is_empty() {
            return Err(format!("transcript[{index}].text must not be empty"));
        }
        lines.push(RevisedTranscriptLine {
            id,
            text,
            speaker,
            source,
            language,
        });
    }
    Ok(lines)
}

fn required_trimmed_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

fn required_text_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    let value = object
        .get(field)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| format!("{field} is required"))?;
    if value.trim().is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

fn is_allowed_remote_speaker(speaker: &str) -> bool {
    matches!(speaker, "對方 A" | "對方 B" | "對方 C" | "未標記來源")
}

pub(crate) fn build_live_state_patch_prompt(
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
  }},
  "coaching": {{
    "cards": [
      {{
        "kind": "say_next|ask_clarifying_question|watch_out|challenge_assumption|confirm_commitment|defer_decision",
        "priority": "low|medium|high",
        "confidence": 0.0,
        "title": "...",
        "suggestedMove": "...",
        "watchOut": "...",
        "reason": "...",
        "evidenceTranscriptIds": []
      }}
    ]
  }}
}}

Rules:
- Write Traditional Chinese in text fields.
- Do not invent owners, dates, commitments, decisions, or speakers.
- Use only allowedEvidenceTranscriptIds for evidence.
- If unsure, return empty arrays and null fields.
- Coaching cards are live meeting interventions. Show a card only when the user can act on it in the next turn.
- Use the meeting brief as the user's goals, known background, must-confirm points, constraints, and risks.
- Coaching should answer one of: what the user should say next, what to clarify, what risk in the other party's words needs attention, or what commitment should be confirmed.
- Do not create coaching cards for summaries, obvious acknowledgements, greetings, or low-confidence guesses.
- Prefer a clarifying question over a directive when evidence is incomplete.
- Return at most one high-value coaching card.
- addMissingInputs item shape: {{"kind":"owner|deadline|acceptance_criteria|rollback_plan|other","text":"...","blocksDecision":true}}
- addRisks item shape: {{"text":"...","severity":"low|medium|high","evidenceTranscriptIds":["..."]}}
- Do not include fields named meetingState, decisionState, fullState, replacementState, transcriptEvents, or suggestions.

Meeting payload:
{payload}
"#
    ))
}

pub(crate) fn run_codex_oauth_prompt(prompt: &str) -> Result<String, String> {
    run_codex_oauth_prompt_with_timeout(prompt, 60_000)
}

pub(crate) fn run_codex_oauth_prompt_with_timeout(
    prompt: &str,
    timeout_ms: u64,
) -> Result<String, String> {
    let output_path =
        std::env::temp_dir().join(format!("meeting-copilot-ai-summary-{}.txt", now_ms()));
    let codex = codex_command_path();
    let mut command = codex_command_from_path(&codex);
    configure_codex_oauth_env(&mut command);
    configure_background_command(&mut command);
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

pub(crate) fn truncate_for_diagnostic(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect::<String>()
}

pub(crate) fn parse_live_state_patch(
    raw_output: &str,
) -> Result<LiveStatePatchEnvelope, (&'static str, String)> {
    let value = parse_json_object_value(raw_output).map_err(|error| ("malformed_json", error))?;
    validate_live_state_patch_value(&value).map_err(|error| ("schema_validation", error))?;
    serde_json::from_value(value).map_err(|error| ("schema_validation", error.to_string()))
}

pub(crate) fn parse_live_coaching_suggestions(
    raw_output: &str,
    session_id: &str,
    events: &[TranscriptEvent],
) -> Result<Vec<NativeSuggestion>, (&'static str, String)> {
    let value = parse_json_object_value(raw_output).map_err(|error| ("malformed_json", error))?;
    let Some(cards) = value
        .get("coaching")
        .and_then(|value| value.get("cards"))
        .and_then(|value| value.as_array())
    else {
        return Ok(vec![]);
    };
    let mut suggestions = vec![];
    let mut dropped_reasons = vec![];
    let mut first_schema_error = None;
    for (index, card) in cards.iter().enumerate() {
        match parse_live_coaching_card(card, index, session_id, events) {
            Ok(CoachingCardParseResult::Accepted(suggestion)) => suggestions.push(suggestion),
            Ok(CoachingCardParseResult::Dropped(reason)) => {
                dropped_reasons.push(format!("coaching.cards[{index}]: {reason}"));
            }
            Err(error) => {
                if first_schema_error.is_none() {
                    first_schema_error = Some(error);
                }
            }
        }
    }
    suggestions.sort_by(|left, right| {
        coaching_priority_rank(&right.priority)
            .cmp(&coaching_priority_rank(&left.priority))
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    if let Some(suggestion) = suggestions.into_iter().next() {
        return Ok(vec![suggestion]);
    }
    if let Some(error) = first_schema_error {
        return Err(("schema_validation", error));
    }
    if !dropped_reasons.is_empty() {
        return Err(("coaching_cards_discarded", dropped_reasons.join("; ")));
    }
    Ok(vec![])
}

enum CoachingCardParseResult {
    Accepted(NativeSuggestion),
    Dropped(&'static str),
}

fn parse_live_coaching_card(
    card: &serde_json::Value,
    index: usize,
    session_id: &str,
    events: &[TranscriptEvent],
) -> Result<CoachingCardParseResult, String> {
    let object = card
        .as_object()
        .ok_or_else(|| format!("coaching.cards[{index}] must be an object"))?;
    let kind = coaching_string_field(object, "kind")?;
    if !is_allowed_coaching_kind(&kind) {
        return Err(format!("coaching.cards[{index}].kind is not allowed"));
    }
    let priority = coaching_string_field(object, "priority")?;
    if !matches!(priority.as_str(), "low" | "medium" | "high") {
        return Err(format!("coaching.cards[{index}].priority is not allowed"));
    }
    let title = coaching_string_field(object, "title")?;
    let suggested_move = coaching_string_field(object, "suggestedMove")?;
    let watch_out = optional_coaching_string_field(object, "watchOut");
    let reason = coaching_string_field(object, "reason")?;
    let confidence = object
        .get("confidence")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.65)
        .clamp(0.0, 1.0);
    if confidence < 0.45 {
        return Ok(CoachingCardParseResult::Dropped("confidence_too_low"));
    }
    let allowed_ids = events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let evidence_transcript_ids = object
        .get("evidenceTranscriptIds")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .filter(|id| allowed_ids.contains(id))
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if evidence_transcript_ids.is_empty() {
        return Ok(CoachingCardParseResult::Dropped(
            "missing_or_invalid_evidence_transcript_ids",
        ));
    }
    let text = if let Some(watch_out) = &watch_out {
        format!("{suggested_move}\n注意：{watch_out}")
    } else {
        suggested_move.clone()
    };
    Ok(CoachingCardParseResult::Accepted(NativeSuggestion {
        id: stable_id(&format!(
            "live_coaching:{session_id}:{kind}:{}",
            evidence_transcript_ids.join(",")
        )),
        session_id: session_id.to_string(),
        shown_at: now_ms_string(),
        kind,
        title: Some(title),
        text,
        suggested_move: Some(suggested_move),
        watch_out,
        reason,
        confidence,
        priority,
        evidence_transcript_ids,
    }))
}

fn coaching_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    optional_coaching_string_field(object, field)
        .ok_or_else(|| format!("coaching card {field} is required"))
}

fn optional_coaching_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    object
        .get(field)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().graphemes(true).take(220).collect::<String>())
        .filter(|value| !value.is_empty())
}

fn is_allowed_coaching_kind(kind: &str) -> bool {
    matches!(
        kind,
        "say_next"
            | "ask_clarifying_question"
            | "watch_out"
            | "challenge_assumption"
            | "confirm_commitment"
            | "defer_decision"
    )
}

fn coaching_priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub(crate) fn parse_json_object_value(raw_output: &str) -> Result<serde_json::Value, String> {
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

pub(crate) fn validate_live_state_patch_value(value: &serde_json::Value) -> Result<(), String> {
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

pub(crate) fn reject_nested_full_rewrite(
    value: &serde_json::Value,
    path: &str,
) -> Result<(), String> {
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

pub(crate) fn codex_command_path() -> PathBuf {
    if let Ok(path) = std::env::var("MEETING_COPILOT_CODEX") {
        let path = PathBuf::from(path);
        // Return the user-provided path as-is so diagnostics can report the exact override.
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    #[cfg(target_os = "windows")]
    {
        for candidate in codex_path_candidates_from_path() {
            if candidate.exists() {
                return candidate;
            }
        }
        let pathext = std::env::var_os("PATHEXT");
        let executable_names = windows_codex_executable_names(pathext.as_deref());
        if let Ok(app_data) = std::env::var("APPDATA") {
            for filename in &executable_names {
                let path = PathBuf::from(&app_data).join("npm").join(filename);
                if path.exists() {
                    return path;
                }
            }
        }
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            for relative in [
                ["Programs", "Codex", "codex.exe"],
                ["Programs", "OpenAI Codex", "codex.exe"],
            ] {
                let path = relative
                    .iter()
                    .fold(PathBuf::from(&local_app_data), |base, segment| {
                        base.join(segment)
                    });
                if path.exists() {
                    return path;
                }
            }
        }
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            let path = PathBuf::from(user_profile)
                .join(".codex")
                .join("bin")
                .join("codex.exe");
            if path.exists() {
                return path;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for candidate in codex_unix_fallback_candidates() {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return path;
            }
        }
    }
    PathBuf::from("codex")
}

pub(crate) fn codex_command_probe(codex: &Path) -> Result<(), String> {
    let mut command = codex_command_from_path(codex);
    configure_codex_oauth_env(&mut command);
    command.arg("--version");
    let output = run_command_output_with_timeout(&mut command, 3_000)?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = truncate_for_diagnostic(
            &format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            400,
        );
        Err(if detail.is_empty() {
            format!("Codex CLI --version exited with {}", output.status)
        } else {
            format!(
                "Codex CLI --version exited with {}: {detail}",
                output.status
            )
        })
    }
}

#[cfg(test)]
pub(crate) fn codex_command_available(codex: &Path) -> bool {
    codex_command_probe(codex).is_ok()
}

pub(crate) fn codex_command_from_path(codex: &Path) -> Command {
    #[cfg(target_os = "windows")]
    {
        if is_windows_command_shim(codex) {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(codex);
            return command;
        }
    }
    Command::new(codex)
}

pub(crate) fn run_command_output_with_timeout(
    command: &mut Command,
    timeout_ms: u64,
) -> Result<Output, String> {
    configure_background_command(command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "child stdout unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "child stderr unavailable".to_string())?;
    let stdout_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let stderr_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                if let Err(error) = child.kill() {
                    let _ = log_app_error_inner(
                        None,
                        "command.timeout.kill",
                        "text_provider",
                        "warning",
                        &error.to_string(),
                        serde_json::json!({"timeoutMs": timeout_ms}),
                    );
                }
                let _ = child.wait();
                let stderr = stderr_reader.join().unwrap_or_default();
                let _ = stdout_reader.join();
                let detail = truncate_for_diagnostic(&String::from_utf8_lossy(&stderr), 400);
                return Err(if detail.is_empty() {
                    format!("command timed out after {timeout_ms}ms")
                } else {
                    format!("command timed out after {timeout_ms}ms: {detail}")
                });
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn configure_background_command(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn configure_background_command(_command: &mut Command) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn codex_unix_fallback_candidates() -> &'static [&'static str] {
    &["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
}

#[cfg(target_os = "windows")]
pub(crate) fn codex_path_candidates_from_path() -> Vec<PathBuf> {
    let Some(paths) = std::env::var_os("PATH") else {
        return vec![];
    };
    let pathext = std::env::var_os("PATHEXT");
    let executable_names = windows_codex_executable_names(pathext.as_deref());
    std::env::split_paths(&paths)
        .flat_map(|dir| {
            executable_names
                .iter()
                .map(move |name| dir.join(name))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_codex_executable_names(pathext: Option<&std::ffi::OsStr>) -> Vec<String> {
    windows_executable_extensions_from_pathext(pathext)
        .into_iter()
        .map(|extension| format!("codex{extension}"))
        .collect()
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_executable_extensions_from_pathext(
    pathext: Option<&std::ffi::OsStr>,
) -> Vec<String> {
    let raw = pathext
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    let mut extensions = Vec::new();
    for part in raw.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let extension = if trimmed.starts_with('.') {
            trimmed.to_ascii_lowercase()
        } else {
            format!(".{}", trimmed.to_ascii_lowercase())
        };
        if !extensions.contains(&extension) {
            extensions.push(extension);
        }
    }
    if extensions.is_empty() {
        extensions.extend([".com", ".exe", ".bat", ".cmd"].map(String::from));
    }
    extensions
}

#[cfg(target_os = "windows")]
pub(crate) fn is_windows_command_shim(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase()),
        Some(extension) if extension == "cmd" || extension == "bat"
    )
}

pub(crate) fn codex_connector_missing_label() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows 找不到 Codex CLI"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "找不到 Codex 訂閱 OAuth connector"
    }
}

pub(crate) fn codex_connector_missing_message() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows 找不到 Codex CLI；請先安裝 Codex CLI，或把 codex.exe 加到 PATH，再重開 Meeting Copilot。也可以設定 MEETING_COPILOT_CODEX 指到 Codex CLI 執行檔。"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "找不到 Codex connector；請確認 codex 在 PATH，或設定 MEETING_COPILOT_CODEX 指到 Codex CLI 執行檔。"
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn cmd_batch_quote_path(path: &Path) -> String {
    let value = path.display().to_string();
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '%' => escaped.push_str("%%"),
            '^' => escaped.push_str("^^"),
            '&' => escaped.push_str("^&"),
            '|' => escaped.push_str("^|"),
            '<' => escaped.push_str("^<"),
            '>' => escaped.push_str("^>"),
            '(' => escaped.push_str("^("),
            ')' => escaped.push_str("^)"),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

pub(crate) fn configure_codex_oauth_env(command: &mut Command) {
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

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn parse_ai_summary_sections(raw_output: &str) -> Result<AiSummarySections, String> {
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

pub(crate) fn parse_prep_summary_points(raw_output: &str) -> Result<Vec<String>, String> {
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

pub(crate) fn parse_transcript_cleanup_text(raw_output: &str) -> Result<String, String> {
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
