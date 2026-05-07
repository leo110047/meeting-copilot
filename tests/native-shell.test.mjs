import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

function renderWhisperCSharp(windowsHelper) {
  const startToken = "-TypeDefinition @'";
  const start = windowsHelper.indexOf(startToken);
  assert.notEqual(start, -1);
  const bodyStart = start + startToken.length;
  const end = windowsHelper.indexOf("'@", bodyStart);
  assert.notEqual(end, -1);
  return windowsHelper
    .slice(bodyStart, end)
    .replaceAll("{{", "{")
    .replaceAll("}}", "}");
}

function maskStrings(source) {
  let output = "";
  let quote;
  let escaping = false;
  for (const char of source) {
    if (quote) {
      if (escaping) {
        escaping = false;
      } else if (char === "\\") {
        escaping = true;
      } else if (char === quote) {
        quote = undefined;
      }
      output += char === "\n" ? "\n" : " ";
    } else if (char === "\"" || char === "'") {
      quote = char;
      output += " ";
    } else {
      output += char;
    }
  }
  return output;
}

function assertBalancedBraces(source) {
  const masked = maskStrings(source);
  let depth = 0;
  for (let index = 0; index < masked.length; index += 1) {
    if (masked[index] === "{") depth += 1;
    if (masked[index] === "}") depth -= 1;
    assert.ok(depth >= 0, `brace depth went negative at ${index}`);
  }
  assert.equal(depth, 0);
}

function extractCSharpBlock(source, marker) {
  const masked = maskStrings(source);
  const markerIndex = source.indexOf(marker);
  assert.notEqual(markerIndex, -1);
  const open = masked.indexOf("{", markerIndex);
  assert.notEqual(open, -1);
  let depth = 0;
  for (let index = open; index < masked.length; index += 1) {
    if (masked[index] === "{") depth += 1;
    if (masked[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(open + 1, index);
  }
  assert.fail(`missing closing brace for ${marker}`);
}

test("Tauri native shell is tray/status-item first", async () => {
  const [
    mainRoot,
    desktopTypes,
    commandsCore,
    commandsAudio,
    shellStorage,
    oauthProvider,
    nativeStorage,
    localStt,
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
    windowsHelper,
    whisperRunner
  ] = await Promise.all([
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/desktop_types.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_core.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands_audio.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/shell_storage.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/oauth_provider.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/native_storage.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/local_stt.rs", import.meta.url), "utf8"),
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
    readFile(new URL("../native/windows/meeting-copilot-windows-speech.rs", import.meta.url), "utf8"),
    readFile(new URL("../crates/meeting-copilot-whisper/src/main.rs", import.meta.url), "utf8")
  ]);
  const desktopShellSource = [mainRoot, desktopTypes, commandsCore, commandsAudio, shellStorage, oauthProvider, nativeStorage, localStt, decisionLogic, macosSpeechBridge].join("\n");
  const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  const tauriConfig = JSON.parse(tauriConfigText);

  assert.match(cargoToml, /features = \["image-png", "tray-icon"\]/);
  assert.match(cargoToml, /unicode-segmentation/);
  assert.match(cargoToml, /\[target\.'cfg\(target_os = "windows"\)'\.dependencies\]/);
  assert.match(cargoToml, /Win32_UI_WindowsAndMessaging/);
  assert.match(desktopShellSource, /TrayIconBuilder::with_id\("meeting-copilot"\)/);
  assert.match(desktopShellSource, /#\[serde\(rename_all = "camelCase"\)\]\s*pub\(crate\) struct DesktopShellPlan/);
  assert.match(desktopShellSource, /start_native_transcription/);
  assert.match(desktopShellSource, /native_transcriber_health/);
  assert.match(desktopShellSource, /request_native_audio_permissions/);
  assert.match(desktopShellSource, /local_stt_status_command/);
  assert.match(desktopShellSource, /set_local_stt_profile_command/);
  assert.match(desktopShellSource, /download_local_stt_model_command/);
  assert.match(desktopShellSource, /local_stt_model_download_progress/);
  assert.match(desktopShellSource, /open_local_stt_model_folder/);
  assert.match(desktopShellSource, /whisper-standard/);
  assert.match(desktopShellSource, /resolve\/5359861c739e955e79d9a303bcbc70fb988958b1\/ggml-small\.bin/);
  assert.match(desktopShellSource, /60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe/);
  assert.match(desktopShellSource, /1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b/);
  assert.match(desktopShellSource, /6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208/);
  assert.match(desktopShellSource, /localWhisperEngineMissing/);
  assert.doesNotMatch(desktopShellSource, /localWhisperEngineUnavailable/);
  assert.match(commandsAudio, /\.arg\("--engine"\)\s*\.arg\("whisper"\)/);
  assert.match(commandsAudio, /\.arg\("--whisper-runner"\)/);
  assert.match(commandsAudio, /\.arg\("--whisper-model"\)/);
  assert.match(commandsAudio, /\.arg\("--stop-file"\)/);
  assert.doesNotMatch(commandsAudio, /write_all\(b"stop\\n"\)/);
  assert.match(commandsAudio, /Closing stdin is the stop signal/);
  assert.match(commandsAudio, /source == "mixed" && whisper_runtime\.is_none\(\)/);
  assert.match(commandsAudio, /helper_source == "mixed" && whisper_runtime\.is_some\(\)/);
  assert.match(desktopShellSource, /ManagedNativeTranscriber/);
  assert.match(desktopShellSource, /request_macos_audio_bridge_permissions/);
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
  assert.match(desktopShellSource, /open_text_provider_install_guide/);
  assert.match(desktopShellSource, /connector_installed/);
  assert.match(desktopShellSource, /npm install -g @openai\/codex/);
  assert.match(desktopShellSource, /npm install -g @anthropic-ai\/claude-code/);
  assert.match(desktopShellSource, /help\.openai\.com\/en\/articles\/11096431-openai-codex-cli-getting-started/);
  assert.match(desktopShellSource, /docs\.anthropic\.com\/en\/docs\/claude-code\/getting-started/);
  assert.match(desktopShellSource, /start_text_provider_login/);
  assert.match(desktopShellSource, /set_session_text_provider/);
  assert.match(desktopShellSource, /CLAUDE_TEXT_PROVIDER_ID/);
  assert.match(desktopShellSource, /run_claude_subscription_prompt_with_timeout/);
  assert.match(desktopShellSource, /claude_command_path/);
  assert.match(desktopShellSource, /parse_claude_auth_status/);
  assert.match(desktopShellSource, /parse_claude_print_result/);
  assert.match(desktopShellSource, /start_subscription_oauth_login/);
  assert.match(desktopShellSource, /generate_ai_summary_oauth/);
  assert.match(desktopShellSource, /revise_transcript_oauth/);
  assert.match(desktopShellSource, /build_transcript_revision_prompt/);
  assert.match(desktopShellSource, /parse_transcript_revision_response/);
  assert.match(desktopShellSource, /generate_prep_summary_oauth/);
  assert.match(desktopShellSource, /build_prep_summary_prompt/);
  assert.match(desktopShellSource, /parse_prep_summary_points/);
  assert.match(desktopShellSource, /extract_live_state_patch_oauth/);
  assert.match(desktopTypes, /live_evidence_event: Option<TranscriptEvent>/);
  assert.match(commandsCore, /event: None/);
  assert.match(commandsCore, /live_evidence_event: Some\(last_event\)/);
  assert.match(commandsAudio, /event: Some\(event\)/);
  assert.match(desktopShellSource, /build_live_state_patch_prompt/);
  assert.match(desktopShellSource, /parse_live_state_patch/);
  assert.match(desktopShellSource, /parse_live_coaching_suggestions/);
  assert.match(desktopShellSource, /suggested_move/);
  assert.match(desktopShellSource, /watch_out/);
  assert.match(desktopShellSource, /coaching_error/);
  assert.match(desktopShellSource, /coaching_cards_discarded/);
  assert.match(desktopShellSource, /Return at most one high-value coaching card/);
  assert.doesNotMatch(commandsAudio, /derive_suggestions/);
  assert.match(desktopShellSource, /apply_live_state_patch/);
  assert.match(desktopShellSource, /log_extraction_failure/);
  assert.match(desktopShellSource, /provider attempted full rewrite field/);
  assert.match(desktopShellSource, /run_codex_oauth_prompt_with_timeout/);
  assert.match(desktopShellSource, /run_text_provider_prompt_with_timeout/);
  assert.match(desktopShellSource, /TEXT_PROVIDER_STATUS_CACHE/);
  assert.match(desktopShellSource, /clear_text_provider_status_cache/);
  assert.match(desktopShellSource, /codex_command_path/);
  assert.match(desktopShellSource, /\.arg\("login"\)\.arg\("status"\)/);
  assert.match(desktopShellSource, /CachedTextProviderStatus/);
  assert.match(desktopShellSource, /SubscriptionOAuthParse::Unknown/);
  assert.match(desktopShellSource, /Duration::from_secs\(30\)/);
  assert.match(desktopShellSource, /create_private_oauth_temp_dir/);
  assert.match(desktopShellSource, /\/dev\/urandom/);
  assert.match(desktopShellSource, /fs::set_permissions/);
  assert.match(desktopShellSource, /Press Return to close this window/);
  assert.match(desktopShellSource, /\.arg\("login"\)/);
  assert.match(desktopShellSource, /\.arg\("-p"\)/);
  assert.match(desktopShellSource, /\.arg\("--tools"\)/);
  assert.match(desktopShellSource, /\.arg\("--no-session-persistence"\)/);
  assert.match(desktopShellSource, /\.arg\("--no-chrome"\)/);
  assert.match(desktopShellSource, /subscription_oauth/);
  assert.match(desktopShellSource, /record_session_text_provider/);
  assert.match(desktopShellSource, /llmProviders/);
  assert.match(desktopShellSource, /providerChanges/);
  assert.match(desktopShellSource, /read_dropped_context_files/);
  assert.match(desktopShellSource, /DROP_READ_GRANTS/);
  assert.match(desktopShellSource, /parse_subscription_oauth_authenticated/);
  assert.match(desktopShellSource, /log_provider_usage/);
  assert.match(desktopShellSource, /log_provider_failure/);
  assert.match(desktopShellSource, /suggestion_json/);
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
  assert.match(desktopShellSource, /native_transcription\.bridge_diagnostic/);
  assert.match(desktopShellSource, /audio_diagnostic/);
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
  assert.doesNotMatch(desktopShellSource, /set_always_on_top/);
  assert.match(desktopShellSource, /show_main_window\(&app\)/);
  assert.match(desktopShellSource, /window\.unminimize\(\)/);
  assert.match(macHelper, /CGPreflightScreenCaptureAccess/);
  assert.match(macHelper, /CGRequestScreenCaptureAccess/);
  assert.match(macHelper, /NSApplication\.shared/);
  assert.match(macHelper, /request-screen-capture/);
  assert.match(macHelper, /screen capture permission is required for system audio/);
  assert.match(macHelper, /if let startupError/);
  assert.match(macHelper, /screenCaptureReady/);
  assert.match(macHelper, /final class WhisperChunkTranscriber/);
  assert.match(macHelper, /final class MicWhisperStreamer/);
  assert.match(macHelper, /final class SystemAudioWhisperStreamer/);
  assert.match(macHelper, /--whisper-runner/);
  assert.match(macBridge, /meeting_copilot_native_speech_start/);
  assert.match(macBridge, /source == "mic" \|\| source == "system" \|\| source == "mixed"/);
  assert.match(macBridge, /meeting_copilot_native_speech_stop/);
  assert.match(macBridge, /meeting_copilot_native_speech_health/);
  assert.match(macBridge, /meeting_copilot_native_speech_status/);
  assert.match(macBridge, /meeting_copilot_native_speech_request_permissions/);
  assert.match(macBridge, /NativeSpeechReleaseContext/);
  assert.match(macBridge, /deinit \{\s*stopSynchronouslyForDeinit\(\)\s*releaseContext\?\(context\)/);
  assert.match(macBridge, /guard result\.isFinal \|\| text != lastText else \{ return \}/);
  assert.match(macBridge, /startRecognitionTask/);
  assert.match(macBridge, /createRecognitionRequest/);
  assert.match(macBridge, /meeting-copilot\.recognition-state/);
  assert.match(macBridge, /finishRecognitionOnQueue/);
  assert.match(macBridge, /recognitionGeneration/);
  assert.match(macBridge, /appendSystemAudioSampleBuffer/);
  assert.match(macBridge, /copyAudioPCMBuffer/);
  assert.match(macBridge, /memcpy/);
  assert.match(macBridge, /meeting_copilot_native_speech_start_whisper/);
  assert.match(macBridge, /source == "mic" \|\| source == "system" \|\| source == "mixed"/);
  assert.match(macBridge, /startMixedCapture/);
  assert.match(macBridge, /WhisperSourceBuffer/);
  assert.match(macBridge, /appendingPathComponent\("meeting-copilot-whisper-\\\(bridgeId\)-\\\(source\)-\\\(chunkIndex\)\.wav"\)/);
  assert.match(macBridge, /meeting_copilot_native_audio_request_permissions/);
  assert.match(macBridge, /final class NativeWhisperBridge/);
  assert.match(macBridge, /"--serve", "--model"/);
  assert.match(macBridge, /meeting-copilot\.whisper-bridge\.capture/);
  assert.match(macBridge, /meeting-copilot\.whisper-bridge\.transcription/);
  assert.match(macBridge, /DispatchSpecificKey<Bool>/);
  assert.match(macBridge, /captureIsStopping/);
  assert.match(macBridge, /local_whisper_pipeline/);
  assert.match(macBridge, /local_whisper_chunk_dropped/);
  assert.match(macBridge, /local_whisper_chunk_dispatched/);
  assert.match(macBridge, /local_whisper_silence_dropped/);
  assert.match(macBridge, /finishResidualBufferLocked/);
  assert.match(macBridge, /flushLocked\(source: source, force: true\)/);
  assert.match(macBridge, /local_whisper_residual_flushed/);
  assert.match(macBridge, /local_whisper_residual_ignored/);
  assert.doesNotMatch(macBridge, /dropResidualBufferLocked/);
  assert.doesNotMatch(macBridge, /local_whisper_residual_dropped/);
  assert.match(macBridge, /setpriority\(PRIO_PROCESS/);
  assert.match(macBridge, /Lower runner priority to keep the capture queue responsive under load/);
  assert.match(macBridge, /local_whisper_priority_failed/);
  assert.match(macBridge, /always: true/);
  assert.match(macBridge, /isLocalWhisperRunnerDiagnostic/);
  assert.match(macBridge, /whisper_backend_init:/);
  assert.match(whisperRunner, /set_n_threads\(whisper_thread_count\(\)\)/);
  assert.doesNotMatch(whisperRunner, /set_single_segment\(true\)/);
  assert.doesNotMatch(whisperRunner, /set_temperature_inc\(0\.0\)/);
  assert.match(whisperRunner, /MEETING_COPILOT_WHISPER_THREADS/);
  assert.match(whisperRunner, /audio chunk is below speech energy threshold/);
  assert.match(macosSpeechBridge, /bridgeSource/);
  assert.match(macosSpeechBridge, /audio_diagnostic_severity/);
  assert.match(macosSpeechBridge, /local_whisper_audio_dropped" \| "local_whisper_chunk_dropped" => "warning"/);
  assert.match(macBridge, /waitForRunnerExit/);
  assert.match(macBridge, /pending\.append\(text\)/);
  assert.match(macBridge, /BridgeDiagnosticLine/);
  assert.match(macBridge, /measureAudioLevel/);
  assert.match(macBridge, /MEETING_COPILOT_AUDIO_DIAGNOSTICS/);
  assert.match(macBridge, /audio_diagnostic/);
  assert.match(macBridge, /audio_input_level/);
  assert.match(macBridge, /handleRecognitionError/);
  assert.match(macBridge, /restartRecognitionAfterRecoverableError/);
  assert.match(macBridge, /code == "no_speech_detected"/);
  assert.match(macBridge, /lastRecoverableRestartAt/);
  assert.match(macBridge, /recognitionRestartWorkItem\?\.cancel\(\)/);
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
  assert.deepEqual(tauriConfig.bundle.externalBin, ["binaries/meeting-copilot-native-speech", "binaries/meeting-copilot-whisper"]);
  assert.equal(tauriConfig.bundle.macOS.infoPlist, "Info.plist");
  assert.match(tauriConfigText, /infoPlist/);
  assert.match(macInfoPlist, /NSMicrophoneUsageDescription/);
  assert.match(macInfoPlist, /NSSpeechRecognitionUsageDescription/);
  assert.match(macInfoPlist, /NSScreenCaptureUsageDescription/);
  assert.match(macInfoPlist, /NSAudioCaptureUsageDescription/);
  assert.match(helperBuildScript, /target === "win32"/);
  assert.match(helperBuildScript, /native\/windows\/meeting-copilot-windows-speech\.rs/);
  assert.match(helperBuildScript, /meeting-copilot-native-speech-\$\{hostTriple\}\.exe/);
  assert.match(helperBuildScript, /meeting-copilot-whisper/);
  assert.match(helperBuildScript, /--package"[\s\S]*"meeting-copilot-whisper"/);
  assert.match(helperBuildScript, /libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(helperBuildScript, /MeetingCopilotSpeechBridge\.swift/);
  assert.match(helperBuildScript, /removeStaleMacHelperApp/);
  assert.match(helperBuildScript, /Meeting Copilot Speech\.app/);
  assert.match(helperBuildScript, /MEETING_COPILOT_CODESIGN_IDENTITY/);
  assert.match(helperBuildScript, /MEETING_COPILOT_CODESIGN_KEYCHAIN/);
  assert.match(helperBuildScript, /MEETING_COPILOT_DISTRIBUTION_SIGNING/);
  assert.match(helperBuildScript, /MEETING_COPILOT_ALLOW_ADHOC_SIGNING/);
  assert.match(helperBuildScript, /MEETING_COPILOT_VERBOSE_SIGNING/);
  assert.match(helperBuildScript, /readDotEnvValue/);
  assert.match(helperBuildScript, /find-identity/);
  assert.match(helperBuildScript, /loginKeychainPath/);
  assert.match(helperBuildScript, /resolveMacSigningKeychain/);
  assert.match(helperBuildScript, /resolveConfiguredMacSigningIdentity/);
  assert.match(helperBuildScript, /firstValidCodesigningIdentity/);
  assert.match(helperBuildScript, /Developer ID Application/);
  assert.match(helperBuildScript, /isDeveloperIdApplicationIdentity/);
  assert.match(helperBuildScript, /verifyMacBundle/);
  assert.match(helperBuildScript, /CSSMERR_/);
  assert.match(helperBuildScript, /codesign/);
  assert.doesNotMatch(helperBuildScript, /Meeting Copilot Local Code Signing/);
  assert.doesNotMatch(helperBuildScript, /LSUIElement/);
  assert.match(helperInstallScript, /Contents\/Frameworks\/libmeeting_copilot_speech_bridge\.dylib/);
  assert.match(helperInstallScript, /Contents\/Helpers\/Meeting Copilot Speech\.app/);
  assert.match(helperInstallScript, /rmSync/);
  assert.match(helperInstallScript, /MEETING_COPILOT_CODESIGN_IDENTITY/);
  assert.match(helperInstallScript, /MEETING_COPILOT_CODESIGN_KEYCHAIN/);
  assert.match(helperInstallScript, /MEETING_COPILOT_DISTRIBUTION_SIGNING/);
  assert.match(helperInstallScript, /MEETING_COPILOT_ALLOW_ADHOC_SIGNING/);
  assert.match(helperInstallScript, /MEETING_COPILOT_VERBOSE_SIGNING/);
  assert.match(helperInstallScript, /MEETING_COPILOT_MAC_BUNDLE_PATH/);
  assert.match(helperInstallScript, /readDotEnvValue/);
  assert.match(helperInstallScript, /find-identity/);
  assert.match(helperInstallScript, /loginKeychainPath/);
  assert.match(helperInstallScript, /resolveMacSigningKeychain/);
  assert.match(helperInstallScript, /resolveConfiguredMacSigningIdentity/);
  assert.match(helperInstallScript, /firstValidCodesigningIdentity/);
  assert.match(helperInstallScript, /Developer ID Application/);
  assert.match(helperInstallScript, /verifyMacBundle/);
  assert.match(helperInstallScript, /CSSMERR_/);
  assert.match(helperInstallScript, /codesign/);
  assert.doesNotMatch(helperInstallScript, /Meeting Copilot Local Code Signing/);
  assert.match(runWithRustScript, /process\.platform === "win32"/);
  assert.match(runWithRustScript, /MEETING_COPILOT_RUST_BIN/);
  assert.doesNotMatch(JSON.stringify(packageJson.scripts), /stable-aarch64-apple-darwin/);
  assert.match(packageJson.scripts["native:build"], /node scripts\/run-with-rust\.mjs npx tauri build --debug/);
  assert.match(packageJson.scripts["native:build"], /node scripts\/install-mac-helper-app\.mjs/);
  assert.match(packageJson.scripts["native:build:mac"], /node scripts\/install-mac-helper-app\.mjs/);
  assert.match(packageJson.scripts["native:build:mac:release"], /MEETING_COPILOT_DISTRIBUTION_SIGNING=1/);
  assert.doesNotMatch(packageJson.scripts["native:build:mac:release"], /export MEETING_COPILOT_DISTRIBUTION_SIGNING/);
  assert.match(packageJson.scripts["native:build:mac:release"], /--bundles app,dmg/);
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
  assert.match(windowsHelper, /public static class WhisperLoop/);
  assert.match(windowsHelper, /WasapiPcmCapture/);
  assert.match(windowsHelper, /--whisper-runner/);
  assert.match(windowsHelper, /--stop-file/);
  assert.match(windowsHelper, /source == "mixed"/);
  assert.match(windowsHelper, /CaptureLane/);
  assert.match(windowsHelper, /lanes\.All\(lane => lane\.Completed\)/);
  assert.match(windowsHelper, /meeting-copilot-whisper-" \+ Process\.GetCurrentProcess\(\)\.Id \+ "-" \+ source \+ "-" \+ index/);
  assert.match(windowsHelper, /WaitForExit\(10000\)/);
  assert.doesNotMatch(windowsHelper, /not enabled in this helper yet/);
  const csharp = renderWhisperCSharp(windowsHelper);
  assertBalancedBraces(csharp);
  const whisperLoopBlock = extractCSharpBlock(csharp, "public static class WhisperLoop");
  assert.match(whisperLoopBlock, /private static void WriteWav/);
  assert.match(whisperLoopBlock, /private static string QuoteArgument/);
  assert.match(whisperLoopBlock, /private static string JsonEscape/);
  const runnerBlock = extractCSharpBlock(csharp, "public sealed class PersistentWhisperRunner");
  assert.match(runnerBlock, /public void Close\(\)/);
  assert.match(runnerBlock, /private static void Forward/);
 });
