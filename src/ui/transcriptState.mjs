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

function shouldReplaceSemanticDuplicate(existing, incoming) {
  return String(existing.id ?? "").startsWith("preview_")
    && !String(incoming.id ?? "").startsWith("preview_");
}

function stripUndefinedFields(value) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined));
}
