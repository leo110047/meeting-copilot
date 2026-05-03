import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { replayFixture } from "../src/replay/replayHarness.mjs";

const fixtureIndex = JSON.parse(readFileSync("fixtures/index.json", "utf8"));

test("golden transcript fixtures replay with precision/recall metrics", async () => {
  for (const fixture of fixtureIndex.fixtures) {
    const report = await replayFixture(fixture);
    assert.ok("precision" in report.metrics);
    assert.ok("recallAtHigh" in report.metrics);
    assert.ok(Array.isArray(report.metrics.falseNegativeIds));
  }
});

test("Layer 3 fixture uses cross-session memory for suggestion", async () => {
  const report = await replayFixture("requirement_scoping_layer3");
  assert.ok(report.suggestions.some((suggestion) => /先前脈絡|上一場|舊決策/.test(suggestion.text)));
  assert.equal(report.metrics.recallAtHigh, 1);
  assert.ok(report.metrics.baselineDelta > 0);
});

test("false positive fixture does not produce noisy suggestion", async () => {
  const report = await replayFixture("mixed_false_positive");
  assert.equal(report.suggestions.length, 0);
  assert.equal(report.metrics.precision, 1);
});

test("shared artifact excludes private copilot state", async () => {
  const report = await replayFixture("requirement_scoping_layer3");
  const serialized = JSON.stringify(report.sharedArtifact);
  assert.equal(serialized.includes("privateSuggestions"), false);
  assert.equal(serialized.includes("participantProfiles"), false);
  assert.equal(serialized.includes("politicalSignals"), false);
  assert.equal(serialized.includes("strategicContext"), false);
});
