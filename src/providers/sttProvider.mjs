import { createTranscriptEvent } from "../transcription/transcriptEvent.mjs";

export class MockStreamingTranscriber {
  constructor({ id = "mock-streaming-stt", fixtureEvents = [], delayMs = 0 } = {}) {
    this.id = id;
    this.kind = "stt";
    this.roles = ["stt"];
    this.fixtureEvents = fixtureEvents;
    this.delayMs = delayMs;
    this.started = false;
  }

  async *start() {
    this.started = true;
    for (const event of this.fixtureEvents) {
      if (this.delayMs > 0) await new Promise((resolve) => setTimeout(resolve, this.delayMs));
      yield event;
    }
  }

  async stop() {
    this.started = false;
  }

  getHealth() {
    return {
      providerId: this.id,
      kind: "stt",
      ready: true,
      supportsStreaming: true,
      supportsDiarization: false,
      supportsSourceHints: true,
      lastError: undefined
    };
  }
}

export class BrowserSpeechTranscriberContract {
  constructor({ id = "browser-speech-recognition" } = {}) {
    this.id = id;
    this.kind = "stt";
    this.roles = ["stt"];
  }

  getHealth(runtime = globalThis) {
    const supported = Boolean(runtime.SpeechRecognition || runtime.webkitSpeechRecognition);
    return {
      providerId: this.id,
      kind: "stt",
      ready: supported,
      supportsStreaming: supported,
      supportsDiarization: false,
      supportsSourceHints: false,
      lastError: supported ? undefined : "Browser SpeechRecognition is not available in this runtime"
    };
  }
}

export function speechRecognitionResultToTranscriptEvent({ resultText, sessionId, index, elapsedMs, source = "mic" }) {
  return createTranscriptEvent({
    id: `live_${sessionId}_${index}`,
    sessionId,
    source,
    speakerConfidence: 0.35,
    startedAtMs: Math.max(0, elapsedMs - 3000),
    endedAtMs: elapsedMs,
    text: resultText,
    isFinal: true
  });
}
