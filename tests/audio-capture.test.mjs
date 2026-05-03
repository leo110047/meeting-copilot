import assert from "node:assert/strict";
import test from "node:test";
import { createAudioFrame, EnergyVoiceActivityGate, NativeAudioCaptureProvider } from "../src/audio/audioCaptureProvider.mjs";

test("AudioFrame contract validates native audio metadata without storing payload", () => {
  const frame = createAudioFrame({
    sessionId: "s1",
    source: "mic",
    durationMs: 20,
    sampleRate: 16000,
    channels: 1,
    payloadRef: "ring://s1/mic/1"
  });

  assert.equal(frame.encoding, "pcm16");
  assert.equal(frame.payloadRef, "ring://s1/mic/1");
});

test("platform audio capture contracts expose only implemented capture paths", () => {
  const mac = new NativeAudioCaptureProvider({ platform: "darwin" }).getHealth();
  const windows = new NativeAudioCaptureProvider({ platform: "win32" }).getHealth();

  assert.equal(mac.microphoneProvider, "AVAudioEngine/CoreAudio");
  assert.equal(mac.systemAudioProvider, "ScreenCaptureKit");
  assert.equal(windows.microphoneProvider, "WASAPI capture");
  assert.equal(windows.supportsSystemAudio, true);
  assert.equal(windows.systemAudioProvider, "WASAPI loopback");
});

test("energy VAD suppresses silence and allows speech-like samples", () => {
  const gate = new EnergyVoiceActivityGate({ threshold: 0.02 });
  const silence = gate.classify({ pcm16Samples: new Int16Array(1600), sampleRate: 16000 });
  const speech = gate.classify({ pcm16Samples: new Int16Array(1600).fill(2400), sampleRate: 16000 });

  assert.equal(silence.speech, false);
  assert.equal(speech.speech, true);
});
