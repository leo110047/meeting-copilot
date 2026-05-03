import assert from "node:assert/strict";
import { Readable } from "node:stream";
import test from "node:test";
import { EventEmitter } from "node:events";
import { NativeCommandTranscriber, parseNativeTranscriptLine } from "../src/providers/nativeSttProvider.mjs";

test("native transcript line parser preserves mixed-language transcript", () => {
  const parsed = parseNativeTranscriptLine(JSON.stringify({
    kind: "transcript",
    text: "owner 還沒定，deadline still unclear",
    isFinal: true,
    confidence: 0.81,
    language: "zh-TW",
    source: "mic",
    startedAtMs: 0,
    endedAtMs: 3000
  }));

  assert.equal(parsed.text, "owner 還沒定，deadline still unclear");
  assert.equal(parsed.source, "mic");
});

test("NativeCommandTranscriber yields TranscriptEvent from native helper stdout", async () => {
  const fakeChild = new EventEmitter();
  fakeChild.stdout = Readable.from([
    JSON.stringify({
      kind: "transcript",
      text: "驗收標準還沒定",
      isFinal: true,
      confidence: 0.8,
      language: "zh-TW",
      source: "mic",
      startedAtMs: 0,
      endedAtMs: 2500
    }) + "\n"
  ]);
  fakeChild.stderr = Readable.from([]);
  fakeChild.kill = () => {};
  const spawnProcess = () => {
    queueMicrotask(() => fakeChild.emit("exit", 0));
    return fakeChild;
  };
  const transcriber = new NativeCommandTranscriber({ command: "helper", spawnProcess });
  const events = [];
  for await (const event of transcriber.start({ sessionId: "s1" })) events.push(event);

  assert.equal(events.length, 1);
  assert.equal(events[0].text, "驗收標準還沒定");
  assert.equal(events[0].source, "mic");
});
