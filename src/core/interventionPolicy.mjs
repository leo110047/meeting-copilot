import { normalizeText } from "../domain/contracts.mjs";

export const DEFAULT_INTERVENTION_POLICY = {
  maxSuggestionsPerMeeting: 5,
  minMinutesBetweenSuggestions: 3,
  minConfidence: 0.72,
  allowLowPriority: false,
  suppressAfterNoisyCount: 2,
  noisySuppressionMinutes: 10,
  allowManualWake: true
};

export class InterventionPolicy {
  constructor(config = {}) {
    this.config = { ...DEFAULT_INTERVENTION_POLICY, ...config };
  }

  decide({ move, meetingState, priorSuggestions = [], feedback = [], nowMs = Date.now(), manualWake = false }) {
    if (!move) return blocked("no_candidate");
    const shown = priorSuggestions.filter((suggestion) => suggestion.shownAt);
    if (shown.length >= this.config.maxSuggestionsPerMeeting) return blocked("max_suggestions");
    if (move.confidence < this.config.minConfidence) return blocked("low_confidence");
    if (move.priority === "low" && !this.config.allowLowPriority) return blocked("low_priority_disabled");
    if (meetingState.phase === "opening" && move.priority !== "high") return blocked("opening_phase");
    if (!manualWake && tooSoon(shown.at(-1), nowMs, this.config.minMinutesBetweenSuggestions)) return blocked("too_soon");
    if (isDuplicate(move, shown)) return blocked("duplicate");

    const noisyCount = consecutiveNoisy(feedback);
    if (!manualWake && noisyCount >= this.config.suppressAfterNoisyCount) {
      if (!(move.priority === "high" && move.confidence >= 0.9)) return blocked("noisy_suppression");
      return allowed("high_priority_breaks_noisy_suppression");
    }

    return allowed("allowed");
  }
}

function allowed(reason) {
  return { shouldShow: true, reason };
}

function blocked(reason) {
  return { shouldShow: false, reason };
}

function tooSoon(lastSuggestion, nowMs, minMinutes) {
  if (!lastSuggestion) return false;
  return nowMs - Date.parse(lastSuggestion.shownAt) < minMinutes * 60_000;
}

function isDuplicate(move, suggestions) {
  const text = normalizeText(move.text);
  return suggestions.some((suggestion) => normalizeText(suggestion.text) === text);
}

function consecutiveNoisy(feedback) {
  let count = 0;
  for (const entry of [...feedback].reverse()) {
    if (entry.value === "noisy") count += 1;
    else break;
  }
  return count;
}
