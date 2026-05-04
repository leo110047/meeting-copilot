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
    const state = this.createSessionState(brief);
    for (const chunk of chunkFinalEvents(transcriptEvents, 3)) {
      await this.ingestFinalEvents(state, chunk);
    }
    return this.resultFromSessionState(state);
  }

  createSessionState(brief) {
    const playbook = this.playbookCompiler.compile(brief);
    const decisionContext = createDecisionContext(brief);
    const meetingState = createMeetingState({ sessionId: brief.sessionId, projectId: brief.projectId, startedAt: brief.startedAt ?? nowIso() });
    const decisionState = createDecisionState({
      sessionId: brief.sessionId,
      decisionType: decisionContext.decisionScope,
      constraints: decisionContext.knownConstraints,
      stakeholders: decisionContext.stakeholders
    });
    return {
      brief,
      playbook,
      decisionContext,
      meetingState,
      decisionState,
      suggestions: [],
      failures: [],
      committedTranscript: []
    };
  }

  async ingestFinalEvents(state, events) {
    const chunk = events.filter((event) => event.isFinal !== false);
    if (chunk.length === 0) return [];
    state.committedTranscript.push(...chunk);
    await this.eventBus?.emit("transcript.final", { events: chunk });
    const layer3Context = this.contextSnapshot({
      meetingState: state.meetingState,
      decisionState: state.decisionState,
      transcriptWindow: state.committedTranscript.slice(-8)
    });
    const extraction = await this.extractionEngine.extract({
      sessionId: state.brief.sessionId,
      priorMeetingState: state.meetingState,
      priorDecisionState: state.decisionState,
      newFinalTranscriptEvents: chunk,
      playbook: state.playbook,
      decisionContext: state.decisionContext,
      layer3Context
    });
    if (!extraction.ok) {
      state.failures.push(extraction.failure);
      return [];
    }
    state.meetingState = applyMeetingStatePatch(state.meetingState, extraction.output.meetingStatePatch, state.brief.sessionId);
    state.decisionState = applyDecisionStatePatch(state.decisionState, extraction.output.decisionStatePatch, state.brief.sessionId);
    state.decisionState.readiness = this.readinessEvaluator.evaluate(state.decisionState);

    const refreshedContext = this.contextSnapshot({
      meetingState: state.meetingState,
      decisionState: state.decisionState,
      transcriptWindow: state.committedTranscript.slice(-8)
    });
    const move = this.coachEngine.generate({
      brief: state.brief,
      decisionContext: state.decisionContext,
      decisionState: state.decisionState,
      playbook: state.playbook,
      state: state.meetingState,
      recentTranscript: state.committedTranscript.slice(-8),
      recentTranscriptWindowMs: 120_000,
      priorSuggestions: state.suggestions,
      layer3Context: refreshedContext
    });
    const policyDecision = this.policy.decide({ move, meetingState: state.meetingState, priorSuggestions: state.suggestions });
    if (!policyDecision.shouldShow) return [];
    const suggestion = {
      ...move,
      sessionId: state.brief.sessionId,
      shownAt: nowIso(),
      policyReason: policyDecision.reason
    };
    state.suggestions.push(suggestion);
    state.meetingState.lastSuggestionAt = suggestion.shownAt;
    await this.eventBus?.emit("suggestion.shown", suggestion);
    return [suggestion];
  }

  resultFromSessionState(state) {
    const memoryCandidates = extractMemoryCandidatesFromSession({
      sessionId: state.brief.sessionId,
      projectId: state.brief.projectId,
      transcriptEvents: state.committedTranscript,
      decisionState: state.decisionState
    });
    for (const candidate of memoryCandidates) this.knowledgeStore.addMemoryCandidate(candidate);

    return {
      brief: state.brief,
      playbook: state.playbook,
      decisionContext: state.decisionContext,
      meetingState: state.meetingState,
      decisionState: state.decisionState,
      suggestions: state.suggestions,
      failures: state.failures,
      memoryCandidates,
      transcriptEvents: state.committedTranscript
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
