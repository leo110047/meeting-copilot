use crate::desktop_types::desktop_shell_plan;
use crate::desktop_types::{
    LocalSttModelDownloadProgress, LocalSttProfile, LocalSttStatus, NativeTranscriberHealth,
};
use crate::shell_storage::app_data_dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

const WHISPER_FAST_PROFILE_ID: &str = "whisper-fast";
const WHISPER_STANDARD_PROFILE_ID: &str = "whisper-standard";
const WHISPER_ACCURATE_PROFILE_ID: &str = "whisper-accurate";
const LOCAL_WHISPER_PROVIDER_ID: &str = "local-whisper";
const WHISPER_HEALTH_CACHE_TTL: Duration = Duration::from_secs(30);
static WHISPER_HEALTH_CACHE: OnceLock<Mutex<HashMap<String, CachedWhisperHealth>>> =
    OnceLock::new();
static LOCAL_STT_DOWNLOADS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct CachedWhisperHealth {
    checked_at: Instant,
    result: Result<(), String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalWhisperRuntime {
    pub(crate) runner_path: PathBuf,
    pub(crate) model_path: PathBuf,
    pub(crate) provider_id: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalSttConfig {
    selected_profile_id: String,
}

pub(crate) fn local_stt_profiles() -> Vec<LocalSttProfile> {
    vec![
        LocalSttProfile {
            id: WHISPER_STANDARD_PROFILE_ID,
            label: "標準",
            detail: "本機 Whisper small。準確度和速度比較平衡，建議作為預設會議模式。",
            engine: "whisper",
            model_file: Some("ggml-small.bin"),
            model_size_mb: Some(500),
            model_sha256: Some("1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
            model_download_url: Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
            ),
            recommended: true,
        },
        LocalSttProfile {
            id: WHISPER_FAST_PROFILE_ID,
            label: "快速",
            detail: "本機 Whisper base。延遲低，適合短會議或硬體較弱的電腦。",
            engine: "whisper",
            model_file: Some("ggml-base.bin"),
            model_size_mb: Some(150),
            model_sha256: Some("60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
            model_download_url: Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
            ),
            recommended: false,
        },
        LocalSttProfile {
            id: WHISPER_ACCURATE_PROFILE_ID,
            label: "高準確",
            detail: "本機 Whisper medium。較準，但模型大、耗電和延遲都會增加。",
            engine: "whisper",
            model_file: Some("ggml-medium.bin"),
            model_size_mb: Some(1500),
            model_sha256: Some("6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
            model_download_url: Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-medium.bin",
            ),
            recommended: false,
        },
    ]
}

pub(crate) fn normalize_local_stt_profile_id(profile_id: Option<&str>) -> &'static str {
    match profile_id.unwrap_or_default() {
        WHISPER_FAST_PROFILE_ID => WHISPER_FAST_PROFILE_ID,
        WHISPER_STANDARD_PROFILE_ID => WHISPER_STANDARD_PROFILE_ID,
        WHISPER_ACCURATE_PROFILE_ID => WHISPER_ACCURATE_PROFILE_ID,
        "" => WHISPER_STANDARD_PROFILE_ID,
        _ => WHISPER_STANDARD_PROFILE_ID,
    }
}

pub(crate) fn is_local_whisper_profile(profile_id: &str) -> bool {
    local_stt_profile(profile_id)
        .map(|profile| profile.engine == "whisper")
        .unwrap_or(false)
}

pub(crate) fn selected_local_stt_profile_id() -> String {
    read_local_stt_config()
        .map(|config| normalize_local_stt_profile_id(Some(&config.selected_profile_id)).to_string())
        .unwrap_or_else(|| WHISPER_STANDARD_PROFILE_ID.to_string())
}

pub(crate) fn set_selected_local_stt_profile(
    profile_id: Option<&str>,
) -> Result<LocalSttStatus, String> {
    let selected_profile_id = normalize_local_stt_profile_id(profile_id).to_string();
    let config = LocalSttConfig {
        selected_profile_id: selected_profile_id.clone(),
    };
    let path = local_stt_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    local_stt_status(Some(&selected_profile_id))
}

pub(crate) fn local_stt_status(profile_id: Option<&str>) -> Result<LocalSttStatus, String> {
    let selected_profile_id = profile_id
        .map(|value| normalize_local_stt_profile_id(Some(value)).to_string())
        .unwrap_or_else(selected_local_stt_profile_id);
    let profile = local_stt_profile(&selected_profile_id)
        .ok_or_else(|| format!("unknown local STT profile: {selected_profile_id}"))?;
    let model_directory = local_stt_model_dir()?;
    let engine_path = local_whisper_engine_path();
    let model_path = profile
        .model_file
        .map(|file_name| model_directory.join(file_name));
    let model_ready = model_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(true);
    let engine_ready = engine_path.is_some();
    let mut last_error = if !engine_ready {
        Some("localWhisperEngineMissing: Meeting Copilot 找不到可用的 Whisper 執行引擎。需要打包 meeting-copilot-whisper 或設定 MEETING_COPILOT_WHISPER_CLI。".to_string())
    } else if !model_ready {
        Some(format!(
            "localWhisperModelMissing: 找不到 {}。請把模型放到 {}。",
            profile.model_file.unwrap_or("Whisper model"),
            model_directory.display()
        ))
    } else {
        None
    };
    if last_error.is_none()
        && let (Some(engine), Some(model)) = (engine_path.as_ref(), model_path.as_ref())
        && let Err(error) = run_cached_whisper_health_check(engine, model)
    {
        last_error = Some(format!("localWhisperHealthFailed: {error}"));
    }
    let ready = last_error.is_none();
    Ok(LocalSttStatus {
        selected_profile_id,
        profiles: local_stt_profiles(),
        provider_id: LOCAL_WHISPER_PROVIDER_ID.to_string(),
        ready,
        engine_ready,
        model_ready,
        model_path: model_path.map(|path| path.display().to_string()),
        model_directory: model_directory.display().to_string(),
        last_error,
    })
}

fn run_whisper_health_check(engine_path: &Path, model_path: &Path) -> Result<(), String> {
    let mut child = Command::new(engine_path)
        .arg("--health")
        .arg("--model")
        .arg(model_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start Whisper health check: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Whisper health stdout unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Whisper health stderr unavailable".to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Whisper health failed: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Whisper health check timed out".to_string());
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut stdout_text = String::new();
    let mut stderr_text = String::new();
    let _ = stdout.read_to_string(&mut stdout_text);
    let _ = stderr.read_to_string(&mut stderr_text);
    if status.success() {
        Ok(())
    } else {
        Err(stdout_text
            .lines()
            .rev()
            .find(|line| line.contains("lastError"))
            .map(str::to_string)
            .or_else(|| {
                let trimmed = stderr_text.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .unwrap_or_else(|| status.to_string()))
    }
}

fn run_cached_whisper_health_check(engine_path: &Path, model_path: &Path) -> Result<(), String> {
    let cache_key = whisper_health_cache_key(engine_path, model_path)?;
    let cache = WHISPER_HEALTH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock()
        && let Some(cached) = guard.get(&cache_key)
        && cached.checked_at.elapsed() < WHISPER_HEALTH_CACHE_TTL
    {
        return cached.result.clone();
    }
    let result = run_whisper_health_check(engine_path, model_path);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(
            cache_key,
            CachedWhisperHealth {
                checked_at: Instant::now(),
                result: result.clone(),
            },
        );
    }
    result
}

fn whisper_health_cache_key(engine_path: &Path, model_path: &Path) -> Result<String, String> {
    let model_metadata = fs::metadata(model_path).map_err(|error| {
        format!(
            "failed to inspect Whisper model {}: {error}",
            model_path.display()
        )
    })?;
    let modified_ms = model_metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Ok(format!(
        "{}|{}|{}|{}",
        engine_path.display(),
        model_path.display(),
        model_metadata.len(),
        modified_ms
    ))
}

pub(crate) fn local_stt_model_directory() -> Result<PathBuf, String> {
    local_stt_model_dir()
}

pub(crate) fn cleanup_stale_whisper_temp_files() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with("meeting-copilot-whisper-") && file_name.ends_with(".wav") {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) async fn download_local_stt_model(
    app: tauri::AppHandle,
    profile_id: Option<&str>,
) -> Result<LocalSttStatus, String> {
    let selected_profile_id = normalize_local_stt_profile_id(profile_id).to_string();
    let _download_guard = acquire_model_download_guard(&selected_profile_id)?;
    let profile = local_stt_profile(&selected_profile_id)
        .ok_or_else(|| format!("unknown local STT profile: {selected_profile_id}"))?;
    let model_file = profile
        .model_file
        .ok_or_else(|| format!("local STT profile has no model file: {selected_profile_id}"))?;
    let expected_sha256 = profile
        .model_sha256
        .ok_or_else(|| format!("local STT profile has no model checksum: {selected_profile_id}"))?;
    let download_url = profile.model_download_url.ok_or_else(|| {
        format!("local STT profile has no model download URL: {selected_profile_id}")
    })?;
    let model_directory = local_stt_model_dir()?;
    let model_path = model_directory.join(model_file);
    fs::create_dir_all(&model_directory).map_err(|error| error.to_string())?;

    emit_model_download_progress(
        &app,
        &selected_profile_id,
        model_file,
        "checking",
        0,
        None,
        Some("正在檢查本機模型。".to_string()),
    );
    if model_path.exists() {
        let actual_sha256 = sha256_file(&model_path)?;
        if actual_sha256 == expected_sha256 {
            emit_model_download_progress(
                &app,
                &selected_profile_id,
                model_file,
                "completed",
                fs::metadata(&model_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                fs::metadata(&model_path)
                    .ok()
                    .map(|metadata| metadata.len()),
                Some("模型已存在且驗證通過。".to_string()),
            );
            return local_stt_status(Some(&selected_profile_id));
        }
        emit_model_download_progress(
            &app,
            &selected_profile_id,
            model_file,
            "replacing",
            0,
            None,
            Some("既有模型驗證失敗，正在重新下載。".to_string()),
        );
    }

    let temp_path = temp_model_download_path(&model_directory, model_file);
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }
    let result = download_model_to_temp(
        &app,
        &selected_profile_id,
        model_file,
        download_url,
        expected_sha256,
        &temp_path,
    )
    .await;
    if let Err(error) = result {
        let _ = fs::remove_file(&temp_path);
        emit_model_download_progress(
            &app,
            &selected_profile_id,
            model_file,
            "failed",
            0,
            None,
            Some(error.clone()),
        );
        return Err(error);
    }
    if model_path.exists() {
        fs::remove_file(&model_path).map_err(|error| {
            format!(
                "failed to replace existing Whisper model {}: {error}",
                model_path.display()
            )
        })?;
    }
    fs::rename(&temp_path, &model_path).map_err(|error| {
        format!(
            "failed to install Whisper model {}: {error}",
            model_path.display()
        )
    })?;
    emit_model_download_progress(
        &app,
        &selected_profile_id,
        model_file,
        "completed",
        fs::metadata(&model_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        fs::metadata(&model_path)
            .ok()
            .map(|metadata| metadata.len()),
        Some("模型下載完成。".to_string()),
    );
    local_stt_status(Some(&selected_profile_id))
}

async fn download_model_to_temp(
    app: &tauri::AppHandle,
    profile_id: &str,
    model_file: &str,
    download_url: &str,
    expected_sha256: &str,
    temp_path: &Path,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("Meeting Copilot local STT model downloader")
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(|error| format!("failed to create Whisper model downloader: {error}"))?;
    let mut response = client
        .get(download_url)
        .send()
        .await
        .map_err(|error| format!("failed to download Whisper model: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Whisper model download failed: HTTP {}",
            response.status()
        ));
    }
    let total_bytes = response.content_length();
    let mut file = fs::File::create(temp_path)
        .map_err(|error| format!("failed to create temporary Whisper model: {error}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded_bytes = 0_u64;
    let mut last_emit = Instant::now();
    emit_model_download_progress(
        app,
        profile_id,
        model_file,
        "downloading",
        downloaded_bytes,
        total_bytes,
        Some("正在下載 Whisper 模型。".to_string()),
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Whisper model download interrupted: {error}"))?
    {
        file.write_all(&chunk)
            .map_err(|error| format!("failed to write Whisper model: {error}"))?;
        hasher.update(&chunk);
        downloaded_bytes += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(250) {
            emit_model_download_progress(
                app,
                profile_id,
                model_file,
                "downloading",
                downloaded_bytes,
                total_bytes,
                None,
            );
            last_emit = Instant::now();
        }
    }
    file.sync_all()
        .map_err(|error| format!("failed to flush Whisper model: {error}"))?;
    drop(file);
    emit_model_download_progress(
        app,
        profile_id,
        model_file,
        "verifying",
        downloaded_bytes,
        total_bytes,
        Some("模型下載完成，正在驗證。".to_string()),
    );
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "Whisper model checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
        ));
    }
    emit_model_download_progress(
        app,
        profile_id,
        model_file,
        "installing",
        downloaded_bytes,
        total_bytes,
        Some("模型驗證通過，正在安裝。".to_string()),
    );
    Ok(())
}

fn emit_model_download_progress(
    app: &tauri::AppHandle,
    profile_id: &str,
    model_file: &str,
    state: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    message: Option<String>,
) {
    let percent = total_bytes
        .filter(|total| *total > 0)
        .map(|total| (downloaded_bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0));
    let _ = app.emit(
        "local_stt_model_download_progress",
        LocalSttModelDownloadProgress {
            profile_id: profile_id.to_string(),
            model_file: model_file.to_string(),
            state: state.to_string(),
            downloaded_bytes,
            total_bytes,
            percent,
            message,
        },
    );
}

fn temp_model_download_path(model_directory: &Path, model_file: &str) -> PathBuf {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    model_directory.join(format!(
        ".{model_file}.{}.{}.download",
        std::process::id(),
        timestamp_ms
    ))
}

struct ModelDownloadGuard {
    profile_id: String,
}

impl Drop for ModelDownloadGuard {
    fn drop(&mut self) {
        if let Some(downloads) = LOCAL_STT_DOWNLOADS.get()
            && let Ok(mut downloads) = downloads.lock()
        {
            downloads.remove(&self.profile_id);
        }
    }
}

fn acquire_model_download_guard(profile_id: &str) -> Result<ModelDownloadGuard, String> {
    let downloads = LOCAL_STT_DOWNLOADS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut downloads = downloads.lock().map_err(|error| error.to_string())?;
    if !downloads.insert(profile_id.to_string()) {
        return Err(format!(
            "Whisper model download is already in progress for {profile_id}"
        ));
    }
    Ok(ModelDownloadGuard {
        profile_id: profile_id.to_string(),
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open Whisper model {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read Whisper model {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn local_whisper_health(profile_id: &str) -> Result<NativeTranscriberHealth, String> {
    let status = local_stt_status(Some(profile_id))?;
    Ok(NativeTranscriberHealth {
        provider_id: status.provider_id,
        kind: "stt".to_string(),
        ready: status.ready,
        supports_streaming: true,
        supports_diarization: false,
        supports_source_hints: true,
        platform: desktop_shell_plan(),
        last_error: status.last_error,
    })
}

pub(crate) fn assert_local_whisper_ready(profile_id: &str) -> Result<(), String> {
    let status = local_stt_status(Some(profile_id))?;
    if status.ready {
        Ok(())
    } else {
        Err(status
            .last_error
            .unwrap_or_else(|| "local Whisper STT is not ready".to_string()))
    }
}

pub(crate) fn local_whisper_runtime(profile_id: &str) -> Result<LocalWhisperRuntime, String> {
    assert_local_whisper_ready(profile_id)?;
    let model_directory = local_stt_model_dir()?;
    let profile = local_stt_profile(profile_id)
        .ok_or_else(|| format!("unknown local STT profile: {profile_id}"))?;
    let model_file = profile
        .model_file
        .ok_or_else(|| format!("local STT profile has no Whisper model file: {profile_id}"))?;
    let runner_path = local_whisper_engine_path().ok_or_else(|| {
        "localWhisperEngineMissing: Meeting Copilot 找不到可用的 Whisper 執行引擎。".to_string()
    })?;
    Ok(LocalWhisperRuntime {
        runner_path,
        model_path: model_directory.join(model_file),
        provider_id: LOCAL_WHISPER_PROVIDER_ID,
    })
}

fn local_stt_profile(profile_id: &str) -> Option<LocalSttProfile> {
    local_stt_profiles()
        .into_iter()
        .find(|profile| profile.id == profile_id)
}

fn read_local_stt_config() -> Option<LocalSttConfig> {
    let path = local_stt_config_path().ok()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn local_stt_config_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("local-stt-config.json"))
}

fn local_stt_model_dir() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("Models").join("Whisper"))
}

fn local_whisper_engine_path() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("MEETING_COPILOT_WHISPER_CLI") {
        let path = PathBuf::from(configured);
        if path.exists() {
            return Some(path);
        }
    }
    for candidate in bundled_whisper_candidates() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // The app needs a streaming runner with Meeting Copilot's JSON-line contract,
    // not an arbitrary one-shot whisper CLI.
    find_on_path(&["meeting-copilot-whisper"])
}

fn bundled_whisper_candidates() -> Vec<PathBuf> {
    let runtime_name = if cfg!(target_os = "windows") {
        "meeting-copilot-whisper.exe"
    } else {
        "meeting-copilot-whisper"
    };
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let host_binary = if cfg!(target_os = "windows") {
            format!(
                "meeting-copilot-whisper-{}.exe",
                crate::native_storage::rust_host_triple()
            )
        } else {
            format!(
                "meeting-copilot-whisper-{}",
                crate::native_storage::rust_host_triple()
            )
        };
        candidates.push(cwd.join("src-tauri").join("binaries").join(host_binary));
        candidates.push(cwd.join("src-tauri").join("binaries").join(runtime_name));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join(runtime_name));
        candidates.push(parent.join("../Resources").join(runtime_name));
    }
    candidates
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_value) {
        for name in names {
            for candidate in executable_candidates(&directory, name) {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn executable_candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let has_extension = Path::new(name).extension().is_some();
        if has_extension {
            return vec![directory.join(name)];
        }
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        pathext
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| directory.join(format!("{name}{extension}")))
            .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![directory.join(name)]
    }
}
