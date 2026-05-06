import assert from "node:assert/strict";
import test from "node:test";
import { groupTranscriptLines, renderTranscriptDrawerView } from "../src/ui/transcriptDrawer.mjs";

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

test("expanded transcript drawer shows the full transcript without duplicating preview", () => {
  const elements = createTranscriptDrawerElements();

  renderTranscriptDrawerView({
    elements,
    transcriptDrawerOpen: true,
    transcriptEvents: [],
    transcriptLines: [],
    currentPartialTranscript: { text: "三", source: "mic" },
    escapeHtml,
    detectUiLanguage: () => "zh-TW"
  });

  assert.equal(elements.transcriptPreview.hidden, true);
  assert.equal(elements.transcriptPreview.innerHTML, "");
  assert.equal(elements.transcriptFull.hidden, false);
  assert.equal(elements.listeningCurtain.classList.has("transcript-expanded"), true);
  assert.equal(countOccurrences(elements.transcriptFull.innerHTML, "記錄中"), 1);
  assert.equal(elements.transcriptDrawerCount.textContent, "0 句｜記錄中");
});

test("collapsed transcript drawer shows only the recent preview", () => {
  const elements = createTranscriptDrawerElements();

  renderTranscriptDrawerView({
    elements,
    transcriptDrawerOpen: false,
    transcriptEvents: [
      { text: "先確認權限。", source: "system", speaker: "對方 A" },
      { text: "我來測。", source: "mic", speaker: "我" }
    ],
    transcriptLines: [],
    currentPartialTranscript: undefined,
    escapeHtml,
    detectUiLanguage: () => "zh-TW"
  });

  assert.equal(elements.transcriptPreview.hidden, false);
  assert.equal(elements.transcriptFull.hidden, true);
  assert.equal(elements.listeningCurtain.classList.has("transcript-expanded"), false);
  assert.match(elements.transcriptPreview.innerHTML, /先確認權限/);
  assert.equal(elements.transcriptFull.innerHTML, "");
  assert.equal(elements.transcriptDrawerCount.textContent, "2 句");
});

test("expanded transcript drawer renders every finalized transcript event", () => {
  const elements = createTranscriptDrawerElements();
  const transcriptEvents = Array.from({ length: 9 }, (_, index) => ({
    text: `第 ${index + 1} 句`,
    source: index % 2 === 0 ? "mic" : "system",
    speaker: index % 2 === 0 ? "我" : "對方 A"
  }));

  renderTranscriptDrawerView({
    elements,
    transcriptDrawerOpen: true,
    transcriptEvents,
    transcriptLines: [],
    currentPartialTranscript: undefined,
    escapeHtml,
    detectUiLanguage: () => "zh-TW"
  });

  for (const event of transcriptEvents) {
    assert.match(elements.transcriptFull.innerHTML, new RegExp(event.text));
  }
  assert.doesNotMatch(elements.transcriptPreview.innerHTML, /第 7 句|第 8 句|第 9 句/);
  assert.equal(elements.transcriptDrawerCount.textContent, "9 句");
});

function createTranscriptDrawerElements() {
  return {
    listeningCurtain: { classList: createClassList() },
    liveTranscript: { classList: { toggle() {} } },
    transcriptDrawerToggle: {
      textContent: "",
      attributes: {},
      setAttribute(name, value) {
        this.attributes[name] = value;
      }
    },
    transcriptDrawerCount: { textContent: "" },
    transcriptPreview: createScrollElement(),
    transcriptFull: createScrollElement()
  };
}

function createClassList() {
  const values = new Set();
  return {
    toggle(name, force) {
      if (force) values.add(name);
      else values.delete(name);
    },
    has(name) {
      return values.has(name);
    }
  };
}

function createScrollElement() {
  return {
    hidden: false,
    innerHTML: "",
    scrollHeight: 0,
    scrollTop: 0
  };
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function countOccurrences(value, needle) {
  return String(value).split(needle).length - 1;
}
