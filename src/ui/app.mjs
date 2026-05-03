const startButton = document.querySelector("#startListening");
const stopButton = document.querySelector("#stopListening");
const sessionState = document.querySelector("#sessionState");
const providerState = document.querySelector("#providerState");
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

let recognition;
let transcriptLines = [];
let transcriptEvents = [];
let currentPartialTranscript;
let suggestionHistory = [];
let latestDecisionState;
let aiSummaryOverride;
let textProviderStatus;
let oauthAiEnabled = false;
let activeSessionId;
let transcriptIndex = 0;
let startedAt = 0;
let nativeListenersInstalled = false;
let droppedFileNames = [];
let droppedContextChunks = [];
let prepDictating = false;
let transcriptDrawerOpen = false;
let liveAiExtractionRunning = false;
let lastAiExtractionEventCount = 0;
let prepSummaryTimer;
let prepSummaryRequestId = 0;
let prepSummaryInFlight = false;
let prepSummaryQueued = false;
let platformShellPlan;
let browserErrorLogs = [];

const nativeInvoke = window.__TAURI__?.core?.invoke;
const nativeListen = window.__TAURI__?.event?.listen;

initializeSetupState();
installNativeDropListeners().catch((error) => {
  logAppError("ui.install_native_drop_listeners", error, {}, "error");
  setupContextMeta.textContent = `檔案拖拉尚未啟用：${formatError(error)}`;
});
installPrepDictationListeners().catch((error) => {
  logAppError("ui.install_prep_dictation_listeners", error, {}, "error");
  setupContextMeta.textContent = `語音輸入尚未啟用：${formatError(error)}`;
});
installOpacityControl();
refreshPlatformShellPlan();
refreshTextProviderStatus();

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
  updateAppState("listening");
  applyWindowOpacityForCurrentState();
  providerState.textContent = "正在開始會議...";
  if (nativeInvoke) {
    try {
      await startNativeListening();
    } catch (error) {
      logAppError("meeting.start_native", error, { source: captureSource?.value ?? "mic" }, "error");
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
    finalizeMeetingReview({ statusText: "會議已結束，正在整理記錄。" });
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
  stopButton.disabled = true;
  sessionState.textContent = "整理中";
  providerState.textContent = "正在結束會議...";
  if (nativeInvoke && activeSessionId) {
    try {
      await stopNativeListening();
    } catch (error) {
      logAppError("meeting.stop_native", error, { sessionId: activeSessionId }, "error");
      providerState.textContent = `結束會議失敗：${error.message}`;
      stopButton.disabled = false;
    }
    return;
  }
  if (recognition) {
    recognition.stop();
  } else {
    finalizeMeetingReview({ statusText: "會議已結束，正在整理記錄。" });
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
  oauthAiEnabled = true;
  renderTextProviderStatus();
  schedulePrepSummaryGeneration();
});

prepDictationButton.addEventListener("click", async () => {
  if (!nativeInvoke) {
    logAppError("prep.dictation_unavailable", "Prep dictation requires the desktop app", {}, "warning");
    setupContextMeta.textContent = "語音輸入需要使用桌面 app。";
    return;
  }
  if (!prepDictating && !canStartWithAi()) {
    logAppError("prep.dictation_blocked_without_ai", "Prep dictation was requested before AI was enabled", { authenticated: Boolean(textProviderStatus?.authenticated) }, "warning");
    syncStartButtonAvailability();
    setupContextMeta.textContent = textProviderStatus?.authenticated
      ? "請先啟用 AI，才能使用語音輸入。"
      : "請先登入 ChatGPT，才能使用語音輸入。";
    return;
  }
  prepDictationButton.disabled = true;
  try {
    if (prepDictating) {
      await nativeInvoke("stop_prep_dictation");
      setPrepDictating(false);
      setupContextMeta.textContent = "語音輸入已停止。";
    } else {
      await nativeInvoke("start_prep_dictation");
      setPrepDictating(true);
      setupContextMeta.textContent = "正在記錄你補充的會議背景。說完後會自動加入文字欄。";
    }
  } catch (error) {
    logAppError("prep.dictation_toggle", error, { nextState: prepDictating ? "stop" : "start" }, "error");
    setPrepDictating(false);
    setupContextMeta.textContent = `語音輸入無法啟動：${formatError(error)}`;
  } finally {
    prepDictationButton.disabled = false;
  }
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
  resetListeningSurface();
  if (prepDictating) {
    await nativeInvoke("stop_prep_dictation").catch((error) => {
      logAppError("prep.dictation_stop_before_meeting", error, {}, "warning");
    });
    setPrepDictating(false);
  }
  sessionState.textContent = "建立會議";
  activeSessionId = await createLiveSession();
  sessionState.textContent = "檢查音訊";
  await installNativeListeners();
  const health = await nativeInvoke("native_transcriber_health");
  if (!health.ready) {
    providerState.textContent = `音訊尚未就緒：${health.lastError ?? "未知原因"}`;
  }
  sessionState.textContent = "開始記錄";
  const started = await nativeInvoke("start_native_transcription", {
    sessionId: activeSessionId,
    request: { language: "zh-TW", source: captureSource?.value ?? "mic" }
  });
  updateAppState("listening");
  sessionState.textContent = "記錄中";
  providerState.textContent = `正在聽｜${labelCaptureSource(started.source)}｜${labelLanguage(started.language)}`;
  startButton.disabled = true;
  stopButton.disabled = false;
}

async function stopNativeListening() {
  await nativeInvoke("stop_native_transcription", { sessionId: activeSessionId });
  await nativeInvoke("stop_session", { sessionId: activeSessionId });
  finalizeMeetingReview({ statusText: "會議已結束，正在整理記錄。" });
}

async function installNativeListeners() {
  if (nativeListenersInstalled || !nativeListen) return;
  try {
    nativeListenersInstalled = true;
    await nativeListen("native_transcript_ingested", (event) => {
      const payload = event.payload;
      if (!payload?.event?.text) return;
      currentPartialTranscript = undefined;
      renderRuntimePayload(payload);
      maybeRunLiveAiExtraction();
    });
    await nativeListen("native_transcript_preview", (event) => {
      const payload = event.payload;
      if (!payload?.text) return;
      currentPartialTranscript = payload;
      renderTranscriptDrawer();
    });
    await nativeListen("native_transcription_error", (event) => {
      const message = String(event.payload ?? "");
      logAppError("native.transcription_event_error", message, { source: captureSource?.value ?? "mic" }, "error");
      providerState.textContent = formatAudioMonitorMessage(message);
      if (message.includes("stopped from tray") && document.body.dataset.state === "listening") {
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
  sessionState.textContent = "無法開始";
  providerState.textContent = "這個環境沒有可用的語音辨識。請改用桌面 app。";
  suggestion.className = "suggestion-empty";
  suggestion.textContent = "目前沒有可用的音訊。";
  feedbackRow.hidden = true;
}

function initializeSetupState() {
  updateAppState("setup");
  resetNativeWindowOpacity();
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
  prepSummaryInFlight = false;
  prepSummaryQueued = false;
  sessionState.textContent = "待機中";
  providerState.textContent = "尚未開始會議。";
  resetListeningSurface();
  renderPrepSummary();
}

function resetListeningSurface() {
  transcriptDrawerOpen = false;
  currentPartialTranscript = undefined;
  renderTranscriptDrawer();
  suggestion.className = "suggestion-empty";
  suggestion.textContent = "目前沒有提醒。";
  feedbackRow.hidden = true;
}

function updateAppState(state) {
  document.body.dataset.state = state;
}

function finalizeMeetingReview({ statusText }) {
  updateAppState("review");
  resetNativeWindowOpacity();
  sessionState.textContent = "整理中";
  providerState.textContent = statusText;
  startButton.disabled = false;
  stopButton.disabled = true;
  renderPostMeetingReview();
  generateOAuthSummaryIfEnabled();
}

function renderPostMeetingReview() {
  const artifact = buildMeetingArtifact();
  postMeetingSummary.innerHTML = [
    renderReviewList("本場重點", artifact.summary.keyPoints),
    renderReviewList("決策與待確認", artifact.summary.decisionsAndOpenQuestions),
    renderReviewList("建議動作", artifact.summary.suggestedActions)
  ].join("");
  postMeetingTranscript.innerHTML = artifact.transcript.length > 0
    ? artifact.transcript.map((line, index) => `<p><strong>${index + 1}.</strong> ${escapeHtml(line.text)}</p>`).join("")
    : `<p class="empty-line">本場沒有收到逐字稿。</p>`;
  downloadState.textContent = artifact.transcript.length > 0
    ? `已整理 ${artifact.transcript.length} 段逐字稿。建議先下載 AI 整理 Markdown 與逐字稿 TXT。`
    : "沒有逐字稿內容；仍可下載 AI 整理。";
}

function renderReviewList(title, items) {
  const safeItems = items.length > 0 ? items : ["本場沒有足夠資訊產生此區塊。"];
  return `<h3>${escapeHtml(title)}</h3><ul>${safeItems.map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>`;
}

function buildMeetingArtifact() {
  const transcript = transcriptEvents.length > 0
    ? transcriptEvents.map((event) => ({ id: event.id, text: event.text, source: event.source, language: event.language }))
    : transcriptLines.map((text, index) => ({ id: `line_${index + 1}`, text, source: "unknown", language: detectUiLanguage(text) }));
  const transcriptText = transcript.map((line) => line.text).join("\n");
  const prepContext = combinedPrepContext();
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
  if (!systemOption || !platformShellPlan) return;
  const supportsSystemAudio = /screencapturekit|wasapi_loopback/i.test(platformShellPlan.audioCapture ?? "");
  systemOption.disabled = !supportsSystemAudio;
  systemOption.textContent = supportsSystemAudio ? "系統音訊" : "系統音訊（此平台尚未支援）";
  if (!supportsSystemAudio && captureSource.value === "system") captureSource.value = "mic";
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
  renderPrepSummary();
}

function canStartWithAi() {
  return Boolean(textProviderStatus?.authenticated && oauthAiEnabled);
}

function syncStartButtonAvailability() {
  if (document.body.dataset.state !== "setup") return;
  startButton.disabled = !canStartWithAi();
}

function scheduleTextProviderRefresh() {
  const delays = [3000, 8000, 15000, 30000];
  for (const delay of delays) {
    setTimeout(() => refreshTextProviderStatus(), delay);
  }
}

async function generateOAuthSummaryIfEnabled() {
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated) return;
  downloadState.textContent = "正在用 ChatGPT 產生 AI 整理。";
  try {
    const artifact = buildMeetingArtifact();
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
    aiSummaryOverride = response.summary;
    renderPostMeetingReview();
    downloadState.textContent = "AI 整理已更新，可下載 Markdown、JSON 或 PDF。";
  } catch (error) {
    logAppError("ai.post_meeting_summary", error, { sessionId: activeSessionId }, "error");
    aiSummaryOverride = undefined;
    renderPostMeetingReview();
    downloadState.textContent = `AI 整理暫時失敗，已保留本機整理：${formatError(error)}`;
  }
}

function summarizeTranscript(text) {
  if (!text.trim()) return [];
  const lines = text
    .split(/\n|。|；|;/)
    .map((line) => line.trim())
    .filter(Boolean);
  const priority = lines.filter((line) => /決定|結論|scope|範圍|deadline|期限|owner|負責|驗收|rollback|風險|risk|blocker/i.test(line));
  return uniqueLimited([...priority, ...lines], 5);
}

function summarizeDecisionState(decisionState, text) {
  const items = [];
  if (decisionState?.readiness) {
    items.push(`決策完整度 ${Math.round(decisionState.readiness.score * 100)}%，${decisionState.readiness.safeToDecide ? "目前可進入決策" : "仍不建議直接承諾"}`);
    for (const blocker of decisionState.readiness.blockers ?? []) items.push(`待補：${blocker}`);
  }
  const openLines = text
    .split(/\n|。|；|;/)
    .map((line) => line.trim())
    .filter((line) => /待確認|未定|還沒|不確定|下次|follow up|確認一下|再確認/i.test(line));
  return uniqueLimited([...items, ...openLines], 6);
}

function summarizeSuggestions(suggestions, text) {
  const shown = suggestions.map((item) => `${labelMove(item.kind)}：${item.text}`);
  const actionLines = text
    .split(/\n|。|；|;/)
    .map((line) => line.trim())
    .filter((line) => /要做|負責|owner|action|todo|下次|follow up|確認|補/i.test(line));
  return uniqueLimited([...shown, ...actionLines], 6);
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

function downloadMeetingArtifact(format) {
  const artifact = buildMeetingArtifact();
  const summaryDocument = buildAiSummaryDocument(artifact);
  const transcriptDocument = buildTranscriptDocument(artifact);
  const baseName = `meeting-copilot-${artifact.sessionId}`;
  const files = {
    "summary-json": {
      name: `${baseName}-ai-summary.json`,
      type: "application/json;charset=utf-8",
      content: JSON.stringify(summaryDocument, null, 2)
    },
    "summary-markdown": {
      name: `${baseName}-ai-summary.md`,
      type: "text/markdown;charset=utf-8",
      content: renderAiSummaryMarkdown(summaryDocument)
    },
    "transcript-json": {
      name: `${baseName}-transcript.json`,
      type: "application/json;charset=utf-8",
      content: JSON.stringify(transcriptDocument, null, 2)
    },
    "transcript-txt": {
      name: `${baseName}-transcript.txt`,
      type: "text/plain;charset=utf-8",
      content: renderTranscriptText(transcriptDocument)
    }
  };
  if (format === "summary-pdf") {
    openPrintableDocument({
      title: `${artifact.title} AI 整理`,
      filenameHint: `${baseName}-ai-summary.pdf`,
      bodyHtml: renderAiSummaryHtml(summaryDocument)
    });
    return;
  }
  if (format === "transcript-pdf") {
    openPrintableDocument({
      title: `${artifact.title} 逐字稿`,
      filenameHint: `${baseName}-transcript.pdf`,
      bodyHtml: renderTranscriptHtml(transcriptDocument)
    });
    return;
  }
  const file = files[format];
  if (!file) return;
  const url = URL.createObjectURL(new Blob([file.content], { type: file.type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = file.name;
  anchor.click();
  URL.revokeObjectURL(url);
  downloadState.textContent = `已準備下載 ${file.name}`;
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

function buildAiSummaryDocument(artifact) {
  return {
    title: `${artifact.title} AI 整理`,
    sessionId: artifact.sessionId,
    generatedAt: artifact.generatedAt,
    prepContext: artifact.prepContext,
    summary: artifact.summary,
    suggestions: artifact.suggestions,
    decisionState: artifact.decisionState
  };
}

function buildTranscriptDocument(artifact) {
  return {
    title: `${artifact.title} 逐字稿`,
    sessionId: artifact.sessionId,
    generatedAt: artifact.generatedAt,
    transcript: artifact.transcript
  };
}

function renderAiSummaryMarkdown(document) {
  const section = (title, items) => [`## ${title}`, ...(items.length > 0 ? items.map((item) => `- ${item}`) : ["- 本場沒有足夠資訊產生此區塊。"])].join("\n");
  return [
    `# ${document.title}`,
    `Session: ${document.sessionId}`,
    `Generated: ${document.generatedAt}`,
    "",
    section("本場重點", document.summary.keyPoints),
    "",
    section("決策與待確認", document.summary.decisionsAndOpenQuestions),
    "",
    section("建議動作", document.summary.suggestedActions)
  ].join("\n");
}

function renderTranscriptText(document) {
  return [
    document.title,
    `Session: ${document.sessionId}`,
    `Generated: ${document.generatedAt}`,
    "",
    ...(document.transcript.length > 0
      ? document.transcript.map((line, index) => `${index + 1}. ${line.text}`)
      : ["本場沒有收到逐字稿。"])
  ].join("\n");
}

function renderAiSummaryHtml(document) {
  const section = (title, items) => `
    <section>
      <h2>${escapeHtml(title)}</h2>
      <ul>${(items.length > 0 ? items : ["本場沒有足夠資訊產生此區塊。"]).map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>
    </section>`;
  return [
    `<h1>${escapeHtml(document.title)}</h1>`,
    `<p>Session: ${escapeHtml(document.sessionId)}<br />Generated: ${escapeHtml(document.generatedAt)}</p>`,
    section("本場重點", document.summary.keyPoints),
    section("決策與待確認", document.summary.decisionsAndOpenQuestions),
    section("建議動作", document.summary.suggestedActions)
  ].join("");
}

function renderTranscriptHtml(document) {
  const lines = document.transcript.length > 0
    ? document.transcript.map((line, index) => `<p><strong>${index + 1}.</strong> ${escapeHtml(line.text)}</p>`).join("")
    : "<p>本場沒有收到逐字稿。</p>";
  return [
    `<h1>${escapeHtml(document.title)}</h1>`,
    `<p>Session: ${escapeHtml(document.sessionId)}<br />Generated: ${escapeHtml(document.generatedAt)}</p>`,
    `<section>${lines}</section>`
  ].join("");
}

function openPrintableDocument({ title, filenameHint, bodyHtml }) {
  const printWindow = window.open("", "_blank");
  if (!printWindow) {
    logAppError("download.pdf_popup_blocked", "Printable PDF window was blocked", { title, filenameHint }, "warning");
    downloadState.textContent = "無法開啟 PDF 列印視窗，請允許彈出視窗後再試。";
    return;
  }
  printWindow.document.write(`<!doctype html>
    <html lang="zh-Hant">
      <head>
        <meta charset="UTF-8" />
        <title>${escapeHtml(title)}</title>
        <style>
          body { color: #111; font: 14px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 36px; }
          h1 { font-size: 28px; line-height: 1.2; margin: 0 0 12px; }
          h2 { font-size: 18px; margin: 24px 0 8px; }
          p { margin: 0 0 10px; }
          li { margin: 4px 0; }
          @page { margin: 18mm; }
        </style>
      </head>
      <body>${bodyHtml}</body>
    </html>`);
  printWindow.document.close();
  printWindow.focus();
  printWindow.print();
  downloadState.textContent = `已開啟 PDF 列印視窗，建議檔名：${filenameHint}`;
}

function upsertTranscriptEvent(event) {
  if (!event?.text) return;
  if (transcriptEvents.some((existing) => existing.id === event.id)) return;
  transcriptEvents.push(event);
  if (!transcriptLines.includes(event.text)) transcriptLines.push(event.text);
}

function renderTranscriptDrawer() {
  const lines = currentTranscriptLines();
  const latest = lines.slice(-3);
  transcriptDrawerToggle.textContent = transcriptDrawerOpen ? "收合" : "展開";
  transcriptDrawerToggle.setAttribute("aria-expanded", String(transcriptDrawerOpen));
  transcriptFull.hidden = !transcriptDrawerOpen;
  const finalCount = transcriptEvents.length > 0 ? transcriptEvents.length : transcriptLines.length;
  transcriptDrawerCount.textContent = currentPartialTranscript?.text ? `${finalCount} 句｜記錄中` : `${finalCount} 句`;
  liveTranscript.classList.toggle("expanded", transcriptDrawerOpen);
  transcriptPreview.innerHTML = latest.length > 0
    ? latest.map((line) => renderTranscriptLine(line)).join("")
    : '<p class="empty-line">最近 3 句會顯示在這裡。</p>';
  transcriptFull.innerHTML = transcriptDrawerOpen
    ? lines.map((line) => renderTranscriptLine(line)).join("")
    : "";
  transcriptPreview.scrollTop = transcriptPreview.scrollHeight;
  transcriptFull.scrollTop = transcriptFull.scrollHeight;
}

function currentTranscriptLines() {
  const partialLine = currentPartialTranscript?.text
    ? {
        text: currentPartialTranscript.text,
        speaker: transcriptSpeakerLabel(currentPartialTranscript),
        source: currentPartialTranscript.source ?? "unknown",
        language: currentPartialTranscript.language,
        partial: true,
        index: transcriptEvents.length
      }
    : undefined;
  if (transcriptEvents.length > 0) {
    const lines = transcriptEvents.map((event, index) => ({
      text: event.text,
      speaker: transcriptSpeakerLabel(event),
      source: event.source ?? "unknown",
      language: event.language,
      index
    }));
    if (partialLine) lines.push(partialLine);
    return lines;
  }
  const lines = transcriptLines.map((text, index) => ({
    text,
    speaker: "未知",
    source: "unknown",
    language: detectUiLanguage(text),
    index
  }));
  if (partialLine) lines.push(partialLine);
  return lines;
}

function renderTranscriptLine(line) {
  return `<p class="transcript-line${line.partial ? " partial" : ""}"><span>${escapeHtml(line.speaker)}</span><span>${escapeHtml(line.text)}${line.partial ? " <em>記錄中</em>" : ""}</span></p>`;
}

function transcriptSpeakerLabel(event) {
  if (event.speaker) return event.speaker;
  if (event.source === "mic") return "我";
  if (event.source === "system") return "系統音訊";
  return "未知";
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
  const context = combinedPrepContext();
  const contextLine = context ? `會議背景：${context.slice(0, 1400)}` : "未提供會議背景，會議中只依照即時內容判斷。";
  return {
    sessionId: `native_${Date.now()}`,
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

setupContext.addEventListener("input", () => {
  const length = setupContext.value.trim().length;
  setupContextMeta.textContent = length > 0
    ? `已加入 ${length} 字會議背景。`
    : "尚未加入會議背景。";
  renderPrepSummary();
  schedulePrepSummaryGeneration();
});

setupDropZone.addEventListener("dragover", (event) => {
  event.preventDefault();
  setupDropZone.classList.add("drag-over");
});

setupDropZone.addEventListener("dragleave", () => {
  setupDropZone.classList.remove("drag-over");
});

setupDropZone.addEventListener("drop", async (event) => {
  event.preventDefault();
  setupDropZone.classList.remove("drag-over");
  const files = [...(event.dataTransfer?.files ?? [])];
  if (files.length === 0) return;
  const loaded = [];
  for (const file of files) {
    loaded.push(await readBrowserDroppedFile(file));
    droppedFileNames.push(file.name);
  }
  appendDroppedContext(loaded.filter(Boolean));
  setupContextMeta.textContent = `已加入檔案：${droppedFileNames.slice(-3).join("、")}。`;
});

async function installPrepDictationListeners() {
  if (!nativeListen) return;
  await nativeListen("prep_dictation_text", (event) => {
    const text = String(event.payload ?? "").trim();
    if (!text) return;
    setupContext.value = [setupContext.value.trim(), text].filter(Boolean).join("\n");
    setupContext.dispatchEvent(new Event("input"));
  });
  await nativeListen("prep_dictation_error", (event) => {
    logAppError("prep.dictation_event_error", String(event.payload ?? ""), {}, "error");
    setupContextMeta.textContent = `語音輸入狀態：${event.payload}`;
  });
}

async function installNativeDropListeners() {
  if (!nativeListen) return;
  await nativeListen("tauri://drag-enter", () => {
    setupDropZone.classList.add("drag-over");
  });
  await nativeListen("tauri://drag-leave", () => {
    setupDropZone.classList.remove("drag-over");
  });
  await nativeListen("tauri://drag-drop", async (event) => {
    setupDropZone.classList.remove("drag-over");
    const paths = event.payload?.paths ?? [];
    if (!Array.isArray(paths) || paths.length === 0 || !nativeInvoke) return;
    let files;
    try {
      files = await nativeInvoke("read_dropped_context_files", { paths });
    } catch (error) {
      logAppError("files.native_drop_read", error, { pathCount: paths.length }, "error");
      setupContextMeta.textContent = `檔案讀取失敗：${formatError(error)}`;
      return;
    }
    const loaded = files
      .filter((file) => file.text)
      .map((file) => ({ name: file.name, text: file.text, truncated: file.truncated }));
    appendDroppedContext(loaded);
    const errors = files.filter((file) => file.error).map((file) => `${file.name}: ${file.error}`);
    for (const file of files.filter((file) => file.error)) {
      logAppError("files.native_drop_file", file.error, { name: file.name }, "warning");
    }
    droppedFileNames.push(...files.map((file) => file.name));
    setupContextMeta.textContent = errors.length > 0
      ? `部分檔案未讀取：${errors.slice(0, 2).join("；")}`
      : `已加入檔案：${droppedFileNames.slice(-3).join("、")}。啟用 AI 後會送出檔案文字內容。`;
  });
}

function appendDroppedContext(chunks) {
  const normalized = chunks
    .map((chunk) => typeof chunk === "string" ? { name: "拖入內容", text: chunk, truncated: false } : chunk)
    .filter((chunk) => chunk.text?.trim());
  if (normalized.length === 0) return;
  droppedContextChunks.push(...normalized);
  droppedFileCount.textContent = `已加入 ${droppedContextChunks.length} 個檔案`;
  renderPrepSummary();
  schedulePrepSummaryGeneration();
}

function readBrowserDroppedFile(file) {
  return new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(`檔案 ${file.name}\n${String(reader.result).slice(0, 8000)}`);
    reader.onerror = () => {
      logAppError("files.browser_drop_file", reader.error ?? "browser FileReader failed", { name: file.name, size: file.size, type: file.type }, "warning");
      resolve("");
    };
    reader.readAsText(file);
  });
}

function setPrepDictating(enabled) {
  prepDictating = enabled;
  prepDictationButton.textContent = enabled ? "停止語音輸入" : "語音輸入";
  prepDictationButton.classList.toggle("recording", enabled);
}

function combinedPrepContext() {
  const typed = setupContext.value.trim();
  const files = droppedContextChunks
    .map((chunk) => `檔案 ${chunk.name}${chunk.truncated ? "（已截斷）" : ""}\n${chunk.text}`)
    .join("\n\n");
  return [typed, files].filter(Boolean).join("\n\n").trim();
}

function renderPrepSummary() {
  const context = combinedPrepContext();
  if (!canStartWithAi()) {
    prepSummary.textContent = "請先登入並啟用 AI。啟用後，這裡會整理會議背景，並開放開始會議。";
    return;
  }
  if (!context) {
    prepSummary.textContent = "加入文字、語音或檔案後，AI 會整理這場會議要注意的重點。";
    return;
  }
  schedulePrepSummaryGeneration();
}

function schedulePrepSummaryGeneration() {
  clearTimeout(prepSummaryTimer);
  if (!nativeInvoke || !canStartWithAi()) return;
  const context = combinedPrepContext();
  if (!context) return;
  prepSummary.textContent = "AI 正在整理會議背景...";
  const requestId = ++prepSummaryRequestId;
  prepSummaryTimer = setTimeout(() => generatePrepSummary(requestId), 650);
}

async function generatePrepSummary(requestId) {
  const context = combinedPrepContext();
  if (!context || !canStartWithAi()) return;
  if (prepSummaryInFlight) {
    prepSummaryQueued = true;
    return;
  }
  prepSummaryInFlight = true;
  try {
    const response = await nativeInvoke("generate_prep_summary_oauth", {
      request: {
        context,
        fileCount: droppedContextChunks.length
      }
    });
    if (requestId !== prepSummaryRequestId) return;
    const points = response.keyPoints ?? [];
    prepSummary.innerHTML = points.length > 0
      ? `<ul>${points.map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>`
      : "AI 沒有從目前內容整理出明確重點。";
  } catch (error) {
    if (requestId !== prepSummaryRequestId) return;
    logAppError("ai.prep_summary", error, { fileCount: droppedContextChunks.length }, "error");
    prepSummary.textContent = `AI 整理會議背景失敗：${formatError(error)}`;
  } finally {
    prepSummaryInFlight = false;
    if (prepSummaryQueued) {
      prepSummaryQueued = false;
      schedulePrepSummaryGeneration();
    }
  }
}

function labelMove(kind) {
  return {
    ask_question: "Ask｜補問",
    defer_decision: "Hold｜先停一下",
    split_decision: "Clarify｜拆清楚",
    challenge_assumption: "Hold｜挑戰假設",
    confirm_commitment: "Decide｜確認承諾",
    surface_tradeoff: "Clarify｜攤開取捨",
    identify_missing_input: "Ask｜補齊條件"
  }[kind] ?? "決策建議";
}

function labelCaptureSource(source) {
  if (source === "mic") return "我的麥克風";
  if (source === "system") return "系統音訊";
  return source ?? "未知音訊";
}

function labelLanguage(language) {
  if (!language) return "自動語言";
  if (/zh/i.test(language)) return "中文";
  if (/en/i.test(language)) return "英文";
  return language;
}

function formatAudioMonitorMessage(message) {
  if (message.includes("stopped from tray")) return "已從系統列結束會議。";
  if (/permission|denied/i.test(message)) return "沒有麥克風或系統音訊權限。";
  if (/read-only file system/i.test(message)) return "目前無法啟動音訊，請重新開啟 app 後再試。";
  return `語音狀態：${message}`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function clamp(value, min, max) {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function detectUiLanguage(text) {
  if (/[\u4e00-\u9fff]/.test(text) && /[a-z]/i.test(text)) return "mixed";
  if (/[\u4e00-\u9fff]/.test(text)) return "zh";
  return "en";
}

function formatError(error) {
  if (typeof error === "string") return error;
  return error?.message ?? JSON.stringify(error);
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
