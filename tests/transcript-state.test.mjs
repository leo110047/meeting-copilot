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

test("transcript state keeps same text from different sources separate", () => {
  const events = [];
  const lines = [];
  upsertTranscriptEventInPlace(events, lines, { id: "mic_1", text: "好", source: "mic" });
  upsertTranscriptEventInPlace(events, lines, { id: "system_1", text: "好", source: "system" });
  assert.equal(events.length, 2);
});
