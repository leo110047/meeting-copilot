export function downloadMeetingArtifact(format, { artifact, downloadState, logAppError }) {
  const summaryDocument = buildAiSummaryDocument(artifact);
  const transcriptDocument = buildTranscriptDocument(artifact);
  const baseName = `meeting-copilot-${artifact.sessionId}`;
  const files = {
    "summary-json": {
      name: `${baseName}-ai-summary.json`,
      type: "application/json;charset=utf-8",
      content: JSON.stringify(summaryDocument, null, 2)
    },
    "summary-markdown": {
      name: `${baseName}-ai-summary.md`,
      type: "text/markdown;charset=utf-8",
      content: renderAiSummaryMarkdown(summaryDocument)
    },
    "transcript-json": {
      name: `${baseName}-transcript.json`,
      type: "application/json;charset=utf-8",
      content: JSON.stringify(transcriptDocument, null, 2)
    },
    "transcript-txt": {
      name: `${baseName}-transcript.txt`,
      type: "text/plain;charset=utf-8",
      content: renderTranscriptText(transcriptDocument)
    }
  };
  if (format === "summary-pdf") {
    openPrintableDocument({
      title: `${artifact.title} AI 整理`,
      filenameHint: `${baseName}-ai-summary.pdf`,
      bodyHtml: renderAiSummaryHtml(summaryDocument),
      downloadState,
      logAppError
    });
    return;
  }
  if (format === "transcript-pdf") {
    openPrintableDocument({
      title: `${artifact.title} 逐字稿`,
      filenameHint: `${baseName}-transcript.pdf`,
      bodyHtml: renderTranscriptHtml(transcriptDocument),
      downloadState,
      logAppError
    });
    return;
  }
  const file = files[format];
  if (!file) return;
  const url = URL.createObjectURL(new Blob([file.content], { type: file.type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = file.name;
  anchor.click();
  URL.revokeObjectURL(url);
  downloadState.textContent = `已準備下載 ${file.name}`;
}

export function buildAiSummaryDocument(artifact) {
  return {
    title: `${artifact.title} AI 整理`,
    sessionId: artifact.sessionId,
    generatedAt: artifact.generatedAt,
    prepContext: artifact.prepContext,
    transcriptCount: artifact.transcript.length,
    recordingNotice: artifact.transcript.length > 0 ? null : "本文件沒有錄音逐字稿；內容只來自會前資料或本機整理狀態。",
    summary: artifact.summary,
    suggestions: artifact.suggestions,
    decisionState: artifact.decisionState
  };
}

export function buildTranscriptDocument(artifact) {
  return {
    title: `${artifact.title} 逐字稿`,
    sessionId: artifact.sessionId,
    generatedAt: artifact.generatedAt,
    transcript: artifact.transcript
  };
}

export function renderAiSummaryMarkdown(document) {
  const section = (title, items) => [`## ${title}`, ...(items.length > 0 ? items.map((item) => `- ${item}`) : ["- 本場沒有足夠資訊產生此區塊。"])].join("\n");
  return [
    `# ${document.title}`,
    `Session: ${document.sessionId}`,
    `Generated: ${document.generatedAt}`,
    document.recordingNotice ? `Notice: ${document.recordingNotice}` : "",
    document.prepContext ? `\n## 會前資料\n${document.prepContext}` : "",
    "",
    section("本場重點", document.summary.keyPoints),
    "",
    section("決策與待確認", document.summary.decisionsAndOpenQuestions),
    "",
    section("建議動作", document.summary.suggestedActions)
  ].join("\n");
}

export function renderTranscriptText(document) {
  return [
    document.title,
    `Session: ${document.sessionId}`,
    `Generated: ${document.generatedAt}`,
    "",
    ...(document.transcript.length > 0
      ? document.transcript.map((line, index) => `${index + 1}. ${line.text}${line.persistenceStatus === "failed" ? " [未儲存]" : ""}`)
      : ["本場沒有收到逐字稿。"])
  ].join("\n");
}

export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function renderAiSummaryHtml(document) {
  const section = (title, items) => `
    <section>
      <h2>${escapeHtml(title)}</h2>
      <ul>${(items.length > 0 ? items : ["本場沒有足夠資訊產生此區塊。"]).map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>
    </section>`;
  return [
    `<h1>${escapeHtml(document.title)}</h1>`,
    `<p>Session: ${escapeHtml(document.sessionId)}<br />Generated: ${escapeHtml(document.generatedAt)}</p>`,
    document.recordingNotice ? `<p><strong>${escapeHtml(document.recordingNotice)}</strong></p>` : "",
    document.prepContext ? `<section><h2>會前資料</h2><pre>${escapeHtml(document.prepContext)}</pre></section>` : "",
    section("本場重點", document.summary.keyPoints),
    section("決策與待確認", document.summary.decisionsAndOpenQuestions),
    section("建議動作", document.summary.suggestedActions)
  ].join("");
}

function renderTranscriptHtml(document) {
  const lines = document.transcript.length > 0
    ? document.transcript.map((line, index) => `<p><strong>${index + 1}.</strong> ${escapeHtml(line.text)}${line.persistenceStatus === "failed" ? " <em>未儲存</em>" : ""}</p>`).join("")
    : "<p>本場沒有收到逐字稿。</p>";
  return [
    `<h1>${escapeHtml(document.title)}</h1>`,
    `<p>Session: ${escapeHtml(document.sessionId)}<br />Generated: ${escapeHtml(document.generatedAt)}</p>`,
    `<section>${lines}</section>`
  ].join("");
}

function openPrintableDocument({ title, filenameHint, bodyHtml, downloadState, logAppError }) {
  const printWindow = window.open("", "_blank");
  if (!printWindow) {
    logAppError("download.pdf_popup_blocked", "Printable PDF window was blocked", { title, filenameHint }, "warning");
    downloadState.textContent = "無法開啟 PDF 列印視窗，請允許彈出視窗後再試。";
    return;
  }
  printWindow.document.write(`<!doctype html>
    <html lang="zh-Hant">
      <head>
        <meta charset="UTF-8" />
        <title>${escapeHtml(title)}</title>
        <style>
          body { color: #111; font: 14px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 36px; }
          h1 { font-size: 28px; line-height: 1.2; margin: 0 0 12px; }
          h2 { font-size: 18px; margin: 24px 0 8px; }
          p { margin: 0 0 10px; }
          li { margin: 4px 0; }
          @page { margin: 18mm; }
        </style>
      </head>
      <body>${bodyHtml}</body>
    </html>`);
  printWindow.document.close();
  printWindow.focus();
  printWindow.print();
  downloadState.textContent = `已開啟 PDF 列印視窗，建議檔名：${filenameHint}`;
}
