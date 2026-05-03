import { createDecisionContext, createDecisionState, createMeetingState, nowIso } from "../domain/contracts.mjs";
import { PlaybookCompiler } from "./playbookCompiler.mjs";
import { RuleBasedStateExtractionEngine } from "./stateExtractionEngine.mjs";
import { applyDecisionStatePatch, applyMeetingStatePatch } from "./reducers.mjs";
import { ReadinessEvaluator } from "./readinessEvaluator.mjs";
import { ContextRetriever } from "./contextRetriever.mjs";
import { CoachEngine } from "./coachEngine.mjs";
import { InterventionPolicy } from "./interventionPolicy.mjs";
import { extractMemoryCandidatesFromSession } from "./knowledgeStore.mjs";

export class SessionRuntime {
  constructor({ knowledgeStore, extractionEngine = new RuleBasedStateExtractionEngine(), policy = new InterventionPolicy(), eventBus } = {}) {
    this.knowledgeStore = knowledgeStore;
    this.extractionEngine = extractionEngine;
    this.policy = policy;
    this.eventBus = eventBus;
    this.playbookCompiler = new PlaybookCompiler();
    this.readinessEvaluator = new ReadinessEvaluator();
    this.coachEngine = new CoachEngine();
  }

  async runManual({ brief, transcriptEvents }) {
    const playbook = this.playbookCompiler.compile(brief);
    const decisionContext = createDecisionContext(brief);
    let meetingState = createMeetingState({ sessionId: brief.sessionId, projectId: brief.projectId, startedAt: brief.startedAt ?? nowIso() });
    let decisionState = createDecisionState({
      sessionId: brief.sessionId,
      decisionType: decisionContext.decisionScope,
      constraints: decisionContext.knownConstraints,
      stakeholders: decisionContext.stakeholders
    });
    const suggestions = [];
    const failures = [];
    const committedTranscript = [];

    for (const chunk of chunkFinalEvents(transcriptEvents, 3)) {
      committedTranscript.push(...chunk);
      await this.eventBus?.emit("transcript.final", { events: chunk });
      const layer3Context = this.contextSnapshot({ meetingState, decisionState, transcriptWindow: committedTranscript.slice(-8) });
      const extraction = await this.extractionEngine.extract({
        sessionId: brief.sessionId,
        priorMeetingState: meetingState,
        priorDecisionState: decisionState,
        newFinalTranscriptEvents: chunk,
        playbook,
        decisionContext,
        layer3Context
      });
      if (!extraction.ok) {
        failures.push(extraction.failure);
        continue;
      }
      meetingState = applyMeetingStatePatch(meetingState, extraction.output.meetingStatePatch, brief.sessionId);
      decisionState = applyDecisionStatePatch(decisionState, extraction.output.decisionStatePatch, brief.sessionId);
      decisionState.readiness = this.readinessEvaluator.evaluate(decisionState);

      const refreshedContext = this.contextSnapshot({ meetingState, decisionState, transcriptWindow: committedTranscript.slice(-8) });
      const move = this.coachEngine.generate({
        brief,
        decisionContext,
        decisionState,
        playbook,
        state: meetingState,
        recentTranscript: committedTranscript.slice(-8),
        recentTranscriptWindowMs: 120_000,
        priorSuggestions: suggestions,
        layer3Context: refreshedContext
      });
      const policyDecision = this.policy.decide({ move, meetingState, priorSuggestions: suggestions });
      if (policyDecision.shouldShow) {
        const suggestion = {
          ...move,
          sessionId: brief.sessionId,
          shownAt: nowIso(),
          policyReason: policyDecision.reason
        };
        suggestions.push(suggestion);
        meetingState.lastSuggestionAt = suggestion.shownAt;
        await this.eventBus?.emit("suggestion.shown", suggestion);
      }
    }

    const memoryCandidates = extractMemoryCandidatesFromSession({
      sessionId: brief.sessionId,
      projectId: brief.projectId,
      transcriptEvents: committedTranscript,
      decisionState
    });
    for (const candidate of memoryCandidates) this.knowledgeStore.addMemoryCandidate(candidate);

    return {
      brief,
      playbook,
      decisionContext,
      meetingState,
      decisionState,
      suggestions,
      failures,
      memoryCandidates,
      transcriptEvents: committedTranscript
    };
  }

  contextSnapshot({ meetingState, decisionState, transcriptWindow }) {
    const retriever = new ContextRetriever({ knowledgeStore: this.knowledgeStore });
    const retrieved = retriever.retrieve({ meetingState, decisionState, transcriptWindow, maxMemories: 12 });
    return {
      projectContext: retrieved.projectContext,
      strategicContext: this.knowledgeStore.strategicContext,
      relevantMemories: retrieved.memories,
      memories: retrieved.memories,
      participantProfiles: retrieved.participantProfiles,
      domainKnowledge: this.knowledgeStore.domainKnowledge[0],
      politicalSignals: []
    };
  }
}

function chunkFinalEvents(events, size) {
  const finals = events.filter((event) => event.isFinal !== false);
  const chunks = [];
  for (let i = 0; i < finals.length; i += size) chunks.push(finals.slice(i, i + size));
  return chunks;
}
