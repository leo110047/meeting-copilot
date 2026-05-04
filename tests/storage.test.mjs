import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import { rmSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { closeAllDatabases, closeDatabase, migrate, listTables, queryScalar } from "../src/storage/sqlite.mjs";
import { SessionRepository } from "../src/storage/sessionRepository.mjs";

afterEach(() => {
  closeAllDatabases();
});

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

test("SessionRepository writes user text through SQLite parameters", () => {
  const dbPath = ".data/test-meeting-copilot-parameters.db";
  rmSync(dbPath, { force: true });
  const repository = new SessionRepository(dbPath);
  repository.saveSession({
    brief: {
      sessionId: "sess_sql",
      projectId: "p1",
      title: "SQL parameter test",
      meetingType: "requirement_scoping",
      goal: "verify storage",
      preferredTone: "direct",
      startedAt: "1"
    }
  });
  repository.saveTranscriptEvent({
    id: "tricky_text",
    sessionId: "sess_sql",
    source: "mic",
    speakerConfidence: 0.5,
    startedAtMs: 0,
    endedAtMs: 1,
    text: "ok'); DROP TABLE meeting_sessions; --",
    isFinal: true
  });
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM meeting_sessions;"), "1");
  assert.equal(queryScalar(dbPath, "SELECT text FROM transcript_events WHERE id = ?;", ["tricky_text"]), "ok'); DROP TABLE meeting_sessions; --");
  repository.close();
});

test("SQLite handles can be closed and reopened", () => {
  const dbPath = ".data/test-meeting-copilot-close.db";
  rmSync(dbPath, { force: true });
  migrate(dbPath);
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM sqlite_master;") !== "", true);
  closeDatabase(dbPath);
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM sqlite_master;") !== "", true);
});

test("Node storage path does not depend on sqlite3 CLI child processes", async () => {
  const sqliteSource = await readFile(new URL("../src/storage/sqlite.mjs", import.meta.url), "utf8");
  const repositorySource = await readFile(new URL("../src/storage/sessionRepository.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(sqliteSource, /spawnSync|"sqlite3"/);
  assert.match(sqliteSource, /export function closeDatabase/);
  assert.match(sqliteSource, /export function closeAllDatabases/);
  assert.match(repositorySource, /closeDatabase\(this\.dbPath\)/);
  assert.doesNotMatch(repositorySource, /sqlString|sqlNumber/);
  assert.match(repositorySource, /VALUES \(\?, \?, \?, \?, \?, NULL, \?, \?\)/);
});
