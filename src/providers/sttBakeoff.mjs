export async function runSttBakeoff({ candidates, expectedTranscriptEvents }) {
  const results = [];
  for (const candidate of candidates) {
    const started = Date.now();
    const events = [];
    try {
      for await (const event of candidate.start()) {
        events.push(event);
      }
      const latencyMs = Date.now() - started;
      results.push({
        provider: candidate.id,
        model: candidate.id,
        avgLatencyMs: events.length === 0 ? latencyMs : latencyMs / events.length,
        p95LatencyMs: latencyMs,
        wordErrorNotes: compareTranscriptText(expectedTranscriptEvents, events),
        speakerSourceBehavior: describeSourceBehavior(events),
        costPer30MinMeetingUsd: 0,
        failureModes: [],
        setupComplexity: "low"
      });
    } catch (error) {
      results.push({
        provider: candidate.id,
        model: candidate.id,
        avgLatencyMs: 0,
        p95LatencyMs: 0,
        wordErrorNotes: "failed before transcript output",
        speakerSourceBehavior: "unknown",
        costPer30MinMeetingUsd: 0,
        failureModes: [error.message],
        setupComplexity: "medium"
      });
    } finally {
      await candidate.stop?.();
    }
  }
  return results;
}

function compareTranscriptText(expected, actual) {
  const expectedText = expected.map((event) => event.text).join(" ");
  const actualText = actual.map((event) => event.text).join(" ");
  if (expectedText === actualText) return "exact transcript fixture match";
  const expectedTokens = tokenSet(expectedText);
  const actualTokens = tokenSet(actualText);
  const missed = [...expectedTokens].filter((token) => !actualTokens.has(token)).slice(0, 8);
  return missed.length === 0 ? "same key tokens, text differs" : `missed key tokens: ${missed.join(", ")}`;
}

function describeSourceBehavior(events) {
  const sources = [...new Set(events.map((event) => event.source))];
  return sources.length > 0 ? `source hints: ${sources.join(", ")}` : "no source hints";
}

function tokenSet(text) {
  return new Set(String(text).toLowerCase().split(/[^\p{L}\p{N}_-]+/u).filter((token) => token.length > 1));
}
