#[derive(Clone)]
struct CachedTextProviderStatus {
    status: TextProviderStatus,
    checked_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionOAuthParse {
    Authenticated,
    Unauthenticated,
    Unknown,
}

static SUBSCRIPTION_OAUTH_STATUS_CACHE: OnceLock<Mutex<Option<CachedTextProviderStatus>>> =
    OnceLock::new();

fn subscription_oauth_status() -> TextProviderStatus {
    let cache = SUBSCRIPTION_OAUTH_STATUS_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.checked_at.elapsed() < subscription_oauth_status_ttl(&cached.status) {
                return cached.status.clone();
            }
        }
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

fn subscription_oauth_status_ttl(status: &TextProviderStatus) -> Duration {
    if status.authenticated {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(3)
    }
}

fn clear_subscription_oauth_status_cache() {
    if let Some(cache) = SUBSCRIPTION_OAUTH_STATUS_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

fn subscription_oauth_status_uncached() -> TextProviderStatus {
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
                    SubscriptionOAuthParse::Authenticated => "已登入 ChatGPT 訂閱 OAuth".to_string(),
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
            status_label: "找不到 Codex 訂閱 OAuth connector".to_string(),
            last_error: Some(format!("{}: {error}", codex.display())),
        },
    }
}

fn parse_subscription_oauth_authenticated(status_text: &str) -> SubscriptionOAuthParse {
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

fn start_subscription_oauth_login() -> Result<(), String> {
    clear_subscription_oauth_status_cache();
    let codex = codex_command_path();
    if !codex.exists() && codex.to_string_lossy() != "codex" {
        return Err(format!("找不到 Codex connector：{}", codex.display()));
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

#[cfg(target_os = "macos")]
fn create_private_oauth_temp_dir() -> Result<PathBuf, String> {
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
