import assert from "node:assert/strict";
import test from "node:test";
import { createPlatformShell, detectPlatformShell } from "../src/platform/platformShell.mjs";

test("macOS shell uses status item and ScreenCaptureKit plan", async () => {
  const shell = createPlatformShell("macos");
  assert.equal(shell.platform, "macos");
  assert.equal(shell.capabilities.statusSurface, "macos_status_item");
  assert.equal(shell.capabilities.systemAudioCapture, "screencapturekit");
  const command = await shell.showSuggestion({ id: "move1", text: "先確認 owner" });
  assert.equal(command.surface, "popover");
});

test("Windows shell uses system tray and exposes WASAPI loopback STT", async () => {
  const shell = createPlatformShell("windows");
  assert.equal(shell.platform, "windows");
  assert.equal(shell.capabilities.statusSurface, "windows_system_tray");
  assert.equal(shell.capabilities.systemAudioCapture, "wasapi_loopback");
  const command = await shell.showSuggestion({ id: "move1", text: "先確認 owner" });
  assert.equal(command.surface, "flyout");
});

test("host platform detection does not silently fallback to macOS", () => {
  assert.equal(detectPlatformShell("darwin").platform, "macos");
  assert.equal(detectPlatformShell("win32").platform, "windows");
  assert.throws(() => detectPlatformShell("linux"), /unsupported host platform/);
});
