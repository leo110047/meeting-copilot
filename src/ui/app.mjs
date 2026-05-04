import { downloadMeetingArtifact as downloadMeetingArtifactFile, escapeHtml } from "./documentExports.mjs";
import { labelMove, summarizeDecisionState, summarizeSuggestions, summarizeTranscript } from "./meetingSummaries.mjs";
import { createSetupController } from "./setupController.mjs";
import { upsertTranscriptEventInPlace } from "./transcriptState.mjs";
import { renderTranscriptDrawerView, transcriptSpeakerLabel } from "./transcriptDrawer.mjs";
import { clamp, detectUiLanguage, formatAudioMonitorMessage, formatError, isScreenRecordingPermissionMessage, labelCaptureSource, labelLanguage, makeClientId } from "./uiUtils.mjs";
const startButton = document.querySelector("#startListening");
const stopButton = document.querySelector("#stopListening");
const sessionState = document.querySelector("#sessionState");
const providerState = document.querySelector("#providerState");
const openAudioPermissionsButton = document.querySelector("#openAudioPermissions");
const setupAudioReadiness = document.querySelector("#setupAudioReadiness");
const setupAudioReadinessText = document.querySelector("#setupAudioReadinessText");
const setupAudioPermissionsButton = document.querySelector("#setupAudioPermissions");
const liveTranscript = document.querySelector("#liveTranscript");
const transcriptDrawerToggle = document.querySelector("#transcriptDrawerToggle");
const transcriptDrawerCount = document.querySelector("#transcriptDrawerCount");
const transcriptPreview = document.querySelector("#transcriptPreview");
const transcriptFull = document.querySelector("#transcriptFull");
const suggestion = document.querySelector("#suggestion");
const captureSource = document.querySelector("#captureSource");
const curtainOpacity = document.querySelector("#curtainOpacity");
const curtainOpacityValue = document.querySelector("#curtainOpacityValue");
const setupContext = document.querySelector("#setupContext");
const setupDropZone = document.querySelector("#setupDropZone");
const setupContextMeta = document.querySelector("#setupContextMeta");
const droppedFileCount = document.querySelector("#droppedFileCount");
const prepDictationButton = document.querySelector("#prepDictation");
const prepSummary = document.querySelector("#prepSummary");
const feedbackRow = document.querySelector("#feedbackRow");
const newMeetingButton = document.querySelector("#newMeeting");
const reviewScreen = document.querySelector(".review-screen");
const reviewStageLabel = document.querySelector("#reviewStageLabel");
const reviewTitle = document.querySelector("#reviewTitle");
const reviewStatus = document.querySelector("#reviewStatus");
const postMeetingSummary = document.querySelector("#postMeetingSummary");
const postMeetingTranscript = document.querySelector("#postMeetingTranscript");
const downloadState = document.querySelector("#downloadState");
const downloadButtons = document.querySelectorAll("[data-download-format]");
const downloadErrorLogButton = document.querySelector("#downloadErrorLog");
const textProviderName = document.querySelector("#textProviderName");
const textProviderDetail = document.querySelector("#textProviderDetail");
const loginTextProviderButton = document.querySelector("#loginTextProvider");
const enableOAuthProviderButton = document.querySelector("#enableOAuthProvider");
const providerSettings = document.querySelector(".provider-settings");
const AI_ENABLED_STORAGE_KEY = "meetingCopilot.aiEnabled";
let recognition;
let transcriptLines = [];
let transcriptEvents = [];
let currentPartialTranscript;
let suggestionHistory = [];
let latestDecisionState;
let aiSummaryOverride;
let textProviderStatus;
let oauthAiEnabled = readAiEnabledPreference();
let activeSessionId;
let transcriptIndex = 0;
let startedAt = 0;
let nativeListenersInstalled = false;
let transcriptDrawerOpen = false;
let liveAiExtractionRunning = false;
let lastAiExtractionEventCount = 0;
let platformShellPlan;
let browserErrorLogs = [];
let textProviderRefreshTimers = [];
let transcriptStallTimer;
let reviewFinalized = false;
let postMeetingAiSummaryStarted = false;
let activeCaptureSource;
const TRANSCRIPT_STALL_MS = 12000;
const NO_REVIEW_CONTENT_MESSAGE = "沒有收到逐字稿，也沒有可整理的會前資料。請確認音訊來源或加入會議背景後再開始下一場。";
const NO_REVIEW_AI_SKIP_MESSAGE = "沒有收到逐字稿，也沒有可整理的會前資料；不會送出空內容給 ChatGPT。";
const nativeInvoke = window.__TAURI__?.core?.invoke;
const nativeListen = window.__TAURI__?.event?.listen;
const setupController = createSetupController({
  elements: {
    setupContext,
    setupDropZone,
    setupContextMeta,
    droppedFileCount,
    prepDictationButton,
    prepSummary
  },
  nativeInvoke,
  nativeListen,
  canStartWithAi,
  syncStartButtonAvailability,
  textProviderAuthenticated: () => Boolean(textProviderStatus?.authenticated),
  logAppError,
  formatError,
  escapeHtml
});
initializeSetupState();
setupController.install();
installOpacityControl();
refreshPlatformShellPlan();
refreshTextProviderStatus();
refreshNativeAudioReadiness("startup");
captureSource?.addEventListener("change", () => {
  refreshNativeAudioReadiness("source_change");
});
startButton.addEventListener("click", async () => {
  if (!canStartWithAi()) {
    logAppError("meeting.start_blocked_without_ai", "Meeting start was requested before AI was enabled", { authenticated: Boolean(textProviderStatus?.authenticated) }, "warning");
    syncStartButtonAvailability();
    textProviderDetail.textContent = textProviderStatus?.authenticated
      ? "請先啟用 AI，Meeting Copilot 才能開始會議。"
      : "請先登入 ChatGPT，Meeting Copilot 才能開始會議。";
    return;
  }
  startButton.disabled = true;
  sessionState.textContent = "檢查音訊";
  providerState.textContent = "正在檢查音訊與權限...";
  if (nativeInvoke) {
    try {
      await startNativeListening();
    } catch (error) {
      logAppError("meeting.start_native", error, { source: captureSource?.value ?? "mic" }, "error");
      handleAudioPermissionProblem(formatError(error));
      updateAppState("setup");
      resetNativeWindowOpacity();
      sessionState.textContent = "啟動失敗";
      providerState.textContent = `會議無法開始：${formatError(error)}`;
      startButton.disabled = false;
      stopButton.disabled = true;
    }
    return;
  }
  const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRecognition) {
    logAppError("meeting.browser_speech_unavailable", "SpeechRecognition is not available", { nativeAvailable: Boolean(nativeInvoke) }, "error");
    setProviderUnavailable();
    startButton.disabled = false;
    updateAppState("setup");
    resetNativeWindowOpacity();
    return;
  }
  transcriptLines = [];
  transcriptEvents = [];
  suggestionHistory = [];
  latestDecisionState = undefined;
  transcriptIndex = 0;
  lastAiExtractionEventCount = 0;
  liveAiExtractionRunning = false;
  activeCaptureSource = undefined;
  startedAt = performance.now();
  resetListeningSurface();
  activeSessionId = await createLiveSession();
  recognition = new SpeechRecognition();
  recognition.lang = "zh-TW";
  recognition.continuous = true;
  recognition.interimResults = false;
  recognition.onstart = () => {
    updateAppState("listening");
    sessionState.textContent = "記錄中";
    providerState.textContent = "正在聽｜瀏覽器語音辨識";
    startButton.disabled = true;
    stopButton.disabled = false;
  };
  recognition.onresult = async (event) => {
    for (let i = event.resultIndex; i < event.results.length; i += 1) {
      const result = event.results[i];
      if (!result.isFinal) continue;
      const text = result[0].transcript.trim();
      if (!text) continue;
      await sendTranscript(text);
    }
  };
  recognition.onerror = (event) => {
    logAppError("meeting.browser_speech_error", event.error ?? "unknown speech recognition error", { error: event.error }, "error");
    providerState.textContent = `語音辨識發生錯誤：${event.error}`;
  };
  recognition.onend = () => {
    if (activeSessionId) {
      fetch(`/api/sessions/${activeSessionId}/stop`, { method: "POST" }).catch((error) => {
        logAppError("meeting.browser_stop_session", error, { sessionId: activeSessionId }, "warning");
      });
    }
    finishPostMeetingReview();
  };
  try {
    recognition.start();
  } catch (error) {
    logAppError("meeting.browser_speech_start", error, {}, "error");
    updateAppState("setup");
    resetNativeWindowOpacity();
    providerState.textContent = `無法啟動語音辨識：${error.message}`;
  }
});
stopButton.addEventListener("click", async () => {
  const stoppingSessionId = activeSessionId;
  stopButton.disabled = true;
  sessionState.textContent = "整理中";
  providerState.textContent = "正在結束會議...";
  await promoteCurrentPartialTranscript("stop_button");
  enterReviewProcessing({ statusText: "正在切換到會後頁。" });
  await waitForReviewPaint();
  finalizeMeetingReview({
    statusText: "已切到會後頁；正在背景停止錄音並補齊最後資料。",
    runAiSummary: false,
    allowNewMeeting: false
  });
  if (nativeInvoke && stoppingSessionId) {
    finishNativeStopInBackground(stoppingSessionId);
    return;
  }
  if (recognition) {
    recognition.stop();
  } else {
    finishPostMeetingReview();
  }
});
transcriptDrawerToggle.addEventListener("click", () => {
  transcriptDrawerOpen = !transcriptDrawerOpen;
  renderTranscriptDrawer();
});
newMeetingButton.addEventListener("click", () => {
  initializeSetupState();
});
for (const button of downloadButtons) {
  button.addEventListener("click", () => {
    const format = button.dataset.downloadFormat;
    downloadMeetingArtifact(format);
  });
}
downloadErrorLogButton?.addEventListener("click", () => {
  downloadErrorLog();
});
openAudioPermissionsButton?.addEventListener("click", () => {
  openScreenRecordingSettings("manual_button");
});
setupAudioPermissionsButton?.addEventListener("click", async () => {
  setupAudioPermissionsButton.disabled = true;
  setupAudioReadinessText.textContent = "正在要求 macOS 重新建立 Meeting Copilot 的系統音訊權限。";
  try {
    await openScreenRecordingSettings("setup_audio_readiness");
  } finally {
    setupAudioPermissionsButton.disabled = false;
    setTimeout(() => refreshNativeAudioReadiness("permission_button"), 800);
  }
});
loginTextProviderButton.addEventListener("click", async () => {
  if (!nativeInvoke) {
    logAppError("ai.login_unavailable", "Text provider login requires the desktop app", {}, "warning");
    textProviderDetail.textContent = "登入需要使用桌面 app。";
    return;
  }
  loginTextProviderButton.disabled = true;
  try {
    await nativeInvoke("start_text_provider_login");
    textProviderDetail.textContent = "已開啟 ChatGPT 登入視窗；登入完成後會自動更新狀態。";
    scheduleTextProviderRefresh();
  } catch (error) {
    logAppError("ai.login", error, {}, "error");
    textProviderDetail.textContent = `無法開啟登入：${formatError(error)}`;
  } finally {
    loginTextProviderButton.disabled = false;
  }
});
enableOAuthProviderButton.addEventListener("click", () => {
  if (!textProviderStatus?.authenticated) {
    logAppError("ai.enable_without_auth", "AI enable was requested without authenticated subscription OAuth", {}, "warning");
    textProviderDetail.textContent = "尚未登入 ChatGPT，無法啟用 AI。";
    return;
  }
  setAiEnabledPreference(true);
  renderTextProviderStatus();
  setupController.schedulePrepSummaryGeneration();
});
async function startNativeListening() {
  transcriptLines = [];
  transcriptEvents = [];
  currentPartialTranscript = undefined;
  suggestionHistory = [];
  latestDecisionState = undefined;
  aiSummaryOverride = undefined;
  transcriptIndex = 0;
  lastAiExtractionEventCount = 0;
  liveAiExtractionRunning = false;
  startedAt = performance.now();
  reviewFinalized = false;
  postMeetingAiSummaryStarted = false;
  resetListeningSurface();
  await setupController.stopPrepDictationBeforeMeeting();
  sessionState.textContent = "檢查音訊";
  const requestedSource = captureSource?.value ?? "mic";
  activeCaptureSource = requestedSource;
  hideAudioPermissionAction();
  let health = await nativeInvoke("request_native_audio_permissions", {
    request: { source: requestedSource }
  });
  if (!health.ready) {
    health = await nativeInvoke("native_transcriber_health", {
      request: { source: requestedSource }
    });
  }
  if (!health.ready) {
    handleAudioPermissionProblem(health.lastError ?? "");
    throw new Error(`音訊尚未就緒：${health.lastError ?? "未知原因"}`);
  }
  health = await nativeInvoke("native_transcriber_health", {
    request: { source: requestedSource }
  });
  if (!health.ready) {
    handleAudioPermissionProblem(health.lastError ?? "");
    throw new Error(`音訊尚未就緒：${health.lastError ?? "未知原因"}`);
  }
  sessionState.textContent = "建立會議";
  activeSessionId = await createLiveSession();
  await installNativeListeners();
  sessionState.textContent = "開始記錄";
  const started = await nativeInvoke("start_native_transcription", {
    sessionId: activeSessionId,
    request: { language: "zh-TW", source: requestedSource }
  });
  updateAppState("listening");
  sessionState.textContent = "記錄中";
  providerState.textContent = `正在聽｜${labelCaptureSource(started.source)}｜${labelLanguage(started.language)}`;
  startTranscriptStallMonitor(started.source);
  startButton.disabled = true;
  stopButton.disabled = false;
}

async function stopNativeListening(sessionId = activeSessionId) {
  if (!sessionId) throw new Error("session id is required to stop native listening");
  await nativeInvoke("stop_native_transcription", { sessionId });
  await nativeInvoke("stop_session", { sessionId });
}

async function finishNativeStopInBackground(sessionId) {
  try {
    await stopNativeListening(sessionId);
    finishPostMeetingReview(sessionId);
  } catch (error) {
    logAppError("meeting.stop_native", error, { sessionId }, "error");
    reviewStatus.textContent = `結束會議失敗：${formatError(error)}`;
    downloadState.textContent = "錄音停止失敗；可下載錯誤紀錄協助排查。";
    downloadErrorLogButton.disabled = false;
    newMeetingButton.disabled = false;
    stopButton.disabled = false;
  }
}

async function installNativeListeners() {
  if (nativeListenersInstalled || !nativeListen) return;
  try {
    nativeListenersInstalled = true;
    await nativeListen("native_transcript_ingested", (event) => {
      const payload = event.payload;
      if (!payload?.event?.text) return;
      clearTranscriptStallMonitor();
      currentPartialTranscript = undefined;
      renderRuntimePayload(payload);
      maybeRunLiveAiExtraction();
    });
    await nativeListen("native_transcript_preview", (event) => {
      const payload = event.payload;
      if (!payload?.text) return;
      clearTranscriptStallMonitor();
      currentPartialTranscript = payload;
      renderTranscriptDrawer();
    });
    await nativeListen("native_transcription_error", (event) => {
      const payload = event.payload;
      const message = typeof payload === "object" && payload !== null
        ? String(payload.message ?? "")
        : String(payload ?? "");
      const source = typeof payload === "object" && payload !== null
        ? payload.source ?? activeCaptureSource ?? captureSource?.value ?? "mic"
        : activeCaptureSource ?? captureSource?.value ?? "mic";
      const code = typeof payload === "object" && payload !== null
        ? String(payload.code ?? "native_transcription_error")
        : classifyNativeTranscriptionError(message);
      clearTranscriptStallMonitor();
      const severity = code === "no_speech_detected" ? "warning" : "error";
      logAppError("native.transcription_event_error", message, { source, code }, severity);
      providerState.textContent = formatAudioMonitorMessage(message, code);
      handleAudioPermissionProblem(message);
      if (code === "stopped_from_tray" && document.body.dataset.state === "listening") {
        finalizeMeetingReview({ statusText: "已從系統列結束會議，正在整理記錄。" });
      }
    });
  } catch (error) {
    logAppError("native.install_listeners", error, {}, "error");
    nativeListenersInstalled = false;
    throw new Error(`無法接收 native 事件：${formatError(error)}`);
  }
}

async function createLiveSession() {
  const request = { brief: createBriefFromSetupContext(), textProviderEnabled: canStartWithAi() };
  if (nativeInvoke) {
    const payload = await nativeInvoke("start_session", { request });
    providerState.textContent = "會議已建立。";
    return payload.sessionId;
  }
  const response = await fetch("/api/sessions", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request)
  });
  if (!response.ok) {
    logAppError("session.browser_start_failed", `session start failed: ${response.status}`, {}, "error");
    throw new Error(`session start failed: ${response.status}`);
  }
  const payload = await response.json();
  providerState.textContent = "會議已建立。";
  return payload.sessionId;
}

async function sendTranscript(text) {
  if (!activeSessionId) return;
  try {
    transcriptIndex += 1;
    const elapsedMs = Math.round(performance.now() - startedAt);
    const payloadBody = {
      id: `browser_${activeSessionId}_${transcriptIndex}`,
      text,
      source: "mic",
      startedAtMs: Math.max(0, elapsedMs - 3000),
      endedAtMs: elapsedMs,
      isFinal: true
    };
    let payload;
    if (nativeInvoke) {
      payload = await nativeInvoke("ingest_transcript", {
        sessionId: activeSessionId,
        input: payloadBody
      });
    } else {
      const response = await fetch(`/api/sessions/${activeSessionId}/transcript`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payloadBody)
      });
      if (!response.ok) {
        logAppError("transcript.browser_write_failed", `transcript write failed: ${response.status}`, { sessionId: activeSessionId }, "error");
        providerState.textContent = `逐字稿寫入失敗：${response.status}`;
        return;
      }
      payload = await response.json();
    }
    renderRuntimePayload(payload);
    maybeRunLiveAiExtraction();
  } catch (error) {
    logAppError("transcript.ingest", error, { sessionId: activeSessionId }, "error");
    providerState.textContent = `逐字稿寫入失敗：${formatError(error)}`;
  }
}

async function promoteCurrentPartialTranscript(reason) {
  const partial = currentPartialTranscript;
  const text = partial?.text?.trim();
  if (!text || !activeSessionId) return;
  const duplicate = transcriptEvents.some((event) =>
    event.text.trim() === text
    && (event.source ?? "unknown") === (partial.source ?? "unknown")
  );
  currentPartialTranscript = undefined;
  if (duplicate) {
    renderTranscriptDrawer();
    return;
  }
  transcriptIndex += 1;
  const elapsedMs = Math.round(performance.now() - startedAt);
  const promotedEvent = {
    id: `preview_${activeSessionId}_${transcriptIndex}`,
    sessionId: activeSessionId,
    source: partial.source ?? activeCaptureSource ?? "unknown",
    speaker: partial.speaker,
    // TODO: keep ASR confidence separate from speaker confidence in the transcript model.
    speakerConfidence: partial.speakerConfidence ?? 0.35,
    language: partial.language ?? detectUiLanguage(text),
    startedAtMs: partial.startedAtMs ?? Math.max(0, elapsedMs - 3000),
    endedAtMs: partial.endedAtMs ?? elapsedMs,
    text,
    isFinal: true,
    persistenceStatus: "pending"
  };
  upsertTranscriptEvent(promotedEvent);
  renderTranscriptDrawer();
  logAppError("native.promote_partial_transcript", "Promoted visible partial transcript before review", {
    sessionId: activeSessionId,
    reason,
    source: promotedEvent.source
  }, "warning");
  if (!nativeInvoke) return;
  try {
    const payload = await nativeInvoke("ingest_transcript", {
      sessionId: activeSessionId,
      input: {
        id: promotedEvent.id,
        text: promotedEvent.text,
        source: promotedEvent.source,
        speaker: promotedEvent.speaker,
        speakerConfidence: promotedEvent.speakerConfidence,
        startedAtMs: promotedEvent.startedAtMs,
        endedAtMs: promotedEvent.endedAtMs,
        isFinal: true
      }
    });
    if (payload.event) payload.event.persistenceStatus = "saved";
    renderRuntimePayload(payload);
  } catch (error) {
    markTranscriptPersistence(promotedEvent.id, "failed");
    logAppError("native.promote_partial_transcript_failed", error, { sessionId: activeSessionId, reason }, "error");
  }
}

function renderRuntimePayload(payload) {
  if (payload.event) {
    upsertTranscriptEvent(payload.event);
    renderTranscriptDrawer();
  }
  if (Array.isArray(payload.suggestions) && payload.suggestions.length > 0) {
    for (const item of payload.suggestions) {
      if (!suggestionHistory.some((existing) => existing.id === item.id)) suggestionHistory.push(item);
    }
  }
  if (payload.decisionState) latestDecisionState = payload.decisionState;
  providerState.textContent = `逐字稿 ${payload.persisted.transcriptEvents} 句｜正在判斷是否需要提醒`;
  if (payload.suggestions.length > 0) {
    const latest = payload.suggestions.at(-1);
    suggestion.className = "suggestion-card";
    suggestion.innerHTML = renderSuggestionCard(latest);
    feedbackRow.hidden = false;
  } else if (payload.decisionState?.readiness) {
    suggestion.className = "suggestion-empty";
    suggestion.innerHTML = renderDecisionOverview(payload.decisionState);
    feedbackRow.hidden = true;
  }
}

function renderSuggestionCard(item) {
  const evidence = renderEvidenceLines(item.evidenceTranscriptIds ?? []);
  return [
    `<strong>${escapeHtml(labelMove(item.kind))}</strong>`,
    `<div>${escapeHtml(item.text)}</div>`,
    `<small>${escapeHtml(item.reason)}</small>`,
    evidence
  ].filter(Boolean).join("");
}

function renderEvidenceLines(ids) {
  const lines = ids
    .map((id) => transcriptEvents.find((event) => event.id === id))
    .filter(Boolean)
    .slice(0, 4);
  if (lines.length === 0) return "";
  return `<details class="evidence-disclosure"><summary>查看依據 ${lines.length} 句</summary>${lines.map((event) => `<p>${escapeHtml(transcriptSpeakerLabel(event))}：${escapeHtml(event.text)}</p>`).join("")}</details>`;
}

function renderDecisionOverview(decisionState) {
  const readiness = Math.round((decisionState.readiness?.score ?? 0) * 100);
  const items = [
    decisionState.currentDecision ? `目前決策：${decisionState.currentDecision}` : "",
    ...(decisionState.meetingItems ?? []).slice(0, 2).map((item) => `會議重點：${item.text ?? ""}`),
    ...(decisionState.options ?? []).slice(0, 2).map((item) => `選項：${item.text ?? ""}`),
    ...(decisionState.missingInputs ?? []).slice(0, 2).map((item) => `缺口：${item.text ?? ""}`),
    ...(decisionState.risks ?? []).slice(0, 1).map((item) => `風險：${item.text ?? ""}`)
  ].filter(Boolean);
  if (items.length === 0) {
    return `<strong>目前沒有提醒</strong><small>決策完整度 ${readiness}%</small>`;
  }
  return `<strong>目前沒有提醒</strong><small>決策完整度 ${readiness}%</small><ul>${items.map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>`;
}

async function maybeRunLiveAiExtraction() {
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated || !activeSessionId) return;
  const eventCount = transcriptEvents.length;
  if (eventCount === 0 || liveAiExtractionRunning) return;
  if (eventCount === lastAiExtractionEventCount) return;
  if (lastAiExtractionEventCount > 0 && eventCount - lastAiExtractionEventCount < 2) return;
  liveAiExtractionRunning = true;
  providerState.textContent = "AI 正在判斷是否需要提醒...";
  try {
    const payload = await nativeInvoke("extract_live_state_patch_oauth", { sessionId: activeSessionId });
    lastAiExtractionEventCount = transcriptEvents.length;
    renderRuntimePayload(payload);
    providerState.textContent = "AI 已更新會議判斷。";
  } catch (error) {
    logAppError("ai.live_state_patch", error, { sessionId: activeSessionId, eventCount }, "error");
    lastAiExtractionEventCount = Math.max(0, lastAiExtractionEventCount - 1);
    providerState.textContent = `AI 暫時無法更新，已保留本機判斷：${formatError(error)}`;
  } finally {
    liveAiExtractionRunning = false;
  }
}

function setProviderUnavailable() {
  clearTranscriptStallMonitor();
  sessionState.textContent = "無法開始";
  providerState.textContent = "這個環境沒有可用的語音辨識。請改用桌面 app。";
  suggestion.className = "suggestion-empty";
  suggestion.textContent = "目前沒有可用的音訊。";
  feedbackRow.hidden = true;
}

function initializeSetupState() {
  clearTranscriptStallMonitor();
  updateAppState("setup");
  resetNativeWindowOpacity();
  reviewScreen.dataset.reviewState = "idle";
  reviewStageLabel.textContent = "會後";
  reviewTitle.textContent = "會議記錄";
  reviewStatus.textContent = "會議結束後會在這裡整理文件。";
  setDownloadActionsEnabled(true);
  newMeetingButton.disabled = false;
  syncStartButtonAvailability();
  stopButton.disabled = true;
  activeSessionId = undefined;
  transcriptLines = [];
  transcriptEvents = [];
  currentPartialTranscript = undefined;
  suggestionHistory = [];
  latestDecisionState = undefined;
  aiSummaryOverride = undefined;
  lastAiExtractionEventCount = 0;
  liveAiExtractionRunning = false;
  activeCaptureSource = undefined;
  reviewFinalized = false;
  postMeetingAiSummaryStarted = false;
  setupController.resetPrepSummaryQueue();
  sessionState.textContent = "待機中";
  providerState.textContent = "尚未開始會議。";
  resetListeningSurface();
  setupController.renderPrepSummary();
}

function resetListeningSurface() {
  transcriptDrawerOpen = false;
  currentPartialTranscript = undefined;
  hideAudioPermissionAction({ includeSetup: false });
  renderTranscriptDrawer();
  suggestion.className = "suggestion-empty";
  suggestion.textContent = "目前沒有提醒。";
  feedbackRow.hidden = true;
}

function handleAudioPermissionProblem(message) {
  if (!isScreenRecordingPermissionMessage(message)) return;
  showAudioPermissionAction();
}

async function refreshNativeAudioReadiness(reason) {
  if (!nativeInvoke) return;
  const source = captureSource?.value ?? "mic";
  try {
    const health = await nativeInvoke("native_transcriber_health", {
      request: { source }
    });
    if (health.ready) {
      hideAudioPermissionAction();
      if (/^音訊權限需處理/.test(providerState.textContent)) {
        providerState.textContent = "尚未開始會議。";
      }
      return;
    }
    const message = health.lastError ?? "native audio is not ready";
    logAppError("permissions.native_audio_health", message, { source, reason }, "warning");
    handleAudioPermissionProblem(message);
    const readinessMessage = formatNativeAudioReadinessMessage(message);
    if (setupAudioReadinessText) setupAudioReadinessText.textContent = readinessMessage;
    providerState.textContent = `音訊權限需處理：${readinessMessage}`;
  } catch (error) {
    logAppError("permissions.native_audio_health", error, { source, reason }, "warning");
  }
}

function formatNativeAudioReadinessMessage(message) {
  if (/screenSystemAudioPreflight=false/i.test(message)) {
    return "macOS 尚未把螢幕與系統錄音權限套用到目前這個 Meeting Copilot。";
  }
  if (/microphone=(denied|restricted|notDetermined)/i.test(message)) {
    return "麥克風權限尚未可用。";
  }
  if (/speechRecognition=(denied|restricted|notDetermined)/i.test(message)) {
    return "語音辨識權限尚未可用。";
  }
  return formatAudioMonitorMessage(message);
}

function showAudioPermissionAction() {
  if (openAudioPermissionsButton) {
    openAudioPermissionsButton.hidden = false;
    openAudioPermissionsButton.disabled = false;
  }
  if (setupAudioReadiness) {
    setupAudioReadiness.hidden = false;
  }
  if (setupAudioPermissionsButton) {
    setupAudioPermissionsButton.disabled = false;
  }
}

function hideAudioPermissionAction(options = {}) {
  const includeSetup = options.includeSetup ?? true;
  if (openAudioPermissionsButton) {
    openAudioPermissionsButton.hidden = true;
    openAudioPermissionsButton.disabled = false;
  }
  if (includeSetup && setupAudioReadiness) {
    setupAudioReadiness.hidden = true;
  }
  if (setupAudioPermissionsButton) {
    setupAudioPermissionsButton.disabled = false;
  }
}

async function openScreenRecordingSettings(reason) {
  if (!nativeInvoke) return;
  try {
    await nativeInvoke("request_screen_recording_permission");
  } catch (error) {
    logAppError("permissions.request_screen_recording", error, { reason }, "warning");
  }
}

function startTranscriptStallMonitor(source) {
  clearTranscriptStallMonitor();
  transcriptStallTimer = setTimeout(() => {
    if (document.body.dataset.state !== "listening") return;
    if (transcriptEvents.length > 0 || currentPartialTranscript?.text) return;
    const sourceLabel = labelCaptureSource(source);
    const hint = source === "mic"
      ? "目前只聽你的麥克風；如果會議聲音從耳機或喇叭出來，請結束後改選麥克風 + 系統音訊。"
      : source === "mixed"
        ? "請確認麥克風、Meeting Copilot 的螢幕與系統錄音權限，並確認會議聲音不是靜音。"
        : "請確認 Meeting Copilot 的螢幕與系統錄音權限，並確認會議聲音不是靜音。";
    providerState.textContent = `還沒收到逐字稿｜${sourceLabel}。${hint}`;
    logAppError("native.transcription_no_input", "No transcript or partial transcript was received after native listening started", { source, timeoutMs: TRANSCRIPT_STALL_MS }, "warning");
  }, TRANSCRIPT_STALL_MS);
}

function clearTranscriptStallMonitor() {
  if (!transcriptStallTimer) return;
  clearTimeout(transcriptStallTimer);
  transcriptStallTimer = undefined;
}

function updateAppState(state) {
  document.body.dataset.state = state;
}

function enterReviewProcessing({ statusText }) {
  clearTranscriptStallMonitor();
  updateAppState("review");
  resetNativeWindowOpacity();
  reviewScreen.dataset.reviewState = "processing";
  reviewStageLabel.textContent = "會後";
  reviewTitle.textContent = "正在整理";
  reviewStatus.textContent = statusText;
  sessionState.textContent = "整理中";
  providerState.textContent = statusText;
  newMeetingButton.disabled = true;
  setDownloadActionsEnabled(false);
  postMeetingSummary.innerHTML = renderProcessingDocument("正在產生 AI 整理");
  postMeetingTranscript.innerHTML = renderProcessingDocument("正在整理逐字稿");
  downloadState.textContent = "整理完成後會開放下載 AI 整理與逐字稿。";
}

function waitForReviewPaint() {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
}

function finishPostMeetingReview(sessionId = activeSessionId) {
  finalizeMeetingReview({
    statusText: "會議已結束，正在整理記錄。",
    runAiSummary: true,
    sessionId
  });
}

function finalizeMeetingReview({ statusText, runAiSummary = true, allowNewMeeting = true, sessionId = activeSessionId }) {
  clearTranscriptStallMonitor();
  updateAppState("review");
  resetNativeWindowOpacity();
  reviewScreen.dataset.reviewState = "ready";
  reviewStageLabel.textContent = "會後";
  reviewTitle.textContent = "會議記錄";
  reviewStatus.textContent = statusText;
  sessionState.textContent = "整理中";
  providerState.textContent = statusText;
  startButton.disabled = false;
  stopButton.disabled = true;
  newMeetingButton.disabled = !allowNewMeeting;
  const artifact = renderPostMeetingReview();
  reviewFinalized = true;
  const hasContent = hasReviewContent(artifact);
  const readyText = !hasContent
    ? NO_REVIEW_CONTENT_MESSAGE
    : oauthAiEnabled && textProviderStatus?.authenticated
    ? runAiSummary
      ? "本機文件已整理完成，正在用 ChatGPT 更新 AI 整理。"
      : "本機文件已先整理完成，錄音正在背景收尾。"
    : "整理完成，可以檢查文件並下載。";
  reviewStatus.textContent = readyText;
  providerState.textContent = readyText;
  if (hasContent && runAiSummary && !postMeetingAiSummaryStarted) {
    postMeetingAiSummaryStarted = true;
    generateOAuthSummaryIfEnabled(sessionId);
  }
}

function renderProcessingDocument(label) {
  return `<div class="processing-block" aria-busy="true" aria-label="${escapeHtml(label)}">
    <p class="empty-line">${escapeHtml(label)}...</p>
    <div class="processing-line"></div>
    <div class="processing-line medium"></div>
    <div class="processing-line short"></div>
  </div>`;
}

function setDownloadActionsEnabled(enabled) {
  for (const button of downloadButtons) button.disabled = !enabled;
  if (downloadErrorLogButton) downloadErrorLogButton.disabled = !enabled;
}

function renderPostMeetingReview() {
  const artifact = buildMeetingArtifact();
  if (!hasReviewContent(artifact)) {
    postMeetingSummary.innerHTML = renderNoReviewInput(artifact);
    postMeetingTranscript.innerHTML = `<p class="empty-line">本場沒有收到逐字稿。</p>`;
    downloadState.textContent = "沒有可整理內容；請下載錯誤紀錄協助排查，或開始下一場。";
    syncReviewDownloadButtons(artifact);
    return artifact;
  }
  postMeetingSummary.innerHTML = [
    renderReviewList("本場重點", artifact.summary.keyPoints),
    renderReviewList("決策與待確認", artifact.summary.decisionsAndOpenQuestions),
    renderReviewList("建議動作", artifact.summary.suggestedActions)
  ].join("");
  postMeetingTranscript.innerHTML = artifact.transcript.length > 0
    ? artifact.transcript.map((line, index) => `<p><strong>${index + 1}.</strong> ${escapeHtml(line.text)}${line.persistenceStatus === "failed" ? " <em>未儲存</em>" : ""}</p>`).join("")
    : `<p class="empty-line">本場沒有收到逐字稿。</p>`;
  downloadState.textContent = artifact.transcript.length > 0
    ? `已整理 ${artifact.transcript.length} 段逐字稿。建議先下載 AI 整理 Markdown 與逐字稿 TXT。`
    : "沒有逐字稿內容；仍可下載 AI 整理。";
  syncReviewDownloadButtons(artifact);
  return artifact;
}

function renderNoReviewInput(artifact) {
  const diagnostics = artifact.contextDiagnostics ?? { failedFiles: [] };
  const failedFiles = diagnostics.failedFiles ?? [];
  const reasons = [
    "沒有收到任何可用的逐字稿。",
    artifact.prepContext ? "" : "沒有可整理的會前資料。",
    ...failedFiles.map((file) => `未加入 ${file.name}：${file.error}`)
  ].filter(Boolean);
  const actions = [
    "確認音訊來源是否正在播放或有人說話。",
    "若要用會前資料整理，請加入支援的文字檔或直接貼上內容。",
    "保留錯誤紀錄 JSON 供排查。"
  ];
  return [
    renderReviewList("本場沒有可整理內容", reasons),
    renderReviewList("下一步", actions)
  ].join("");
}

function hasReviewContent(artifact) {
  return artifact.transcript.length > 0 || artifact.prepContext.trim().length > 0;
}

function classifyNativeTranscriptionError(message) {
  if (/No speech detected|未偵測到語音|未检测到语音/i.test(message)) return "no_speech_detected";
  if (/stopped from tray/i.test(message)) return "stopped_from_tray";
  if (isScreenRecordingPermissionMessage(message)) return "screen_recording_permission";
  return "native_transcription_error";
}

function syncReviewDownloadButtons(artifact) {
  const hasContent = hasReviewContent(artifact);
  for (const button of downloadButtons) {
    const format = button.dataset.downloadFormat ?? "";
    button.disabled = !hasContent || (format.startsWith("transcript") && artifact.transcript.length === 0);
  }
  if (downloadErrorLogButton) downloadErrorLogButton.disabled = false;
}

function renderReviewList(title, items) {
  const safeItems = items.length > 0 ? items : ["本場沒有足夠資訊產生此區塊。"];
  return `<h3>${escapeHtml(title)}</h3><ul>${safeItems.map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>`;
}

function buildMeetingArtifact() {
  const transcript = transcriptEvents.length > 0
    ? transcriptEvents.map((event) => ({ id: event.id, text: event.text, source: event.source, language: event.language, persistenceStatus: event.persistenceStatus ?? "saved" }))
    : transcriptLines.map((text, index) => ({ id: `line_${index + 1}`, text, source: "unknown", language: detectUiLanguage(text) }));
  const transcriptText = transcript.map((line) => line.text).join("\n");
  const prepContext = setupController.combinedPrepContext();
  const contextDiagnostics = setupController.contextDiagnostics();
  const localSummary = {
    keyPoints: summarizeTranscript(transcriptText),
    decisionsAndOpenQuestions: summarizeDecisionState(latestDecisionState, transcriptText),
    suggestedActions: summarizeSuggestions(suggestionHistory, transcriptText)
  };
  const summary = aiSummaryOverride ?? localSummary;
  return {
    title: "即時會議",
    sessionId: activeSessionId ?? "local_review",
    generatedAt: new Date().toISOString(),
    prepContext,
    summary,
    localSummary,
    summaryProvider: aiSummaryOverride ? "codex-chatgpt-oauth" : "local-rule",
    transcript,
    contextDiagnostics,
    suggestions: suggestionHistory,
    decisionState: latestDecisionState ?? null
  };
}

async function refreshTextProviderStatus() {
  if (!nativeInvoke) {
    textProviderStatus = {
      providerId: "browser-local-rule",
      kind: "local",
      authenticated: false,
      active: false,
      statusLabel: "瀏覽器預覽無法登入 ChatGPT"
    };
    renderTextProviderStatus();
    return;
  }
  textProviderDetail.textContent = "正在檢查 ChatGPT 登入狀態。";
  try {
    textProviderStatus = await nativeInvoke("text_provider_status");
  } catch (error) {
    logAppError("ai.text_provider_status", error, {}, "error");
    textProviderStatus = {
      providerId: "codex-chatgpt-oauth",
      kind: "subscription_oauth",
      authenticated: false,
      active: false,
      statusLabel: "無法檢查 ChatGPT 登入",
      lastError: formatError(error)
    };
  } finally {
    renderTextProviderStatus();
  }
}

async function refreshPlatformShellPlan() {
  if (!nativeInvoke) return;
  try {
    platformShellPlan = await nativeInvoke("desktop_shell_plan_command");
    applyPlatformCaptureAvailability();
  } catch (error) {
    logAppError("platform.shell_plan", error, {}, "warning");
    platformShellPlan = undefined;
  }
}

function applyPlatformCaptureAvailability() {
  const systemOption = captureSource?.querySelector('option[value="system"]');
  const mixedOption = captureSource?.querySelector('option[value="mixed"]');
  if (!systemOption || !mixedOption || !platformShellPlan) return;
  const supportsSystemAudio = /screencapturekit|wasapi_loopback/i.test(platformShellPlan.audioCapture ?? "");
  systemOption.disabled = !supportsSystemAudio;
  systemOption.textContent = supportsSystemAudio ? "系統音訊" : "系統音訊（此平台尚未支援）";
  mixedOption.disabled = !supportsSystemAudio;
  mixedOption.textContent = supportsSystemAudio ? "麥克風 + 系統音訊" : "麥克風 + 系統音訊（此平台尚未支援）";
  if (!supportsSystemAudio && ["system", "mixed"].includes(captureSource.value)) captureSource.value = "mic";
}

function renderTextProviderStatus() {
  const authenticated = Boolean(textProviderStatus?.authenticated);
  providerSettings.classList.toggle("enabled", authenticated && oauthAiEnabled);
  textProviderName.textContent = oauthAiEnabled && authenticated
    ? "ChatGPT 已啟用"
    : "AI 尚未啟用";
  textProviderDetail.textContent = oauthAiEnabled && authenticated
    ? "會議背景整理、即時提醒與會後整理會送到 ChatGPT；語音辨識會分開處理。"
    : authenticated
      ? "已偵測到 ChatGPT 登入；按啟用後才會把會議內容送去 AI 整理。"
      : `${textProviderStatus?.statusLabel ?? "尚未登入 ChatGPT"}；目前不會傳送會議內容。`;
  enableOAuthProviderButton.disabled = !authenticated || oauthAiEnabled;
  enableOAuthProviderButton.textContent = oauthAiEnabled && authenticated ? "AI 已啟用" : "啟用 AI";
  loginTextProviderButton.hidden = authenticated;
  syncStartButtonAvailability();
  setupController.renderPrepSummary();
}

function canStartWithAi() {
  return Boolean(textProviderStatus?.authenticated && oauthAiEnabled);
}

function syncStartButtonAvailability() {
  if (document.body.dataset.state !== "setup") return;
  const ready = canStartWithAi();
  startButton.disabled = !ready;
  updateStartButtonCopy(ready);
}

function updateStartButtonCopy(ready) {
  const title = startButton?.querySelector("span");
  const subtitle = startButton?.querySelector("small");
  if (title) title.textContent = ready ? "開始會議" : "等待 AI";
  if (subtitle) {
    subtitle.textContent = ready
      ? "開始記錄會議音訊"
      : textProviderStatus?.authenticated
        ? "請先按左側啟用 AI"
        : "請先登入 ChatGPT";
  }
}

function readAiEnabledPreference() {
  try {
    return window.localStorage?.getItem(AI_ENABLED_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function setAiEnabledPreference(enabled) {
  oauthAiEnabled = enabled;
  try {
    window.localStorage?.setItem(AI_ENABLED_STORAGE_KEY, String(enabled));
  } catch {
    // Local storage is only for restoring the user's opt-in after restart.
  }
}

function scheduleTextProviderRefresh() {
  for (const timer of textProviderRefreshTimers) clearTimeout(timer);
  textProviderRefreshTimers = [];
  const delays = [3000, 8000, 15000, 30000];
  for (const delay of delays) {
    textProviderRefreshTimers.push(setTimeout(() => refreshTextProviderStatus(), delay));
  }
}

async function generateOAuthSummaryIfEnabled(sessionId = activeSessionId) {
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated) return;
  if (!isCurrentReviewSession(sessionId)) return;
  const artifact = buildMeetingArtifact();
  if (!hasReviewContent(artifact)) {
    renderPostMeetingReview();
    reviewStatus.textContent = NO_REVIEW_AI_SKIP_MESSAGE;
    downloadState.textContent = "沒有可整理內容；可下載錯誤紀錄協助排查。";
    return;
  }
  downloadState.textContent = "正在用 ChatGPT 產生 AI 整理。";
  reviewStatus.textContent = "本機文件已可檢查；ChatGPT 正在更新 AI 整理。";
  try {
    const response = await nativeInvoke("generate_ai_summary_oauth", {
      request: {
        title: artifact.title,
        sessionId: artifact.sessionId,
        generatedAt: artifact.generatedAt,
        prepContext: artifact.prepContext,
        localSummary: artifact.localSummary,
        transcript: artifact.transcript
      }
    });
    if (!isCurrentReviewSession(sessionId)) return;
    aiSummaryOverride = response.summary;
    renderPostMeetingReview();
    reviewStatus.textContent = "AI 整理已更新，可以檢查文件並下載。";
    downloadState.textContent = "AI 整理已更新，可下載 Markdown、JSON 或 PDF。";
  } catch (error) {
    logAppError("ai.post_meeting_summary", error, { sessionId }, "error");
    if (!isCurrentReviewSession(sessionId)) return;
    aiSummaryOverride = undefined;
    renderPostMeetingReview();
    reviewStatus.textContent = "ChatGPT 整理暫時失敗；已保留本機整理，可以先下載。";
    downloadState.textContent = `AI 整理暫時失敗，已保留本機整理：${formatError(error)}`;
  }
}

function isCurrentReviewSession(sessionId) {
  return document.body.dataset.state === "review" && (!sessionId || activeSessionId === sessionId);
}

function downloadMeetingArtifact(format) {
  const artifact = buildMeetingArtifact();
  downloadMeetingArtifactFile(format, { artifact, downloadState, logAppError });
}

async function downloadErrorLog() {
  let records = browserErrorLogs;
  if (nativeInvoke) {
    try {
      records = await nativeInvoke("export_app_error_logs", { sessionId: activeSessionId ?? null });
    } catch (error) {
      logAppError("logs.export", error, { sessionId: activeSessionId ?? null }, "error");
      records = browserErrorLogs;
    }
  }
  const artifact = buildMeetingArtifact();
  const baseName = `meeting-copilot-${artifact.sessionId}`;
  const payload = {
    title: "Meeting Copilot 錯誤紀錄",
    sessionId: activeSessionId ?? null,
    generatedAt: new Date().toISOString(),
    records
  };
  const fileName = `${baseName}-error-log.json`;
  const url = URL.createObjectURL(new Blob([JSON.stringify(payload, null, 2)], { type: "application/json;charset=utf-8" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
  downloadState.textContent = `已準備下載 ${fileName}`;
}

function upsertTranscriptEvent(event) {
  upsertTranscriptEventInPlace(transcriptEvents, transcriptLines, event);
}

function markTranscriptPersistence(id, status) {
  const event = transcriptEvents.find((item) => item.id === id);
  if (event) event.persistenceStatus = status;
}


function renderTranscriptDrawer() {
  renderTranscriptDrawerView({
    elements: { liveTranscript, transcriptDrawerToggle, transcriptDrawerCount, transcriptPreview, transcriptFull },
    transcriptDrawerOpen,
    transcriptEvents,
    transcriptLines,
    currentPartialTranscript,
    escapeHtml,
    detectUiLanguage
  });
}

function installOpacityControl() {
  const apply = () => {
    const clamped = clamp(Number(curtainOpacity.value), 10, 100);
    curtainOpacity.value = String(clamped);
    curtainOpacityValue.textContent = `${clamped}%`;
    document.documentElement.style.setProperty("--curtain-alpha", String(clamped / 100));
    applyWindowOpacityForCurrentState();
  };
  curtainOpacity.addEventListener("input", apply);
  apply();
}

function applyWindowOpacityForCurrentState() {
  if (!nativeInvoke || document.body.dataset.state !== "listening") return;
  const percent = clamp(Number(curtainOpacity.value), 10, 100);
  nativeInvoke("set_window_opacity", { percent }).catch((error) => {
    logAppError("window.opacity", error, { percent }, "warning");
    providerState.textContent = `視窗透明度調整失敗：${formatError(error)}`;
  });
}

function resetNativeWindowOpacity() {
  if (!nativeInvoke) return;
  nativeInvoke("set_window_opacity", { percent: 100 }).catch((error) => {
    logAppError("window.opacity_reset", error, { percent: 100 }, "warning");
  });
}

function createBriefFromSetupContext() {
  const context = setupController.combinedPrepContext();
  const contextLine = context ? `會議背景：${context.slice(0, 1400)}` : "未提供會議背景，會議中只依照即時內容判斷。";
  return {
    sessionId: makeClientId("native"),
    projectId: "live_default_project",
    meetingType: "live_decision_copilot",
    title: "即時會議",
    goal: context
      ? `依據會議背景追蹤會議決策：${context.slice(0, 160)}`
      : "即時追蹤會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope",
    mustConfirm: ["owner", "deadline", "驗收標準", "rollback plan"],
    risks: ["未定義 owner/deadline 就做承諾", "demo scope 和正式版 scope 混在一起"],
    constraints: ["先確認決策條件再承諾交付", contextLine],
    knownParticipants: [],
    preferredTone: "direct",
    startedAt: new Date().toISOString()
  };
}

function logAppError(stage, error, detail = {}, severity = "error") {
  const message = formatError(error);
  const record = {
    sessionId: activeSessionId ?? null,
    stage,
    source: "ui",
    severity,
    message,
    detailJson: sanitizeLogDetail(detail),
    createdAt: new Date().toISOString()
  };
  browserErrorLogs.push(record);
  console.error("[Meeting Copilot]", stage, message, record.detailJson);
  if (!nativeInvoke) return;
  nativeInvoke("log_app_error", {
    input: {
      sessionId: record.sessionId,
      stage: record.stage,
      source: record.source,
      severity: record.severity,
      message: record.message,
      detailJson: record.detailJson
    }
  }).catch((logError) => {
    console.error("[Meeting Copilot] failed to persist app error log", formatError(logError));
  });
}

function sanitizeLogDetail(value) {
  try {
    return JSON.parse(JSON.stringify(value ?? {}));
  } catch {
    return { serializationError: "detail was not JSON serializable" };
  }
}
