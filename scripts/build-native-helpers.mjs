import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { platform } from "node:os";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const target = platform();
const host = detectRustHost();
const distributionSigning = readBooleanConfig("MEETING_COPILOT_DISTRIBUTION_SIGNING");
const allowAdhocSigning = readBooleanConfig("MEETING_COPILOT_ALLOW_ADHOC_SIGNING");
const verboseSigning = readBooleanConfig("MEETING_COPILOT_VERBOSE_SIGNING");
const macSigningIdentity = resolveMacSigningIdentity();
const macSigningKeychain = resolveMacSigningKeychain();
if (target === "darwin" && verboseSigning) console.log(`Using macOS codesign identity: ${macSigningIdentity}`);

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
  logVerbose(`Built native helper: ${output}`);
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
  verifyMacBundle(output);
  logVerbose(`Built macOS speech bridge: ${output}`);
}

function verifyMacBundle(bundlePath) {
  const result = spawnSync("codesign", ["--verify", "--deep", "--strict", "--verbose=2", bundlePath], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
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
  logVerbose(`Built native helper: ${output}`);
}

function logVerbose(message) {
  if (verboseSigning) console.log(message);
}

function resolveMacSigningIdentity() {
  const configured = process.env.MEETING_COPILOT_CODESIGN_IDENTITY ?? readDotEnvValue("MEETING_COPILOT_CODESIGN_IDENTITY");
  if (target !== "darwin") return "-";
  const result = spawnSync("security", ["find-identity", "-v", "-p", "codesigning", loginKeychainPath()], { encoding: "utf8" });
  if (configured) {
    assertDistributionIdentityIfNeeded(result.stdout, configured);
    return resolveConfiguredMacSigningIdentity(result.stdout, configured);
  }
  if (distributionSigning) {
    const developerIdIdentity = firstValidCodesigningIdentity(result.stdout, undefined, isDeveloperIdApplicationIdentity);
    if (developerIdIdentity) return developerIdIdentity;
    throw new Error("macOS distribution builds require a valid Developer ID Application signing identity. Set MEETING_COPILOT_CODESIGN_IDENTITY to that identity's SHA-1 hash or certificate name.");
  }
  const fallbackIdentity = firstValidCodesigningIdentity(result.stdout);
  if (fallbackIdentity) return fallbackIdentity;
  assertAdhocSigningAllowed();
  return "-";
}

function resolveConfiguredMacSigningIdentity(output, configuredIdentity) {
  if (configuredIdentity === "-") {
    assertAdhocSigningAllowed();
    return configuredIdentity;
  }
  const match = findCodesigningIdentity(output, configuredIdentity);
  return match?.hash ?? configuredIdentity;
}

function assertAdhocSigningAllowed() {
  if (allowAdhocSigning) return;
  throw new Error("macOS native audio builds require a stable code-signing identity so Screen Recording/System Audio permissions stay attached to the app. Set MEETING_COPILOT_CODESIGN_IDENTITY to an Apple Development or Developer ID Application SHA-1 hash, or set MEETING_COPILOT_ALLOW_ADHOC_SIGNING=1 only for throwaway local builds.");
}

function loginKeychainPath() {
  return `${process.env.HOME}/Library/Keychains/login.keychain-db`;
}

function resolveMacSigningKeychain() {
  const configured = process.env.MEETING_COPILOT_CODESIGN_KEYCHAIN ?? readDotEnvValue("MEETING_COPILOT_CODESIGN_KEYCHAIN");
  if (configured) return configured;
  return target === "darwin" ? loginKeychainPath() : undefined;
}

function readDotEnvValue(name) {
  const envPath = resolve(".env");
  if (!existsSync(envPath)) return undefined;
  const prefix = `${name}=`;
  const line = readFileSync(envPath, "utf8")
    .split(/\r?\n/)
    .find((candidate) => candidate.trim().startsWith(prefix));
  if (!line) return undefined;
  return line.slice(line.indexOf("=") + 1).trim().replace(/^["']|["']$/g, "");
}

function readBooleanConfig(name) {
  const value = process.env[name] ?? readDotEnvValue(name);
  return /^(1|true|yes)$/i.test(String(value ?? ""));
}

function firstValidCodesigningIdentity(output, preferredName, predicate) {
  // Return the SHA-1 hash so duplicate certificate names do not make codesign ambiguous.
  for (const line of output.split("\n")) {
    if (line.includes("CSSMERR_")) continue;
    const match = line.match(/^\s*\d+\)\s+([A-F0-9]+)\s+"([^"]+)"/);
    if (!match) continue;
    if (preferredName && match[2] !== preferredName) continue;
    if (predicate && !predicate(match[2])) continue;
    return match[1];
  }
  return undefined;
}

function assertDistributionIdentityIfNeeded(output, configuredIdentity) {
  if (!distributionSigning) return;
  const match = findCodesigningIdentity(output, configuredIdentity);
  if (!match || !isDeveloperIdApplicationIdentity(match.name)) {
    throw new Error("MEETING_COPILOT_DISTRIBUTION_SIGNING=1 requires MEETING_COPILOT_CODESIGN_IDENTITY to reference a valid Developer ID Application identity, not Apple Development or a local self-signed identity.");
  }
}

function findCodesigningIdentity(output, configuredIdentity) {
  for (const line of output.split("\n")) {
    if (line.includes("CSSMERR_")) continue;
    const match = line.match(/^\s*\d+\)\s+([A-F0-9]+)\s+"([^"]+)"/);
    if (!match) continue;
    if (match[1] === configuredIdentity || match[2] === configuredIdentity) {
      return { hash: match[1], name: match[2] };
    }
  }
  return undefined;
}

function isDeveloperIdApplicationIdentity(name) {
  return /^Developer ID Application: /.test(name);
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
