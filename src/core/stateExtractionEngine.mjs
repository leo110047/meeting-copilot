import { validateExtractionOutput, makeId, normalizeText } from "../domain/contracts.mjs";

export class StateExtractionEngine {
  constructor({ provider, promptVersion = "extract_state_patch.v1", timeoutMs = 2500 } = {}) {
    this.provider = provider;
    this.promptVersion = promptVersion;
    this.timeoutMs = timeoutMs;
  }

  async extract(input) {
    try {
      const response = await withTimeout(
        this.provider.runStructured({
          callType: "extract_state_patch",
          promptVersion: this.promptVersion,
          input,
          latencyBudgetMs: this.timeoutMs
        }),
        this.timeoutMs
      );
      const parsed = parsePossiblyMalformedJson(response.output);
      const errors = validateExtractionOutput(parsed);
      if (errors.length > 0) {
        return failure(input.sessionId, "schema_validation", this.promptVersion, this.provider.id, errors.join("; "));
      }
      if (isLowConfidence(parsed)) {
        return failure(input.sessionId, "low_confidence", this.promptVersion, this.provider.id, "low confidence patch");
      }
      return { ok: true, output: parsed, usage: response.usage };
    } catch (error) {
      const kind = error.code === "MALFORMED_JSON"
        ? "malformed_json"
        : error.code === "TIMEOUT" || /timeout/i.test(error.message)
          ? "timeout"
          : "api_error";
      return failure(input.sessionId, kind, this.promptVersion, this.provider?.id ?? "unknown", error.message);
    }
  }
}

export class RuleBasedStateExtractionEngine {
  constructor({ promptVersion = "extract_state_patch.rule.v1" } = {}) {
    this.promptVersion = promptVersion;
    this.provider = { id: "local-rule-extractor" };
  }

  async extract(input) {
    const events = input.newFinalTranscriptEvents ?? [];
    const text = normalizeText(events.map((event) => event.text).join(" "));
    const evidenceTranscriptIds = events.map((event) => event.id);
    const addItems = [];
    const addRisks = [];
    const addMissingInputs = [];
    const addOptions = [];
    let currentDecision;
    let phaseChange;

    if (/(決定|先這樣|要不要|scope|範圍|v1|commit)/i.test(text)) {
      phaseChange = "decision";
      currentDecision = inferDecisionText(events);
      addItems.push(item("decision", currentDecision, evidenceTranscriptIds, events[0]?.startedAtMs ?? 0, 0.78));
    } else if (/(風險|risk|rollback|卡住|爆掉)/i.test(text)) {
      phaseChange = "discussion";
    }

    if (/(owner|負責|誰處理|誰接)/i.test(text) && /(還沒|沒定|未定|不知道|先不要)/i.test(text)) {
      addMissingInputs.push(missing("owner", "還沒有明確 owner", true));
    }
    if (/(deadline|時程|什麼時候|週五|月底)/i.test(text) && /(還沒|沒定|先不要|再說)/i.test(text)) {
      addMissingInputs.push(missing("deadline", "deadline 還沒有明確承諾", true));
    }
    if (/(驗收|acceptance|成功標準|怎麼算完成)/i.test(text) && /(還沒|沒講|不清楚|先不要)/i.test(text)) {
      addMissingInputs.push(missing("acceptance_criteria", "驗收標準還沒定", true));
    }
    if (/(rollback|回滾|失敗怎麼辦)/i.test(text) && /(沒有|還沒|先不)/i.test(text)) {
      addMissingInputs.push(missing("rollback_plan", "rollback plan 還沒定", true));
    }
    if (/(風險|risk|卡住|爆掉|來不及|scope creep)/i.test(text)) {
      addRisks.push({
        text: inferRiskText(events),
        severity: /(爆掉|來不及|high|嚴重)/i.test(text) ? "high" : "medium",
        evidenceTranscriptIds
      });
    }
    if (/(方案|option|做法|拆成|v1|正式版)/i.test(text)) {
      addOptions.push({ label: inferOptionText(events), tradeoffs: [], risks: [], evidenceTranscriptIds });
    }

    return {
      ok: true,
      output: {
        meetingStatePatch: {
          addItems,
          updateItems: [],
          resolveItemIds: [],
          phaseChange,
          evidenceTranscriptIds
        },
        decisionStatePatch: {
          currentDecision,
          addOptions,
          updateOptions: [],
          addRisks,
          addMissingInputs,
          readinessPatch: undefined,
          evidenceTranscriptIds
        }
      }
    };
  }
}

function parsePossiblyMalformedJson(output) {
  if (typeof output === "object") return output;
  try {
    return JSON.parse(output);
  } catch {
    const start = output.indexOf("{");
    const end = output.lastIndexOf("}");
    if (start >= 0 && end > start) return JSON.parse(output.slice(start, end + 1));
    const err = new Error("malformed json");
    err.code = "MALFORMED_JSON";
    throw err;
  }
}

function failure(sessionId, failureKind, promptVersion, provider, rawOutputRef) {
  return {
    ok: false,
    failure: {
      id: makeId("extraction_failure"),
      sessionId,
      callType: "extract_state_patch",
      promptVersion,
      provider,
      failureKind,
      rawOutputRef,
      createdAt: new Date().toISOString()
    }
  };
}

function isLowConfidence(output) {
  const confidenceValues = [
    ...(output.meetingStatePatch.addItems ?? []).map((item) => item.confidence ?? 1),
    ...(output.meetingStatePatch.updateItems ?? []).map((item) => item.confidence ?? 1)
  ];
  return confidenceValues.some((value) => value < 0.35);
}

function item(kind, text, evidenceTranscriptIds, firstSeenAtMs, confidence) {
  return { kind, text, status: "open", confidence, evidenceTranscriptIds, firstSeenAtMs, lastUpdatedAtMs: firstSeenAtMs };
}

function missing(kind, text, blocksDecision) {
  return { kind, text, blocksDecision };
}

function inferDecisionText(events) {
  return events.at(-1)?.text.slice(0, 120) ?? "目前正在形成決策";
}

function inferRiskText(events) {
  return events.find((event) => /(風險|risk|卡住|爆掉|來不及|scope creep)/i.test(event.text))?.text ?? "未解風險";
}

function inferOptionText(events) {
  return events.find((event) => /(方案|option|做法|拆成|v1|正式版)/i.test(event.text))?.text.slice(0, 100) ?? "待比較方案";
}

function withTimeout(promise, timeoutMs) {
  let timeout;
  const timeoutPromise = new Promise((_, reject) => {
    timeout = setTimeout(() => {
      const err = new Error("timeout");
      err.code = "TIMEOUT";
      reject(err);
    }, timeoutMs);
  });
  return Promise.race([promise, timeoutPromise]).finally(() => clearTimeout(timeout));
}
