import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("Tauri native shell is tray/status-item first", async () => {
  const [mainRs, cargoToml, tauriConfigText, helperBuildScript, runWithRustScript, windowsHelper] = await Promise.all([
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    readFile(new URL("../scripts/build-native-helpers.mjs", import.meta.url), "utf8"),
    readFile(new URL("../scripts/run-with-rust.mjs", import.meta.url), "utf8"),
    readFile(new URL("../native/windows/meeting-copilot-windows-speech.rs", import.meta.url), "utf8")
  ]);
  const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  const tauriConfig = JSON.parse(tauriConfigText);

  assert.match(cargoToml, /features = \["image-png", "tray-icon"\]/);
  assert.match(cargoToml, /\[target\.'cfg\(target_os = "windows"\)'\.dependencies\]/);
  assert.match(cargoToml, /Win32_UI_WindowsAndMessaging/);
  assert.match(mainRs, /TrayIconBuilder::with_id\("meeting-copilot"\)/);
  assert.match(mainRs, /start_native_transcription/);
  assert.match(mainRs, /native_transcriber_health/);
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
  assert.match(mainRs, /source != "mic" && source != "system"/);
  assert.match(mainRs, /Stop Listening/);
  assert.match(mainRs, /stop_all_native_transcribers/);
  assert.match(mainRs, /set_listening_window_mode/);
  assert.match(mainRs, /set_always_on_top\(enabled\)/);
  assert.match(mainRs, /show_main_window\(&app\)/);
  assert.match(mainRs, /window\.unminimize\(\)/);
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
  assert.match(helperBuildScript, /target === "win32"/);
  assert.match(helperBuildScript, /native\/windows\/meeting-copilot-windows-speech\.rs/);
  assert.match(helperBuildScript, /meeting-copilot-native-speech-\$\{hostTriple\}\.exe/);
  assert.match(runWithRustScript, /process\.platform === "win32"/);
  assert.match(runWithRustScript, /MEETING_COPILOT_RUST_BIN/);
  assert.doesNotMatch(JSON.stringify(packageJson.scripts), /stable-aarch64-apple-darwin/);
  assert.match(packageJson.scripts["native:build"], /node scripts\/run-with-rust\.mjs npx tauri build --debug/);
  assert.match(packageJson.scripts["native:build:windows"], /--bundles nsis,msi/);
  assert.match(packageJson.scripts["rust:test"], /node scripts\/run-with-rust\.mjs cargo test --workspace/);
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
