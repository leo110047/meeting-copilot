#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { stdin } from "node:process";
import { loadFixture, replayFixture } from "../src/replay/replayHarness.mjs";
import { EventBus } from "../src/core/eventBus.mjs";
import { KnowledgeStore } from "../src/core/knowledgeStore.mjs";
import { SessionRuntime } from "../src/core/sessionRuntime.mjs";
import { MockStreamingTranscriber } from "../src/providers/sttProvider.mjs";
import { runSttBakeoff } from "../src/providers/sttBakeoff.mjs";

const [command, ...args] = process.argv.slice(2);

if (command === "replay") {
  const fixture = valueOf(args, "--fixture") ?? "requirement_scoping_layer3";
  const provider = valueOf(args, "--provider") ?? "local";
  const promptVersion = valueOf(args, "--prompt-version") ?? "rule.v1";
  const report = await replayFixture(fixture, { provider, promptVersion });
  console.log(JSON.stringify(report, null, 2));
} else if (command === "manual") {
  const fixture = valueOf(args, "--fixture");
  const report = fixture ? await replayFixture(fixture, { provider: "manual-fixture" }) : await runInteractiveManual();
  console.log(JSON.stringify({
    suggestions: report.suggestions,
    finalDecisionState: report.finalDecisionState,
    metrics: report.metrics
  }, null, 2));
} else if (command === "stt-bakeoff") {
  const fixtureName = valueOf(args, "--fixture") ?? "mixed_scope_owner";
  const fixture = loadFixture(fixtureName);
  const results = await runSttBakeoff({
    expectedTranscriptEvents: fixture.transcriptEvents,
    candidates: [
      new MockStreamingTranscriber({
        id: "mock-streaming-stt",
        fixtureEvents: fixture.transcriptEvents
      })
    ]
  });
  console.log(JSON.stringify({ fixture: fixtureName, results }, null, 2));
} else {
  console.error("Usage: meeting-copilot replay --fixture NAME | meeting-copilot manual [--fixture NAME] | meeting-copilot stt-bakeoff [--fixture NAME]");
  process.exit(1);
}

function valueOf(args, key) {
  const index = args.indexOf(key);
  return index >= 0 ? args[index + 1] : undefined;
}

async function runInteractiveManual() {
  const text = await readAllStdin();
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const brief = {
    sessionId: `manual_${Date.now()}`,
    meetingType: "requirement_scoping",
    title: "Manual transcript",
    goal: "確認這次討論是否可以安全做出 scope / owner / deadline 決策",
    mustConfirm: ["owner", "deadline", "驗收標準"],
    risks: ["未定義 rollback 或風險 owner"],
    constraints: [],
    knownParticipants: [],
    preferredTone: "direct"
  };
  const transcriptEvents = lines.map((line, index) => ({
    id: `manual_t${index + 1}`,
    sessionId: brief.sessionId,
    source: "unknown",
    speakerConfidence: 0.4,
    language: /[a-zA-Z]/.test(line) && /[\u4e00-\u9fff]/.test(line) ? "mixed" : "zh-TW",
    startedAtMs: index * 10_000,
    endedAtMs: index * 10_000 + 4_000,
    text: line,
    isFinal: true
  }));
  const runtime = new SessionRuntime({ knowledgeStore: new KnowledgeStore(), eventBus: new EventBus() });
  const result = await runtime.runManual({ brief, transcriptEvents });
  return {
    suggestions: result.suggestions,
    finalDecisionState: result.decisionState,
    metrics: { suggestionCount: result.suggestions.length }
  };
}

function readAllStdin() {
  if (stdin.isTTY) return Promise.resolve(readFileSync(0, "utf8"));
  return new Promise((resolve, reject) => {
    let data = "";
    stdin.setEncoding("utf8");
    stdin.on("data", (chunk) => data += chunk);
    stdin.on("end", () => resolve(data));
    stdin.on("error", reject);
  });
}
