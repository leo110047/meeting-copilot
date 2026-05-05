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
    readFile(new URL("../src-tauri/src/desktop_types.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_core.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_audio.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/shell_storage.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/oauth_provider.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/native_storage.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/decision_logic.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/macos_speech_bridge.rs", import.meta.url), "utf8"),
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
  const desktopShellSource = [mainRoot, desktopTypes, commandsCore, commandsAudio, shellStorage, oauthProvider, nativeStorage, decisionLogic, macosSpeechBridge].join("\n");
  const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  const tauriConfig = JSON.parse(tauriConfigText);

  assert.match(cargoToml, /features = \["image-png", "tray-icon"\]/);
  assert.match(cargoToml, /\[target\.'cfg\(target_os = "windows"\)'\.dependencies\]/);
  assert.match(cargoToml, /Win32_UI_WindowsAndMessaging/);
  assert.match(desktopShellSource, /TrayIconBuilder::with_id\("meeting-copilot"\)/);
  assert.match(desktopShellSource, /#\[serde\(rename_all = "camelCase"\)\]\s*pub\(crate\) struct DesktopShellPlan/);
  assert.match(desktopShellSource, /start_native_transcription/);
  assert.match(desktopShellSource, /native_transcriber_health/);
  assert.match(desktopShellSource, /request_native_audio_permissions/);
  assert.match(desktopShellSource, /NativeTranscriberHealthRequest/);
  assert.match(desktopShellSource, /run_native_transcriber_health_check/);
  assert.match(desktopShellSource, /\.arg\("--health"\)/);
  assert.match(desktopShellSource, /\.arg\("--source"\)/);
  assert.match(desktopShellSource, /source == "mixed"/);
  assert.match(desktopShellSource, /request_screen_recording_permission/);
  assert.match(desktopShellSource, /macos_speech_bridge_path/);
  assert.match(desktopShellSource, /macos_speech_bridge_health/);
  assert.match(desktopShellSource, /macos_speech_bridge_status/);
  assert.match(desktopShellSource, /macos_speech_bridge_status_error/);
  assert.match(desktopShellSource, /screenSystemAudioPreflight=false/);
  assert.match(desktopShellSource, /statusBits=/);
  assert.match(desktopShellSource, /start_macos_speech_bridge/);
  assert.match(desktopShellSource, /start_macos_prep_dictation_bridge/);
  assert.match(desktopShellSource, /request_macos_speech_bridge_permissions/);
  assert.match(desktopShellSource, /x-apple\.systempreferences:com\.apple\.preference\.security\?Privacy_ScreenCapture/);
  assert.match(desktopShellSource, /text_provider_status/);
  assert.match(desktopShellSource, /start_text_provider_login/);
  assert.match(desktopShellSource, /start_subscription_oauth_login/);
  assert.match(desktopShellSource, /generate_ai_summary_oauth/);
  assert.match(desktopShellSource, /revise_transcript_oauth/);
  assert.match(desktopShellSource, /build_transcript_revision_prompt/);
  assert.match(desktopShellSource, /parse_transcript_revision_response/);
  assert.match(desktopShellSource, /generate_prep_summary_oauth/);
  assert.match(desktopShellSource, /build_prep_summary_prompt/);
  assert.match(desktopShellSource, /parse_prep_summary_points/);
  assert.match(desktopShellSource, /extract_live_state_patch_oauth/);
  assert.match(desktopShellSource, /build_live_state_patch_prompt/);
  assert.match(desktopShellSource, /parse_live_state_patch/);
  assert.match(desktopShellSource, /apply_live_state_patch/);
  assert.match(desktopShellSource, /log_extraction_failure/);
  assert.match(desktopShellSource, /provider attempted full rewrite field/);
  assert.match(desktopShellSource, /run_codex_oauth_prompt_with_timeout/);
  assert.match(desktopShellSource, /codex_command_path/);
  assert.match(desktopShellSource, /\.arg\("login"\)\.arg\("status"\)/);
  assert.match(desktopShellSource, /CachedTextProviderStatus/);
  assert.match(desktopShellSource, /SubscriptionOAuthParse::Unknown/);
  assert.match(desktopShellSource, /Duration::from_secs\(30\)/);
  assert.match(desktopShellSource, /create_private_oauth_temp_dir/);
  assert.match(desktopShellSource, /\/dev\/urandom/);
  assert.match(desktopShellSource, /fs::set_permissions/);
  assert.match(desktopShellSource, /trap cleanup EXIT/);
  assert.match(desktopShellSource, /\.arg\("login"\)/);
  assert.match(desktopShellSource, /\.arg\("exec"\)/);
  assert.match(desktopShellSource, /subscription_oauth/);
  assert.match(desktopShellSource, /read_dropped_context_files/);
  assert.match(desktopShellSource, /DROP_READ_GRANTS/);
  assert.match(desktopShellSource, /parse_subscription_oauth_authenticated/);
  assert.match(desktopShellSource, /log_provider_usage/);
  assert.match(desktopShellSource, /log_provider_failure/);
  assert.match(desktopShellSource, /app_error_logs/);
  assert.match(desktopShellSource, /log_app_error/);
  assert.match(desktopShellSource, /export_app_error_logs/);
  assert.match(desktopShellSource, /log_app_error_inner/);
  assert.match(desktopShellSource, /read_dropped_context_file/);
  assert.match(desktopShellSource, /set_window_opacity/);
  assert.match(desktopShellSource, /set_native_window_opacity/);
  assert.match(desktopShellSource, /setAlphaValue/);
  assert.match(desktopShellSource, /SetLayeredWindowAttributes/);
  assert.match(desktopShellSource, /WS_EX_LAYERED/);
  assert.match(desktopShellSource, /wasapi_capture\+wasapi_loopback/);
  assert.match(desktopShellSource, /windows-speech-native/);
  assert.match(desktopShellSource, /meeting-copilot-native-speech/);
  assert.match(desktopShellSource, /start_prep_dictation/);
  assert.match(desktopShellSource, /stop_prep_dictation/);
  assert.match(desktopShellSource, /prep_dictation_text/);
  assert.match(desktopShellSource, /native_transcript_preview/);
  assert.match(desktopShellSource, /!helper_line\.is_final/);
  assert.match(desktopShellSource, /monitor_native_transcriber_exit/);
  assert.match(desktopShellSource, /native_transcription\.process_exit/);
  assert.match(desktopShellSource, /native_transcription\.bridge_error/);
  assert.match(desktopShellSource, /serde_json::from_str::<serde_json::Value>\(&line\)/);
  assert.match(desktopShellSource, /try_wait\(\)/);
  assert.match(desktopShellSource, /source != "mic" && source != "system" && source != "mixed"/);
  assert.match(desktopShellSource, /vec!\["mic"\.to_string\(\), "system"\.to_string\(\)\]/);
  assert.match(desktopShellSource, /native_transcriber_key/);
  assert.match(desktopShellSource, /key\.starts_with\(&prefix\)/);
  assert.match(desktopShellSource, /libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(desktopShellSource, /\.\.\/Frameworks/);
  assert.match(desktopShellSource, /MacosSpeechReleaseContext/);
  assert.match(desktopShellSource, /macos_speech_bridge_release_context/);
  assert.doesNotMatch(desktopShellSource, /drop\(Box::from_raw\(context_ptr\)\)/);
  assert.doesNotMatch(desktopShellSource, /stale\.context|session\.context/);
  assert.match(desktopShellSource, /Stop Listening/);
  assert.match(desktopShellSource, /stop_all_native_transcribers/);
  assert.match(desktopShellSource, /set_listening_window_mode/);
  assert.match(desktopShellSource, /set_always_on_top\(enabled\)/);
  assert.match(desktopShellSource, /show_main_window\(&app\)/);
  assert.match(desktopShellSource, /window\.unminimize\(\)/);
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
  assert.match(desktopShellSource, /Open Meeting Copilot/);
  assert.match(desktopShellSource, /set_activation_policy\(tauri::ActivationPolicy::Regular\)/);
  assert.match(desktopShellSource, /tauri::RunEvent::Reopen/);
  assert.match(desktopShellSource, /WindowEvent::CloseRequested/);
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
