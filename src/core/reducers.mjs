import { canonicalKey, stateItemId } from "../domain/contracts.mjs";

const KIND_TO_COLLECTION = {
  open_question: "openQuestions",
  decision: "decisions",
  risk: "risks"
};

export function applyMeetingStatePatch(state, patch, sessionId = state.sessionId) {
  const next = structuredClone(state);
  if (patch.phaseChange && canAdvancePhase(next.phase, patch.phaseChange)) {
    next.phase = patch.phaseChange;
  }

  for (const incoming of patch.addItems ?? []) {
    upsertStateItem(next, incoming, sessionId);
  }
  for (const incoming of patch.updateItems ?? []) {
    upsertStateItem(next, incoming, sessionId);
  }
  for (const id of patch.resolveItemIds ?? []) {
    for (const collection of Object.values(KIND_TO_COLLECTION)) {
      const item = next[collection].find((candidate) => candidate.id === id);
      if (item) item.status = "resolved";
    }
  }
  return next;
}

export function applyDecisionStatePatch(state, patch, sessionId = state.sessionId) {
  const next = structuredClone(state);
  if (patch.currentDecision) next.currentDecision = patch.currentDecision;
  next.options = mergeById(next.options, patch.addOptions ?? [], sessionId, "option", "label", mergeDecisionOption);
  next.options = mergeById(next.options, patch.updateOptions ?? [], sessionId, "option", "label", mergeDecisionOption);
  next.unresolvedRisks = mergeById(next.unresolvedRisks, patch.addRisks ?? [], sessionId, "risk", "text", noRegressRisk);
  next.missingInputs = mergeMissingInputs(next.missingInputs, patch.addMissingInputs ?? [], sessionId);
  if (patch.readinessPatch) {
    next.readiness = {
      ...next.readiness,
      ...patch.readinessPatch
    };
  }
  next.evidenceTranscriptIds = unique([...(next.evidenceTranscriptIds ?? []), ...(patch.evidenceTranscriptIds ?? [])]);
  return next;
}

function upsertStateItem(state, incoming, sessionId) {
  const kind = incoming.kind ?? "open_question";
  const collection = KIND_TO_COLLECTION[kind] ?? "openQuestions";
  const key = incoming.canonicalKey ?? canonicalKey(kind, incoming.text, incoming.linkedPlaybookItemId);
  const id = incoming.id ?? stateItemId(sessionId, kind, key);
  const candidate = {
    ...incoming,
    id,
    canonicalKey: key,
    status: incoming.status ?? "open",
    confidence: clamp(incoming.confidence ?? 0.5),
    evidenceTranscriptIds: unique(incoming.evidenceTranscriptIds ?? []),
    firstSeenAtMs: incoming.firstSeenAtMs ?? 0,
    lastUpdatedAtMs: incoming.lastUpdatedAtMs ?? incoming.firstSeenAtMs ?? 0
  };
  const index = state[collection].findIndex((item) => item.id === id || item.canonicalKey === key);
  if (index === -1) {
    state[collection].push(candidate);
    return;
  }
  state[collection][index] = mergeStateItem(state[collection][index], candidate);
}

function mergeStateItem(existing, incoming) {
  // Fallbacks keep legacy persisted snapshots readable; new patches should
  // still normalize these fields in upsertStateItem before they reach merge.
  const existingConfidence = existing.confidence ?? 0;
  const incomingConfidence = incoming.confidence ?? 0;
  const existingFirstSeenAt = existing.firstSeenAtMs ?? incoming.firstSeenAtMs ?? 0;
  const incomingFirstSeenAt = incoming.firstSeenAtMs ?? existingFirstSeenAt;
  const existingLastUpdatedAt = existing.lastUpdatedAtMs ?? existingFirstSeenAt;
  const incomingLastUpdatedAt = incoming.lastUpdatedAtMs ?? incomingFirstSeenAt;
  return {
    ...existing,
    text: incomingConfidence >= existingConfidence ? incoming.text : existing.text,
    status: existing.status === "resolved" ? "resolved" : incoming.status,
    confidence: Math.max(existingConfidence, incomingConfidence),
    evidenceTranscriptIds: unique([...(existing.evidenceTranscriptIds ?? []), ...(incoming.evidenceTranscriptIds ?? [])]),
    firstSeenAtMs: Math.min(existingFirstSeenAt, incomingFirstSeenAt),
    lastUpdatedAtMs: Math.max(existingLastUpdatedAt, incomingLastUpdatedAt)
  };
}

function mergeById(existing, incoming, sessionId, kind, labelField, merge = (a, b) => ({ ...a, ...b })) {
  const next = [...existing];
  for (const item of incoming) {
    const key = item.canonicalKey ?? canonicalKey(kind, item[labelField] ?? item.text);
    const id = item.id ?? stateItemId(sessionId, kind, key);
    const candidate = { ...item, id, canonicalKey: key };
    const index = next.findIndex((entry) => entry.id === id || entry.canonicalKey === key);
    if (index === -1) next.push(candidate);
    else next[index] = merge(next[index], candidate);
  }
  return next;
}

function mergeMissingInputs(existing, incoming, sessionId) {
  return mergeById(existing, incoming, sessionId, "missing_input", "text", (a, b) => ({
    ...a,
    ...b,
    blocksDecision: a.blocksDecision || b.blocksDecision
  }));
}

function mergeDecisionOption(existing, incoming) {
  return {
    ...existing,
    ...incoming,
    evidenceTranscriptIds: unique([...(existing.evidenceTranscriptIds ?? []), ...(incoming.evidenceTranscriptIds ?? [])])
  };
}

function noRegressRisk(existing, incoming) {
  return {
    ...existing,
    ...incoming,
    owner: incoming.owner ?? existing.owner,
    severity: maxSeverity(existing.severity, incoming.severity),
    evidenceTranscriptIds: unique([...(existing.evidenceTranscriptIds ?? []), ...(incoming.evidenceTranscriptIds ?? [])])
  };
}

function maxSeverity(a = "low", b = "low") {
  const order = { low: 0, medium: 1, high: 2 };
  return order[b] > order[a] ? b : a;
}

function canAdvancePhase(current, next) {
  const order = ["opening", "discovery", "discussion", "decision", "wrap_up"];
  if (current === "unknown") return true;
  if (next === "unknown") return false;
  return order.indexOf(next) >= order.indexOf(current);
}

function clamp(value) {
  return Math.max(0, Math.min(1, value));
}

function unique(values) {
  return [...new Set(values.filter(Boolean))];
}
