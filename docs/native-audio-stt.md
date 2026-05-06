# Native Audio And STT

Meeting Copilot keeps audio capture, STT, and text decision providers separate.

## Implemented

- macOS platform-speech transcription uses `native/macos/MeetingCopilotSpeechBridge.swift`, loaded in-process by the desktop app as `libmeeting_copilot_speech_bridge.dylib`.
- macOS local Whisper transcription uses the in-process `native/macos/MeetingCopilotSpeechBridge.swift` dylib so microphone and Screen/System Audio permissions belong to the visible `Meeting Copilot` app. It captures microphone audio with `AVAudioEngine`, captures system audio with `ScreenCaptureKit`, chunks PCM into short WAV files, and invokes the bundled `meeting-copilot-whisper` runner in persistent `--serve` mode.
- Microphone, Speech Recognition, and Screen/System Audio Recording permissions belong to the visible `Meeting Copilot` app instead of a hidden helper.
- Windows platform-speech microphone transcription uses `native/windows/meeting-copilot-windows-speech.rs` and Windows `System.Speech`.
- Windows local Whisper transcription uses the same native helper with WASAPI capture. Microphone uses the default capture endpoint; system audio uses WASAPI loopback from the default render endpoint. Both paths convert captured audio to 16 kHz mono PCM, chunk it as WAV, and invoke the bundled `meeting-copilot-whisper` runner.
- Tauri commands start/stop the helper and emit `native_transcript_preview` for partial text and `native_transcript_ingested` for final text.
- The live `mixed` source starts microphone and system-audio helpers in the same session, preserving each transcript event's source instead of downmixing audio into one opaque stream.
- Partial transcript previews update the UI drawer only. Final transcript events are cleaned through the enabled text provider when available, then persisted through the same SQLite decision loop as replay/manual fixtures.
- `src/audio/audioCaptureProvider.mjs` defines the shared `AudioFrame` contract and client-side VAD gate.
- `src/providers/nativeSttProvider.mjs` defines the native command STT adapter for tests and future bake-offs.
- The setup screen exposes local STT profiles through the native `local_stt_status_command` and `set_local_stt_profile_command` contract.
- `whisper-standard` is the default profile. `whisper-fast`, `whisper-standard`, and `whisper-accurate` require a Meeting Copilot compatible streaming Whisper runner plus the matching local model file.
- Missing Whisper runtime or model files are product errors. The app must block live listening instead of falling back to platform speech.
- Missing model files are recoverable inside the setup screen. The app exposes a download action that fetches the selected `ggml-*.bin` model from a commit-pinned whisper.cpp URL to a temporary file, streams progress to the UI, verifies the pinned SHA-256 checksum, then installs the file under the app data `Models/Whisper` folder.

## Platform Contracts

- macOS microphone: `AVAudioEngine/CoreAudio`.
- macOS system audio: `ScreenCaptureKit`.
- Windows microphone: native helper path backed by Windows SpeechRecognition for live transcript events.
- Windows system audio: native helper path backed by WASAPI loopback and Windows SpeechRecognition.
- Local Whisper runner: sidecar named `meeting-copilot-whisper` with Meeting Copilot JSON-line output. The app deliberately does not treat a generic one-shot `whisper-cli` as ready, because live meetings need Meeting Copilot's source-aware event contract.

## Boundaries

- Raw audio is not written to SQLite by default.
- STT provider output becomes `TranscriptEvent`.
- Text decision providers consume transcript/context and output extraction patches or decision suggestions.
- Subscription/OAuth text connectors do not handle STT.
- Whisper model files live outside the app bundle under the app data directory's `Models/Whisper` folder so users can choose quality and reclaim disk space without reinstalling the app.
