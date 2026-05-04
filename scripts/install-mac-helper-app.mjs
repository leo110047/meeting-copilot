import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
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
const macSigningIdentity = process.env.MEETING_COPILOT_CODESIGN_IDENTITY || "-";
const macSigningKeychain = process.env.MEETING_COPILOT_CODESIGN_KEYCHAIN;

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
