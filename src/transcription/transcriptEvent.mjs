import { nowIso } from "../domain/contracts.mjs";

export function createTranscriptEvent({
  id,
  sessionId,
  source = "unknown",
  speaker,
  speakerConfidence = 0.4,
  language,
  startedAtMs,
  endedAtMs,
  text,
  isFinal = true
}) {
  return {
    id,
    sessionId,
    source,
    speaker,
    speakerConfidence,
    language: language ?? detectLanguage(text),
    languageSegments: undefined,
    startedAtMs,
    endedAtMs,
    text,
    isFinal,
    receivedAt: nowIso()
  };
}

export function detectLanguage(text) {
  const hasChinese = /[\u4e00-\u9fff]/.test(text);
  const hasEnglish = /[a-zA-Z]/.test(text);
  if (hasChinese && hasEnglish) return "mixed";
  if (hasChinese) return "zh-TW";
  if (hasEnglish) return "en";
  return "unknown";
}

export async function* transcriptEventsFromFixture(events, { delayMs = 0 } = {}) {
  for (const event of events) {
    if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
    yield event;
  }
}
