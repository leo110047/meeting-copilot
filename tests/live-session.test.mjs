import assert from "node:assert/strict";
import test from "node:test";
import { EventBus } from "../src/core/eventBus.mjs";
import { KnowledgeStore } from "../src/core/knowledgeStore.mjs";
import { LiveSessionRuntime } from "../src/core/liveSessionRuntime.mjs";
import { MockStreamingTranscriber } from "../src/providers/sttProvider.mjs";
import { loadFixture } from "../src/replay/replayHarness.mjs";

test("LiveSessionRuntime consumes STT transcript stream without manual paste", async () => {
  const fixture = loadFixture("mixed_scope_owner");
  const transcriber = new MockStreamingTranscriber({ fixtureEvents: fixture.transcriptEvents });
  const runtime = new LiveSessionRuntime({
    knowledgeStore: new KnowledgeStore({
      projects: [fixture.projectContext],
      memories: fixture.knowledgeMemories
    }),
    eventBus: new EventBus()
  });

  const result = await runtime.runTranscriptStream({
    brief: fixture.brief,
    transcriptStream: transcriber.start()
  });

  assert.ok(result.transcriptEvents.length > 0);
  assert.ok(result.suggestions.length >= 1);
  assert.equal(result.decisionState.readiness.safeToDecide, false);
  assert.ok(result.decisionState.missingInputs.some((input) => input.kind === "owner"));
});
