import { platform as hostPlatform } from "node:os";
import { makeId } from "../domain/contracts.mjs";

export const AUDIO_SOURCES = ["mic", "system", "mixed", "unknown"];

export function createAudioFrame({
  id = makeId("audio_frame"),
  sessionId,
  source = "unknown",
  capturedAtMs = Date.now(),
  durationMs,
  sampleRate,
  channels,
  encoding = "pcm16",
  payloadRef
}) {
  const errors = [];
  if (!sessionId) errors.push("sessionId is required");
  if (!AUDIO_SOURCES.includes(source)) errors.push(`source is invalid: ${source}`);
  if (!Number.isFinite(durationMs) || durationMs <= 0) errors.push("durationMs must be positive");
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) errors.push("sampleRate must be positive");
  if (!Number.isInteger(channels) || channels <= 0) errors.push("channels must be positive integer");
  if (encoding !== "pcm16") errors.push("encoding must be pcm16");
  if (!payloadRef) errors.push("payloadRef is required");
  if (errors.length > 0) {
    const error = new Error(errors.join("; "));
    error.code = "INVALID_AUDIO_FRAME";
    throw error;
  }
  return { id, sessionId, source, capturedAtMs, durationMs, sampleRate, channels, encoding, payloadRef };
}

export class NativeAudioCaptureProvider {
  constructor({ platform = hostPlatform() } = {}) {
    this.platform = normalizePlatform(platform);
  }

  getHealth() {
    const capabilities = platformCapabilities(this.platform);
    return {
      providerId: `${this.platform}-native-audio-capture`,
      kind: "audio_capture",
      ready: capabilities.ready,
      supportsMicrophone: capabilities.supportsMicrophone,
      supportsSystemAudio: capabilities.supportsSystemAudio,
      microphoneProvider: capabilities.microphoneProvider,
      systemAudioProvider: capabilities.systemAudioProvider,
      lastError: capabilities.lastError
    };
  }
}

export class EnergyVoiceActivityGate {
  constructor({ threshold = 0.02, preRollMs = 500 } = {}) {
    this.threshold = threshold;
    this.preRollMs = preRollMs;
  }

  classify({ pcm16Samples, sampleRate, source = "unknown", startedAtMs = 0 }) {
    if (!pcm16Samples || pcm16Samples.length === 0) {
      return { speech: false, speechProbability: 0, source, startedAtMs, endedAtMs: startedAtMs };
    }
    const energy = rootMeanSquare(pcm16Samples);
    const probability = Math.max(0, Math.min(1, energy / this.threshold));
    const durationMs = Math.round((pcm16Samples.length / sampleRate) * 1000);
    return {
      speech: probability >= 1,
      speechProbability: probability,
      source,
      startedAtMs: Math.max(0, startedAtMs - this.preRollMs),
      endedAtMs: startedAtMs + durationMs
    };
  }
}

function normalizePlatform(platform) {
  if (platform === "darwin" || platform === "macos") return "macos";
  if (platform === "win32" || platform === "windows") return "windows";
  return "unsupported";
}

function platformCapabilities(platform) {
  if (platform === "macos") {
    return {
      ready: true,
      supportsMicrophone: true,
      supportsSystemAudio: true,
      microphoneProvider: "AVAudioEngine/CoreAudio",
      systemAudioProvider: "ScreenCaptureKit",
      lastError: undefined
    };
  }
  if (platform === "windows") {
    return {
      ready: true,
      supportsMicrophone: true,
      supportsSystemAudio: true,
      microphoneProvider: "WASAPI capture",
      systemAudioProvider: "WASAPI loopback",
      lastError: undefined
    };
  }
  return {
    ready: false,
    supportsMicrophone: false,
    supportsSystemAudio: false,
    microphoneProvider: "none",
    systemAudioProvider: "none",
    lastError: `unsupported platform: ${platform}`
  };
}

function rootMeanSquare(samples) {
  let total = 0;
  for (const sample of samples) {
    const normalized = sample / 32768;
    total += normalized * normalized;
  }
  return Math.sqrt(total / samples.length);
}
