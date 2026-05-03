import { createServer } from "node:http";
import { createReadStream, existsSync, statSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { EventBus } from "../core/eventBus.mjs";
import { KnowledgeStore } from "../core/knowledgeStore.mjs";
import { SessionRuntime } from "../core/sessionRuntime.mjs";
import { sha16, nowIso } from "../domain/contracts.mjs";
import { createTranscriptEvent } from "../transcription/transcriptEvent.mjs";
import { SessionRepository } from "../storage/sessionRepository.mjs";

const mime = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8"
};

export function createLiveApiServer({ distRoot = "dist", dbPath = ".data/meeting-copilot.db" } = {}) {
  const root = resolve(distRoot);
  const service = new LiveSessionService({ dbPath });

  return createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", `http://${request.headers.host}`);
      if (url.pathname.startsWith("/api/")) {
        await handleApi({ request, response, url, service });
        return;
      }
      serveStatic({ response, root, pathname: decodeURIComponent(url.pathname) });
    } catch (error) {
      try {
        service.logError({
          stage: "live_api.request",
          source: "node_server",
          severity: "error",
          message: error.message,
          detail: { method: request.method, url: request.url }
        });
      } catch (logError) {
        console.error("failed to persist live API error log", logError);
      }
      json(response, 500, { error: error.message });
    }
  });
}

export function startLiveApiServer({ port = 8767, host = "127.0.0.1", distRoot = "dist", dbPath = ".data/meeting-copilot.db" } = {}) {
  const server = createLiveApiServer({ distRoot, dbPath });
  return new Promise((resolve) => {
    server.listen(port, host, () => resolve(server));
  });
}

export class LiveSessionService {
  constructor({ dbPath = ".data/meeting-copilot.db" } = {}) {
    this.repository = new SessionRepository(dbPath);
    this.sessions = new Map();
  }

  startSession({ brief: briefOverride } = {}) {
    const brief = createLiveBrief(briefOverride);
    const state = {
      brief,
      events: [],
      shownSuggestionIds: new Set(),
      knowledgeStore: new KnowledgeStore({
        projects: [defaultProject()],
        memories: defaultMemories(brief.projectId)
      })
    };
    this.sessions.set(brief.sessionId, state);
    this.repository.saveSession({
      brief,
      processingDisclosure: {
        sttProvider: "browser_speech_recognition_shell",
        llmProvider: "local_rule_engine",
        sentAudioToCloud: false,
        sentTranscriptToCloud: false,
        sentMemoryToCloud: false
      }
    });
    return { sessionId: brief.sessionId, brief };
  }

  async ingestTranscript(sessionId, body) {
    const state = this.sessions.get(sessionId);
    if (!state) return undefined;
    const event = createTranscriptEvent({
      id: body.id ?? `live_${sessionId}_${state.events.length + 1}`,
      sessionId,
      source: body.source ?? "mic",
      speaker: body.speaker,
      speakerConfidence: body.speakerConfidence ?? 0.35,
      startedAtMs: body.startedAtMs ?? state.events.length * 5000,
      endedAtMs: body.endedAtMs ?? state.events.length * 5000 + 3000,
      text: body.text,
      isFinal: body.isFinal ?? true
    });
    state.events.push(event);
    this.repository.saveTranscriptEvent(event);

    const runtime = new SessionRuntime({ knowledgeStore: state.knowledgeStore, eventBus: new EventBus() });
    const result = await runtime.runManual({ brief: state.brief, transcriptEvents: state.events });
    const newSuggestions = result.suggestions.filter((suggestion) => !state.shownSuggestionIds.has(suggestion.id));
    for (const suggestion of newSuggestions) {
      state.shownSuggestionIds.add(suggestion.id);
      this.repository.saveSuggestion(suggestion);
    }
    const snapshotId = sha16(`${sessionId}:${Date.now()}:${JSON.stringify(result.decisionState)}`);
    this.repository.saveDecisionSnapshot({
      id: snapshotId,
      sessionId,
      createdAtMs: Date.now(),
      decisionState: result.decisionState
    });
    for (const candidate of result.memoryCandidates) this.repository.saveMemoryCandidate(candidate);

    return {
      event,
      suggestions: newSuggestions,
      decisionState: result.decisionState,
      meetingState: result.meetingState,
      persisted: {
        transcriptEvents: state.events.length,
        newSuggestions: newSuggestions.length,
        decisionSnapshotId: snapshotId
      }
    };
  }

  stopSession(sessionId) {
    this.repository.endSession(sessionId);
    this.sessions.delete(sessionId);
    return { sessionId, endedAt: nowIso() };
  }

  logError(errorLog) {
    this.repository.saveAppErrorLog(errorLog);
  }
}

async function handleApi({ request, response, url, service }) {
  if (request.method === "POST" && url.pathname === "/api/sessions") {
    const body = await readJson(request);
    json(response, 200, service.startSession({ brief: body?.brief }));
    return;
  }

  const transcriptMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)\/transcript$/);
  if (request.method === "POST" && transcriptMatch) {
    const sessionId = transcriptMatch[1];
    const body = await readJson(request);
    const payload = await service.ingestTranscript(sessionId, body);
    if (!payload) {
      json(response, 404, { error: "session not found" });
      return;
    }
    json(response, 200, payload);
    return;
  }

  const stopMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)\/stop$/);
  if (request.method === "POST" && stopMatch) {
    const sessionId = stopMatch[1];
    json(response, 200, service.stopSession(sessionId));
    return;
  }

  json(response, 404, { error: "not found" });
}

function createLiveBrief(override = {}) {
  const sessionId = override.sessionId ?? `live_${Date.now()}`;
  return {
    sessionId,
    projectId: override.projectId ?? "live_default_project",
    meetingType: override.meetingType ?? "requirement_scoping",
    title: override.title ?? "即時會議",
    goal: override.goal ?? "即時監聽會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope",
    mustConfirm: override.mustConfirm ?? ["owner", "deadline", "驗收標準", "rollback plan"],
    risks: override.risks ?? ["未定義 owner/deadline 就做承諾", "demo scope 和正式版 scope 混在一起"],
    constraints: override.constraints ?? ["先確認決策條件再承諾交付"],
    knownParticipants: override.knownParticipants ?? [],
    preferredTone: override.preferredTone ?? "direct",
    startedAt: nowIso()
  };
}

function defaultProject() {
  return {
    id: "live_default_project",
    name: "Live Meeting",
    strategicGoal: "把會議中的模糊承諾轉成可驗證決策",
    currentPriorities: ["確認 owner", "確認 deadline", "確認驗收標準"],
    constraints: ["不在條件缺漏時承諾 scope"],
    recurringRisks: ["模糊承諾"],
    keyDecisions: []
  };
}

function defaultMemories(projectId) {
  return [
    {
      id: "live_memory_decision_conditions",
      projectId,
      participantIds: [],
      kind: "pattern",
      text: "過去會議常見問題是 scope 快被承諾時，owner、deadline 或驗收標準還沒有一起定。",
      sourceSessionIds: ["bootstrap"],
      evidenceTranscriptIds: ["bootstrap_decision_conditions"],
      createdAt: nowIso(),
      confidence: 0.76
    }
  ];
}

function serveStatic({ response, root, pathname }) {
  const candidate = resolve(join(root, pathname === "/" ? "index.html" : pathname.slice(1)));
  if (!candidate.startsWith(root) || !existsSync(candidate) || !statSync(candidate).isFile()) {
    response.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    response.end("Not found");
    return;
  }
  response.writeHead(200, { "content-type": mime[extname(candidate)] ?? "application/octet-stream" });
  createReadStream(candidate).pipe(response);
}

function readJson(request) {
  return new Promise((resolve, reject) => {
    let data = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => data += chunk);
    request.on("end", () => {
      if (!data) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(data));
      } catch (error) {
        reject(error);
      }
    });
    request.on("error", reject);
  });
}

function json(response, status, payload) {
  response.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  response.end(JSON.stringify(payload, null, 2));
}
