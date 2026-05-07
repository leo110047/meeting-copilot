import assert from "node:assert/strict";
import test from "node:test";
import { upsertTranscriptEventInPlace } from "../src/ui/transcriptState.mjs";

test("transcript state replaces promoted preview with later native final instead of duplicating", () => {
  const events = [];
  const lines = [];
  const preview = {
    id: "preview_session_1",
    text: "這是最後一句。",
    source: "mic",
    persistenceStatus: "pending"
  };
  const final = {
    id: "native_final_hash",
    text: "這是最後一句",
    source: "mic",
    persistenceStatus: "saved"
  };

  assert.equal(upsertTranscriptEventInPlace(events, lines, preview).reason, "inserted");
  assert.equal(upsertTranscriptEventInPlace(events, lines, final).reason, "semantic_replace");
  assert.equal(events.length, 1);
  assert.equal(events[0].id, "native_final_hash");
  assert.equal(events[0].persistenceStatus, "saved");
  assert.deepEqual(lines, ["這是最後一句。"]);
});

test("transcript state keeps same text from different sources separate when timing differs", () => {
  const events = [];
  const lines = [];
  upsertTranscriptEventInPlace(events, lines, { id: "mic_1", text: "好", source: "mic", startedAtMs: 1000, endedAtMs: 2000 });
  upsertTranscriptEventInPlace(events, lines, { id: "system_1", text: "好", source: "system", startedAtMs: 10000, endedAtMs: 11000 });
  assert.equal(events.length, 2);
});

test("transcript state keeps short same-text replies even when timing is close", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_short",
      text: "好",
      source: "system",
      startedAtMs: 1000,
      endedAtMs: 1600
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_reply",
      text: "好",
      source: "mic",
      startedAtMs: 2200,
      endedAtMs: 2800
    }).reason,
    "inserted"
  );
  assert.equal(events.length, 2);
});

test("transcript state keeps same-text cross-source events separate without time overlap", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_phrase",
      text: "如果你有在關注",
      source: "system",
      startedAtMs: 1000,
      endedAtMs: 2000
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_later_phrase",
      text: "如果你有在關注",
      source: "mic",
      startedAtMs: 2600,
      endedAtMs: 3600
    }).reason,
    "inserted"
  );
  assert.equal(events.length, 2);
});

test("transcript state suppresses mixed capture echo by preferring system audio", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_echo",
      text: "如果你有在關注",
      source: "mic",
      startedAtMs: 1000,
      endedAtMs: 3000
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_final",
      text: "如果你有在關注",
      source: "system",
      startedAtMs: 1200,
      endedAtMs: 3200,
      persistenceStatus: "saved"
    }).reason,
    "cross_source_echo_replace"
  );
  assert.equal(events.length, 1);
  assert.equal(events[0].id, "system_final");
  assert.equal(events[0].source, "system");

  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_late_echo",
      text: "如果你有在關注",
      source: "mic",
      startedAtMs: 1300,
      endedAtMs: 3300
    }).reason,
    "cross_source_echo_suppressed"
  );
  assert.equal(events.length, 1);
  assert.equal(events[0].source, "system");
});

test("transcript state keeps echo detection thresholds centralized", async () => {
  const source = await import("node:fs/promises")
    .then(({ readFile }) => readFile(new URL("../src/ui/transcriptState.mjs", import.meta.url), "utf8"));
  assert.match(source, /EXACT_ECHO_MIN_COMPACT_CHARS/);
  assert.match(source, /FUZZY_ECHO_BIGRAM_SIMILARITY/);
  assert.doesNotMatch(source, /if \(minLength < FUZZY_ECHO_MIN_COMPACT_CHARS\) return false;/);
});

test("transcript state treats full-width spaces as compactable whitespace", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_full_width_space",
      text: "如果　你有在關注",
      source: "mic",
      startedAtMs: 1000,
      endedAtMs: 3000
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_no_space",
      text: "如果你有在關注",
      source: "system",
      startedAtMs: 1100,
      endedAtMs: 3100
    }).reason,
    "cross_source_echo_replace"
  );
  assert.equal(events.length, 1);
});

test("transcript state suppresses high-overlap fuzzy mixed-capture echoes", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_fuzzy_echo",
      text: "現在每間公司都在搶想去 AI 的人很重要",
      source: "mic",
      startedAtMs: 1000,
      endedAtMs: 4200
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_fuzzy_echo",
      text: "現在美間公司都在搶想去 AI 的人很重要",
      source: "system",
      startedAtMs: 1120,
      endedAtMs: 4300,
      persistenceStatus: "saved"
    }).reason,
    "cross_source_echo_replace"
  );
  assert.equal(events.length, 1);
  assert.equal(events[0].id, "system_fuzzy_echo");
  assert.equal(events[0].text, "現在美間公司都在搶想去 AI 的人很重要");
});

test("transcript state catches observed ASR-divergent echoes only when timing is tightly aligned", () => {
  const events = [];
  const lines = [];
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "system_divergent_echo",
      text: "現在每間公司都在搶想去 AI 的人很重要",
      source: "system",
      startedAtMs: 3000,
      endedAtMs: 6500
    }).reason,
    "inserted"
  );
  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_divergent_echo",
      text: "現在美金公司都在搶想去 A&M 的人",
      source: "mic",
      startedAtMs: 3120,
      endedAtMs: 6400
    }).reason,
    "cross_source_echo_suppressed"
  );
  assert.equal(events.length, 1);

  assert.equal(
    upsertTranscriptEventInPlace(events, lines, {
      id: "mic_similar_later",
      text: "現在美金公司都在搶想去 A&M 的人",
      source: "mic",
      startedAtMs: 7600,
      endedAtMs: 10900
    }).reason,
    "inserted"
  );
  assert.equal(events.length, 2);
});
