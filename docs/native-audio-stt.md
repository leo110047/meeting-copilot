# Native Audio And STT

Meeting Copilot keeps audio capture, STT, and text decision providers separate.

## Implemented

- macOS live transcription uses `native/macos/MeetingCopilotSpeechBridge.swift`, loaded in-process by the desktop app as `libmeeting_copilot_speech_bridge.dylib`.
- macOS microphone capture uses `AVAudioEngine` and `Speech` for native streaming STT.
- macOS system-audio capture uses `ScreenCaptureKit` audio sample buffers and the same `Speech` request pipeline.
- Microphone, Speech Recognition, and Screen/System Audio Recording permissions belong to the visible `Meeting Copilot` app instead of a hidden helper.
- Windows microphone live transcription uses `native/windows/meeting-copilot-windows-speech.rs` and Windows `System.Speech`.
- Windows system-audio live transcription uses WASAPI loopback from the default render endpoint, converts captured audio to 16 kHz mono PCM, and feeds it into Windows `SpeechRecognitionEngine`.
- Tauri commands start/stop the helper and emit `native_transcript_preview` for partial text and `native_transcript_ingested` for final text.
- The live `mixed` source starts microphone and system-audio helpers in the same session, preserving each transcript event's source instead of downmixing audio into one opaque stream.
- Partial transcript previews update the UI drawer only. Final transcript events are cleaned through the enabled text provider when available, then persisted through the same SQLite decision loop as replay/manual fixtures.
- `src/audio/audioCaptureProvider.mjs` defines the shared `AudioFrame` contract and client-side VAD gate.
- `src/providers/nativeSttProvider.mjs` defines the native command STT adapter for tests and future bake-offs.

## Platform Contracts

- macOS microphone: `AVAudioEngine/CoreAudio`.
- macOS system audio: `ScreenCaptureKit`.
- Windows microphone: native helper path backed by Windows SpeechRecognition for live transcript events.
- Windows system audio: native helper path backed by WASAPI loopback and Windows SpeechRecognition.

## Boundaries

- Raw audio is not written to SQLite by default.
- STT provider output becomes `TranscriptEvent`.
- Text decision providers consume transcript/context and output extraction patches or decision suggestions.
- Subscription/OAuth text connectors do not handle STT.
