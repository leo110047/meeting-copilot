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
  const groups = groupTranscriptLines(lines);
  const latest = groups.slice(-3);
  transcriptDrawerToggle.textContent = transcriptDrawerOpen ? "收合" : "展開";
  transcriptDrawerToggle.setAttribute("aria-expanded", String(transcriptDrawerOpen));
  transcriptFull.hidden = !transcriptDrawerOpen;
  const finalCount = transcriptEvents.length > 0 ? transcriptEvents.length : transcriptLines.length;
  transcriptDrawerCount.textContent = currentPartialTranscript?.text ? `${finalCount} 句｜記錄中` : `${finalCount} 句`;
  liveTranscript.classList.toggle("expanded", transcriptDrawerOpen);
  transcriptPreview.innerHTML = latest.length > 0
    ? latest.map((group) => renderTranscriptGroup(group, escapeHtml)).join("")
    : '<p class="empty-line">最近對話會顯示在這裡。</p>';
  transcriptFull.innerHTML = transcriptDrawerOpen
    ? groups.map((group) => renderTranscriptGroup(group, escapeHtml)).join("")
    : "";
  transcriptPreview.scrollTop = transcriptPreview.scrollHeight;
  transcriptFull.scrollTop = transcriptFull.scrollHeight;
}

export function transcriptSpeakerLabel(event) {
  if (event.speaker) return event.speaker;
  if (event.source === "mic") return "我";
  if (event.source === "system") return "系統音訊";
  return "未標記來源";
}

export function groupTranscriptLines(lines) {
  const groups = [];
  for (const line of lines) {
    const previous = groups.at(-1);
    if (previous && previous.speaker === line.speaker && previous.source === line.source) {
      previous.lines.push(line);
      continue;
    }
    groups.push({
      speaker: line.speaker,
      source: line.source,
      partial: Boolean(line.partial),
      lines: [line]
    });
  }
  return groups;
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
    speaker: "未標記來源",
    source: "unknown",
    language: detectUiLanguage(text),
    index
  }));
  if (partialLine) lines.push(partialLine);
  return lines;
}

function renderTranscriptGroup(group, escapeHtml) {
  const count = group.lines.length > 1 ? `<span class="transcript-count">${group.lines.length} 句</span>` : "";
  const body = group.lines
    .map((line) => `<p class="transcript-line${line.partial ? " partial" : ""}">${escapeHtml(line.text)}${line.partial ? " <em>記錄中</em>" : ""}</p>`)
    .join("");
  return `<section class="transcript-group${group.partial ? " partial" : ""}"><header><span>${escapeHtml(group.speaker)}</span>${count}</header>${body}</section>`;
}
