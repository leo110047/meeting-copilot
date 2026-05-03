import { executeSql, migrate, sqlNumber, sqlString } from "./sqlite.mjs";

export class SessionRepository {
  constructor(dbPath) {
    this.dbPath = migrate(dbPath);
  }

  saveSession({ brief, processingDisclosure = {} }) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO meeting_sessions (
        id, project_id, title, meeting_type, started_at, ended_at, brief_json, processing_disclosure_json
      ) VALUES (
        ${sqlString(brief.sessionId)},
        ${sqlString(brief.projectId)},
        ${sqlString(brief.title)},
        ${sqlString(brief.meetingType)},
        ${sqlString(brief.startedAt ?? new Date().toISOString())},
        NULL,
        ${sqlString(JSON.stringify(brief))},
        ${sqlString(JSON.stringify(processingDisclosure))}
      );
    `);
  }

  endSession(sessionId, endedAt = new Date().toISOString()) {
    executeSql(this.dbPath, `
      UPDATE meeting_sessions SET ended_at = ${sqlString(endedAt)} WHERE id = ${sqlString(sessionId)};
    `);
  }

  saveTranscriptEvent(event) {
    executeSql(this.dbPath, `
      INSERT OR IGNORE INTO transcript_events (
        id, session_id, source, speaker, speaker_confidence, language, language_segments_json,
        started_at_ms, ended_at_ms, text, is_final
      ) VALUES (
        ${sqlString(event.id)},
        ${sqlString(event.sessionId)},
        ${sqlString(event.source)},
        ${sqlString(event.speaker)},
        ${sqlNumber(event.speakerConfidence)},
        ${sqlString(event.language ?? "unknown")},
        ${sqlString(event.languageSegments ? JSON.stringify(event.languageSegments) : null)},
        ${sqlNumber(event.startedAtMs)},
        ${sqlNumber(event.endedAtMs)},
        ${sqlString(event.text)},
        ${event.isFinal === false ? 0 : 1}
      );
    `);
  }

  saveDecisionSnapshot({ id, sessionId, createdAtMs, decisionState, sourceExtractionId }) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO decision_state_snapshots (
        id, session_id, created_at_ms, decision_state_json, source_extraction_id
      ) VALUES (
        ${sqlString(id)},
        ${sqlString(sessionId)},
        ${sqlNumber(createdAtMs)},
        ${sqlString(JSON.stringify(decisionState))},
        ${sqlString(sourceExtractionId)}
      );
    `);
  }

  saveSuggestion(suggestion) {
    executeSql(this.dbPath, `
      INSERT OR IGNORE INTO suggestions (
        id, session_id, shown_at, text, reason, trigger_rule_id,
        confidence, priority, evidence_transcript_ids_json, feedback
      ) VALUES (
        ${sqlString(suggestion.id)},
        ${sqlString(suggestion.sessionId)},
        ${sqlString(suggestion.shownAt)},
        ${sqlString(suggestion.text)},
        ${sqlString(suggestion.reason)},
        ${sqlString(suggestion.triggerRuleId)},
        ${sqlNumber(suggestion.confidence)},
        ${sqlString(suggestion.priority)},
        ${sqlString(JSON.stringify(suggestion.evidenceTranscriptIds ?? []))},
        ${sqlString(suggestion.feedback)}
      );
    `);
  }

  saveMemoryCandidate(candidate) {
    executeSql(this.dbPath, `
      INSERT OR REPLACE INTO memory_candidates (
        id, project_id, kind, text, source_session_ids_json, evidence_transcript_ids_json,
        confidence, review_status, created_at, reviewed_at
      ) VALUES (
        ${sqlString(candidate.id)},
        ${sqlString(candidate.suggestedProjectId ?? candidate.projectId)},
        ${sqlString(candidate.kind)},
        ${sqlString(candidate.text)},
        ${sqlString(JSON.stringify(candidate.sourceSessionIds ?? []))},
        ${sqlString(JSON.stringify(candidate.evidenceTranscriptIds ?? []))},
        ${sqlNumber(candidate.confidence)},
        ${sqlString(candidate.reviewStatus ?? "pending")},
        ${sqlString(candidate.createdAt ?? new Date().toISOString())},
        ${sqlString(candidate.reviewedAt)}
      );
    `);
  }

  saveAppErrorLog({ id, sessionId, stage, source, severity = "error", message, detail = {}, createdAt = new Date().toISOString() }) {
    executeSql(this.dbPath, `
      INSERT INTO app_error_logs (
        id, session_id, stage, source, severity, message, detail_json, created_at
      ) VALUES (
        ${sqlString(id ?? `app_error_${Date.now()}_${Math.random().toString(16).slice(2)}`)},
        ${sqlString(sessionId)},
        ${sqlString(stage)},
        ${sqlString(source)},
        ${sqlString(severity)},
        ${sqlString(message)},
        ${sqlString(JSON.stringify(detail ?? {}))},
        ${sqlString(createdAt)}
      );
    `);
  }
}
