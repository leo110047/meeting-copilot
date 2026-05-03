export class ReadinessEvaluator {
  evaluate(decisionState) {
    const blockers = [];
    for (const input of decisionState.missingInputs ?? []) {
      if (input.blocksDecision) blockers.push(input.text);
    }
    for (const risk of decisionState.unresolvedRisks ?? []) {
      if (risk.severity === "high" && !risk.owner) blockers.push(`高風險缺 owner：${risk.text}`);
    }
    const hasDecision = Boolean(decisionState.currentDecision);
    const score = hasDecision ? Math.max(0, 1 - blockers.length * 0.22) : 0;
    return {
      score,
      safeToDecide: hasDecision && blockers.length === 0 && score >= 0.72,
      blockers,
      evidenceTranscriptIds: decisionState.evidenceTranscriptIds ?? []
    };
  }
}
