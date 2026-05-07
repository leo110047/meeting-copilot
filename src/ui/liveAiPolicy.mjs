export const LIVE_AI_POLICY = Object.freeze({
  enabled: true,
  watchedSources: Object.freeze(["system"]),
  debounceMs: 3500,
  minWatchedEvents: 8,
  maxIntervalMs: 30000,
  cooldownMs: 60000,
  maxConsecutiveFailures: 3,
  failureBackoffMs: 5 * 60 * 1000
});

export function isLiveAiWatchedSource(source, policy = LIVE_AI_POLICY) {
  return policy.watchedSources.includes(source ?? "unknown");
}

export function createLiveAiPipelineState() {
  return {
    lastCompletedWatchedEventCount: 0,
    lastCompletedAtMs: 0,
    cooldownUntilMs: 0,
    suspendedUntilMs: 0,
    consecutiveFailures: 0,
    running: false
  };
}

export function shouldTriggerLiveAiForEvent(event, policy = LIVE_AI_POLICY) {
  return policy.enabled && isLiveAiWatchedSource(event?.source, policy);
}

export function countLiveAiWatchedEvents(transcriptEvents, policy = LIVE_AI_POLICY) {
  return transcriptEvents.filter((event) => isLiveAiWatchedSource(event.source, policy)).length;
}

export function hasPendingLiveAiWatchedEvents(transcriptEvents, state, policy = LIVE_AI_POLICY) {
  return countLiveAiWatchedEvents(transcriptEvents, policy) > state.lastCompletedWatchedEventCount;
}

export function nextLiveAiDecision({
  transcriptEvents,
  state,
  nowMs,
  policy = LIVE_AI_POLICY
}) {
  if (!policy.enabled) return { action: "skip", reason: "disabled" };
  if (state.running) return { action: "skip", reason: "running" };
  const watchedEventCount = countLiveAiWatchedEvents(transcriptEvents, policy);
  if (state.suspendedUntilMs > nowMs) {
    return {
      action: "delay",
      reason: "failure_backoff",
      delayMs: state.suspendedUntilMs - nowMs,
      watchedEventCount
    };
  }
  if (watchedEventCount < policy.minWatchedEvents) {
    return { action: "skip", reason: "not_enough_watched_events", watchedEventCount };
  }
  const newWatchedEvents = watchedEventCount - state.lastCompletedWatchedEventCount;
  if (newWatchedEvents <= 0) {
    return { action: "skip", reason: "no_new_watched_events", watchedEventCount };
  }
  if (state.cooldownUntilMs > nowMs) {
    return {
      action: "delay",
      reason: "cooldown",
      delayMs: state.cooldownUntilMs - nowMs,
      watchedEventCount
    };
  }
  const firstRun = state.lastCompletedWatchedEventCount === 0;
  const batchReady = firstRun || newWatchedEvents >= policy.minWatchedEvents;
  const intervalReady = state.lastCompletedAtMs > 0 && nowMs - state.lastCompletedAtMs >= policy.maxIntervalMs;
  if (batchReady || intervalReady) {
    return {
      action: "run",
      reason: batchReady ? "batch_ready" : "interval_ready",
      watchedEventCount
    };
  }
  return {
    action: "delay",
    reason: "waiting_for_batch_or_interval",
    delayMs: Math.max(500, policy.maxIntervalMs - (nowMs - state.lastCompletedAtMs)),
    watchedEventCount
  };
}

export function markLiveAiSuccess(state, { watchedEventCount, nowMs }) {
  return {
    ...state,
    running: false,
    lastCompletedWatchedEventCount: watchedEventCount,
    lastCompletedAtMs: nowMs,
    cooldownUntilMs: 0,
    suspendedUntilMs: 0,
    consecutiveFailures: 0,
  };
}

export function markLiveAiFailure(state, { nowMs, policy = LIVE_AI_POLICY }) {
  const consecutiveFailures = state.consecutiveFailures + 1;
  const shouldSuspend = consecutiveFailures >= policy.maxConsecutiveFailures;
  return {
    ...state,
    running: false,
    consecutiveFailures,
    cooldownUntilMs: shouldSuspend ? 0 : nowMs + policy.cooldownMs,
    suspendedUntilMs: shouldSuspend ? nowMs + policy.failureBackoffMs : 0
  };
}
