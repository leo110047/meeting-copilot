import assert from "node:assert/strict";
import test from "node:test";
import { rmSync } from "node:fs";
import { LiveSessionService } from "../src/server/liveApi.mjs";
import { queryScalar } from "../src/storage/sqlite.mjs";

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
