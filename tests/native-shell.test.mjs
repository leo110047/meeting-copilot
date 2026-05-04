import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("Tauri native shell is tray/status-item first", async () => {
  const [
    mainRoot,
    desktopTypes,
    commandsCore,
    commandsAudio,
    shellStorage,
    oauthProvider,
    nativeStorage,
    decisionLogic,
    macosSpeechBridge,
    cargoToml,
    tauriConfigText,
    macInfoPlist,
    helperBuildScript,
    helperInstallScript,
    runWithRustScript,
    macBridge,
    macHelper,
    windowsHelper
  ] = await Promise.all([
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/desktop_types.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_core.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_audio.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/shell_storage.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/oauth_provider.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/native_storage.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/decision_logic.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/macos_speech_bridge.inc.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/Info.plist", import.meta.url), "utf8"),
    readFile(new URL("../scripts/build-native-helpers.mjs", import.meta.url), "utf8"),
    readFile(new URL("../scripts/install-mac-helper-app.mjs", import.meta.url), "utf8"),
    readFile(new URL("../scripts/run-with-rust.mjs", import.meta.url), "utf8"),
    readFile(new URL("../native/macos/MeetingCopilotSpeechBridge.swift", import.meta.url), "utf8"),
    readFile(new URL("../native/macos/MeetingCopilotSpeech.swift", import.meta.url), "utf8"),
    readFile(new URL("../native/windows/meeting-copilot-windows-speech.rs", import.meta.url), "utf8")
  ]);
  const mainRs = [mainRoot, desktopTypes, commandsCore, commandsAudio, shellStorage, oauthProvider, nativeStorage, decisionLogic, macosSpeechBridge].join("\n");
  const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  const tauriConfig = JSON.parse(tauriConfigText);

  assert.match(cargoToml, /features = \["image-png", "tray-icon"\]/);
  assert.match(cargoToml, /\[target\.'cfg\(target_os = "windows"\)'\.dependencies\]/);
  assert.match(cargoToml, /Win32_UI_WindowsAndMessaging/);
  assert.match(mainRs, /TrayIconBuilder::with_id\("meeting-copilot"\)/);
  assert.match(mainRs, /#\[serde\(rename_all = "camelCase"\)\]\s*struct DesktopShellPlan/);
  assert.match(mainRs, /start_native_transcription/);
  assert.match(mainRs, /native_transcriber_health/);
  assert.match(mainRs, /request_native_audio_permissions/);
  assert.match(mainRs, /NativeTranscriberHealthRequest/);
  assert.match(mainRs, /run_native_transcriber_health_check/);
  assert.match(mainRs, /\.arg\("--health"\)/);
  assert.match(mainRs, /\.arg\("--source"\)/);
  assert.match(mainRs, /source == "mixed"/);
  assert.match(mainRs, /request_screen_recording_permission/);
  assert.match(mainRs, /macos_speech_bridge_path/);
  assert.match(mainRs, /macos_speech_bridge_health/);
  assert.match(mainRs, /macos_speech_bridge_status/);
  assert.match(mainRs, /macos_speech_bridge_status_error/);
  assert.match(mainRs, /screenSystemAudioPreflight=false/);
  assert.match(mainRs, /statusBits=/);
  assert.match(mainRs, /start_macos_speech_bridge/);
  assert.match(mainRs, /start_macos_prep_dictation_bridge/);
  assert.match(mainRs, /request_macos_speech_bridge_permissions/);
  assert.match(mainRs, /x-apple\.systempreferences:com\.apple\.preference\.security\?Privacy_ScreenCapture/);
  assert.match(mainRs, /text_provider_status/);
  assert.match(mainRs, /start_text_provider_login/);
  assert.match(mainRs, /start_subscription_oauth_login/);
  assert.match(mainRs, /generate_ai_summary_oauth/);
  assert.match(mainRs, /generate_prep_summary_oauth/);
  assert.match(mainRs, /build_prep_summary_prompt/);
  assert.match(mainRs, /parse_prep_summary_points/);
  assert.match(mainRs, /extract_live_state_patch_oauth/);
  assert.match(mainRs, /build_live_state_patch_prompt/);
  assert.match(mainRs, /parse_live_state_patch/);
  assert.match(mainRs, /apply_live_state_patch/);
  assert.match(mainRs, /log_extraction_failure/);
  assert.match(mainRs, /provider attempted full rewrite field/);
  assert.match(mainRs, /run_codex_oauth_prompt_with_timeout/);
  assert.match(mainRs, /codex_command_path/);
  assert.match(mainRs, /\.arg\("login"\)\.arg\("status"\)/);
  assert.match(mainRs, /CachedTextProviderStatus/);
  assert.match(mainRs, /SubscriptionOAuthParse::Unknown/);
  assert.match(mainRs, /Duration::from_secs\(30\)/);
  assert.match(mainRs, /create_private_oauth_temp_dir/);
  assert.match(mainRs, /\/dev\/urandom/);
  assert.match(mainRs, /fs::set_permissions/);
  assert.match(mainRs, /trap cleanup EXIT/);
  assert.match(mainRs, /\.arg\("login"\)/);
  assert.match(mainRs, /\.arg\("exec"\)/);
  assert.match(mainRs, /subscription_oauth/);
  assert.match(mainRs, /read_dropped_context_files/);
  assert.match(mainRs, /DROP_READ_GRANTS/);
  assert.match(mainRs, /parse_subscription_oauth_authenticated/);
  assert.match(mainRs, /log_provider_usage/);
  assert.match(mainRs, /log_provider_failure/);
  assert.match(mainRs, /app_error_logs/);
  assert.match(mainRs, /log_app_error/);
  assert.match(mainRs, /export_app_error_logs/);
  assert.match(mainRs, /log_app_error_inner/);
  assert.match(mainRs, /read_dropped_context_file/);
  assert.match(mainRs, /set_window_opacity/);
  assert.match(mainRs, /set_native_window_opacity/);
  assert.match(mainRs, /setAlphaValue/);
  assert.match(mainRs, /SetLayeredWindowAttributes/);
  assert.match(mainRs, /WS_EX_LAYERED/);
  assert.match(mainRs, /wasapi_capture\+wasapi_loopback/);
  assert.match(mainRs, /windows-speech-native/);
  assert.match(mainRs, /meeting-copilot-native-speech/);
  assert.match(mainRs, /start_prep_dictation/);
  assert.match(mainRs, /stop_prep_dictation/);
  assert.match(mainRs, /prep_dictation_text/);
  assert.match(mainRs, /native_transcript_preview/);
  assert.match(mainRs, /!helper_line\.is_final/);
  assert.match(mainRs, /monitor_native_transcriber_exit/);
  assert.match(mainRs, /native_transcription\.process_exit/);
  assert.match(mainRs, /native_transcription\.bridge_error/);
  assert.match(mainRs, /serde_json::from_str::<serde_json::Value>\(&line\)/);
  assert.match(mainRs, /try_wait\(\)/);
  assert.match(mainRs, /source != "mic" && source != "system" && source != "mixed"/);
  assert.match(mainRs, /vec!\["mic"\.to_string\(\), "system"\.to_string\(\)\]/);
  assert.match(mainRs, /native_transcriber_key/);
  assert.match(mainRs, /key\.starts_with\(&prefix\)/);
  assert.match(mainRs, /libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(mainRs, /\.\.\/Frameworks/);
  assert.match(mainRs, /MacosSpeechReleaseContext/);
  assert.match(mainRs, /macos_speech_bridge_release_context/);
  assert.doesNotMatch(mainRs, /drop\(Box::from_raw\(context_ptr\)\)/);
  assert.doesNotMatch(mainRs, /stale\.context|session\.context/);
  assert.match(mainRs, /Stop Listening/);
  assert.match(mainRs, /stop_all_native_transcribers/);
  assert.match(mainRs, /set_listening_window_mode/);
  assert.match(mainRs, /set_always_on_top\(enabled\)/);
  assert.match(mainRs, /show_main_window\(&app\)/);
  assert.match(mainRs, /window\.unminimize\(\)/);
  assert.match(macHelper, /CGPreflightScreenCaptureAccess/);
  assert.match(macHelper, /CGRequestScreenCaptureAccess/);
  assert.match(macHelper, /NSApplication\.shared/);
  assert.match(macHelper, /request-screen-capture/);
  assert.match(macHelper, /screen capture permission is required for system audio/);
  assert.match(macHelper, /if let startupError/);
  assert.match(macHelper, /screenCaptureReady/);
  assert.match(macBridge, /meeting_copilot_native_speech_start/);
  assert.match(macBridge, /meeting_copilot_native_speech_stop/);
  assert.match(macBridge, /meeting_copilot_native_speech_health/);
  assert.match(macBridge, /meeting_copilot_native_speech_status/);
  assert.match(macBridge, /meeting_copilot_native_speech_request_permissions/);
  assert.match(macBridge, /NativeSpeechReleaseContext/);
  assert.match(macBridge, /deinit \{\s*releaseContext\?\(context\)/);
  assert.match(macBridge, /guard result\.isFinal \|\| text != lastText else \{ return \}/);
  assert.match(macBridge, /self\.stream = nil/);
  assert.match(macBridge, /CGPreflightScreenCaptureAccess/);
  assert.match(macBridge, /AVCaptureDevice\.requestAccess/);
  assert.match(macBridge, /microphone permission is required/);
  assert.match(macBridge, /SCStreamConfiguration/);
  assert.match(macBridge, /AVAudioEngine/);
  assert.match(mainRs, /Open Meeting Copilot/);
  assert.match(mainRs, /set_activation_policy\(tauri::ActivationPolicy::Regular\)/);
  assert.match(mainRs, /tauri::RunEvent::Reopen/);
  assert.match(mainRs, /WindowEvent::CloseRequested/);
  assert.equal(tauriConfig.app.windows[0].visible, true);
  assert.equal(tauriConfig.app.windows[0].transparent, true);
  assert.equal(tauriConfig.app.windows[0].width, 1080);
  assert.equal(tauriConfig.app.windows[0].height, 760);
  assert.equal(tauriConfig.app.windows[0].minWidth, 860);
  assert.equal(tauriConfig.app.windows[0].minHeight, 620);
  assert.equal(tauriConfig.app.withGlobalTauri, true);
  assert.deepEqual(tauriConfig.bundle.targets, ["app"]);
  assert.deepEqual(tauriConfig.bundle.icon, [
    "icons/32x32.png",
    "icons/128x128.png",
    "icons/128x128@2x.png",
    "icons/icon.icns",
    "icons/icon.ico"
  ]);
  assert.deepEqual(tauriConfig.bundle.externalBin, ["binaries/meeting-copilot-native-speech"]);
  assert.equal(tauriConfig.bundle.macOS.infoPlist, "Info.plist");
  assert.match(tauriConfigText, /infoPlist/);
  assert.match(macInfoPlist, /NSMicrophoneUsageDescription/);
  assert.match(macInfoPlist, /NSSpeechRecognitionUsageDescription/);
  assert.match(macInfoPlist, /NSScreenCaptureUsageDescription/);
  assert.match(macInfoPlist, /NSAudioCaptureUsageDescription/);
  assert.match(helperBuildScript, /target === "win32"/);
  assert.match(helperBuildScript, /native\/windows\/meeting-copilot-windows-speech\.rs/);
  assert.match(helperBuildScript, /meeting-copilot-native-speech-\$\{hostTriple\}\.exe/);
  assert.match(helperBuildScript, /libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(helperBuildScript, /MeetingCopilotSpeechBridge\.swift/);
  assert.match(helperBuildScript, /removeStaleMacHelperApp/);
  assert.match(helperBuildScript, /Meeting Copilot Speech\.app/);
  assert.match(helperBuildScript, /MEETING_COPILOT_CODESIGN_IDENTITY/);
  assert.match(helperBuildScript, /MEETING_COPILOT_CODESIGN_KEYCHAIN/);
  assert.match(helperBuildScript, /readDotEnvValue/);
  assert.match(helperBuildScript, /Meeting Copilot Local Code Signing/);
  assert.match(helperBuildScript, /find-identity/);
  assert.match(helperBuildScript, /loginKeychainPath/);
  assert.match(helperBuildScript, /resolveMacSigningKeychain/);
  assert.match(helperBuildScript, /firstValidCodesigningIdentity/);
  assert.match(helperBuildScript, /CSSMERR_/);
  assert.match(helperBuildScript, /codesign/);
  assert.doesNotMatch(helperBuildScript, /LSUIElement/);
  assert.match(helperInstallScript, /Contents\/Frameworks\/libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(helperInstallScript, /Contents\/Helpers\/Meeting Copilot Speech\.app/);
  assert.match(helperInstallScript, /rmSync/);
  assert.match(helperInstallScript, /MEETING_COPILOT_CODESIGN_IDENTITY/);
  assert.match(helperInstallScript, /MEETING_COPILOT_CODESIGN_KEYCHAIN/);
  assert.match(helperInstallScript, /readDotEnvValue/);
  assert.match(helperInstallScript, /Meeting Copilot Local Code Signing/);
  assert.match(helperInstallScript, /find-identity/);
  assert.match(helperInstallScript, /loginKeychainPath/);
  assert.match(helperInstallScript, /resolveMacSigningKeychain/);
  assert.match(helperInstallScript, /firstValidCodesigningIdentity/);
  assert.match(helperInstallScript, /CSSMERR_/);
  assert.match(helperInstallScript, /codesign/);
  assert.match(runWithRustScript, /process\.platform === "win32"/);
  assert.match(runWithRustScript, /MEETING_COPILOT_RUST_BIN/);
  assert.doesNotMatch(JSON.stringify(packageJson.scripts), /stable-aarch64-apple-darwin/);
  assert.match(packageJson.scripts["native:build"], /node scripts\/run-with-rust\.mjs npx tauri build --debug/);
  assert.match(packageJson.scripts["native:build"], /node scripts\/install-mac-helper-app\.mjs/);
  assert.match(packageJson.scripts["native:build:mac"], /node scripts\/install-mac-helper-app\.mjs/);
  assert.match(packageJson.scripts["native:build:windows"], /--bundles nsis,msi/);
  assert.match(packageJson.scripts["rust:test"], /node scripts\/run-with-rust\.mjs cargo test --workspace/);
  assert.equal(packageJson.dependencies?.["better-sqlite3"], undefined);
  assert.equal(packageJson.devDependencies?.["better-sqlite3"], "^12.9.0");
  assert.match(windowsHelper, /System\.Speech\.Recognition/);
  assert.match(windowsHelper, /WASAPI loopback capture started/);
  assert.match(windowsHelper, /IAudioCaptureClient/);
  assert.match(windowsHelper, /SetInputToAudioStream/);
  assert.match(windowsHelper, /source\\\":\\\"system/);
  assert.match(windowsHelper, /powershell\.exe/);
  assert.match(windowsHelper, /sanitize_language/);
  assert.match(windowsHelper, /sanitize_source/);
  assert.doesNotMatch(windowsHelper, /not enabled in this helper yet/);
});
