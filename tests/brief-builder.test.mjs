import assert from "node:assert/strict";
import test from "node:test";
import { buildLiveMeetingBrief } from "../src/ui/briefBuilder.mjs";

test("live meeting brief keeps counterparty context outside the generic context truncation", () => {
  const longPrepContext = `${"一般背景".repeat(300)}\n這段很長，會被 generic context 截斷。`;
  const counterpartyContext = "對方是 PM，可能會把正式版 scope 放進 demo review。";
  const brief = buildLiveMeetingBrief({
    series: undefined,
    prepContext: longPrepContext,
    counterpartyContext,
    makeSessionId: () => "native_test",
    nowIso: () => "2026-05-10T00:00:00.000Z"
  });

  assert.equal(brief.sessionId, "native_test");
  assert.ok(brief.constraints.some((line) => line.includes(counterpartyContext)));
  assert.ok(brief.constraints.some((line) => line.startsWith("會議背景：")));
  assert.ok(brief.constraints.find((line) => line.startsWith("會議背景：")).length <= "會議背景：".length + 1400);
});

test("live meeting brief does not duplicate counterparty context when prep context already has the legacy marker", () => {
  const counterpartyContext = "對方是 PM，可能會把正式版 scope 放進 demo review。";
  const brief = buildLiveMeetingBrief({
    series: undefined,
    prepContext: `一般背景\n\n對方背景與可能會議策略\n${counterpartyContext}\n\n檔案 agenda.md\n只談 demo`,
    counterpartyContext,
    makeSessionId: () => "native_test",
    nowIso: () => "2026-05-10T00:00:00.000Z"
  });
  const renderedConstraints = brief.constraints.join("\n");

  assert.equal(countOccurrences(renderedConstraints, counterpartyContext), 1);
});

test("live meeting brief keeps the explicit fallback when counterparty context is empty", () => {
  const brief = buildLiveMeetingBrief({
    series: undefined,
    prepContext: "只確認 demo scope",
    counterpartyContext: "",
    makeSessionId: () => "native_test",
    nowIso: () => "2026-05-10T00:00:00.000Z"
  });

  assert.ok(brief.constraints.includes("未提供對方背景與會議策略。"));
  assert.deepEqual(brief.mustConfirm, ["owner", "deadline", "驗收標準", "rollback plan"]);
  assert.deepEqual(brief.risks, ["未定義 owner/deadline 就做承諾", "demo scope 和正式版 scope 混在一起"]);
});

test("live meeting brief preserves selected meeting series title and goal", () => {
  const brief = buildLiveMeetingBrief({
    series: { id: "series_demo", title: "Demo Review" },
    prepContext: "只確認 demo scope",
    counterpartyContext: "",
    makeSessionId: () => "native_test",
    nowIso: () => "2026-05-10T00:00:00.000Z"
  });

  assert.equal(brief.title, "Demo Review");
  assert.match(brief.goal, /延續「Demo Review」追蹤本場會議決策/);
  assert.ok(brief.constraints.includes("本場選用既有會議脈絡：Demo Review"));
});

function countOccurrences(text, needle) {
  return text.split(needle).length - 1;
}
