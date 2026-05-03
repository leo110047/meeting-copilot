import { spawn } from "node:child_process";
import { createTranscriptEvent } from "../transcription/transcriptEvent.mjs";

export class NativeCommandTranscriber {
  constructor({
    id = "native-speech",
    command,
    args = [],
    language = "zh-TW",
    source = "mic",
    spawnProcess = spawn
  } = {}) {
    this.id = id;
    this.kind = "stt";
    this.roles = ["stt"];
    this.command = command;
    this.args = args;
    this.language = language;
    this.source = source;
    this.spawnProcess = spawnProcess;
    this.child = undefined;
    this.lastError = undefined;
  }

  async *start({ sessionId }) {
    if (!this.command) {
      this.lastError = "native STT command is not configured";
      throw new Error(this.lastError);
    }
    this.child = this.spawnProcess(this.command, [
      ...this.args,
      "--language",
      this.language,
      "--source",
      this.source
    ], { stdio: ["ignore", "pipe", "pipe"] });
    const exitPromise = waitForExit(this.child);
    const stderrChunks = [];
    this.child.stderr?.on("data", (chunk) => stderrChunks.push(String(chunk)));
    for await (const line of readLines(this.child.stdout)) {
      const parsed = parseNativeTranscriptLine(line);
      if (!parsed || parsed.kind !== "transcript" || !parsed.isFinal) continue;
      yield createTranscriptEvent({
        id: `native_${sessionId}_${parsed.endedAtMs}`,
        sessionId,
        source: parsed.source ?? this.source,
        speakerConfidence: parsed.confidence ?? 0.55,
        language: parsed.language,
        startedAtMs: parsed.startedAtMs,
        endedAtMs: parsed.endedAtMs,
        text: parsed.text,
        isFinal: true
      });
    }
    const exitCode = await exitPromise;
    if (exitCode !== 0) {
      this.lastError = stderrChunks.join("").trim() || `native STT command exited ${exitCode}`;
      throw new Error(this.lastError);
    }
  }

  async stop() {
    this.child?.kill();
    this.child = undefined;
  }

  getHealth() {
    return {
      providerId: this.id,
      kind: "stt",
      ready: Boolean(this.command),
      supportsStreaming: true,
      supportsDiarization: false,
      supportsSourceHints: true,
      lastError: this.lastError
    };
  }
}

export function parseNativeTranscriptLine(line) {
  if (!line || !line.trim()) return undefined;
  const parsed = JSON.parse(line);
  if (parsed.kind !== "transcript") return parsed;
  if (typeof parsed.text !== "string") throw new Error("native transcript text must be string");
  return parsed;
}

async function* readLines(stream) {
  let buffer = "";
  stream.setEncoding("utf8");
  for await (const chunk of stream) {
    buffer += chunk;
    let newlineIndex;
    while ((newlineIndex = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      yield line;
    }
  }
  if (buffer.trim()) yield buffer;
}

function waitForExit(child) {
  return new Promise((resolve) => {
    child.once("exit", (code) => resolve(code ?? 0));
  });
}
