import { randomUUID } from "node:crypto";
import { closeDatabase, executeSql, migrate } from "./sqlite.mjs";

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
        confidence, priority, evidence_transcript_ids_json, feedback
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
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
