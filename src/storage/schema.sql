PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meeting_sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  title TEXT,
  meeting_type TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  brief_json TEXT NOT NULL,
  processing_disclosure_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS transcript_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  source TEXT NOT NULL,
  speaker TEXT,
  speaker_confidence REAL,
  language TEXT NOT NULL DEFAULT 'unknown',
  language_segments_json TEXT,
  started_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER,
  text TEXT NOT NULL,
  is_final INTEGER NOT NULL,
  FOREIGN KEY (session_id) REFERENCES meeting_sessions(id)
);

CREATE TABLE IF NOT EXISTS state_items (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  canonical_key TEXT NOT NULL,
  text TEXT NOT NULL,
  status TEXT NOT NULL,
  confidence REAL NOT NULL,
  evidence_transcript_ids_json TEXT NOT NULL,
  first_seen_at_ms INTEGER NOT NULL,
  last_updated_at_ms INTEGER NOT NULL,
  UNIQUE(session_id, kind, canonical_key)
);

CREATE TABLE IF NOT EXISTS decision_state_snapshots (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  decision_state_json TEXT NOT NULL,
  source_extraction_id TEXT,
  FOREIGN KEY (session_id) REFERENCES meeting_sessions(id)
);

CREATE TABLE IF NOT EXISTS suggestions (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  shown_at TEXT NOT NULL,
  text TEXT NOT NULL,
  reason TEXT NOT NULL,
  trigger_rule_id TEXT,
  confidence REAL NOT NULL,
  priority TEXT NOT NULL,
  evidence_transcript_ids_json TEXT NOT NULL,
  feedback TEXT,
  FOREIGN KEY (session_id) REFERENCES meeting_sessions(id)
);

CREATE TABLE IF NOT EXISTS llm_usage_logs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  call_type TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  prompt_hash TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  cached_input_tokens INTEGER,
  output_tokens INTEGER NOT NULL,
  audio_input_tokens INTEGER,
  estimated_cost_usd REAL,
  latency_ms INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS extraction_failure_logs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  call_type TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  provider TEXT NOT NULL,
  failure_kind TEXT NOT NULL,
  raw_output_ref TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (session_id) REFERENCES meeting_sessions(id)
);

CREATE TABLE IF NOT EXISTS ai_provider_failure_logs (
  id TEXT PRIMARY KEY,
  audit_id TEXT NOT NULL,
  call_type TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  provider TEXT NOT NULL,
  failure_kind TEXT NOT NULL,
  raw_output_ref TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_error_logs (
  id TEXT PRIMARY KEY,
  session_id TEXT,
  stage TEXT NOT NULL,
  source TEXT NOT NULL,
  severity TEXT NOT NULL,
  message TEXT NOT NULL,
  detail_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  strategic_goal TEXT,
  context_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS participant_profiles (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  role TEXT,
  organization TEXT,
  sensitive INTEGER NOT NULL DEFAULT 1,
  deleted_at TEXT,
  profile_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS knowledge_memories (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  source_session_ids_json TEXT NOT NULL,
  evidence_transcript_ids_json TEXT NOT NULL,
  confidence REAL NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_candidates (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  source_session_ids_json TEXT NOT NULL,
  evidence_transcript_ids_json TEXT NOT NULL,
  confidence REAL NOT NULL,
  review_status TEXT NOT NULL DEFAULT 'pending',
  created_at TEXT NOT NULL,
  reviewed_at TEXT
);

CREATE TABLE IF NOT EXISTS political_signals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  confidence REAL NOT NULL,
  evidence_transcript_ids_json TEXT NOT NULL,
  suggested_interpretation TEXT NOT NULL,
  review_status TEXT NOT NULL DEFAULT 'candidate',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS shared_meeting_artifacts (
  id TEXT PRIMARY KEY,
  shared_meeting_id TEXT NOT NULL,
  linked_local_session_ids_json TEXT NOT NULL,
  artifact_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS shared_conflict_candidates (
  id TEXT PRIMARY KEY,
  shared_artifact_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  description TEXT NOT NULL,
  local_evidence_ids_json TEXT NOT NULL,
  remote_evidence_ids_json TEXT NOT NULL,
  resolution_status TEXT NOT NULL,
  FOREIGN KEY (shared_artifact_id) REFERENCES shared_meeting_artifacts(id)
);

CREATE TABLE IF NOT EXISTS shared_artifact_approvals (
  id TEXT PRIMARY KEY,
  shared_artifact_id TEXT NOT NULL,
  item_kind TEXT NOT NULL,
  item_id TEXT NOT NULL,
  approval_status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (shared_artifact_id) REFERENCES shared_meeting_artifacts(id)
);
