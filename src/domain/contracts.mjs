import { createHash, randomUUID } from "node:crypto";

export const MEETING_TYPES = [
  "requirement_scoping",
  "customer_interview",
  "sales_discovery",
  "one_on_one",
  "open_discussion"
];

export const DECISION_TYPES = [
  "scope",
  "priority",
  "commitment",
  "tradeoff",
  "owner",
  "timeline",
  "budget",
  "technical_direction",
  "unknown"
];

export const MODEL_PROVIDER_KINDS = ["subscription_oauth", "api", "stt", "local"];
export const MODEL_PROVIDER_ROLES = ["text_decision", "stt", "memory_extraction", "replay"];

export function nowIso() {
  return new Date().toISOString();
}

export function makeId(prefix = "id") {
  return `${prefix}_${randomUUID()}`;
}

export function sha16(input) {
  return createHash("sha256").update(input).digest("hex").slice(0, 16);
}

export function normalizeText(input) {
  return String(input ?? "")
    .trim()
    .toLowerCase()
    .replace(/[，。,.!?？：:；;「」"'`]/g, " ")
    .replace(/\s+/g, " ");
}

export function canonicalKey(kind, semanticLabel, linkedPlaybookItemId = "") {
  const normalized = normalizeText(semanticLabel)
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s+/g, "_");
  return [kind, normalized, linkedPlaybookItemId].filter(Boolean).join(":");
}

export function stateItemId(sessionId, kind, key) {
  return sha16(`${sessionId}:${kind}:${key}`);
}

export function createMeetingState({ sessionId, projectId, startedAt = nowIso() }) {
  return {
    sessionId,
    projectId,
    startedAt,
    currentTopic: undefined,
    phase: "opening",
    openQuestions: [],
    decisions: [],
    actionItems: [],
    risks: [],
    participants: [],
    lastSuggestionAt: undefined
  };
}

export function createDecisionState({ sessionId, decisionType = "unknown", constraints = [], stakeholders = [] }) {
  return {
    sessionId,
    currentDecision: undefined,
    decisionType,
    options: [],
    constraints,
    unresolvedRisks: [],
    missingInputs: [],
    stakeholders,
    readiness: {
      score: 0,
      safeToDecide: false,
      blockers: [],
      evidenceTranscriptIds: []
    },
    evidenceTranscriptIds: []
  };
}

export function createDecisionContext(brief) {
  return {
    sessionId: brief.sessionId,
    projectId: brief.projectId,
    primaryDecisionGoal: brief.goal,
    decisionScope: inferDecisionScope(brief),
    knownConstraints: (brief.constraints ?? []).map((text) => ({
      id: sha16(`brief-constraint:${text}`),
      text,
      source: "brief"
    })),
    strategicPriorities: [],
    unacceptableCommitments: [],
    stakeholders: (brief.knownParticipants ?? []).map((participant) => ({
      id: sha16(`stakeholder:${participant.name}:${participant.role ?? ""}`),
      name: participant.name,
      role: participant.role,
      decisionPower: "unknown"
    }))
  };
}

function inferDecisionScope(brief) {
  const text = normalizeText([brief.goal, ...(brief.mustConfirm ?? []), ...(brief.risks ?? [])].join(" "));
  if (text.includes("scope") || text.includes("範圍")) return "scope";
  if (text.includes("owner") || text.includes("負責")) return "owner";
  if (text.includes("deadline") || text.includes("時程")) return "timeline";
  if (text.includes("rollback") || text.includes("技術")) return "technical_direction";
  return "unknown";
}

export function validateMeetingBrief(brief) {
  const errors = [];
  if (!brief || typeof brief !== "object") errors.push("brief must be an object");
  if (!brief?.sessionId) errors.push("brief.sessionId is required");
  if (!MEETING_TYPES.includes(brief?.meetingType)) errors.push(`brief.meetingType is invalid: ${brief?.meetingType}`);
  if (!brief?.goal) errors.push("brief.goal is required");
  if (!["direct", "gentle", "curious", "skeptical"].includes(brief?.preferredTone)) {
    errors.push("brief.preferredTone is invalid");
  }
  return errors;
}

export function validateExtractionOutput(output) {
  const errors = [];
  if (!output || typeof output !== "object") return ["output must be an object"];
  if (!output.meetingStatePatch || typeof output.meetingStatePatch !== "object") {
    errors.push("meetingStatePatch is required");
  }
  if (!output.decisionStatePatch || typeof output.decisionStatePatch !== "object") {
    errors.push("decisionStatePatch is required");
  }
  for (const field of ["addItems", "updateItems", "resolveItemIds", "evidenceTranscriptIds"]) {
    if (output.meetingStatePatch && !Array.isArray(output.meetingStatePatch[field])) {
      errors.push(`meetingStatePatch.${field} must be an array`);
    }
  }
  for (const field of ["addOptions", "updateOptions", "addRisks", "addMissingInputs", "evidenceTranscriptIds"]) {
    if (output.decisionStatePatch && !Array.isArray(output.decisionStatePatch[field])) {
      errors.push(`decisionStatePatch.${field} must be an array`);
    }
  }
  return errors;
}
