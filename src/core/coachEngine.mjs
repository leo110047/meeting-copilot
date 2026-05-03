import { sha16, normalizeText } from "../domain/contracts.mjs";

export class CoachEngine {
  generate(input) {
    const readiness = input.decisionState.readiness;
    const recentText = normalizeText((input.recentTranscript ?? []).map((event) => event.text).join(" "));
    const evidenceTranscriptIds = [
      ...(input.decisionState.evidenceTranscriptIds ?? []),
      ...(input.recentTranscript ?? []).map((event) => event.id)
    ];

    const layer3Move = layer3Suggestion(input, evidenceTranscriptIds);
    if (layer3Move) return layer3Move;

    if (input.decisionState.currentDecision && readiness && !readiness.safeToDecide && readiness.blockers?.length > 0) {
      const blocker = readiness.blockers[0];
      return move({
        kind: "identify_missing_input",
        text: `先不要定案，這裡還缺「${trimForSpeech(blocker)}」。可以先問誰負責、deadline 跟驗收標準。`,
        reason: `Decision readiness score ${readiness.score.toFixed(2)}，仍有 blocker`,
        priority: "high",
        confidence: 0.86,
        evidenceTranscriptIds
      });
    }

    if (/(先這樣|就照這個|commit|承諾)/i.test(recentText) && /(還沒|沒定|不清楚|先不要)/i.test(recentText)) {
      return move({
        kind: "confirm_commitment",
        text: "這句聽起來快變承諾了，可以先補一句：今天只確認方向，owner、deadline、驗收標準還要另外定。",
        reason: "Transcript contains commitment language while required inputs remain unclear",
        priority: "high",
        confidence: 0.82,
        evidenceTranscriptIds
      });
    }

    return undefined;
  }
}

function layer3Suggestion(input, evidenceTranscriptIds) {
  const memory = (input.layer3Context.relevantMemories ?? input.layer3Context.memories ?? [])
    .find((candidate) => candidate.confidence >= 0.72 && (candidate.evidenceTranscriptIds ?? []).length > 0);
  const project = input.layer3Context.projectContext;
  if (!memory || !input.decisionState.currentDecision) return undefined;

  const transcriptText = normalizeText((input.recentTranscript ?? []).map((event) => event.text).join(" "));
  if (!/(決定|scope|v1|deadline|owner|承諾|先這樣)/i.test(transcriptText)) return undefined;

  return move({
    kind: "challenge_assumption",
    text: `先停一下：這和先前脈絡「${trimForSpeech(memory.text)}」有關，建議確認今天是否要取代舊決策。`,
    reason: `Used Layer 3 memory${project ? ` from project ${project.name}` : ""}`,
    priority: "high",
    confidence: 0.9,
    evidenceTranscriptIds: [...new Set([...evidenceTranscriptIds, ...(memory.evidenceTranscriptIds ?? [])])]
  });
}

function move({ kind, text, reason, confidence, priority, evidenceTranscriptIds }) {
  return {
    id: sha16(`${kind}:${text}:${evidenceTranscriptIds.join(",")}`),
    kind,
    text: text.slice(0, 160),
    reason,
    confidence,
    priority,
    evidenceTranscriptIds: [...new Set(evidenceTranscriptIds.filter(Boolean))]
  };
}

function trimForSpeech(text) {
  return String(text).replace(/\s+/g, " ").slice(0, 42);
}
