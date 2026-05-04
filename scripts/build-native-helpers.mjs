import { mkdirSync, rmSync } from "node:fs";
import { platform } from "node:os";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const target = platform();
const host = detectRustHost();
const macSigningIdentity = process.env.MEETING_COPILOT_CODESIGN_IDENTITY || "-";
const macSigningKeychain = process.env.MEETING_COPILOT_CODESIGN_KEYCHAIN;

if (target === "darwin") {
  removeStaleMacHelperApp();
  buildMacHelper(host);
  buildMacSpeechBridge();
  process.exit(0);
}

if (target === "win32") {
  buildWindowsHelper(host);
  process.exit(0);
}

console.log(`No native helper build needed on ${target}.`);

function removeStaleMacHelperApp() {
  const staleHelperApp = resolve("src-tauri/binaries/Meeting Copilot Speech.app");
  rmSync(staleHelperApp, { recursive: true, force: true });
}

function buildMacHelper(hostTriple) {
  const output = resolve(`src-tauri/binaries/meeting-copilot-native-speech-${hostTriple}`);
  mkdirSync(dirname(output), { recursive: true });
  const result = spawnSync("swiftc", [
    // The standalone helper uses NSApplication for the Screen Recording prompt flow.
    "-framework", "AppKit",
    "-framework", "Foundation",
    "-framework", "AVFoundation",
    "-framework", "CoreMedia",
    "-framework", "ScreenCaptureKit",
    "-framework", "Speech",
    "native/macos/MeetingCopilotSpeech.swift",
    "-o",
    output
  ], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  console.log(`Built native helper: ${output}`);
}

function buildMacSpeechBridge() {
  const output = resolve("src-tauri/binaries/libmeeting_copilot_speech_bridge.dylib");
  mkdirSync(dirname(output), { recursive: true });
  const result = spawnSync("swiftc", [
    "-emit-library",
    "-module-name", "MeetingCopilotSpeechBridge",
    "-framework", "Foundation",
    "-framework", "AVFoundation",
    "-framework", "CoreGraphics",
    "-framework", "CoreMedia",
    "-framework", "ScreenCaptureKit",
    "-framework", "Speech",
    "native/macos/MeetingCopilotSpeechBridge.swift",
    "-o",
    output
  ], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  const signArgs = ["--force"];
  if (macSigningKeychain) signArgs.push("--keychain", macSigningKeychain);
  signArgs.push("--sign", macSigningIdentity, output);
  const signResult = spawnSync("codesign", signArgs, { stdio: "inherit" });
  if (signResult.status !== 0) process.exit(signResult.status ?? 1);
  console.log(`Built macOS speech bridge: ${output}`);
}

function buildWindowsHelper(hostTriple) {
  const output = resolve(`src-tauri/binaries/meeting-copilot-native-speech-${hostTriple}.exe`);
  mkdirSync(dirname(output), { recursive: true });
  const result = spawnSync("rustc", [
    "native/windows/meeting-copilot-windows-speech.rs",
    "-O",
    "-o",
    output
  ], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  console.log(`Built native helper: ${output}`);
}

function detectRustHost() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  if (result.status !== 0) {
    if (target === "darwin") return process.arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
    if (target === "win32") return process.arch === "arm64" ? "aarch64-pc-windows-msvc" : "x86_64-pc-windows-msvc";
    return `${process.arch}-${target}`;
  }
  const hostLine = result.stdout.split("\n").find((line) => line.startsWith("host: "));
  return hostLine?.slice("host: ".length).trim() ?? `${process.arch}-${target}`;
}
