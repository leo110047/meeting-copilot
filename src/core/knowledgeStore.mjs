import { sha16, nowIso, normalizeText } from "../domain/contracts.mjs";

export class KnowledgeStore {
  constructor({ memories = [], memoryCandidates = [], projects = [], participantProfiles = [], domainKnowledge = [], strategicContext } = {}) {
    this.memories = [...memories];
    this.memoryCandidates = [...memoryCandidates];
    this.projects = [...projects];
    this.participantProfiles = [...participantProfiles];
    this.domainKnowledge = [...domainKnowledge];
    this.strategicContext = strategicContext;
  }

  upsertProject(project) {
    this.projects = upsert(this.projects, project);
    return project;
  }

  upsertParticipantProfile(profile) {
    this.participantProfiles = upsert(this.participantProfiles, { sensitive: true, ...profile });
    return profile;
  }

  addMemoryCandidate(candidate) {
    const next = {
      id: candidate.id ?? sha16(`candidate:${candidate.kind}:${candidate.text}`),
      reviewStatus: "pending",
      createdAt: nowIso(),
      ...candidate
    };
    this.memoryCandidates = upsert(this.memoryCandidates, next);
    return next;
  }

  approveMemoryCandidate(candidateId) {
    const candidate = this.memoryCandidates.find((item) => item.id === candidateId);
    if (!candidate) return undefined;
    candidate.reviewStatus = "approved";
    candidate.reviewedAt = nowIso();
    const memory = {
      id: sha16(`memory:${candidate.kind}:${candidate.text}:${candidate.sourceSessionIds?.join(",")}`),
      projectId: candidate.suggestedProjectId,
      participantIds: [],
      kind: candidate.kind,
      text: candidate.text,
      sourceSessionIds: candidate.sourceSessionIds ?? [],
      evidenceTranscriptIds: candidate.evidenceTranscriptIds ?? [],
      createdAt: nowIso(),
      confidence: candidate.confidence
    };
    this.memories = upsert(this.memories, memory);
    return memory;
  }

  getProject(projectId) {
    return this.projects.find((project) => project.id === projectId);
  }

  listMemories() {
    return [...this.memories];
  }
}

export function extractMemoryCandidatesFromSession({ sessionId, projectId, transcriptEvents, decisionState }) {
  const candidates = [];
  if (decisionState.currentDecision) {
    candidates.push({
      id: sha16(`decision:${sessionId}:${decisionState.currentDecision}`),
      kind: "decision",
      text: decisionState.currentDecision,
      sourceSessionIds: [sessionId],
      evidenceTranscriptIds: decisionState.evidenceTranscriptIds ?? [],
      suggestedProjectId: projectId,
      confidence: 0.82,
      reviewStatus: "pending"
    });
  }
  for (const risk of decisionState.unresolvedRisks ?? []) {
    candidates.push({
      id: sha16(`risk:${sessionId}:${risk.text}`),
      kind: "risk",
      text: risk.text,
      sourceSessionIds: [sessionId],
      evidenceTranscriptIds: risk.evidenceTranscriptIds ?? [],
      suggestedProjectId: projectId,
      confidence: risk.severity === "high" ? 0.8 : 0.66,
      reviewStatus: "pending"
    });
  }
  const transcriptText = normalizeText(transcriptEvents.map((event) => event.text).join(" "));
  if (/下次|follow up|待確認|未解|再確認/i.test(transcriptText)) {
    candidates.push({
      id: sha16(`open_issue:${sessionId}:${transcriptText.slice(0, 80)}`),
      kind: "open_issue",
      text: "本場仍有待確認項目，需要下次追蹤",
      sourceSessionIds: [sessionId],
      evidenceTranscriptIds: transcriptEvents.map((event) => event.id),
      suggestedProjectId: projectId,
      confidence: 0.62,
      reviewStatus: "pending"
    });
  }
  return candidates;
}

function upsert(items, item) {
  const index = items.findIndex((candidate) => candidate.id === item.id);
  if (index === -1) return [...items, item];
  const next = [...items];
  next[index] = { ...next[index], ...item };
  return next;
}
