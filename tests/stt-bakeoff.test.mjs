import assert from "node:assert/strict";
import test from "node:test";
import { runSttBakeoff } from "../src/providers/sttBakeoff.mjs";
import { BrowserSpeechTranscriberContract, MockStreamingTranscriber, speechRecognitionResultToTranscriptEvent } from "../src/providers/sttProvider.mjs";
import { loadFixture } from "../src/replay/replayHarness.mjs";

test("STT bakeoff produces ProviderEvalResult shape", async () => {
  const fixture = loadFixture("mixed_scope_owner");
  const results = await runSttBakeoff({
    expectedTranscriptEvents: fixture.transcriptEvents,
    candidates: [new MockStreamingTranscriber({ fixtureEvents: fixture.transcriptEvents })]
  });

  assert.equal(results.length, 1);
  assert.equal(results[0].provider, "mock-streaming-stt");
  assert.equal(results[0].wordErrorNotes, "exact transcript fixture match");
  assert.equal(results[0].setupComplexity, "low");
});

test("Browser speech contract reports unavailable runtime without pretending STT exists", () => {
  const health = new BrowserSpeechTranscriberContract().getHealth({});
  assert.equal(health.ready, false);
  assert.match(health.lastError, /SpeechRecognition/);
});

test("speechRecognitionResultToTranscriptEvent keeps STT separate from text decision provider", () => {
  const event = speechRecognitionResultToTranscriptEvent({
    resultText: "owner 還沒定",
    sessionId: "s1",
    index: 1,
    elapsedMs: 5000
  });
  assert.equal(event.source, "mic");
  assert.equal(event.language, "mixed");
  assert.equal(event.isFinal, true);
});
