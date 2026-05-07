const EXACT_ECHO_MIN_COMPACT_CHARS = 6;
const FUZZY_ECHO_MIN_COMPACT_CHARS = 10;
const FUZZY_ECHO_BIGRAM_SIMILARITY = 0.55;
const FUZZY_ECHO_MIN_OVERLAP_RATIO = 0.7;
const EXACT_ECHO_MIN_OVERLAP_RATIO = 0.45;
const FUZZY_ECHO_MAX_START_DELTA_MS = 750;
const EXACT_ECHO_MAX_START_DELTA_MS = 1500;
const FUZZY_ECHO_MAX_CENTER_DELTA_MS = 1000;
const EXACT_ECHO_MAX_CENTER_DELTA_MS = 2000;

export function upsertTranscriptEventInPlace(transcriptEvents, transcriptLines, event) {
  if (!event?.text) return { changed: false, reason: "empty" };
  const sameId = transcriptEvents.find((existing) => existing.id === event.id);
  if (sameId) {
    Object.assign(sameId, stripUndefinedFields(event));
    return { changed: true, reason: "same_id" };
  }
  const semanticDuplicate = transcriptEvents.find((existing) => transcriptEventsMatch(existing, event));
  if (semanticDuplicate) {
    if (shouldReplaceSemanticDuplicate(semanticDuplicate, event)) {
      Object.assign(semanticDuplicate, stripUndefinedFields(event), {
        persistenceStatus: event.persistenceStatus ?? "saved"
      });
      return { changed: true, reason: "semantic_replace" };
    }
    return { changed: false, reason: "semantic_duplicate" };
  }
  const echoDuplicate = transcriptEvents.find((existing) => transcriptEventsAreCrossSourceEchoes(existing, event));
  if (echoDuplicate) {
    if ((echoDuplicate.source ?? "unknown") === "mic" && (event.source ?? "unknown") === "system") {
      Object.assign(echoDuplicate, stripUndefinedFields(event), {
        persistenceStatus: event.persistenceStatus ?? "saved"
      });
      return { changed: true, reason: "cross_source_echo_replace" };
    }
    return { changed: false, reason: "cross_source_echo_suppressed" };
  }
  transcriptEvents.push(event);
  if (!transcriptLines.includes(event.text)) transcriptLines.push(event.text);
  return { changed: true, reason: "inserted" };
}

export function transcriptEventsMatch(left, right) {
  return normalizeTranscriptText(left.text) === normalizeTranscriptText(right.text)
    && (left.source ?? "unknown") === (right.source ?? "unknown");
}

export function normalizeTranscriptText(text) {
  return String(text ?? "")
    .trim()
    .replace(/[。！？；，、.,!?;:]+$/g, "")
    .replace(/\s+/g, " ")
    .toLowerCase();
}

export function transcriptEventsAreCrossSourceEchoes(left, right) {
  const leftText = normalizeTranscriptText(left.text);
  const rightText = normalizeTranscriptText(right.text);
  const textMatch = transcriptTextEchoMatch(leftText, rightText);
  if (!textMatch) return false;
  const leftSource = left.source ?? "unknown";
  const rightSource = right.source ?? "unknown";
  const hasMicAndSystem =
    (leftSource === "mic" && rightSource === "system")
    || (leftSource === "system" && rightSource === "mic");
  if (!hasMicAndSystem) return false;
  const leftTiming = transcriptEventTiming(left);
  const rightTiming = transcriptEventTiming(right);
  if (!leftTiming || !rightTiming) return false;
  return transcriptTimingsLookLikeCaptureEchoes(leftTiming, rightTiming, textMatch.kind);
}

function transcriptTextEchoMatch(leftText, rightText) {
  const leftCompact = compactTranscriptText(leftText);
  const rightCompact = compactTranscriptText(rightText);
  const minLength = Math.min(leftCompact.length, rightCompact.length);
  if (leftCompact === rightCompact) return minLength >= EXACT_ECHO_MIN_COMPACT_CHARS ? { kind: "exact" } : null;
  if (minLength < FUZZY_ECHO_MIN_COMPACT_CHARS) return null;
  return characterBigramSimilarity(leftCompact, rightCompact) >= FUZZY_ECHO_BIGRAM_SIMILARITY ? { kind: "fuzzy" } : null;
}

function compactTranscriptText(text) {
  return String(text ?? "").replace(/[\s。！？；，、.,!?;:()[\]{}（）「」『』"'`]+/g, "");
}

function characterBigramSimilarity(left, right) {
  if (left === right) return 1;
  if (left.length < 2 || right.length < 2) return 0;
  const leftCounts = bigramCounts(left);
  const rightCounts = bigramCounts(right);
  let intersection = 0;
  for (const [bigram, leftCount] of leftCounts) {
    intersection += Math.min(leftCount, rightCounts.get(bigram) ?? 0);
  }
  return (2 * intersection) / ((left.length - 1) + (right.length - 1));
}

function bigramCounts(text) {
  const counts = new Map();
  for (let index = 0; index < text.length - 1; index += 1) {
    const bigram = text.slice(index, index + 2);
    counts.set(bigram, (counts.get(bigram) ?? 0) + 1);
  }
  return counts;
}

function transcriptEventTiming(event) {
  const started = Number(event.startedAtMs);
  const ended = Number(event.endedAtMs);
  if (!Number.isFinite(started) || !Number.isFinite(ended) || ended <= started) {
    return undefined;
  }
  return {
    started,
    ended,
    center: (started + ended) / 2,
    duration: ended - started
  };
}

function transcriptTimingsLookLikeCaptureEchoes(left, right, textMatchKind) {
  const overlapMs = Math.max(0, Math.min(left.ended, right.ended) - Math.max(left.started, right.started));
  const shorterDuration = Math.min(left.duration, right.duration);
  const fuzzy = textMatchKind === "fuzzy";
  const minimumOverlapRatio = fuzzy ? FUZZY_ECHO_MIN_OVERLAP_RATIO : EXACT_ECHO_MIN_OVERLAP_RATIO;
  const maximumStartDeltaMs = fuzzy ? FUZZY_ECHO_MAX_START_DELTA_MS : EXACT_ECHO_MAX_START_DELTA_MS;
  const maximumCenterDeltaMs = fuzzy ? FUZZY_ECHO_MAX_CENTER_DELTA_MS : EXACT_ECHO_MAX_CENTER_DELTA_MS;
  if (shorterDuration <= 0 || overlapMs / shorterDuration < minimumOverlapRatio) return false;
  if (Math.abs(left.started - right.started) > maximumStartDeltaMs) return false;
  if (Math.abs(left.center - right.center) > maximumCenterDeltaMs) return false;
  return true;
}

function shouldReplaceSemanticDuplicate(existing, incoming) {
  return String(existing.id ?? "").startsWith("preview_")
    && !String(incoming.id ?? "").startsWith("preview_");
}

function stripUndefinedFields(value) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined));
}
