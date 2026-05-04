import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { platform } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

if (platform() !== "darwin") {
  process.exit(0);
}

const bridgeSource = resolve("src-tauri/binaries/libmeeting_copilot_speech_bridge.dylib");
const targetBundle = resolve("target/debug/bundle/macos/Meeting Copilot.app");
const bridgeTarget = resolve(targetBundle, "Contents/Frameworks/libmeeting_copilot_speech_bridge.dylib");
const staleHelperTarget = resolve(targetBundle, "Contents/Helpers/Meeting Copilot Speech.app");
const localMacSigningIdentity = "Meeting Copilot Local Code Signing";
const macSigningIdentity = resolveMacSigningIdentity();
const macSigningKeychain = resolveMacSigningKeychain();

if (!existsSync(bridgeSource)) {
  throw new Error(`macOS speech bridge is missing: ${bridgeSource}`);
}

if (!existsSync(targetBundle)) {
  console.log(`Skipping macOS speech helper install; app bundle is missing: ${targetBundle}`);
  process.exit(0);
}

mkdirSync(resolve(targetBundle, "Contents/Frameworks"), { recursive: true });
copyFileSync(bridgeSource, bridgeTarget);
if (existsSync(staleHelperTarget)) {
  rmSync(staleHelperTarget, { recursive: true, force: true });
  console.log(`Removed stale macOS speech helper app: ${staleHelperTarget}`);
}
signMacBundle(bridgeTarget);
signMacBundle(targetBundle);
console.log(`Installed macOS speech bridge: ${bridgeTarget}`);

function signMacBundle(bundlePath) {
  const signArgs = ["--force", "--deep"];
  if (macSigningKeychain) signArgs.push("--keychain", macSigningKeychain);
  signArgs.push("--sign", macSigningIdentity, bundlePath);
  const result = spawnSync("codesign", signArgs, { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function resolveMacSigningIdentity() {
  const configured = process.env.MEETING_COPILOT_CODESIGN_IDENTITY ?? readDotEnvValue("MEETING_COPILOT_CODESIGN_IDENTITY");
  if (configured) return configured;
  const result = spawnSync("security", ["find-identity", "-v", "-p", "codesigning", loginKeychainPath()], { encoding: "utf8" });
  if (result.status === 0 && result.stdout.includes(`"${localMacSigningIdentity}"`)) {
    return localMacSigningIdentity;
  }
  const fallbackIdentity = firstValidCodesigningIdentity(result.stdout);
  if (fallbackIdentity) return fallbackIdentity;
  return "-";
}

function loginKeychainPath() {
  return `${process.env.HOME}/Library/Keychains/login.keychain-db`;
}

function resolveMacSigningKeychain() {
  const configured = process.env.MEETING_COPILOT_CODESIGN_KEYCHAIN ?? readDotEnvValue("MEETING_COPILOT_CODESIGN_KEYCHAIN");
  if (configured) return configured;
  return loginKeychainPath();
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

function firstValidCodesigningIdentity(output) {
  for (const line of output.split("\n")) {
    if (line.includes("CSSMERR_")) continue;
    const match = line.match(/^\s*\d+\)\s+[A-F0-9]+\s+"([^"]+)"/);
    if (match) return match[1];
  }
  return undefined;
}
