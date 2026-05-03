import { existsSync } from "node:fs";
import { delimiter } from "node:path";
import { spawnSync } from "node:child_process";

const command = process.argv[2];
const args = process.argv.slice(3);

if (!command) {
  console.error("usage: node scripts/run-with-rust.mjs <command> [...args]");
  process.exit(2);
}

const env = { ...process.env };
const extraPath = rustToolchainBin();
if (extraPath) {
  env.PATH = [extraPath, env.PATH].filter(Boolean).join(delimiter);
}

const result = spawnSync(command, args, {
  env,
  shell: process.platform === "win32",
  stdio: "inherit"
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);

function rustToolchainBin() {
  if (process.env.MEETING_COPILOT_RUST_BIN) {
    return process.env.MEETING_COPILOT_RUST_BIN;
  }

  const home = process.env.HOME ?? process.env.USERPROFILE;
  if (!home) return "";

  const candidates = process.platform === "darwin"
    ? [
        `${home}/.rustup/toolchains/stable-aarch64-apple-darwin/bin`,
        `${home}/.rustup/toolchains/stable-x86_64-apple-darwin/bin`,
        `${home}/.cargo/bin`
      ]
    : process.platform === "win32"
      ? [
          `${home}\\.cargo\\bin`
        ]
      : [
          `${home}/.cargo/bin`
        ];

  return candidates.find((candidate) => existsSync(candidate)) ?? "";
}
