export function renderTranscriptDrawerView({
  elements,
  transcriptDrawerOpen,
  transcriptEvents,
  transcriptLines,
  currentPartialTranscript,
  escapeHtml,
  detectUiLanguage
}) {
  const { liveTranscript, transcriptDrawerToggle, transcriptDrawerCount, transcriptPreview, transcriptFull } = elements;
  const lines = currentTranscriptLines({ transcriptEvents, transcriptLines, currentPartialTranscript, detectUiLanguage });
  const latest = lines.slice(-3);
  transcriptDrawerToggle.textContent = transcriptDrawerOpen ? "收合" : "展開";
  transcriptDrawerToggle.setAttribute("aria-expanded", String(transcriptDrawerOpen));
  transcriptFull.hidden = !transcriptDrawerOpen;
  const finalCount = transcriptEvents.length > 0 ? transcriptEvents.length : transcriptLines.length;
  transcriptDrawerCount.textContent = currentPartialTranscript?.text ? `${finalCount} 句｜記錄中` : `${finalCount} 句`;
  liveTranscript.classList.toggle("expanded", transcriptDrawerOpen);
  transcriptPreview.innerHTML = latest.length > 0
    ? latest.map((line) => renderTranscriptLine(line, escapeHtml)).join("")
    : '<p class="empty-line">最近 3 句會顯示在這裡。</p>';
  transcriptFull.innerHTML = transcriptDrawerOpen
    ? lines.map((line) => renderTranscriptLine(line, escapeHtml)).join("")
    : "";
  transcriptPreview.scrollTop = transcriptPreview.scrollHeight;
  transcriptFull.scrollTop = transcriptFull.scrollHeight;
}

export function transcriptSpeakerLabel(event) {
  if (event.speaker) return event.speaker;
  if (event.source === "mic") return "我";
  if (event.source === "system") return "系統音訊";
  return "未知";
}

function currentTranscriptLines({ transcriptEvents, transcriptLines, currentPartialTranscript, detectUiLanguage }) {
  const partialLine = currentPartialTranscript?.text
    ? {
        text: currentPartialTranscript.text,
        speaker: transcriptSpeakerLabel(currentPartialTranscript),
        source: currentPartialTranscript.source ?? "unknown",
        language: currentPartialTranscript.language,
        partial: true,
        index: transcriptEvents.length
      }
    : undefined;
  if (transcriptEvents.length > 0) {
    const lines = transcriptEvents.map((event, index) => ({
      text: event.text,
      speaker: transcriptSpeakerLabel(event),
      source: event.source ?? "unknown",
      language: event.language,
      index
    }));
    if (partialLine) lines.push(partialLine);
    return lines;
  }
  const lines = transcriptLines.map((text, index) => ({
    text,
    speaker: "未知",
    source: "unknown",
    language: detectUiLanguage(text),
    index
  }));
  if (partialLine) lines.push(partialLine);
  return lines;
}

function renderTranscriptLine(line, escapeHtml) {
  return `<p class="transcript-line${line.partial ? " partial" : ""}"><span>${escapeHtml(line.speaker)}</span><span>${escapeHtml(line.text)}${line.partial ? " <em>記錄中</em>" : ""}</span></p>`;
}
