import assert from "node:assert/strict";
import test from "node:test";
import { rmSync } from "node:fs";
import { LiveSessionService, createLiveApiServer } from "../src/server/liveApi.mjs";
import { queryScalar } from "../src/storage/sqlite.mjs";
import { SessionRuntime } from "../src/core/sessionRuntime.mjs";
import { RuleBasedStateExtractionEngine } from "../src/core/stateExtractionEngine.mjs";

test("live API persists session transcript suggestion and decision snapshot", async () => {
  const dbPath = ".data/test-live-api.db";
  rmSync(dbPath, { force: true });
  const service = new LiveSessionService({ dbPath });
  const session = service.startSession();

  for (const text of [
    "我們今天要決定 v1 scope。",
    "owner 還沒定，deadline 先不要寫死。",
    "驗收標準還沒講清楚，但好像要先這樣決定。"
  ]) {
    const result = await service.ingestTranscript(session.sessionId, { text, source: "mic", isFinal: true });
    assert.ok(result);
  }

  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM meeting_sessions;"), "1");
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM transcript_events;"), "3");
  assert.ok(Number(queryScalar(dbPath, "SELECT COUNT(*) FROM decision_state_snapshots;")) >= 1);
  assert.ok(Number(queryScalar(dbPath, "SELECT COUNT(*) FROM suggestions;")) >= 1);
});

test("live API processes only new transcript chunks after session start", async () => {
  const dbPath = ".data/test-live-api-incremental.db";
  rmSync(dbPath, { force: true });
  const observedChunkSizes = [];
  const delegate = new RuleBasedStateExtractionEngine();
  const service = new LiveSessionService({
    dbPath,
    runtimeFactory: ({ knowledgeStore }) => new SessionRuntime({
      knowledgeStore,
      extractionEngine: {
        extract(input) {
          observedChunkSizes.push(input.newFinalTranscriptEvents.length);
          return delegate.extract(input);
        }
      }
    })
  });
  const session = service.startSession();

  await service.ingestTranscript(session.sessionId, { text: "我們今天要決定 v1 scope。", source: "mic", isFinal: true });
  await service.ingestTranscript(session.sessionId, { text: "owner 還沒定，deadline 先不要寫死。", source: "mic", isFinal: true });
  await service.ingestTranscript(session.sessionId, { text: "驗收標準還沒講清楚。", source: "mic", isFinal: true });

  assert.deepEqual(observedChunkSizes, [1, 1, 1]);
});

test("live API rejects oversized JSON bodies before buffering indefinitely", async () => {
  const dbPath = ".data/test-live-api-size-limit.db";
  rmSync(dbPath, { force: true });
  const server = createLiveApiServer({ dbPath, distRoot: "dist" });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  try {
    const response = await fetch(`http://127.0.0.1:${port}/api/sessions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ brief: { notes: "x".repeat(1024 * 1024 + 1) } })
    });
    assert.equal(response.status, 413);
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});
