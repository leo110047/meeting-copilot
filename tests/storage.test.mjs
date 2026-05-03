import assert from "node:assert/strict";
import test from "node:test";
import { rmSync } from "node:fs";
import { migrate, listTables, queryScalar } from "../src/storage/sqlite.mjs";
import { SessionRepository } from "../src/storage/sessionRepository.mjs";

test("SQLite migration creates core, Layer 3, and shared meeting tables", () => {
  const dbPath = ".data/test-meeting-copilot.db";
  rmSync(dbPath, { force: true });
  migrate(dbPath);
  const tables = listTables(dbPath);
  for (const table of [
    "meeting_sessions",
    "transcript_events",
    "state_items",
    "decision_state_snapshots",
    "suggestions",
    "llm_usage_logs",
    "extraction_failure_logs",
    "ai_provider_failure_logs",
    "app_error_logs",
    "projects",
    "participant_profiles",
    "knowledge_memories",
    "memory_candidates",
    "political_signals",
    "shared_meeting_artifacts",
    "shared_conflict_candidates",
    "shared_artifact_approvals"
  ]) {
    assert.ok(tables.includes(table), `${table} missing`);
  }
});

test("SessionRepository persists cross-project app error logs", () => {
  const dbPath = ".data/test-meeting-copilot-errors.db";
  rmSync(dbPath, { force: true });
  const repository = new SessionRepository(dbPath);
  repository.saveAppErrorLog({
    id: "err_test",
    sessionId: "sess_test",
    stage: "live_api.request",
    source: "node_server",
    severity: "error",
    message: "boom",
    detail: { route: "/api/sessions" },
    createdAt: "1"
  });
  assert.equal(queryScalar(dbPath, "SELECT stage FROM app_error_logs WHERE id = 'err_test';"), "live_api.request");
  assert.equal(queryScalar(dbPath, "SELECT json_extract(detail_json, '$.route') FROM app_error_logs WHERE id = 'err_test';"), "/api/sessions");
});
