export const PARTIAL_TRANSCRIPT_COMMIT_IDLE_MS = 2200;
export const PARTIAL_TRANSCRIPT_REPLACED_MIN_AGE_MS = 900;

export function shouldCommitIdlePartial(partial, idleMs, { idleThresholdMs = PARTIAL_TRANSCRIPT_COMMIT_IDLE_MS } = {}) {
  return isUsefulPartialTranscript(partial?.text) && idleMs >= idleThresholdMs;
}

export function shouldCommitReplacedPartial(previous, next, ageMs, { minAgeMs = PARTIAL_TRANSCRIPT_REPLACED_MIN_AGE_MS } = {}) {
  if (!isUsefulPartialTranscript(previous?.text)) return false;
  if (ageMs < minAgeMs) return false;
  if ((previous?.source ?? "unknown") !== (next?.source ?? "unknown")) return true;
  return !isLikelySamePartialUpdate(previous?.text, next?.text);
}

export function isUsefulPartialTranscript(text) {
  const normalized = normalizePartialText(text);
  if (!normalized) return false;
  if (/[A-Za-z0-9]{2,}/.test(normalized)) return true;
  return Array.from(normalized).length >= 2;
}

export function isLikelySamePartialUpdate(previousText, nextText) {
  const previous = normalizePartialText(previousText);
  const next = normalizePartialText(nextText);
  if (!previous || !next) return true;
  return previous === next || previous.startsWith(next) || next.startsWith(previous);
}

function normalizePartialText(text) {
  return String(text ?? "")
    .trim()
    .replace(/\s+/g, " ");
}
