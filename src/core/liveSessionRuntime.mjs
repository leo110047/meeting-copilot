import { SessionRuntime } from "./sessionRuntime.mjs";

export class LiveSessionRuntime extends SessionRuntime {
  async runTranscriptStream({ brief, transcriptStream, maxEvents = Infinity }) {
    const events = [];
    for await (const event of transcriptStream) {
      if (event.isFinal === false) continue;
      events.push(event);
      if (events.length >= maxEvents) break;
    }
    return this.runManual({ brief, transcriptEvents: events });
  }
}
