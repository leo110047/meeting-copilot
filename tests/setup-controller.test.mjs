import assert from "node:assert/strict";
import test from "node:test";
import { createSetupController } from "../src/ui/setupController.mjs";

test("setup controller keeps counterparty notes out of export prep context but includes them in AI prep context", () => {
  const counterpartyContext = "對方可能會把正式版 scope 放進 demo review。";
  const controller = createSetupController({
    elements: {
      setupContext: { value: "這場只確認 demo scope。" },
      counterpartyContext: { value: counterpartyContext },
      counterpartyContextMeta: { textContent: "" },
      setupDropZone: {},
      setupContextMeta: { textContent: "" },
      droppedFileCount: { textContent: "" },
      prepDictationButton: { textContent: "", classList: { toggle() {} } },
      prepSummary: { textContent: "", innerHTML: "" }
    },
    nativeInvoke: undefined,
    nativeListen: undefined,
    canStartWithAi: () => false,
    syncStartButtonAvailability: () => {},
    textProviderAuthenticated: () => false,
    selectedTextProviderId: () => "codex",
    selectedTextProviderLabel: () => "Codex",
    logAppError: () => {},
    formatError: (error) => String(error),
    escapeHtml: (value) => String(value)
  });

  assert.equal(controller.combinedPrepContext(), "這場只確認 demo scope。");
  assert.equal(controller.counterpartyContext(), counterpartyContext);
  assert.doesNotMatch(controller.combinedPrepContext(), /對方背景與可能會議策略/);
  assert.match(controller.aiPrepContext(), /對方背景與可能會議策略/);
  assert.match(controller.aiPrepContext(), new RegExp(counterpartyContext));
});
