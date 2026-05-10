export function buildLiveMeetingBrief({
  series,
  prepContext,
  counterpartyContext,
  makeSessionId,
  nowIso = () => new Date().toISOString(),
  defaultMeetingTitle = "即時會議"
}) {
  const counterparty = String(counterpartyContext ?? "").trim();
  const context = removeCounterpartyContextBlock(String(prepContext ?? "").trim(), counterparty);
  const contextLine = context
    ? `會議背景：${context.slice(0, 1400)}`
    : "未提供會議背景，會議中只依照即時內容判斷。";
  const counterpartyLine = counterparty
    ? `對方背景與可能會議策略：${counterparty.slice(0, 800)}`
    : "未提供對方背景與會議策略。";
  const title = series?.title ?? defaultMeetingTitle;
  return {
    sessionId: makeSessionId("native"),
    projectId: "live_default_project",
    meetingType: "live_decision_copilot",
    title,
    goal: series
      ? `延續「${series.title}」追蹤本場會議決策，確認舊脈絡是否仍成立`
      : context
        ? `依據會議背景追蹤會議決策：${context.slice(0, 160)}`
        : "即時追蹤會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope",
    mustConfirm: ["owner", "deadline", "驗收標準", "rollback plan"],
    risks: ["未定義 owner/deadline 就做承諾", "demo scope 和正式版 scope 混在一起"],
    constraints: [
      "先確認決策條件再承諾交付",
      series ? `本場選用既有會議脈絡：${series.title}` : "本場未選用既有會議脈絡",
      counterpartyLine,
      contextLine
    ],
    knownParticipants: [],
    preferredTone: "direct",
    startedAt: nowIso()
  };
}

function removeCounterpartyContextBlock(context, counterparty) {
  if (!context || !counterparty) return context;
  const marker = "對方背景與可能會議策略";
  return context
    .split(/\n{2,}/)
    .filter((section) => {
      const trimmed = section.trim();
      if (!trimmed.startsWith(marker)) return true;
      const body = trimmed.slice(marker.length).replace(/^[:：]?\s*/, "").trim();
      return body !== counterparty;
    })
    .join("\n\n")
    .trim();
}
