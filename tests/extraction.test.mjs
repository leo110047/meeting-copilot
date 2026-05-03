import assert from "node:assert/strict";
import test from "node:test";
import { StateExtractionEngine } from "../src/core/stateExtractionEngine.mjs";
import { BrokenProvider, StaticJsonProvider } from "../src/providers/modelProvider.mjs";

const validPatch = {
  meetingStatePatch: { addItems: [], updateItems: [], resolveItemIds: [], evidenceTranscriptIds: [] },
  decisionStatePatch: { addOptions: [], updateOptions: [], addRisks: [], addMissingInputs: [], evidenceTranscriptIds: [] }
};

test("StateExtractionEngine accepts structured patch output", async () => {
  const engine = new StateExtractionEngine({ provider: new StaticJsonProvider({ output: validPatch }) });
  const result = await engine.extract({ sessionId: "s1" });
  assert.equal(result.ok, true);
});

test("StateExtractionEngine repairs JSON wrapped in provider prose", async () => {
  const engine = new StateExtractionEngine({ provider: new StaticJsonProvider({ output: `ok ${JSON.stringify(validPatch)} done` }) });
  const result = await engine.extract({ sessionId: "s1" });
  assert.equal(result.ok, true);
});

test("StateExtractionEngine logs malformed JSON without polluting state", async () => {
  const engine = new StateExtractionEngine({ provider: new BrokenProvider({ id: "bad-json", output: "not json" }) });
  const result = await engine.extract({ sessionId: "s1" });
  assert.equal(result.ok, false);
  assert.equal(result.failure.failureKind, "malformed_json");
});

test("StateExtractionEngine logs schema validation failure", async () => {
  const engine = new StateExtractionEngine({ provider: new StaticJsonProvider({ output: { meetingStatePatch: {} } }) });
  const result = await engine.extract({ sessionId: "s1" });
  assert.equal(result.ok, false);
  assert.equal(result.failure.failureKind, "schema_validation");
});

test("StateExtractionEngine logs timeout and API error", async () => {
  const timeout = await new StateExtractionEngine({ provider: new BrokenProvider({ id: "timeout-provider", errorKind: "timeout" }), timeoutMs: 5 }).extract({ sessionId: "s1" });
  assert.equal(timeout.ok, false);
  assert.equal(timeout.failure.failureKind, "timeout");
  const api = await new StateExtractionEngine({ provider: new BrokenProvider({ id: "api-provider", errorKind: "api_error" }) }).extract({ sessionId: "s1" });
  assert.equal(api.ok, false);
  assert.equal(api.failure.failureKind, "api_error");
});

test("StateExtractionEngine rejects low confidence committed patch", async () => {
  const output = {
    meetingStatePatch: {
      addItems: [{ text: "weak", confidence: 0.1, evidenceTranscriptIds: ["t1"] }],
      updateItems: [],
      resolveItemIds: [],
      evidenceTranscriptIds: ["t1"]
    },
    decisionStatePatch: { addOptions: [], updateOptions: [], addRisks: [], addMissingInputs: [], evidenceTranscriptIds: [] }
  };
  const result = await new StateExtractionEngine({ provider: new StaticJsonProvider({ output }) }).extract({ sessionId: "s1" });
  assert.equal(result.ok, false);
  assert.equal(result.failure.failureKind, "low_confidence");
});
