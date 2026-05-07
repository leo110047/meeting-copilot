import { randomUUID } from "node:crypto";
import { closeDatabase, executeSql, migrate, queryRows, runTransaction } from "./sqlite.mjs";

export class SessionRepository {
  constructor(dbPath) {
    this.dbPath = migrate(dbPath);
  }

  saveSession({ brief, processingDisclosure = {} }) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO meeting_sessions (
        id, project_id, title, meeting_type, started_at, ended_at, brief_json, processing_disclosure_json
      ) VALUES (?, ?, ?, ?, ?, NULL, ?, ?);
    `, [
      brief.sessionId,
      brief.projectId,
      brief.title,
      brief.meetingType,
      brief.startedAt ?? new Date().toISOString(),
      JSON.stringify(brief),
      JSON.stringify(processingDisclosure)
    ]);
  }

  endSession(sessionId, endedAt = new Date().toISOString()) {
    executeSql(this.dbPath, "UPDATE meeting_sessions SET ended_at = ? WHERE id = ?;", [endedAt, sessionId]);
  }

  saveTranscriptEvent(event) {
    executeSql(this.dbPath, `
      INSERT OR IGNORE INTO transcript_events (
        id, session_id, source, speaker, speaker_confidence, language, language_segments_json,
        started_at_ms, ended_at_ms, text, is_final
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
    `, [
      event.id,
      event.sessionId,
      event.source,
      event.speaker ?? null,
      numberOrNull(event.speakerConfidence),
      event.language ?? "unknown",
      event.languageSegments ? JSON.stringify(event.languageSegments) : null,
      numberOrNull(event.startedAtMs),
      numberOrNull(event.endedAtMs),
      event.text,
      event.isFinal === false ? 0 : 1
    ]);
  }

  saveDecisionSnapshot({ id, sessionId, createdAtMs, decisionState, sourceExtractionId }) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO decision_state_snapshots (
        id, session_id, created_at_ms, decision_state_json, source_extraction_id
      ) VALUES (?, ?, ?, ?, ?);
    `, [id, sessionId, numberOrNull(createdAtMs), JSON.stringify(decisionState), sourceExtractionId ?? null]);
  }

  saveSuggestion(suggestion) {
    executeSql(this.dbPath, `
      INSERT OR IGNORE INTO suggestions (
        id, session_id, shown_at, text, reason, trigger_rule_id,
        confidence, priority, evidence_transcript_ids_json, suggestion_json, feedback
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
    `, [
      suggestion.id,
      suggestion.sessionId,
      suggestion.shownAt,
      suggestion.text,
      suggestion.reason,
      suggestion.triggerRuleId ?? null,
      numberOrNull(suggestion.confidence),
      suggestion.priority,
      JSON.stringify(suggestion.evidenceTranscriptIds ?? []),
      JSON.stringify(suggestion),
      suggestion.feedback ?? null
    ]);
  }

  saveMemoryCandidate(candidate) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO memory_candidates (
        id, project_id, kind, text, source_session_ids_json, evidence_transcript_ids_json,
        confidence, review_status, created_at, reviewed_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
    `, [
      candidate.id,
      candidate.suggestedProjectId ?? candidate.projectId,
      candidate.kind,
      candidate.text,
      JSON.stringify(candidate.sourceSessionIds ?? []),
      JSON.stringify(candidate.evidenceTranscriptIds ?? []),
      numberOrNull(candidate.confidence),
      candidate.reviewStatus ?? "pending",
      candidate.createdAt ?? new Date().toISOString(),
      candidate.reviewedAt ?? null
    ]);
  }

  saveMeetingHistory({ id, sessionId, seriesId, seriesTitle, artifact, allowAiContext = true, createdAt = new Date().toISOString() }) {
    return runTransaction(this.dbPath, (db) => {
      const requestedTitle = normalizedTitle(seriesTitle ?? artifact?.title ?? "未命名會議");
      const resolved = resolveMeetingSeriesIdentity(db, { seriesId, title: requestedTitle });
      const entryId = id ?? `history_${stableId(`${resolved.id}:${sessionId ?? "local"}:${createdAt}`)}`;
      const latestContext = allowAiContext
        ? buildLatestContext({ entryId, sessionId, title: resolved.title, artifact, createdAt })
        : {};
      if (resolved.exists) {
        db.prepare(`
          UPDATE meeting_series
          SET
            summary = CASE WHEN ? = 1 THEN ? ELSE summary END,
            latest_context_json = CASE WHEN ? = 1 THEN ? ELSE latest_context_json END,
            updated_at = ?,
            archived_at = NULL
          WHERE id = ?;
        `).run(
          allowAiContext ? 1 : 0,
          latestContext.summaryText ?? "",
          allowAiContext ? 1 : 0,
          JSON.stringify(latestContext),
          createdAt,
          resolved.id
        );
      } else {
        db.prepare(`
          INSERT INTO meeting_series (id, title, summary, latest_context_json, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?);
        `).run(
          resolved.id,
          resolved.title,
          latestContext.summaryText ?? "",
          JSON.stringify(latestContext),
          createdAt,
          createdAt
        );
      }
      db.prepare(`
        INSERT OR REPLACE INTO meeting_history_entries (
          id, series_id, session_id, title, artifact_json, allow_ai_context, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?);
      `).run(
        entryId,
        resolved.id,
        sessionId ?? null,
        resolved.title,
        JSON.stringify(artifact ?? {}),
        allowAiContext ? 1 : 0,
        createdAt
      );
      return {
        entryId,
        series: readMeetingSeriesById(db, resolved.id)
      };
    });
  }

  listMeetingSeries() {
    const rows = queryRows(this.dbPath, `
      SELECT
        s.id,
        s.title,
        s.summary,
        s.latest_context_json,
        s.updated_at,
        COUNT(h.id) AS history_count
      FROM meeting_series s
      LEFT JOIN meeting_history_entries h ON h.series_id = s.id
      WHERE s.archived_at IS NULL
      GROUP BY s.id
      ORDER BY s.updated_at DESC, s.title ASC;
    `);
    return rows.map((row) => ({
      id: row.id,
      title: row.title,
      summary: row.summary ?? "",
      latestContext: parseJsonObject(row.latest_context_json),
      lastSavedAt: row.updated_at,
      historyCount: Number(row.history_count ?? 0)
    }));
  }

  saveAppErrorLog({ id, sessionId, stage, source, severity = "error", message, detail = {}, createdAt = new Date().toISOString() }) {
    executeSql(this.dbPath, `
      INSERT INTO app_error_logs (
        id, session_id, stage, source, severity, message, detail_json, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?);
    `, [
      id ?? `app_error_${randomUUID()}`,
      sessionId ?? null,
      stage,
      source,
      severity,
      message,
      JSON.stringify(detail ?? {}),
      createdAt
    ]);
  }

  close() {
    closeDatabase(this.dbPath);
  }
}

function numberOrNull(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return null;
  return Number(value);
}

function normalizedTitle(value) {
  const title = String(value ?? "").trim();
  return [...(title || "未命名會議")].slice(0, 120).join("");
}

function stableId(value) {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(String(value))) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash.toString(16).padStart(16, "0");
}

function parseJsonObject(text) {
  try {
    const value = JSON.parse(text || "{}");
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  } catch {
    return {};
  }
}

function buildLatestContext({ entryId, sessionId, title, artifact, createdAt }) {
  const summary = artifact?.summary ?? {};
  const transcript = Array.isArray(artifact?.transcript) ? artifact.transcript : [];
  const keyPoints = limitedStringArray(summary.keyPoints, 6);
  const unresolved = limitedStringArray(summary.decisionsAndOpenQuestions, 6);
  const suggestedActions = limitedStringArray(summary.suggestedActions, 6);
  return {
    entryId,
    sessionId: sessionId ?? null,
    title,
    updatedAt: createdAt,
    summaryText: keyPoints.slice(0, 2).join("；"),
    keyPoints,
    unresolved,
    suggestedActions,
    transcriptPreview: transcript.slice(-6).map((line) => ({
      speaker: normalizedContextText(line.speaker ?? "未標記來源"),
      text: normalizedContextText(line.text ?? "")
    })).filter((line) => line.text)
  };
}

function resolveMeetingSeriesIdentity(db, { seriesId, title }) {
  const requestedId = String(seriesId ?? "").trim();
  if (requestedId) {
    const byId = db.prepare("SELECT id, title FROM meeting_series WHERE id = ?;").get(requestedId);
    if (byId) return { id: byId.id, title: byId.title, exists: true };
  }
  const byTitle = db.prepare("SELECT id, title FROM meeting_series WHERE title = ?;").get(title);
  if (byTitle) return { id: byTitle.id, title: byTitle.title, exists: true };
  return { id: requestedId || `series_${stableId(title)}`, title, exists: false };
}

function readMeetingSeriesById(db, id) {
  const row = db.prepare(`
    SELECT
      s.id,
      s.title,
      s.summary,
      s.latest_context_json,
      s.updated_at,
      (SELECT COUNT(*) FROM meeting_history_entries h WHERE h.series_id = s.id) AS history_count
    FROM meeting_series s
    WHERE s.id = ? AND s.archived_at IS NULL;
  `).get(id);
  if (!row) return undefined;
  return {
    id: row.id,
    title: row.title,
    summary: row.summary ?? "",
    latestContext: parseJsonObject(row.latest_context_json),
    lastSavedAt: row.updated_at,
    historyCount: Number(row.history_count ?? 0)
  };
}

function limitedStringArray(value, limit) {
  return Array.isArray(value)
    ? value.map(normalizedContextText).filter(Boolean).slice(0, limit)
    : [];
}

function normalizedContextText(value) {
  return String(value ?? "").replace(/\s+/gu, " ").trim();
}
