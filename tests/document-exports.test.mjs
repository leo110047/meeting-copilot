import assert from "node:assert/strict";
import test from "node:test";
import { buildAiSummaryDocument, renderAiSummaryMarkdown, renderTranscriptText } from "../src/ui/documentExports.mjs";

test("AI summary export marks documents that have prep context but no transcript", () => {
  const document = buildAiSummaryDocument({
    title: "即時會議",
    sessionId: "session_1",
    generatedAt: "2026-05-04T00:00:00.000Z",
    prepContext: "檔案 agenda.md\n只討論活動交接",
    transcript: [],
    summary: {
      keyPoints: ["只討論活動交接"],
      decisionsAndOpenQuestions: [],
      suggestedActions: []
    },
    suggestions: [],
    decisionState: null
  });

  const markdown = renderAiSummaryMarkdown(document);
  assert.match(markdown, /Notice: 本文件沒有錄音逐字稿/);
  assert.match(markdown, /## 會前資料/);
  assert.match(markdown, /只討論活動交接/);
});

test("transcript export marks locally visible transcript rows that failed persistence", () => {
  const text = renderTranscriptText({
    title: "即時會議 逐字稿",
    sessionId: "session_1",
    generatedAt: "2026-05-04T00:00:00.000Z",
    transcript: [{ text: "最後一句", speaker: "我", source: "mic", persistenceStatus: "failed" }]
  });
  assert.match(text, /\[我\] 最後一句 \[未儲存\]/);
});

test("transcript export falls back to source labels", () => {
  const text = renderTranscriptText({
    title: "即時會議 逐字稿",
    sessionId: "session_1",
    generatedAt: "2026-05-04T00:00:00.000Z",
    transcript: [{ text: "我這邊看得到畫面", source: "system" }]
  });
  assert.match(text, /\[系統音訊\] 我這邊看得到畫面/);
});
