import assert from "node:assert/strict";
import test from "node:test";
import {
  isUsefulPartialTranscript,
  shouldCommitIdlePartial,
  shouldCommitReplacedPartial
} from "../src/ui/partialTranscriptPolicy.mjs";

test("partial transcript policy ignores one-character noise", () => {
  assert.equal(isUsefulPartialTranscript("三"), false);
  assert.equal(shouldCommitIdlePartial({ text: "三" }, 5000), false);
});

test("partial transcript policy commits stable useful partial text", () => {
  assert.equal(shouldCommitIdlePartial({ text: "功能壞掉了" }, 2300), true);
  assert.equal(shouldCommitIdlePartial({ text: "功能壞掉了" }, 500), false);
});

test("partial transcript policy keeps cumulative partial updates in the same draft", () => {
  assert.equal(
    shouldCommitReplacedPartial(
      { text: "功能壞", source: "mic" },
      { text: "功能壞掉了", source: "mic" },
      1500
    ),
    false
  );
});

test("partial transcript policy commits previous draft when recognition starts a new phrase", () => {
  assert.equal(
    shouldCommitReplacedPartial(
      { text: "功能壞掉了", source: "mic" },
      { text: "下一個問題", source: "mic" },
      1500
    ),
    true
  );
});

test("partial transcript policy commits previous draft when the source changes", () => {
  assert.equal(
    shouldCommitReplacedPartial(
      { text: "系統音訊剛剛講了這件事", source: "system" },
      { text: "我想補充", source: "mic" },
      1500
    ),
    true
  );
});
