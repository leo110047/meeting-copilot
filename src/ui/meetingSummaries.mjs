export function summarizeTranscript(text) {
  if (!text.trim()) return [];
  const lines = splitSentences(text);
  const priority = lines.filter((line) => /決定|結論|scope|範圍|deadline|期限|owner|負責|驗收|rollback|風險|risk|blocker/i.test(line));
  return uniqueLimited([...priority, ...lines], 5);
}

export function summarizeDecisionState(decisionState, text) {
  const items = [];
  if (decisionState?.readiness) {
    items.push(`決策完整度 ${Math.round(decisionState.readiness.score * 100)}%，${decisionState.readiness.safeToDecide ? "目前可進入決策" : "仍不建議直接承諾"}`);
    for (const blocker of decisionState.readiness.blockers ?? []) items.push(`待補：${blocker}`);
  }
  const openLines = splitSentences(text)
    .filter((line) => /待確認|未定|還沒|不確定|下次|follow up|確認一下|再確認/i.test(line));
  return uniqueLimited([...items, ...openLines], 6);
}

export function summarizeSuggestions(suggestions, text) {
  const shown = suggestions.map((item) => `${labelMove(item.kind)}：${item.suggestedMove ?? item.text}`);
  const actionLines = splitSentences(text)
    .filter((line) => /要做|負責|owner|action|todo|下次|follow up|確認|補/i.test(line));
  return uniqueLimited([...shown, ...actionLines], 6);
}

export function labelMove(kind) {
  return {
    ask_question: "Ask｜補問",
    ask_clarifying_question: "Ask｜補問",
    say_next: "Say｜接下來說",
    watch_out: "Watch｜注意",
    defer_decision: "Hold｜先停一下",
    challenge_assumption: "Hold｜挑戰假設",
    confirm_commitment: "Decide｜確認承諾",
    split_decision: "Clarify｜拆清楚",
    surface_tradeoff: "Clarify｜攤開取捨",
    identify_missing_input: "Ask｜補齊條件"
  }[kind] ?? "決策建議";
}

function splitSentences(text) {
  return text
    .split(/\n|。|；|;|\.|！|!|？|\?/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function uniqueLimited(items, limit) {
  const unique = [];
  for (const item of items) {
    const compact = String(item).replace(/\s+/g, " ").slice(0, 160);
    if (compact && !unique.includes(compact)) unique.push(compact);
    if (unique.length >= limit) break;
  }
  return unique;
}
