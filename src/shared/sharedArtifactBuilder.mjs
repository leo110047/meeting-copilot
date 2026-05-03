import { sha16, nowIso } from "../domain/contracts.mjs";

export function buildSharedMeetingArtifact({ sharedMeetingId, linkedLocalSessionIds, transcriptEvents, meetingState, decisionState, approvedMemories = [] }) {
  const decisions = [
    ...meetingState.decisions.map((item) => ({
      id: item.id,
      text: item.text,
      evidenceTranscriptIds: item.evidenceTranscriptIds,
      confidence: item.confidence
    })),
    ...(decisionState.currentDecision
      ? [{
          id: sha16(`shared-decision:${decisionState.sessionId}:${decisionState.currentDecision}`),
          text: decisionState.currentDecision,
          evidenceTranscriptIds: decisionState.evidenceTranscriptIds,
          confidence: decisionState.readiness.score
        }]
      : [])
  ];
  const artifact = {
    id: sha16(`shared:${sharedMeetingId}:${linkedLocalSessionIds.join(",")}`),
    sharedMeetingId,
    linkedLocalSessionIds,
    transcriptEvents,
    decisions,
    actionItems: meetingState.actionItems,
    unresolvedQuestions: meetingState.openQuestions.filter((item) => item.status !== "resolved"),
    agreedMemories: approvedMemories.filter((memory) => memory.reviewStatus === "approved" || memory.approved === true),
    conflictCandidates: [],
    createdAt: nowIso(),
    updatedAt: nowIso()
  };
  assertNoPrivateCopilotState(artifact);
  return artifact;
}

export function assertNoPrivateCopilotState(artifact) {
  const serialized = JSON.stringify(artifact);
  const privateKeys = [
    "meetingBrief",
    "strategicContext",
    "privateSuggestions",
    "feedback",
    "participantProfiles",
    "politicalSignals",
    "ParticipantProfile",
    "PoliticalSignal"
  ];
  for (const key of privateKeys) {
    if (serialized.includes(key)) {
      throw new Error(`shared artifact contains private copilot state: ${key}`);
    }
  }
}
