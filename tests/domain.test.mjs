import assert from "node:assert/strict";
import test from "node:test";
import { canonicalKey, createDecisionState, createMeetingState, validateMeetingBrief } from "../src/domain/contracts.mjs";
import { PlaybookCompiler } from "../src/core/playbookCompiler.mjs";
import { applyDecisionStatePatch, applyMeetingStatePatch } from "../src/core/reducers.mjs";
import { ReadinessEvaluator } from "../src/core/readinessEvaluator.mjs";

test("MeetingBrief validation catches invalid required fields", () => {
  assert.deepEqual(validateMeetingBrief({}), [
    "brief.sessionId is required",
    "brief.meetingType is invalid: undefined",
    "brief.goal is required",
    "brief.preferredTone is invalid"
  ]);
});

test("PlaybookCompiler does not invent goals and caps intervention rules", () => {
  const brief = {
    sessionId: "s1",
    meetingType: "requirement_scoping",
    goal: "決定 v1 scope",
    mustConfirm: ["owner", "deadline", "驗收標準", "scope", "rollback", "budget", "extra"],
    risks: ["scope creep", "owner missing"],
    constraints: ["只能 demo"],
    knownParticipants: [],
    preferredTone: "direct"
  };
  const playbook = new PlaybookCompiler().compile(brief);
  assert.equal(playbook.objective, brief.goal);
  assert.ok(playbook.interventionRules.length <= 8);
  assert.ok(playbook.interventionRules.every((rule) => !/new goal/i.test(rule.suggestedMove)));
});

test("canonical keys are stable for semantic labels", () => {
  assert.equal(
    canonicalKey("risk", " Owner 還沒定 ", "must-owner"),
    canonicalKey("risk", "owner 還沒定", "must-owner")
  );
});

test("MeetingStateReducer dedups and does not regress resolved state", () => {
  const initial = createMeetingState({ sessionId: "s1" });
  const patch = {
    addItems: [
      { kind: "risk", text: "owner 還沒定", status: "open", confidence: 0.8, evidenceTranscriptIds: ["t1"], firstSeenAtMs: 1, lastUpdatedAtMs: 1 }
    ],
    updateItems: [],
    resolveItemIds: [],
    evidenceTranscriptIds: ["t1"]
  };
  const once = applyMeetingStatePatch(initial, patch, "s1");
  const twice = applyMeetingStatePatch(once, patch, "s1");
  assert.equal(twice.risks.length, 1);
  const resolved = applyMeetingStatePatch(twice, { addItems: [], updateItems: [], resolveItemIds: [twice.risks[0].id], evidenceTranscriptIds: ["t2"] }, "s1");
  const regressed = applyMeetingStatePatch(resolved, patch, "s1");
  assert.equal(regressed.risks[0].status, "resolved");
});

test("DecisionStateReducer preserves owner and blocks unsafe decisions", () => {
  const initial = createDecisionState({ sessionId: "s1" });
  const patched = applyDecisionStatePatch(initial, {
    currentDecision: "commit v1",
    addOptions: [],
    updateOptions: [],
    addRisks: [{ text: "rollback 沒有 owner", owner: "Leo", severity: "high", evidenceTranscriptIds: ["t1"] }],
    addMissingInputs: [{ kind: "deadline", text: "deadline missing", blocksDecision: true }],
    evidenceTranscriptIds: ["t1"]
  }, "s1");
  const updated = applyDecisionStatePatch(patched, {
    addOptions: [],
    updateOptions: [],
    addRisks: [{ text: "rollback 沒有 owner", severity: "low", evidenceTranscriptIds: ["t2"] }],
    addMissingInputs: [],
    evidenceTranscriptIds: ["t2"]
  }, "s1");
  assert.equal(updated.unresolvedRisks[0].owner, "Leo");
  assert.equal(updated.unresolvedRisks[0].severity, "high");
  const readiness = new ReadinessEvaluator().evaluate(updated);
  assert.equal(readiness.safeToDecide, false);
  assert.ok(readiness.blockers.some((blocker) => blocker.includes("deadline")));
});
