import { normalizeText } from "../domain/contracts.mjs";

export class ContextRetriever {
  constructor({ knowledgeStore, maxMemories = 12 } = {}) {
    this.knowledgeStore = knowledgeStore;
    this.maxMemories = maxMemories;
  }

  retrieve(input) {
    const query = normalizeText([
      input.decisionState.currentDecision,
      ...(input.decisionState.missingInputs ?? []).map((item) => item.text),
      ...(input.decisionState.unresolvedRisks ?? []).map((risk) => risk.text),
      ...(input.transcriptWindow ?? []).map((event) => event.text)
    ].join(" "));
    const projectContext = input.projectContext ?? this.knowledgeStore.getProject(input.meetingState.projectId);
    const scored = this.knowledgeStore.listMemories()
      .filter((memory) => !projectContext || !memory.projectId || memory.projectId === projectContext.id)
      .map((memory) => ({ memory, score: scoreMemory(query, memory) }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, input.maxMemories ?? this.maxMemories)
      .map((entry) => entry.memory);

    return {
      memories: scored,
      projectContext,
      participantProfiles: this.knowledgeStore.participantProfiles.filter((profile) => !profile.deletedAt),
      retrievalMethod: "keyword_prefilter",
      evidence: scored.flatMap((memory) => memory.evidenceTranscriptIds ?? [])
    };
  }
}

function scoreMemory(query, memory) {
  const haystack = normalizeText([memory.text, memory.kind].join(" "));
  const words = query.split(" ").filter((word) => word.length >= 2);
  let score = 0;
  for (const word of words) {
    if (haystack.includes(word)) score += 1;
  }
  if (memory.confidence >= 0.8) score += 0.5;
  return score;
}
