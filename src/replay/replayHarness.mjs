import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { EventBus } from "../core/eventBus.mjs";
import { KnowledgeStore } from "../core/knowledgeStore.mjs";
import { SessionRuntime } from "../core/sessionRuntime.mjs";
import { buildSharedMeetingArtifact } from "../shared/sharedArtifactBuilder.mjs";

export function loadFixture(name) {
  const path = resolve("fixtures", `${name}.json`);
  return JSON.parse(readFileSync(path, "utf8"));
}

export async function replayFixture(name, { provider = "local", promptVersion = "rule.v1" } = {}) {
  const fixture = loadFixture(name);
  const knowledgeStore = new KnowledgeStore({
    memories: fixture.knowledgeMemories ?? [],
    projects: fixture.projectContext ? [fixture.projectContext] : [],
    participantProfiles: fixture.participantProfiles ?? [],
    domainKnowledge: fixture.domainKnowledge ? [fixture.domainKnowledge] : [],
    strategicContext: fixture.strategicContext
  });
  const runtime = new SessionRuntime({ knowledgeStore, eventBus: new EventBus() });
  const result = await runtime.runManual({ brief: fixture.brief, transcriptEvents: fixture.transcriptEvents });
  const metrics = evaluateReplay({ fixture, suggestions: result.suggestions });
  const sharedArtifact = buildSharedMeetingArtifact({
    sharedMeetingId: `${fixture.brief.sessionId}_shared`,
    linkedLocalSessionIds: [fixture.brief.sessionId],
    transcriptEvents: fixture.transcriptEvents,
    meetingState: result.meetingState,
    decisionState: result.decisionState,
    approvedMemories: []
  });
  return {
    fixture: name,
    provider,
    promptVersion,
    suggestions: result.suggestions,
    finalDecisionState: result.decisionState,
    finalMeetingState: result.meetingState,
    memoryCandidates: result.memoryCandidates,
    sharedArtifact,
    metrics
  };
}

export function evaluateReplay({ fixture, suggestions }) {
  const expected = fixture.expectedInterventionMoments ?? [];
  const high = expected.filter((moment) => moment.severity === "high");
  const matchedExpectedIds = new Set();
  const falsePositives = [];

  for (const suggestion of suggestions) {
    const match = expected.find((moment) => {
      const afterMoment = suggestion.evidenceTranscriptIds.includes(moment.transcriptEventId) ||
        transcriptIndex(fixture, suggestion.evidenceTranscriptIds.at(-1)) >= transcriptIndex(fixture, moment.transcriptEventId);
      const textMatch = moment.acceptableSuggestionPatterns.some((pattern) => new RegExp(pattern, "i").test(suggestion.text));
      return afterMoment && textMatch;
    });
    if (match) matchedExpectedIds.add(match.id);
    else falsePositives.push(suggestion.id);
  }

  const falseNegatives = expected.filter((moment) => !matchedExpectedIds.has(moment.id)).map((moment) => moment.id);
  const highMatched = high.filter((moment) => matchedExpectedIds.has(moment.id)).length;
  const precision = suggestions.length === 0 ? 1 : (suggestions.length - falsePositives.length) / suggestions.length;
  const recallAtHigh = high.length === 0 ? 1 : highMatched / high.length;
  const baselineHits = (fixture.baselineChecklist ?? []).filter((item) => expected.some((moment) => new RegExp(item.pattern, "i").test(moment.expectedReason))).length;

  return {
    suggestionCount: suggestions.length,
    precision,
    recallAtHigh,
    falseNegativeIds: falseNegatives,
    falsePositiveSuggestionIds: falsePositives,
    baselineChecklistHits: baselineHits,
    baselineDelta: matchedExpectedIds.size - baselineHits,
    interventionTimeline: suggestions.map((suggestion) => ({
      suggestionId: suggestion.id,
      evidenceTranscriptIds: suggestion.evidenceTranscriptIds,
      text: suggestion.text,
      reason: suggestion.reason
    }))
  };
}

function transcriptIndex(fixture, id) {
  return fixture.transcriptEvents.findIndex((event) => event.id === id);
}
