import assert from "node:assert/strict";
import test from "node:test";
import {
  LIVE_AI_POLICY,
  countLiveAiWatchedEvents,
  createLiveAiPipelineState,
  hasPendingLiveAiWatchedEvents,
  isLiveAiWatchedSource,
  markLiveAiFailure,
  markLiveAiSuccess,
  nextLiveAiDecision,
  shouldTriggerLiveAiForEvent
} from "../src/ui/liveAiPolicy.mjs";

const policy = {
  ...LIVE_AI_POLICY,
  debounceMs: 1000,
  minWatchedEvents: 3,
  maxIntervalMs: 10000,
  cooldownMs: 60000,
  maxConsecutiveFailures: 3,
  failureBackoffMs: 300000
};

function event(source, id) {
  return { id, source, text: `${source}-${id}` };
}

test("live AI is triggered only by system transcript events", () => {
  assert.equal(isLiveAiWatchedSource("system", policy), true);
  assert.equal(isLiveAiWatchedSource("mic", policy), false);
  assert.equal(shouldTriggerLiveAiForEvent(event("system", "s1"), policy), true);
  assert.equal(shouldTriggerLiveAiForEvent(event("mic", "m1"), policy), false);
  assert.equal(shouldTriggerLiveAiForEvent(event("unknown", "u1"), policy), false);
});

test("live AI policy counts only watched source events", () => {
  const events = [
    event("mic", "m1"),
    event("system", "s1"),
    event("mic", "m2"),
    event("system", "s2")
  ];
  assert.equal(countLiveAiWatchedEvents(events, policy), 2);
});

test("live AI waits for a system micro-batch before running", () => {
  const state = createLiveAiPipelineState();
  assert.equal(
    nextLiveAiDecision({
      transcriptEvents: [event("system", "s1"), event("mic", "m1")],
      state,
      nowMs: 0,
      policy
    }).reason,
    "not_enough_watched_events"
  );
  const decision = nextLiveAiDecision({
    transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
    state,
    nowMs: 0,
    policy
  });
  assert.equal(decision.action, "run");
  assert.equal(decision.reason, "batch_ready");
  assert.equal(decision.delayMs, undefined);
});

test("live AI can be disabled by policy", () => {
  const decision = nextLiveAiDecision({
    transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
    state: createLiveAiPipelineState(),
    nowMs: 0,
    policy: { ...policy, enabled: false }
  });
  assert.equal(decision.action, "skip");
  assert.equal(decision.reason, "disabled");
});

test("live AI coalesces new events while a job is running", () => {
  const decision = nextLiveAiDecision({
    transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
    state: { ...createLiveAiPipelineState(), running: true },
    nowMs: 0,
    policy
  });
  assert.equal(decision.action, "skip");
  assert.equal(decision.reason, "running");
});

test("live AI uses cooldown and then recoverable backoff after repeated failures", () => {
  const firstFailure = markLiveAiFailure(createLiveAiPipelineState(), { nowMs: 1000, policy });
  assert.equal(firstFailure.suspendedUntilMs, 0);
  assert.equal(firstFailure.cooldownUntilMs, 61000);
  assert.equal(
    nextLiveAiDecision({
      transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
      state: firstFailure,
      nowMs: 2000,
      policy
    }).reason,
    "cooldown"
  );

  const secondFailure = markLiveAiFailure(firstFailure, { nowMs: 62000, policy });
  assert.equal(secondFailure.suspendedUntilMs, 0);
  assert.equal(secondFailure.cooldownUntilMs, 122000);

  const thirdFailure = markLiveAiFailure(secondFailure, { nowMs: 123000, policy });
  assert.equal(thirdFailure.cooldownUntilMs, 0);
  assert.equal(thirdFailure.suspendedUntilMs, 423000);
  assert.equal(
    nextLiveAiDecision({
      transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
      state: thirdFailure,
      nowMs: 124000,
      policy
    }).reason,
    "failure_backoff"
  );
  assert.equal(
    nextLiveAiDecision({
      transcriptEvents: [event("system", "s1"), event("system", "s2"), event("system", "s3")],
      state: thirdFailure,
      nowMs: 423000,
      policy
    }).action,
    "run"
  );
});

test("live AI success records watched count and clears retry state", () => {
  const failedOnce = markLiveAiFailure(createLiveAiPipelineState(), { nowMs: 1000, policy });
  const failedTwice = markLiveAiFailure(failedOnce, { nowMs: 62000, policy });
  const failed = markLiveAiFailure(failedTwice, { nowMs: 123000, policy });
  const success = markLiveAiSuccess(failed, { watchedEventCount: 5, nowMs: 5000 });
  assert.equal(success.lastCompletedWatchedEventCount, 5);
  assert.equal(success.lastCompletedAtMs, 5000);
  assert.equal(success.cooldownUntilMs, 0);
  assert.equal(success.suspendedUntilMs, 0);
  assert.equal(success.consecutiveFailures, 0);
});

test("live AI waits for interval when the next micro-batch is not ready", () => {
  const events = [
    event("system", "s1"),
    event("system", "s2"),
    event("system", "s3"),
    event("system", "s4")
  ];
  const state = {
    ...createLiveAiPipelineState(),
    lastCompletedWatchedEventCount: 3,
    lastCompletedAtMs: 1000
  };
  const waiting = nextLiveAiDecision({ transcriptEvents: events, state, nowMs: 5000, policy });
  assert.equal(waiting.action, "delay");
  assert.equal(waiting.reason, "waiting_for_batch_or_interval");
  assert.equal(waiting.delayMs, 6000);

  const intervalReady = nextLiveAiDecision({ transcriptEvents: events, state, nowMs: 11000, policy });
  assert.equal(intervalReady.action, "run");
  assert.equal(intervalReady.reason, "interval_ready");
});

test("live AI keeps watched events pending when they arrive during a running job", () => {
  const eventsAtStart = [event("system", "s1"), event("system", "s2"), event("system", "s3")];
  const runDecision = nextLiveAiDecision({
    transcriptEvents: eventsAtStart,
    state: createLiveAiPipelineState(),
    nowMs: 0,
    policy
  });
  assert.equal(runDecision.action, "run");

  const eventsAfterProviderStarted = [
    ...eventsAtStart,
    event("system", "s4"),
    event("system", "s5"),
    event("system", "s6")
  ];
  const settledState = markLiveAiSuccess(
    { ...createLiveAiPipelineState(), running: true },
    { watchedEventCount: runDecision.watchedEventCount, nowMs: 5000 }
  );

  assert.equal(hasPendingLiveAiWatchedEvents(eventsAfterProviderStarted, settledState, policy), true);
  assert.equal(
    nextLiveAiDecision({
      transcriptEvents: eventsAfterProviderStarted,
      state: settledState,
      nowMs: 5000,
      policy
    }).action,
    "run"
  );
});
