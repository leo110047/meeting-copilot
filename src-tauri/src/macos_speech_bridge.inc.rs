#[cfg(target_os = "macos")]
type MacosSpeechCallback = unsafe extern "C" fn(*const c_char, *mut c_void);
#[cfg(target_os = "macos")]
type MacosSpeechReleaseContext = unsafe extern "C" fn(*mut c_void);

#[cfg(target_os = "macos")]
type MacosSpeechStartFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    MacosSpeechCallback,
    *mut c_void,
    MacosSpeechReleaseContext,
) -> c_int;
#[cfg(target_os = "macos")]
type MacosSpeechStopFn = unsafe extern "C" fn(c_int);
#[cfg(target_os = "macos")]
type MacosSpeechHealthFn = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;
#[cfg(target_os = "macos")]
type MacosSpeechStatusFn = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;
#[cfg(target_os = "macos")]
type MacosSpeechRequestPermissionsFn = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct MacosSpeechBridgeApi {
    start: MacosSpeechStartFn,
    stop: MacosSpeechStopFn,
    health: MacosSpeechHealthFn,
    status: MacosSpeechStatusFn,
    request_permissions: MacosSpeechRequestPermissionsFn,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct MacosSpeechBridgeStatus {
    raw: c_int,
}

#[cfg(target_os = "macos")]
const MACOS_STATUS_SPEECH_AUTHORIZED: c_int = 1 << 0;
#[cfg(target_os = "macos")]
const MACOS_STATUS_SPEECH_DENIED: c_int = 1 << 1;
#[cfg(target_os = "macos")]
const MACOS_STATUS_SPEECH_RESTRICTED: c_int = 1 << 2;
#[cfg(target_os = "macos")]
const MACOS_STATUS_SPEECH_NOT_DETERMINED: c_int = 1 << 3;
#[cfg(target_os = "macos")]
const MACOS_STATUS_RECOGNIZER_AVAILABLE: c_int = 1 << 4;
#[cfg(target_os = "macos")]
const MACOS_STATUS_MICROPHONE_AUTHORIZED: c_int = 1 << 5;
#[cfg(target_os = "macos")]
const MACOS_STATUS_MICROPHONE_DENIED: c_int = 1 << 6;
#[cfg(target_os = "macos")]
const MACOS_STATUS_MICROPHONE_RESTRICTED: c_int = 1 << 7;
#[cfg(target_os = "macos")]
const MACOS_STATUS_MICROPHONE_NOT_DETERMINED: c_int = 1 << 8;
#[cfg(target_os = "macos")]
const MACOS_STATUS_SCREEN_CAPTURE_PREFLIGHT: c_int = 1 << 9;
#[cfg(target_os = "macos")]
const MACOS_STATUS_MACOS_13_OR_NEWER: c_int = 1 << 12;

#[cfg(target_os = "macos")]
struct MacosSpeechBridgeSession {
    handle: c_int,
}

#[cfg(target_os = "macos")]
struct MacosSpeechCallbackContext {
    app: tauri::AppHandle,
    session_id: String,
    source: String,
    purpose: String,
}

#[cfg(target_os = "macos")]
static MACOS_SPEECH_BRIDGE_API: OnceLock<Result<MacosSpeechBridgeApi, String>> = OnceLock::new();
#[cfg(target_os = "macos")]
static MACOS_SPEECH_BRIDGES: OnceLock<Mutex<HashMap<String, MacosSpeechBridgeSession>>> =
    OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

#[cfg(target_os = "macos")]
const RTLD_NOW: c_int = 0x2;

#[cfg(target_os = "macos")]
fn macos_speech_bridge_api() -> Result<MacosSpeechBridgeApi, String> {
    MACOS_SPEECH_BRIDGE_API
        .get_or_init(load_macos_speech_bridge_api)
        .clone()
}

#[cfg(target_os = "macos")]
fn load_macos_speech_bridge_api() -> Result<MacosSpeechBridgeApi, String> {
    let path = macos_speech_bridge_path()?;
    let path = std::ffi::CString::new(path.display().to_string()).map_err(|error| error.to_string())?;
    unsafe {
        let handle = dlopen(path.as_ptr(), RTLD_NOW);
        if handle.is_null() {
            return Err(format!("failed to load macOS speech bridge: {}", dlerror_text()));
        }
        Ok(MacosSpeechBridgeApi {
            start: load_symbol(handle, "meeting_copilot_native_speech_start")?,
            stop: load_symbol(handle, "meeting_copilot_native_speech_stop")?,
            health: load_symbol(handle, "meeting_copilot_native_speech_health")?,
            status: load_symbol(handle, "meeting_copilot_native_speech_status")?,
            request_permissions: load_symbol(
                handle,
                "meeting_copilot_native_speech_request_permissions",
            )?,
        })
    }
}

#[cfg(target_os = "macos")]
unsafe fn load_symbol<T>(handle: *mut c_void, name: &str) -> Result<T, String>
where
    T: Copy,
{
    let symbol_name = std::ffi::CString::new(name).map_err(|error| error.to_string())?;
    let symbol = unsafe { dlsym(handle, symbol_name.as_ptr()) };
    if symbol.is_null() {
        return Err(format!("macOS speech bridge symbol missing: {name}: {}", dlerror_text()));
    }
    // POSIX dlsym returns a data pointer even for function symbols. This macOS
    // bridge relies on the platform ABI allowing that pointer to be called.
    Ok(unsafe { std::mem::transmute_copy(&symbol) })
}

#[cfg(target_os = "macos")]
fn dlerror_text() -> String {
    unsafe {
        let error = dlerror();
        if error.is_null() {
            "unknown dlopen error".to_string()
        } else {
            std::ffi::CStr::from_ptr(error).to_string_lossy().into_owned()
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_speech_bridge_health(source: &str, language: &str) -> Result<bool, String> {
    let api = macos_speech_bridge_api()?;
    let source = std::ffi::CString::new(source).map_err(|error| error.to_string())?;
    let language = std::ffi::CString::new(language).map_err(|error| error.to_string())?;
    Ok(unsafe { (api.health)(source.as_ptr(), language.as_ptr()) == 1 })
}

#[cfg(target_os = "macos")]
fn macos_speech_bridge_status(
    source: &str,
    language: &str,
) -> Result<MacosSpeechBridgeStatus, String> {
    let api = macos_speech_bridge_api()?;
    let source = std::ffi::CString::new(source).map_err(|error| error.to_string())?;
    let language = std::ffi::CString::new(language).map_err(|error| error.to_string())?;
    Ok(MacosSpeechBridgeStatus {
        raw: unsafe { (api.status)(source.as_ptr(), language.as_ptr()) },
    })
}

#[cfg(target_os = "macos")]
impl MacosSpeechBridgeStatus {
    fn has(self, flag: c_int) -> bool {
        self.raw & flag != 0
    }

    fn speech_state(self) -> &'static str {
        if self.has(MACOS_STATUS_SPEECH_AUTHORIZED) {
            "authorized"
        } else if self.has(MACOS_STATUS_SPEECH_DENIED) {
            "denied"
        } else if self.has(MACOS_STATUS_SPEECH_RESTRICTED) {
            "restricted"
        } else if self.has(MACOS_STATUS_SPEECH_NOT_DETERMINED) {
            "notDetermined"
        } else {
            "unknown"
        }
    }

    fn microphone_state(self) -> &'static str {
        if self.has(MACOS_STATUS_MICROPHONE_AUTHORIZED) {
            "authorized"
        } else if self.has(MACOS_STATUS_MICROPHONE_DENIED) {
            "denied"
        } else if self.has(MACOS_STATUS_MICROPHONE_RESTRICTED) {
            "restricted"
        } else if self.has(MACOS_STATUS_MICROPHONE_NOT_DETERMINED) {
            "notDetermined"
        } else {
            "unknown"
        }
    }

    fn recognizer_available(self) -> bool {
        self.has(MACOS_STATUS_RECOGNIZER_AVAILABLE)
    }

    fn screen_preflight(self) -> bool {
        self.has(MACOS_STATUS_SCREEN_CAPTURE_PREFLIGHT)
    }

    fn macos_13_or_newer(self) -> bool {
        self.has(MACOS_STATUS_MACOS_13_OR_NEWER)
    }
}

#[cfg(target_os = "macos")]
fn macos_speech_bridge_status_error(
    source: &str,
    language: &str,
    status: MacosSpeechBridgeStatus,
) -> String {
    let mut reasons = Vec::new();
    if status.speech_state() != "authorized" {
        reasons.push(format!("speechRecognition={}", status.speech_state()));
    }
    if !status.recognizer_available() {
        reasons.push(format!("speechRecognizerAvailable=false language={language}"));
    }
    if source == "mic" && status.microphone_state() != "authorized" {
        reasons.push(format!("microphone={}", status.microphone_state()));
    }
    if source == "system" && !status.screen_preflight() {
        reasons.push("screenSystemAudioPreflight=false".to_string());
    }
    if source == "system" && !status.macos_13_or_newer() {
        reasons.push("macOS13OrNewer=false".to_string());
    }
    if reasons.is_empty() {
        reasons.push("nativeHealthReturnedFalseWithoutMissingStatusFlag".to_string());
    }
    format!(
        "macOS native audio is not ready for {source}: {} (statusBits={})",
        reasons.join(", "),
        status.raw
    )
}

#[cfg(target_os = "macos")]
fn request_macos_speech_bridge_permissions(source: &str, language: &str) -> Result<bool, String> {
    let api = macos_speech_bridge_api()?;
    let source = std::ffi::CString::new(source).map_err(|error| error.to_string())?;
    let language = std::ffi::CString::new(language).map_err(|error| error.to_string())?;
    Ok(unsafe { (api.request_permissions)(source.as_ptr(), language.as_ptr()) == 1 })
}

#[cfg(target_os = "macos")]
fn start_macos_speech_bridge(
    app: tauri::AppHandle,
    session_id: &str,
    source: &str,
    language: &str,
) -> Result<(), String> {
    let api = macos_speech_bridge_api()?;
    let source_c = std::ffi::CString::new(source).map_err(|error| error.to_string())?;
    let language_c = std::ffi::CString::new(language).map_err(|error| error.to_string())?;
    let context = Box::new(MacosSpeechCallbackContext {
        app,
        session_id: session_id.to_string(),
        source: source.to_string(),
        purpose: "live".to_string(),
    });
    let context_ptr = Box::into_raw(context);
    let handle = unsafe {
        (api.start)(
            source_c.as_ptr(),
            language_c.as_ptr(),
            macos_speech_bridge_callback,
            context_ptr.cast::<c_void>(),
            macos_speech_bridge_release_context,
        )
    };
    if handle <= 0 {
        return Err(format!("macOS speech bridge failed to start {source}: {handle}"));
    }
    let key = native_transcriber_key(session_id, source);
    let mut bridges = MACOS_SPEECH_BRIDGES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(stale) = bridges.remove(&key) {
        unsafe {
            (api.stop)(stale.handle);
        }
    }
    bridges.insert(
        key,
        MacosSpeechBridgeSession { handle },
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn start_macos_prep_dictation_bridge(app: tauri::AppHandle, language: &str) -> Result<(), String> {
    let api = macos_speech_bridge_api()?;
    let source_c = std::ffi::CString::new("mic").map_err(|error| error.to_string())?;
    let language_c = std::ffi::CString::new(language).map_err(|error| error.to_string())?;
    let context = Box::new(MacosSpeechCallbackContext {
        app,
        session_id: "prep_dictation".to_string(),
        source: "mic".to_string(),
        purpose: "prep".to_string(),
    });
    let context_ptr = Box::into_raw(context);
    let handle = unsafe {
        (api.start)(
            source_c.as_ptr(),
            language_c.as_ptr(),
            macos_speech_bridge_callback,
            context_ptr.cast::<c_void>(),
            macos_speech_bridge_release_context,
        )
    };
    if handle <= 0 {
        return Err(format!("macOS prep dictation bridge failed to start: {handle}"));
    }
    let key = "prep_dictation::mic".to_string();
    let mut bridges = MACOS_SPEECH_BRIDGES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(stale) = bridges.remove(&key) {
        unsafe {
            (api.stop)(stale.handle);
        }
    }
    bridges.insert(
        key,
        MacosSpeechBridgeSession { handle },
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn stop_macos_prep_dictation_bridge() -> Result<(), String> {
    let api = macos_speech_bridge_api()?;
    let Some(bridges) = MACOS_SPEECH_BRIDGES.get() else {
        return Ok(());
    };
    let mut bridges = bridges.lock().map_err(|error| error.to_string())?;
    if let Some(session) = bridges.remove("prep_dictation::mic") {
        unsafe {
            (api.stop)(session.handle);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn stop_macos_speech_bridge(session_id: &str, source: Option<&str>) -> Result<(), String> {
    let api = macos_speech_bridge_api()?;
    let Some(bridges) = MACOS_SPEECH_BRIDGES.get() else {
        return Ok(());
    };
    let mut bridges = bridges.lock().map_err(|error| error.to_string())?;
    let prefix = format!("{session_id}::");
    let keys = bridges
        .keys()
        .filter(|key| {
            key.starts_with(&prefix)
                && source
                    .map(|source| key == &&native_transcriber_key(session_id, source))
                    .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(session) = bridges.remove(&key) {
            unsafe {
                (api.stop)(session.handle);
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn macos_speech_bridge_release_context(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(context as *mut MacosSpeechCallbackContext));
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn macos_speech_bridge_callback(line: *const c_char, context: *mut c_void) {
    if line.is_null() || context.is_null() {
        return;
    }
    let line = unsafe { std::ffi::CStr::from_ptr(line) }
        .to_string_lossy()
        .into_owned();
    let context = unsafe { &*(context as *const MacosSpeechCallbackContext) };
    if let Ok(error_line) = serde_json::from_str::<serde_json::Value>(&line) {
        if error_line.get("kind").and_then(|kind| kind.as_str()) == Some("error") {
            let message = error_line
                .get("message")
                .and_then(|message| message.as_str())
                .unwrap_or("macOS speech bridge error")
                .to_string();
            let _ = log_app_error_inner(
                Some(&context.session_id),
                "native_transcription.bridge_error",
                "macos_speech_bridge",
                "error",
                &message,
                serde_json::json!({"source": context.source}),
            );
            let _ = context.app.emit("native_transcription_error", message);
            return;
        }
    }
    let parsed: Result<HelperTranscriptLine, _> = serde_json::from_str(&line);
    match parsed {
        Ok(helper_line)
            if context.purpose == "prep"
                && helper_line.kind == "transcript"
                && helper_line.is_final =>
        {
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
            let _ = context.app.emit("prep_dictation_text", cleaned_text);
        }
        Ok(helper_line) if helper_line.kind == "transcript" && !helper_line.is_final => {
            let _ = context.app.emit("native_transcript_preview", helper_line);
        }
        Ok(helper_line) if helper_line.kind == "transcript" && helper_line.is_final => {
            handle_native_transcript_line(&context.app, &context.session_id, helper_line);
        }
        Ok(_) => {}
        Err(error) => {
            let message = format!("failed to parse macOS speech bridge line: {error}");
            let _ = log_app_error_inner(
                Some(&context.session_id),
                "native_transcription.bridge_parse_line",
                "macos_speech_bridge",
                "error",
                &message,
                serde_json::json!({"rawLineHash": stable_id(&line), "source": context.source}),
            );
            let _ = context.app.emit("native_transcription_error", message);
        }
    }
}
