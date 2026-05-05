import assert from "node:assert/strict";
import test from "node:test";
import { groupTranscriptLines } from "../src/ui/transcriptDrawer.mjs";

test("transcript drawer groups consecutive lines from the same speaker", () => {
  const groups = groupTranscriptLines([
    { speaker: "對方 A", source: "system", text: "先確認權限。" },
    { speaker: "對方 A", source: "system", text: "再測 Windows。" },
    { speaker: "我", source: "mic", text: "我來處理。" },
    { speaker: "對方 A", source: "system", text: "記得補紀錄。" }
  ]);

  assert.equal(groups.length, 3);
  assert.equal(groups[0].speaker, "對方 A");
  assert.deepEqual(groups[0].lines.map((line) => line.text), ["先確認權限。", "再測 Windows。"]);
  assert.equal(groups[1].speaker, "我");
  assert.equal(groups[2].speaker, "對方 A");
});

test("transcript drawer keeps partial transcript in the current speaker group", () => {
  const groups = groupTranscriptLines([
    { speaker: "對方 A", source: "system", text: "先確認權限。" },
    { speaker: "對方 A", source: "system", text: "正在說", partial: true }
  ]);

  assert.equal(groups.length, 1);
  assert.equal(groups[0].partial, false);
  assert.equal(groups[0].lines.length, 2);
  assert.equal(groups[0].lines[1].partial, true);
});
