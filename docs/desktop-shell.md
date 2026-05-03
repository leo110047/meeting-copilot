# Desktop Shell Plan

Meeting Copilot is a dual-platform desktop product. The browser UI is only a development shell; the product entry point is the Tauri native status item / system tray app.

## Shared Invariant

- Domain core, storage schema, replay, provider interfaces, and policy logic are shared.
- UI shell commands are expressed through `PlatformShell`.
- STT provider and text decision provider remain separate.
- Floating overlay is optional and not the primary interface.

## macOS Shell

- Status surface: macOS status item.
- Suggestion surface: popover.
- Microphone capture: CoreAudio/native adapter.
- System audio capture: ScreenCaptureKit.
- Permission model: macOS TCC.

## Windows Shell

- Status surface: Windows system tray.
- Suggestion surface: flyout.
- Microphone capture: Windows native speech helper backed by Windows SpeechRecognition.
- System audio capture: WASAPI loopback contract; production capture/STT still needs Windows-runner validation.
- Permission model: Windows privacy settings.

## Current Verified State

- `src/platform/platformShell.mjs` contains macOS and Windows adapters with separate capabilities.
- `tests/platform-shell.test.mjs` verifies neither platform silently falls back to the other.
- `src-tauri` builds a Tauri app with a tray/status item, native menu actions, hidden-on-close window behavior, SQLite-backed Tauri commands, native microphone transcription commands, and generated platform icons.
- `tests/native-shell.test.mjs` verifies the native shell contract stays wired to tray/status item behavior.
