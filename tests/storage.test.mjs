import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import { rmSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { closeAllDatabases, closeDatabase, executeSql, migrate, listTables, queryScalar } from "../src/storage/sqlite.mjs";
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
    "meeting_series",
    "meeting_history_entries",
    "political_signals",
    "shared_meeting_artifacts",
    "shared_conflict_candidates",
    "shared_artifact_approvals"
  ]) {
    assert.ok(tables.includes(table), `${table} missing`);
  }
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM pragma_table_info('suggestions') WHERE name = 'suggestion_json';"), "1");
  assert.equal(queryScalar(dbPath, "PRAGMA foreign_keys;"), "1");
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_meeting_history_series';"), "1");
});

test("SessionRepository saves explicit meeting history as reusable meeting series", () => {
  const dbPath = ".data/test-meeting-copilot-history.db";
  rmSync(dbPath, { force: true });
  const repository = new SessionRepository(dbPath);
  const saved = repository.saveMeetingHistory({
    sessionId: "sess_history",
    seriesTitle: "每週產品同步",
    allowAiContext: true,
    artifact: {
      title: "每週產品同步",
      summary: {
        keyPoints: ["Demo scope needs confirmation", "Backend owner is unclear"],
        decisionsAndOpenQuestions: ["誰負責 rollback plan？"],
        suggestedActions: ["下次先確認 owner"]
      },
      transcript: [{ speaker: "對方 A", text: "我們下週再確認 owner" }]
    },
    createdAt: "2026-05-07T10:00:00.000Z"
  });
  assert.equal(saved.series.title, "每週產品同步");
  const series = repository.listMeetingSeries();
  assert.equal(series.length, 1);
  assert.equal(series[0].historyCount, 1);
  assert.deepEqual(series[0].latestContext.keyPoints.slice(0, 1), ["Demo scope needs confirmation"]);
  repository.saveMeetingHistory({
    sessionId: "sess_private",
    seriesTitle: "每週產品同步",
    allowAiContext: false,
    artifact: {
      title: "每週產品同步",
      summary: {
        keyPoints: ["這場不要更新 future context"],
        decisionsAndOpenQuestions: [],
        suggestedActions: []
      },
      transcript: []
    },
    createdAt: "2026-05-07T11:00:00.000Z"
  });
  const updated = repository.listMeetingSeries()[0];
  assert.equal(updated.historyCount, 2);
  assert.deepEqual(updated.latestContext.keyPoints.slice(0, 1), ["Demo scope needs confirmation"]);
  assert.match(updated.lastSavedAt, /^2026-05-07T11:00:00\.000Z$/);
  assert.equal(queryScalar(dbPath, "SELECT COUNT(*) FROM meeting_history_entries WHERE session_id = ?;", ["sess_history"]), "1");
  assert.throws(() => executeSql(dbPath, `
    INSERT INTO meeting_history_entries (id, series_id, session_id, title, artifact_json, allow_ai_context, created_at)
    VALUES ('bad_fk', 'missing_series', 'bad', 'Bad', '{}', 0, '2026-05-07T12:00:00.000Z');
  `), /FOREIGN KEY/);
  repository.close();
  rmSync(dbPath, { force: true });
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
  repository.saveSuggestion({
    id: "coach_1",
    sessionId: "sess_sql",
    shownAt: "1",
    kind: "ask_clarifying_question",
    title: "先確認正式版定義",
    text: "你可以問：正式版是指 demo 還是 production？",
    suggestedMove: "你可以問：正式版是指 demo 還是 production？",
    watchOut: "對方把模糊時程推成承諾。",
    reason: "對方要求下週上線但沒有驗收標準。",
    confidence: 0.88,
    priority: "high",
    evidenceTranscriptIds: ["tricky_text"]
  });
  assert.equal(queryScalar(dbPath, "SELECT json_extract(suggestion_json, '$.suggestedMove') FROM suggestions WHERE id = ?;", ["coach_1"]), "你可以問：正式版是指 demo 還是 production？");
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
  assert.match(repositorySource, /suggestion_json/);
});
