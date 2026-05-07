import { downloadMeetingArtifact as downloadMeetingArtifactFile, escapeHtml } from "./documentExports.mjs";
import { labelMove, summarizeDecisionState, summarizeSuggestions, summarizeTranscript } from "./meetingSummaries.mjs";
import {
  LIVE_AI_POLICY,
  createLiveAiPipelineState,
  hasPendingLiveAiWatchedEvents,
  markLiveAiFailure,
  markLiveAiSuccess,
  nextLiveAiDecision,
  shouldTriggerLiveAiForEvent
} from "./liveAiPolicy.mjs";
import { PARTIAL_TRANSCRIPT_COMMIT_IDLE_MS, shouldCommitIdlePartial, shouldCommitReplacedPartial } from "./partialTranscriptPolicy.mjs";
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
const listeningCurtain = document.querySelector(".listening-curtain");
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
const meetingSeriesChoice = document.querySelector("#meetingSeriesChoice");
const meetingSeriesDetail = document.querySelector("#meetingSeriesDetail");
const refreshMeetingSeriesButton = document.querySelector("#refreshMeetingSeries");
const feedbackRow = document.querySelector("#feedbackRow");
const newMeetingButton = document.querySelector("#newMeeting");
const reviewScreen = document.querySelector(".review-screen");
const reviewStageLabel = document.querySelector("#reviewStageLabel");
const reviewTitle = document.querySelector("#reviewTitle");
const reviewStatus = document.querySelector("#reviewStatus");
const postMeetingSummary = document.querySelector("#postMeetingSummary");
const postMeetingTranscript = document.querySelector("#postMeetingTranscript");
const downloadState = document.querySelector("#downloadState");
const historySeriesTarget = document.querySelector("#historySeriesTarget");
const historySeriesTitle = document.querySelector("#historySeriesTitle");
const historyAllowAiContext = document.querySelector("#historyAllowAiContext");
const saveMeetingHistoryButton = document.querySelector("#saveMeetingHistory");
const historySaveState = document.querySelector("#historySaveState");
const downloadButtons = document.querySelectorAll("[data-download-format]");
const downloadErrorLogButton = document.querySelector("#downloadErrorLog");
const textProviderName = document.querySelector("#textProviderName");
const textProviderDetail = document.querySelector("#textProviderDetail");
const loginTextProviderButton = document.querySelector("#loginTextProvider");
const enableOAuthProviderButton = document.querySelector("#enableOAuthProvider");
const openTextProviderInstallGuideButton = document.querySelector("#openTextProviderInstallGuide");
const copyTextProviderInstallCommandButton = document.querySelector("#copyTextProviderInstallCommand");
const refreshTextProviderStatusButton = document.querySelector("#refreshTextProviderStatus");
const providerSettings = document.querySelector(".provider-settings");
const textProviderChoices = document.querySelectorAll("[data-text-provider-choice]");
const sttSettings = document.querySelector(".stt-settings");
const sttProfileName = document.querySelector("#sttProfileName");
const sttProfileDetail = document.querySelector("#sttProfileDetail");
const sttProfileChoice = document.querySelector("#sttProfileChoice");
const downloadSttModelButton = document.querySelector("#downloadSttModel");
const openSttModelFolderButton = document.querySelector("#openSttModelFolder");
const sttDownloadProgress = document.querySelector("#sttDownloadProgress");
const sttDownloadProgressFill = document.querySelector("#sttDownloadProgress span");
const TEXT_PROVIDER_STORAGE_KEY = "meetingCopilot.textProvider";
const LEGACY_AI_STORAGE_KEY = "meetingCopilot.aiEnabled";
const LOCAL_STT_PROFILE_STORAGE_KEY = "meetingCopilot.localSttProfile";
const DEFAULT_MEETING_TITLE = "即時會議";
const TEXT_PROVIDERS = {
  "codex-chatgpt-oauth": {
    label: "Codex CLI",
    serviceLabel: "ChatGPT",
    installCommand: "npm install -g @openai/codex",
    installUrl: "https://help.openai.com/en/articles/11096431-openai-codex-cli-getting-started",
    loginLabel: "登入 Codex CLI",
    unavailableStatus: "瀏覽器預覽無法使用 Codex CLI",
    missingLogin: "請先登入 Codex CLI"
  },
  "claude-code-oauth": {
    label: "Claude Code CLI",
    serviceLabel: "Claude",
    installCommand: "npm install -g @anthropic-ai/claude-code",
    installUrl: "https://docs.anthropic.com/en/docs/claude-code/getting-started",
    loginLabel: "登入 Claude Code CLI",
    unavailableStatus: "瀏覽器預覽無法使用 Claude Code CLI",
    missingLogin: "請先登入 Claude Code CLI"
  }
};
let recognition;
let transcriptLines = [];
let transcriptEvents = [];
let revisedTranscriptEvents = [];
let transcriptRevisionMeta = new Map();
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
let transcriptDrawerOpen = false;
let liveAiExtractionTimer;
let liveAiPipelineState = createLiveAiPipelineState();
let transcriptRevisionRunning = false;
let lastTranscriptRevisionEventCount = 0;
let lastTranscriptRevisionAt = 0;
let transcriptRevisionTimer;
const aiActivityMessages = new Map();
let platformShellPlan;
let browserErrorLogs = [];
let textProviderRefreshTimers = [];
let transcriptStallTimer;
let partialTranscriptCommitTimer;
let partialTranscriptLastUpdatedAt = 0;
let partialTranscriptFirstSeenAt = 0;
let reviewFinalized = false;
let postMeetingAiSummaryStarted = false;
let activeCaptureSource;
let selectedTextProviderId = readTextProviderPreference();
let selectedLocalSttProfileId = readLocalSttProfilePreference();
let meetingSeriesOptions = [];
let selectedMeetingSeriesId = "";
let localSttStatus;
let localSttDownloadState;
let sttFolderMessageTimer;
const TRANSCRIPT_STALL_MS = 12000;
const TRANSCRIPT_REVISION_DEBOUNCE_MS = 3500;
const TRANSCRIPT_REVISION_CONTEXT_WINDOW_SIZE = 30;
const TRANSCRIPT_REVISION_EDITABLE_WINDOW_SIZE = 12;
const TRANSCRIPT_REVISION_MIN_NEW_EVENTS = 3;
const TRANSCRIPT_REVISION_MAX_WAIT_MS = 10000;
const TRANSCRIPT_REVISION_STABLE_AFTER_REVISIONS = 2;
const TRANSCRIPT_REVISION_STABLE_AFTER_TRAILING_EVENTS = 4;
const TRANSCRIPT_REVISION_META_PRUNE_THRESHOLD = 200;
const LIVE_TRANSCRIPT_REVISION_DURING_CAPTURE_ENABLED = false;
const NO_REVIEW_CONTENT_MESSAGE = "沒有收到逐字稿，也沒有可整理的會前資料。請確認音訊來源或加入會議背景後再開始下一場。";
const NO_REVIEW_AI_SKIP_MESSAGE = "沒有收到逐字稿，也沒有可整理的會前資料；不會送出空內容給 AI。";
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
  selectedTextProviderId: () => selectedTextProviderId,
  selectedTextProviderLabel,
  logAppError,
  formatError,
  escapeHtml
});
syncTextProviderChoiceControls();
initializeSetupState();
setupController.install();
installOpacityControl();
installLocalSttDownloadListener();
refreshPlatformShellPlan();
loadMeetingSeriesOptions();
refreshTextProviderStatus();
refreshLocalSttStatus("startup");
refreshNativeAudioReadiness("startup");
for (const choice of textProviderChoices) {
  choice.addEventListener("change", () => {
    setSelectedTextProvider(choice.value);
  });
}
captureSource?.addEventListener("change", () => {
  refreshNativeAudioReadiness("source_change");
});
sttProfileChoice?.addEventListener("change", () => {
  setSelectedLocalSttProfile(sttProfileChoice.value);
});
meetingSeriesChoice?.addEventListener("change", () => {
  selectedMeetingSeriesId = meetingSeriesChoice.value;
  applySelectedMeetingSeriesContext();
  renderHistorySaveControls();
});
refreshMeetingSeriesButton?.addEventListener("click", () => {
  loadMeetingSeriesOptions({ forceStatus: true });
});
downloadSttModelButton?.addEventListener("click", () => {
  downloadSelectedLocalSttModel();
});
openSttModelFolderButton?.addEventListener("click", async () => {
  if (!nativeInvoke) return;
  openSttModelFolderButton.disabled = true;
  try {
    await nativeInvoke("open_local_stt_model_folder");
    sttProfileDetail.textContent = "已開啟 Whisper 模型資料夾。";
    clearTimeout(sttFolderMessageTimer);
    sttFolderMessageTimer = setTimeout(() => {
      renderLocalSttStatus();
    }, 2500);
  } catch (error) {
    logAppError("stt.open_model_folder", error, {}, "warning");
    sttProfileDetail.textContent = `無法開啟模型資料夾：${formatError(error)}`;
  } finally {
    openSttModelFolderButton.disabled = false;
  }
});
startButton.addEventListener("click", async () => {
  if (!canStartWithAi()) {
    logAppError("meeting.start_blocked_without_ai", "Meeting start was requested before AI was enabled", { authenticated: Boolean(textProviderStatus?.authenticated) }, "warning");
    syncStartButtonAvailability();
    textProviderDetail.textContent = textProviderStatus?.authenticated
      ? "請先啟用 AI，Meeting Copilot 才能開始會議。"
      : `${providerStartBlockedCopy()}，Meeting Copilot 才能開始會議。`;
    return;
  }
  if (!canStartWithSelectedStt()) {
    const message = formatNativeAudioReadinessMessage(localSttStatus?.lastError ?? "localWhisperEngineMissing");
    logAppError("meeting.start_blocked_without_stt", message, { sttProfileId: selectedLocalSttProfileId }, "warning");
    syncStartButtonAvailability();
    providerState.textContent = `語音轉文字尚未就緒：${message}`;
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
  resetTranscriptState();
  suggestionHistory = [];
  latestDecisionState = undefined;
  resetAiActivityState();
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
  clearLiveTranscriptRevisionTimer();
  clearLiveAiExtractionTimer();
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
historySeriesTarget?.addEventListener("change", () => {
  syncHistorySeriesTitleInput();
});
saveMeetingHistoryButton?.addEventListener("click", () => {
  saveCurrentMeetingHistory();
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
  if (textProviderStatus?.connectorInstalled === false) {
    textProviderDetail.textContent = `請先安裝 ${selectedTextProviderLabel()}，再回來登入。`;
    return;
  }
  if (!nativeInvoke) {
    logAppError("ai.login_unavailable", "Text provider login requires the desktop app", {}, "warning");
    textProviderDetail.textContent = "登入需要使用桌面 app。";
    return;
  }
  loginTextProviderButton.disabled = true;
  try {
    await nativeInvoke("start_text_provider_login", { providerId: selectedTextProviderId });
    textProviderDetail.textContent = `已開啟 ${selectedTextProviderLabel()} 登入視窗；登入完成後會自動更新狀態。`;
    scheduleTextProviderRefresh();
  } catch (error) {
    logAppError("ai.login", error, { providerId: selectedTextProviderId }, "error");
    textProviderDetail.textContent = `無法開啟登入：${formatError(error)}`;
  } finally {
    loginTextProviderButton.disabled = false;
  }
});
openTextProviderInstallGuideButton?.addEventListener("click", async () => {
  await openSelectedTextProviderInstallGuide();
});
copyTextProviderInstallCommandButton?.addEventListener("click", async () => {
  await copySelectedTextProviderInstallCommand();
});
refreshTextProviderStatusButton?.addEventListener("click", () => {
  refreshTextProviderStatus({ forceRefresh: true });
});

async function loadMeetingSeriesOptions({ forceStatus = false } = {}) {
  if (!meetingSeriesChoice) return;
  if (!nativeInvoke) {
    meetingSeriesOptions = [];
    renderMeetingSeriesChoices();
    meetingSeriesDetail.textContent = "會議脈絡需要使用桌面 app；目前會以臨時會議開始。";
    return;
  }
  if (refreshMeetingSeriesButton) refreshMeetingSeriesButton.disabled = true;
  if (forceStatus && meetingSeriesDetail) meetingSeriesDetail.textContent = "正在讀取已保存的會議脈絡。";
  try {
    meetingSeriesOptions = await nativeInvoke("list_meeting_series_command");
  } catch (error) {
    logAppError("history.list_meeting_series", error, {}, "warning");
    meetingSeriesOptions = [];
    if (meetingSeriesDetail) meetingSeriesDetail.textContent = `無法讀取會議脈絡：${formatError(error)}`;
  }
  renderMeetingSeriesChoices();
  applySelectedMeetingSeriesContext();
  renderHistorySaveControls();
  if (refreshMeetingSeriesButton) refreshMeetingSeriesButton.disabled = false;
}

function renderMeetingSeriesChoices() {
  if (!meetingSeriesChoice) return;
  const previous = selectedMeetingSeriesId || meetingSeriesChoice.value;
  meetingSeriesChoice.innerHTML = [
    `<option value="">新增臨時會議</option>`,
    ...meetingSeriesOptions.map((series) => `<option value="${escapeHtml(series.id)}">${escapeHtml(series.title)}</option>`)
  ].join("");
  selectedMeetingSeriesId = meetingSeriesOptions.some((series) => series.id === previous) ? previous : "";
  meetingSeriesChoice.value = selectedMeetingSeriesId;
  if (refreshMeetingSeriesButton) refreshMeetingSeriesButton.disabled = false;
}

function selectedMeetingSeries() {
  return meetingSeriesOptions.find((series) => series.id === selectedMeetingSeriesId);
}

function applySelectedMeetingSeriesContext() {
  const series = selectedMeetingSeries();
  const context = series ? buildMeetingSeriesPrepContext(series) : "";
  setupController.setMeetingSeriesContext(context);
  if (!meetingSeriesDetail) return;
  if (!series) {
    meetingSeriesDetail.textContent = meetingSeriesOptions.length > 0
      ? "未選擇既有會議；本場會以新的臨時會議開始。"
      : "尚未有保存過的會議脈絡；會後保存後，下次會出現在這裡。";
    return;
  }
  const count = Number(series.historyCount ?? 0);
  meetingSeriesDetail.textContent = `已選「${series.title}」；會帶入上次摘要與未完成事項。已保存 ${count} 場。`;
}

function buildMeetingSeriesPrepContext(series) {
  const context = series.latestContext ?? {};
  const lines = [`既有會議脈絡：${cleanPromptItem(series.title)}`];
  const keyPoints = arrayOfStrings(context.keyPoints);
  const unresolved = arrayOfStrings(context.unresolved);
  const suggestedActions = arrayOfStrings(context.suggestedActions);
  const transcriptPreview = Array.isArray(context.transcriptPreview) ? context.transcriptPreview : [];
  if (keyPoints.length > 0) lines.push(`上次重點：${keyPoints.join("；")}`);
  if (unresolved.length > 0) lines.push(`上次待確認：${unresolved.join("；")}`);
  if (suggestedActions.length > 0) lines.push(`上次建議動作：${suggestedActions.join("；")}`);
  if (transcriptPreview.length > 0) {
    lines.push(`上次最後片段：${transcriptPreview.map((line) => `${cleanPromptItem(line.speaker ?? "未標記來源")}：${cleanPromptItem(line.text ?? "")}`).join(" / ")}`);
  }
  lines.push("請把以上視為舊脈絡，只用來提醒本場要確認，不要假設舊結論仍然成立。");
  return lines.join("\n");
}

function arrayOfStrings(value) {
  return Array.isArray(value) ? value.map(cleanPromptItem).filter(Boolean) : [];
}

function cleanPromptItem(value) {
  return String(value ?? "").replace(/[\s；;]+/gu, " ").trim();
}

async function openSelectedTextProviderInstallGuide() {
  const providerId = selectedTextProviderId;
  const url = textProviderStatus?.installUrl ?? providerConfig(providerId).installUrl;
  if (nativeInvoke) {
    try {
      await nativeInvoke("open_text_provider_install_guide", { providerId });
      textProviderDetail.textContent = `已開啟 ${selectedTextProviderLabel()} 官方安裝教學。`;
      return;
    } catch (error) {
      logAppError("ai.open_install_guide", error, { providerId, url }, "warning");
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

async function copySelectedTextProviderInstallCommand() {
  const providerId = selectedTextProviderId;
  const command = textProviderStatus?.installCommand ?? providerConfig(providerId).installCommand;
  try {
    await navigator.clipboard.writeText(command);
    textProviderDetail.textContent = `已複製安裝指令：${command}`;
  } catch (error) {
    logAppError("ai.copy_install_command", error, { providerId }, "warning");
    textProviderDetail.textContent = `請在 Terminal 執行：${command}`;
  }
}

enableOAuthProviderButton.addEventListener("click", () => {
  if (!textProviderStatus?.authenticated) {
    logAppError("ai.enable_without_auth", "AI enable was requested without authenticated subscription OAuth", { providerId: selectedTextProviderId }, "warning");
    textProviderDetail.textContent = `尚未登入 ${selectedTextProviderLabel()}，無法啟用 AI。`;
    return;
  }
  setAiEnabledPreference(true);
  renderTextProviderStatus();
  setupController.schedulePrepSummaryGeneration();
});
async function startNativeListening() {
  resetTranscriptState();
  suggestionHistory = [];
  latestDecisionState = undefined;
  aiSummaryOverride = undefined;
  resetAiActivityState();
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
    request: { source: requestedSource, sttProfileId: selectedLocalSttProfileId }
  });
  if (!health.ready) {
    health = await nativeInvoke("native_transcriber_health", {
      request: { source: requestedSource, sttProfileId: selectedLocalSttProfileId }
    });
  }
  if (!health.ready) {
    handleAudioPermissionProblem(health.lastError ?? "");
    throw new Error(`音訊尚未就緒：${health.lastError ?? "未知原因"}`);
  }
  health = await nativeInvoke("native_transcriber_health", {
    request: { source: requestedSource, sttProfileId: selectedLocalSttProfileId }
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
    request: { language: "zh-TW", source: requestedSource, sttProfileId: selectedLocalSttProfileId }
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
      clearPartialTranscriptCommitTimer();
      currentPartialTranscript = undefined;
      renderRuntimePayload(payload);
      scheduleLiveTranscriptRevision();
      scheduleLiveAiExtractionForEvent(payload.event);
    });
    await nativeListen("native_transcript_preview", (event) => {
      const payload = event.payload;
      if (!payload?.text) return;
      clearTranscriptStallMonitor();
      handleNativeTranscriptPreview(payload);
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

async function installLocalSttDownloadListener() {
  if (!nativeListen) return;
  try {
    await nativeListen("local_stt_model_download_progress", (event) => {
      const payload = event.payload;
      if (!payload || payload.profileId !== selectedLocalSttProfileId) return;
      localSttDownloadState = payload;
      renderLocalSttStatus();
    });
  } catch (error) {
    logAppError("stt.install_download_listener", error, {}, "warning");
  }
}

async function createLiveSession() {
  const request = {
    brief: createBriefFromSetupContext(),
    textProviderEnabled: canStartWithAi(),
    textProviderId: selectedTextProviderId
  };
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
    scheduleLiveTranscriptRevision();
    scheduleLiveAiExtractionForEvent(payload.event);
  } catch (error) {
    logAppError("transcript.ingest", error, { sessionId: activeSessionId }, "error");
    providerState.textContent = `逐字稿寫入失敗：${formatError(error)}`;
  }
}

async function promoteCurrentPartialTranscript(reason) {
  await promotePartialTranscript(currentPartialTranscript, reason, { clearCurrent: true });
}

async function promotePartialTranscript(partial, reason, { clearCurrent = false } = {}) {
  const text = partial?.text?.trim();
  if (!text || !activeSessionId) return;
  const duplicate = transcriptEvents.some((event) =>
    event.text.trim() === text
    && (event.source ?? "unknown") === (partial.source ?? "unknown")
  );
  if (clearCurrent && partial === currentPartialTranscript) {
    clearPartialTranscriptCommitTimer();
    currentPartialTranscript = undefined;
    partialTranscriptLastUpdatedAt = 0;
    partialTranscriptFirstSeenAt = 0;
  }
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
    scheduleLiveTranscriptRevision();
    scheduleLiveAiExtractionForEvent(payload.event);
  } catch (error) {
    markTranscriptPersistence(promotedEvent.id, "failed");
    logAppError("native.promote_partial_transcript_failed", error, { sessionId: activeSessionId, reason }, "error");
  }
}

function handleNativeTranscriptPreview(payload) {
  const now = performance.now();
  const previous = currentPartialTranscript;
  const previousAgeMs = partialTranscriptFirstSeenAt > 0 ? now - partialTranscriptFirstSeenAt : 0;
  const commitPrevious = previous?.text && shouldCommitReplacedPartial(previous, payload, previousAgeMs);
  if (commitPrevious) {
    promotePartialTranscript(previous, "partial_replaced");
  }
  if (!previous?.text || commitPrevious) {
    partialTranscriptFirstSeenAt = now;
  }
  currentPartialTranscript = payload;
  partialTranscriptLastUpdatedAt = now;
  schedulePartialTranscriptCommit();
  renderTranscriptDrawer();
}

function schedulePartialTranscriptCommit() {
  clearPartialTranscriptCommitTimer();
  partialTranscriptCommitTimer = setTimeout(() => {
    partialTranscriptCommitTimer = undefined;
    if (document.body.dataset.state !== "listening") return;
    const idleMs = performance.now() - partialTranscriptLastUpdatedAt;
    if (shouldCommitIdlePartial(currentPartialTranscript, idleMs)) {
      promotePartialTranscript(currentPartialTranscript, "partial_idle", { clearCurrent: true });
      return;
    }
    if (currentPartialTranscript?.text) schedulePartialTranscriptCommit();
  }, PARTIAL_TRANSCRIPT_COMMIT_IDLE_MS);
}

function clearPartialTranscriptCommitTimer() {
  if (!partialTranscriptCommitTimer) return;
  clearTimeout(partialTranscriptCommitTimer);
  partialTranscriptCommitTimer = undefined;
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
  } else if (payload.coachingError) {
    suggestion.className = "suggestion-empty";
    suggestion.innerHTML = `<strong>AI 暫時無法產生提醒</strong><small>${escapeHtml(payload.coachingError)}</small>`;
    feedbackRow.hidden = true;
  } else if (payload.decisionState?.readiness) {
    suggestion.className = "suggestion-empty";
    suggestion.innerHTML = renderDecisionOverview(payload.decisionState);
    feedbackRow.hidden = true;
  }
}

function renderSuggestionCard(item) {
  const evidence = renderEvidenceLines(item.evidenceTranscriptIds ?? []);
  const title = item.title || labelMove(item.kind);
  const suggestedMove = item.suggestedMove || item.text;
  return [
    `<strong>${escapeHtml(labelMove(item.kind))}</strong>`,
    `<div class="suggestion-title">${escapeHtml(title)}</div>`,
    `<p class="suggestion-move">${escapeHtml(suggestedMove)}</p>`,
    item.watchOut ? `<p class="suggestion-watch">注意：${escapeHtml(item.watchOut)}</p>` : "",
    `<small>${escapeHtml(item.reason)}</small>`,
    evidence
  ].filter(Boolean).join("");
}

function renderEvidenceLines(ids) {
  const lines = ids
    .map((id) => displayedTranscriptEvents().find((event) => event.id === id))
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
  if (document.body.dataset.state !== "listening") return;
  const decision = nextLiveAiDecision({
    transcriptEvents,
    state: liveAiPipelineState,
    nowMs: Date.now()
  });
  if (decision.action === "delay") {
    scheduleLiveAiExtractionAfter(decision.delayMs);
    return;
  }
  if (decision.action !== "run") return;
  liveAiPipelineState = { ...liveAiPipelineState, running: true };
  const watchedEventCountAtStart = decision.watchedEventCount;
  setAiActivity("live_state", "AI 正在盯對方發言，判斷是否需要提醒");
  try {
    const payload = await nativeInvoke("extract_live_state_patch_oauth", { sessionId: activeSessionId });
    liveAiPipelineState = markLiveAiSuccess(liveAiPipelineState, {
      watchedEventCount: watchedEventCountAtStart,
      nowMs: Date.now()
    });
    renderRuntimePayload(payload);
    clearAiActivity("live_state", payload.coachingError ? `AI 已更新會議判斷；提醒暫時失敗：${payload.coachingError}` : "AI 已更新會議判斷。");
  } catch (error) {
    liveAiPipelineState = markLiveAiFailure(liveAiPipelineState, { nowMs: Date.now() });
    logAppError("ai.live_state_patch", error, {
      sessionId: activeSessionId,
      watchedSources: LIVE_AI_POLICY.watchedSources,
      watchedEventCount: watchedEventCountAtStart,
      consecutiveFailures: liveAiPipelineState.consecutiveFailures,
      suspendedUntilMs: liveAiPipelineState.suspendedUntilMs
    }, "warning");
    const message = liveAiPipelineState.suspendedUntilMs > Date.now()
      ? `AI 即時提醒正在退避重試，逐字稿仍會繼續記錄；約 ${formatRetryDelay(liveAiPipelineState.suspendedUntilMs - Date.now())} 後自動再試。`
      : `AI 即時提醒暫時降速，逐字稿仍會繼續記錄：${formatError(error)}`;
    clearAiActivity("live_state", message);
  } finally {
    liveAiPipelineState = { ...liveAiPipelineState, running: false };
    if (hasPendingLiveAiWatchedEvents(transcriptEvents, liveAiPipelineState)) {
      scheduleLiveAiExtraction();
    }
  }
}

function scheduleLiveAiExtractionForEvent(event) {
  if (!shouldTriggerLiveAiForEvent(event)) return;
  scheduleLiveAiExtraction();
}

function scheduleLiveAiExtraction() {
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated || !activeSessionId) return;
  if (document.body.dataset.state !== "listening") return;
  const decision = nextLiveAiDecision({
    transcriptEvents,
    state: liveAiPipelineState,
    nowMs: Date.now()
  });
  if (decision.action === "run") {
    scheduleLiveAiExtractionAfter(LIVE_AI_POLICY.debounceMs);
  } else if (decision.action === "delay") {
    scheduleLiveAiExtractionAfter(decision.delayMs);
  }
}

function scheduleLiveAiExtractionAfter(delayMs) {
  if (liveAiExtractionTimer) clearTimeout(liveAiExtractionTimer);
  liveAiExtractionTimer = setTimeout(() => {
    liveAiExtractionTimer = undefined;
    maybeRunLiveAiExtraction();
  }, Math.max(0, delayMs));
}

function scheduleLiveTranscriptRevision() {
  if (!LIVE_TRANSCRIPT_REVISION_DURING_CAPTURE_ENABLED) return;
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated || !activeSessionId) return;
  if (document.body.dataset.state !== "listening") return;
  if (transcriptEvents.length < 2) return;
  scheduleLiveTranscriptRevisionAfter(TRANSCRIPT_REVISION_DEBOUNCE_MS);
}

function scheduleLiveTranscriptRevisionAfter(delayMs) {
  if (transcriptRevisionTimer) clearTimeout(transcriptRevisionTimer);
  transcriptRevisionTimer = setTimeout(() => {
    transcriptRevisionTimer = undefined;
    maybeRunLiveTranscriptRevision();
  }, Math.max(0, delayMs));
}

async function maybeRunLiveTranscriptRevision({ allowReview = false, force = false } = {}) {
  if (!nativeInvoke || !oauthAiEnabled || !textProviderStatus?.authenticated || !activeSessionId) return;
  const appState = document.body.dataset.state;
  if (appState !== "listening" && !(allowReview && appState === "review")) return;
  const eventCount = transcriptEvents.length;
  if (eventCount < 2 || transcriptRevisionRunning) return;
  if (!force && eventCount === lastTranscriptRevisionEventCount) return;
  if (!force && !shouldRunLiveTranscriptRevision(eventCount)) {
    scheduleLiveTranscriptRevisionAfter(liveTranscriptRevisionRetryDelay());
    return;
  }
  const revisionSnapshot = buildTranscriptRevisionSnapshot();
  if (revisionSnapshot.transcript.length === 0) return;
  transcriptRevisionRunning = true;
  setAiActivity("transcript_revision", "AI 正在修正逐字稿與判斷說話者");
  try {
    const response = await nativeInvoke("revise_transcript_oauth", {
      request: {
        sessionId: revisionSnapshot.sessionId,
        textProviderId: selectedTextProviderId,
        transcript: revisionSnapshot.transcript
      }
    });
    if (activeSessionId !== revisionSnapshot.sessionId) {
      clearAiActivity("transcript_revision");
      return;
    }
    if (!Array.isArray(response?.transcript)) throw new Error("transcript revision response missing transcript");
    applyRevisedTranscript(response.transcript, revisionSnapshot);
    lastTranscriptRevisionEventCount = revisionSnapshot.endCount;
    lastTranscriptRevisionAt = Date.now();
    renderTranscriptDrawer();
    if (document.body.dataset.state === "review") renderPostMeetingReview();
    clearAiActivity("transcript_revision", "AI 已更新逐字稿說話者。");
  } catch (error) {
    logAppError("ai.live_transcript_revision", error, { sessionId: activeSessionId, eventCount }, "warning");
    clearAiActivity("transcript_revision", `AI 暫時無法修正逐字稿，已保留目前版本：${formatError(error)}`);
  } finally {
    transcriptRevisionRunning = false;
    if (transcriptEvents.length > eventCount) scheduleLiveTranscriptRevision();
  }
}

function shouldRunLiveTranscriptRevision(eventCount) {
  if (lastTranscriptRevisionEventCount === 0) return true;
  const newEvents = eventCount - lastTranscriptRevisionEventCount;
  if (newEvents >= TRANSCRIPT_REVISION_MIN_NEW_EVENTS) return true;
  return Date.now() - lastTranscriptRevisionAt >= TRANSCRIPT_REVISION_MAX_WAIT_MS;
}

function liveTranscriptRevisionRetryDelay() {
  if (lastTranscriptRevisionAt === 0) return TRANSCRIPT_REVISION_DEBOUNCE_MS;
  const remainingMs = TRANSCRIPT_REVISION_MAX_WAIT_MS - (Date.now() - lastTranscriptRevisionAt);
  return Math.max(500, remainingMs);
}

function buildTranscriptRevisionSnapshot() {
  refreshTranscriptRevisionStability();
  const endCount = transcriptEvents.length;
  const startIndex = Math.max(0, endCount - TRANSCRIPT_REVISION_CONTEXT_WINDOW_SIZE);
  const editableStartIndex = Math.max(startIndex, endCount - TRANSCRIPT_REVISION_EDITABLE_WINDOW_SIZE);
  const revisedById = new Map(revisedTranscriptEvents.map((event) => [event.id, event]));
  return {
    sessionId: activeSessionId,
    startIndex,
    editableStartIndex,
    endCount,
    transcript: transcriptEvents.slice(startIndex, endCount).map((event, offset) => {
      const index = startIndex + offset;
      const meta = transcriptRevisionMetadata(event.id);
      const editable = index >= editableStartIndex && !meta.stable;
      return transcriptRevisionLine(event, revisedById, meta, editable);
    })
  };
}

function transcriptRevisionLine(event, revisedById, meta, editable) {
  const revised = revisedById.get(event.id);
  return {
    id: event.id,
    text: editable ? event.text : revised?.text ?? event.text,
    speaker: transcriptRevisionSpeaker(event, revised),
    source: event.source ?? "unknown",
    language: event.language ?? detectUiLanguage(event.text),
    editable,
    stability: meta.stable ? "stable" : editable ? "tentative" : "context",
    revisionCount: meta.revisionCount
  };
}

function transcriptRevisionSpeaker(event, revised) {
  if (revised?.speaker) return revised.speaker;
  if (event.speaker) return event.speaker;
  if (event.source === "mic") return "我";
  if (event.source === "system") return "未標記來源";
  return "未標記來源";
}

function applyRevisedTranscript(lines, revisionSnapshot) {
  const mergedRevisions = new Map(revisedTranscriptEvents.map((event) => [event.id, event]));
  const rawById = new Map(transcriptEvents.map((event) => [event.id, event]));
  const editableIds = new Set(revisionSnapshot.transcript.filter((line) => line.editable).map((line) => line.id));
  const appliedIds = new Set();
  for (const revised of lines) {
    if (!revised?.id) continue;
    const event = rawById.get(revised.id);
    if (!event) continue;
    if (!editableIds.has(revised.id)) continue;
    mergedRevisions.set(revised.id, {
      ...event,
      text: revised.text,
      speaker: revised.speaker,
      source: revised.source ?? event.source,
      language: revised.language ?? event.language,
      revisedByAi: true
    });
    appliedIds.add(revised.id);
  }
  updateTranscriptRevisionMetadata(appliedIds);
  revisedTranscriptEvents = transcriptEvents.map((event) => mergedRevisions.get(event.id) ?? event);
}

function transcriptRevisionMetadata(id) {
  const existing = transcriptRevisionMeta.get(id);
  if (existing) return existing;
  const meta = { revisionCount: 0, stable: false, lastRevisedAt: 0 };
  transcriptRevisionMeta.set(id, meta);
  return meta;
}

function updateTranscriptRevisionMetadata(appliedIds) {
  const now = Date.now();
  for (const id of appliedIds) {
    const meta = transcriptRevisionMetadata(id);
    meta.revisionCount += 1;
    meta.lastRevisedAt = now;
  }
  refreshTranscriptRevisionStability();
}

function refreshTranscriptRevisionStability() {
  transcriptEvents.forEach((event, index) => {
    const meta = transcriptRevisionMetadata(event.id);
    if (meta.stable) return;
    const trailingEvents = transcriptEvents.length - 1 - index;
    const confirmedStable =
      meta.revisionCount >= TRANSCRIPT_REVISION_STABLE_AFTER_REVISIONS
      && trailingEvents >= TRANSCRIPT_REVISION_STABLE_AFTER_TRAILING_EVENTS;
    const outsideEditableWindow =
      meta.revisionCount >= 1
      && trailingEvents >= TRANSCRIPT_REVISION_EDITABLE_WINDOW_SIZE;
    if (confirmedStable || outsideEditableWindow) {
      meta.stable = true;
    }
  });
}

function displayedTranscriptEvents() {
  if (revisedTranscriptEvents.length === 0) return transcriptEvents;
  const revisedById = new Map(revisedTranscriptEvents.map((event) => [event.id, event]));
  return transcriptEvents.map((event) => revisedById.get(event.id) ?? event);
}

function resetTranscriptState() {
  transcriptLines = [];
  transcriptEvents = [];
  revisedTranscriptEvents = [];
  transcriptRevisionMeta = new Map();
  currentPartialTranscript = undefined;
  partialTranscriptLastUpdatedAt = 0;
  partialTranscriptFirstSeenAt = 0;
  transcriptIndex = 0;
  liveAiPipelineState = createLiveAiPipelineState();
  lastTranscriptRevisionEventCount = 0;
  lastTranscriptRevisionAt = 0;
  transcriptRevisionRunning = false;
  clearPartialTranscriptCommitTimer();
  clearLiveAiExtractionTimer();
  clearLiveTranscriptRevisionTimer();
}

function clearLiveAiExtractionTimer() {
  if (!liveAiExtractionTimer) return;
  clearTimeout(liveAiExtractionTimer);
  liveAiExtractionTimer = undefined;
}

function clearLiveTranscriptRevisionTimer() {
  if (!transcriptRevisionTimer) return;
  clearTimeout(transcriptRevisionTimer);
  transcriptRevisionTimer = undefined;
}

function setAiActivity(key, message) {
  aiActivityMessages.set(key, message);
  renderAiActivityState();
}

function clearAiActivity(key, fallbackMessage) {
  aiActivityMessages.delete(key);
  if (aiActivityMessages.size > 0) {
    renderAiActivityState();
  } else if (fallbackMessage) {
    providerState.textContent = fallbackMessage;
  }
}

function resetAiActivityState() {
  aiActivityMessages.clear();
}

function renderAiActivityState() {
  if (aiActivityMessages.size === 0) return;
  providerState.textContent = Array.from(aiActivityMessages.values()).join("；");
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
  clearLegacyAiEnabledPreference();
  updateAppState("setup");
  setAiEnabledPreference(false);
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
  resetTranscriptState();
  suggestionHistory = [];
  latestDecisionState = undefined;
  aiSummaryOverride = undefined;
  resetAiActivityState();
  activeCaptureSource = undefined;
  reviewFinalized = false;
  postMeetingAiSummaryStarted = false;
  setupController.resetPrepSummaryQueue();
  sessionState.textContent = "待機中";
  providerState.textContent = "尚未開始會議。";
  resetListeningSurface();
  renderTextProviderStatus();
  setupController.renderPrepSummary();
}

function resetListeningSurface() {
  clearLiveTranscriptRevisionTimer();
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
      request: { source, sttProfileId: selectedLocalSttProfileId }
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
    if (/localWhisper/i.test(message)) {
      hideAudioPermissionAction();
      providerState.textContent = `語音轉文字需處理：${readinessMessage}`;
      return;
    }
    if (setupAudioReadinessText) setupAudioReadinessText.textContent = readinessMessage;
    providerState.textContent = `音訊權限需處理：${readinessMessage}`;
  } catch (error) {
    logAppError("permissions.native_audio_health", error, { source, reason }, "warning");
  }
}

function formatNativeAudioReadinessMessage(message) {
  if (/localWhisperEngineMissing/i.test(message)) {
    return "Whisper 引擎尚未打包或設定，無法開始會議。";
  }
  if (/localWhisperModelMissing/i.test(message)) {
    return "Whisper 模型尚未放到本機模型資料夾，無法開始會議。";
  }
  if (/localWhisperHealthFailed/i.test(message)) {
    return "Whisper 模型或引擎健康檢查失敗，請重新下載模型或改選其他模型。";
  }
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

async function refreshLocalSttStatus(reason) {
  syncLocalSttChoiceControl();
  if (!nativeInvoke) {
    localSttStatus = {
      selectedProfileId: selectedLocalSttProfileId,
      ready: false,
      providerId: "browser-speech-recognition",
      profiles: fallbackLocalSttProfiles(),
      lastError: "localWhisperEngineMissing: 桌面 app 才能使用本機 Whisper。"
    };
    renderLocalSttStatus();
    return;
  }
  try {
    localSttStatus = await nativeInvoke("local_stt_status_command", { profileId: selectedLocalSttProfileId });
    selectedLocalSttProfileId = normalizeLocalSttProfileId(localSttStatus.selectedProfileId);
    writeLocalSttProfilePreference(selectedLocalSttProfileId);
    syncLocalSttChoiceControl();
  } catch (error) {
    logAppError("stt.local_status", error, { profileId: selectedLocalSttProfileId, reason }, "warning");
    localSttStatus = {
      selectedProfileId: selectedLocalSttProfileId,
      ready: false,
      providerId: "unknown",
      profiles: fallbackLocalSttProfiles(),
      lastError: formatError(error)
    };
  } finally {
    renderLocalSttStatus();
  }
}

async function setSelectedLocalSttProfile(profileId) {
  const nextProfileId = normalizeLocalSttProfileId(profileId);
  if (isLocalSttDownloadActive()) {
    sttProfileDetail.textContent = "模型下載中，完成後再切換品質。";
    syncLocalSttChoiceControl();
    return;
  }
  selectedLocalSttProfileId = nextProfileId;
  localSttDownloadState = undefined;
  writeLocalSttProfilePreference(nextProfileId);
  syncLocalSttChoiceControl();
  if (!nativeInvoke) {
    await refreshLocalSttStatus("profile_change");
    return;
  }
  try {
    localSttStatus = await nativeInvoke("set_local_stt_profile_command", { profileId: nextProfileId });
  } catch (error) {
    logAppError("stt.set_profile", error, { profileId: nextProfileId }, "warning");
  }
  await refreshLocalSttStatus("profile_change");
  refreshNativeAudioReadiness("stt_profile_change");
}

async function downloadSelectedLocalSttModel() {
  if (!nativeInvoke || isLocalSttDownloadActive()) return;
  const profileId = selectedLocalSttProfileId;
  localSttDownloadState = {
    profileId,
    state: "starting",
    downloadedBytes: 0,
    totalBytes: undefined,
    percent: 0,
    message: "準備下載 Whisper 模型。"
  };
  renderLocalSttStatus();
  try {
    localSttStatus = await nativeInvoke("download_local_stt_model_command", { profileId });
    selectedLocalSttProfileId = normalizeLocalSttProfileId(localSttStatus.selectedProfileId);
    writeLocalSttProfilePreference(selectedLocalSttProfileId);
    localSttDownloadState = {
      profileId: selectedLocalSttProfileId,
      state: "completed",
      downloadedBytes: 0,
      totalBytes: undefined,
      percent: 100,
      message: "模型下載完成。"
    };
  } catch (error) {
    logAppError("stt.download_model", error, { profileId }, "error");
    localSttDownloadState = {
      profileId,
      state: "failed",
      downloadedBytes: 0,
      totalBytes: undefined,
      percent: undefined,
      message: formatError(error)
    };
  }
  await refreshLocalSttStatus("model_download");
  refreshNativeAudioReadiness("model_download");
}

function renderLocalSttStatus() {
  if (!sttSettings || !sttProfileName || !sttProfileDetail) return;
  const profile = currentLocalSttProfile();
  const ready = Boolean(localSttStatus?.ready);
  const modelMissing = localSttStatus?.modelReady === false;
  const engineReady = localSttStatus?.engineReady !== false;
  const healthFailed = /localWhisperHealthFailed/i.test(localSttStatus?.lastError ?? "");
  const downloadActive = isLocalSttDownloadActive();
  const downloadAvailable = modelMissing || healthFailed;
  sttSettings.classList.toggle("ready", ready);
  sttSettings.classList.toggle("warning", !ready);
  sttProfileName.textContent = profile?.recommended ? `${profile.label}（建議）` : profile?.label ?? "標準";
  if (downloadActive || localSttDownloadState?.state === "failed") {
    sttProfileDetail.textContent = formatLocalSttDownloadMessage(localSttDownloadState, profile);
  } else if (!ready) {
    sttProfileDetail.textContent = formatNativeAudioReadinessMessage(localSttStatus?.lastError ?? "localWhisperEngineMissing");
  } else {
    const modelText = profile?.modelSizeMb ? `模型約 ${profile.modelSizeMb} MB。` : "";
    sttProfileDetail.textContent = `${profile?.detail ?? "使用本機 Whisper。"}${modelText ? ` ${modelText}` : ""}`;
  }
  renderLocalSttDownloadProgress(localSttDownloadState, downloadActive);
  if (downloadSttModelButton) {
    downloadSttModelButton.hidden = !downloadAvailable && !downloadActive && localSttDownloadState?.state !== "failed";
    downloadSttModelButton.disabled = !nativeInvoke || !engineReady || downloadActive;
    downloadSttModelButton.textContent = downloadActive
      ? "下載中"
      : localSttDownloadState?.state === "failed"
        ? "重新下載"
        : "下載模型";
  }
  if (openSttModelFolderButton) {
    openSttModelFolderButton.disabled = !nativeInvoke || downloadActive;
    openSttModelFolderButton.hidden = false;
  }
  if (sttProfileChoice) sttProfileChoice.disabled = downloadActive;
  syncStartButtonAvailability();
}

function isLocalSttDownloadActive() {
  return ["starting", "checking", "replacing", "downloading", "verifying", "installing"].includes(localSttDownloadState?.state);
}

function formatLocalSttDownloadMessage(progress, profile) {
  if (!progress) return "準備下載 Whisper 模型。";
  const size = progress.totalBytes
    ? `${formatBytes(progress.downloadedBytes)} / ${formatBytes(progress.totalBytes)}`
    : formatBytes(progress.downloadedBytes ?? 0);
  if (progress.state === "failed") {
    return `模型下載失敗：${progress.message ?? "請稍後重試。"}`;
  }
  if (progress.state === "checking") return "正在檢查本機模型。";
  if (progress.state === "replacing") return "既有模型驗證失敗，正在重新下載。";
  if (progress.state === "verifying") return "模型下載完成，正在驗證。";
  if (progress.state === "installing") return "模型驗證通過，正在安裝。";
  if (progress.state === "completed") return "模型下載完成。";
  const percent = typeof progress.percent === "number" ? `${Math.round(progress.percent)}%` : "下載中";
  const modelName = profile?.modelFile ?? progress.modelFile ?? "Whisper 模型";
  return `正在下載 ${modelName}：${percent}（${size}）。`;
}

function renderLocalSttDownloadProgress(progress, active) {
  if (!sttDownloadProgress || !sttDownloadProgressFill) return;
  const visible = active || progress?.state === "failed";
  sttDownloadProgress.hidden = !visible;
  const percent = typeof progress?.percent === "number" ? clamp(progress.percent, 0, 100) : 0;
  sttDownloadProgress.style.setProperty("--progress", `${percent}%`);
  sttDownloadProgressFill.textContent = progress?.state === "failed" ? "下載失敗" : `${Math.round(percent)}%`;
}

function formatBytes(bytes) {
  const value = Number(bytes ?? 0);
  if (value >= 1024 * 1024 * 1024) return `${(value / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (value >= 1024 * 1024) return `${Math.round(value / 1024 / 1024)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${value} B`;
}

function currentLocalSttProfile() {
  const profiles = localSttStatus?.profiles?.length ? localSttStatus.profiles : fallbackLocalSttProfiles();
  return profiles.find((profile) => profile.id === selectedLocalSttProfileId) ?? profiles[0];
}

function fallbackLocalSttProfiles() {
  return [
    { id: "whisper-standard", label: "標準", detail: "本機 Whisper small。", engine: "whisper", modelFile: "ggml-small.bin", modelSizeMb: 500, recommended: true },
    { id: "whisper-fast", label: "快速", detail: "本機 Whisper base。", engine: "whisper", modelFile: "ggml-base.bin", modelSizeMb: 150 },
    { id: "whisper-accurate", label: "高準確", detail: "本機 Whisper medium。", engine: "whisper", modelFile: "ggml-medium.bin", modelSizeMb: 1500 }
  ];
}

function normalizeLocalSttProfileId(profileId) {
  return ["whisper-fast", "whisper-standard", "whisper-accurate"].includes(profileId)
    ? profileId
    : "whisper-standard";
}

function readLocalSttProfilePreference() {
  try {
    return normalizeLocalSttProfileId(window.localStorage?.getItem(LOCAL_STT_PROFILE_STORAGE_KEY));
  } catch {
    return "whisper-standard";
  }
}

function writeLocalSttProfilePreference(profileId) {
  try {
    window.localStorage?.setItem(LOCAL_STT_PROFILE_STORAGE_KEY, normalizeLocalSttProfileId(profileId));
  } catch {
    // Local storage only restores the selected transcription quality.
  }
}

function syncLocalSttChoiceControl() {
  if (!sttProfileChoice) return;
  sttProfileChoice.value = selectedLocalSttProfileId;
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
      ? `本機文件已整理完成，正在用 ${selectedTextProviderLabel()} 更新 AI 整理。`
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
  renderHistorySaveControls(artifact);
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
    ? artifact.transcript.map((line, index) => `<p><strong>${index + 1}. ${escapeHtml(line.speaker ?? "未標記來源")}</strong> ${escapeHtml(line.text)}${line.persistenceStatus === "failed" ? " <em>未儲存</em>" : ""}</p>`).join("")
    : `<p class="empty-line">本場沒有收到逐字稿。</p>`;
  downloadState.textContent = artifact.transcript.length > 0
    ? `已整理 ${artifact.transcript.length} 段逐字稿。建議先下載 AI 整理 Markdown 與逐字稿 TXT。`
    : "沒有逐字稿內容；仍可下載 AI 整理。";
  syncReviewDownloadButtons(artifact);
  return artifact;
}

function renderHistorySaveControls(artifact = buildMeetingArtifact()) {
  if (!historySeriesTarget || !historySeriesTitle || !historySaveState || !saveMeetingHistoryButton) return;
  historySeriesTarget.innerHTML = [
    `<option value="__new__">建立新會議脈絡</option>`,
    ...meetingSeriesOptions.map((series) => `<option value="${escapeHtml(series.id)}">${escapeHtml(series.title)}</option>`)
  ].join("");
  const currentSeries = selectedMeetingSeries();
  historySeriesTarget.value = currentSeries ? currentSeries.id : "__new__";
  historySeriesTitle.value = currentSeries?.title ?? inferredHistoryTitle(artifact);
  syncHistorySeriesTitleInput();
  const hasContent = hasReviewContent(artifact);
  saveMeetingHistoryButton.disabled = !hasContent;
  if (!hasContent) {
    historySaveState.textContent = "本場沒有可保存內容。";
  } else if (currentSeries) {
    historySaveState.textContent = `本場會保存到「${currentSeries.title}」，下次會前可直接選。`;
  } else {
    historySaveState.textContent = "保存後，這個會議會出現在下次會前準備的選項。";
  }
}

function syncHistorySeriesTitleInput() {
  if (!historySeriesTarget || !historySeriesTitle) return;
  const selected = meetingSeriesOptions.find((series) => series.id === historySeriesTarget.value);
  if (selected) {
    historySeriesTitle.value = selected.title;
    historySeriesTitle.disabled = true;
  } else {
    historySeriesTitle.disabled = false;
  }
}

function inferredHistoryTitle(artifact = buildMeetingArtifact()) {
  const selected = selectedMeetingSeries();
  if (selected?.title) return selected.title;
  const title = artifact.title && artifact.title !== DEFAULT_MEETING_TITLE ? artifact.title : "";
  return title || "未命名會議";
}

async function saveCurrentMeetingHistory() {
  if (!nativeInvoke) {
    historySaveState.textContent = "保存會議脈絡需要使用桌面 app。";
    return;
  }
  const artifact = buildMeetingArtifact();
  if (!hasReviewContent(artifact)) {
    historySaveState.textContent = "本場沒有可保存內容。";
    return;
  }
  saveMeetingHistoryButton.disabled = true;
  historySaveState.textContent = "正在保存本場會議。";
  try {
    const selectedTarget = meetingSeriesOptions.find((series) => series.id === historySeriesTarget?.value);
    const response = await nativeInvoke("save_meeting_history_command", {
      request: {
        sessionId: activeSessionId ?? null,
        seriesId: selectedTarget?.id ?? null,
        seriesTitle: selectedTarget?.title ?? historySeriesTitle?.value ?? artifact.title,
        allowAiContext: Boolean(historyAllowAiContext?.checked),
        artifact
      }
    });
    selectedMeetingSeriesId = response.series?.id ?? selectedMeetingSeriesId;
    await loadMeetingSeriesOptions();
    historySaveState.textContent = `已保存到「${response.series?.title ?? historySeriesTitle?.value ?? "會議脈絡"}」。`;
  } catch (error) {
    logAppError("history.save_meeting", error, { sessionId: activeSessionId }, "error");
    historySaveState.textContent = `保存失敗：${formatError(error)}`;
  } finally {
    saveMeetingHistoryButton.disabled = false;
  }
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
  if (/Recognition request (was )?cancell?ed/i.test(message)) return "recognition_request_canceled";
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
  const visibleTranscriptEvents = displayedTranscriptEvents();
  const transcript = visibleTranscriptEvents.length > 0
    ? visibleTranscriptEvents.map((event) => ({
        id: event.id,
        text: event.text,
        speaker: transcriptSpeakerLabel(event),
        source: event.source ?? "unknown",
        language: event.language,
        revisedByAi: Boolean(event.revisedByAi),
        persistenceStatus: event.persistenceStatus ?? "saved"
      }))
    : transcriptLines.map((text, index) => ({
        id: `line_${index + 1}`,
        text,
        speaker: "未標記來源",
        source: "unknown",
        language: detectUiLanguage(text)
      }));
  const transcriptText = transcript.map((line) => line.text).join("\n");
  const rawTranscript = transcriptEvents.map((event) => ({
    id: event.id,
    text: event.text,
    speaker: transcriptSpeakerLabel(event),
    source: event.source ?? "unknown",
    language: event.language,
    persistenceStatus: event.persistenceStatus ?? "saved"
  }));
  const prepContext = setupController.combinedPrepContext();
  const contextDiagnostics = setupController.contextDiagnostics();
  const localSummary = {
    keyPoints: summarizeTranscript(transcriptText),
    decisionsAndOpenQuestions: summarizeDecisionState(latestDecisionState, transcriptText),
    suggestedActions: summarizeSuggestions(suggestionHistory, transcriptText)
  };
  const summary = aiSummaryOverride ?? localSummary;
  return {
    title: DEFAULT_MEETING_TITLE,
    sessionId: activeSessionId ?? "local_review",
    generatedAt: new Date().toISOString(),
    prepContext,
    summary,
    localSummary,
    summaryProvider: aiSummaryOverride ? selectedTextProviderId : "local-rule",
    transcript,
    rawTranscript,
    contextDiagnostics,
    suggestions: suggestionHistory,
    decisionState: latestDecisionState ?? null
  };
}

async function refreshTextProviderStatus({ forceRefresh = false } = {}) {
  const requestedProviderId = selectedTextProviderId;
  if (!nativeInvoke) {
    textProviderStatus = {
      providerId: "browser-local-rule",
      kind: "local",
      connectorInstalled: false,
      connectorLabel: selectedTextProviderLabel(),
      authenticated: false,
      active: false,
      statusLabel: providerConfig().unavailableStatus,
      installCommand: providerConfig().installCommand,
      installUrl: providerConfig().installUrl
    };
    renderTextProviderStatus();
    return;
  }
  textProviderDetail.textContent = `正在檢查 ${selectedTextProviderLabel()} 登入狀態。`;
  if (refreshTextProviderStatusButton) refreshTextProviderStatusButton.disabled = true;
  try {
    const status = await nativeInvoke("text_provider_status", { providerId: requestedProviderId, forceRefresh });
    if (requestedProviderId !== selectedTextProviderId) return;
    textProviderStatus = status;
  } catch (error) {
    if (requestedProviderId !== selectedTextProviderId) return;
    logAppError("ai.text_provider_status", error, { providerId: selectedTextProviderId }, "error");
    textProviderStatus = {
      providerId: selectedTextProviderId,
      kind: "subscription_oauth",
      connectorInstalled: false,
      connectorLabel: selectedTextProviderLabel(),
      authenticated: false,
      active: false,
      statusLabel: `無法檢查 ${selectedTextProviderLabel()} 登入`,
      installCommand: providerConfig().installCommand,
      installUrl: providerConfig().installUrl,
      lastError: formatError(error)
    };
  } finally {
    if (refreshTextProviderStatusButton) refreshTextProviderStatusButton.disabled = false;
    if (requestedProviderId === selectedTextProviderId) renderTextProviderStatus();
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
  const connectorInstalled = textProviderStatus?.connectorInstalled !== false;
  providerSettings.classList.toggle("enabled", authenticated && oauthAiEnabled);
  providerSettings.classList.toggle("warning", !connectorInstalled);
  textProviderName.textContent = oauthAiEnabled && authenticated
    ? `${selectedTextProviderLabel()} 已啟用`
    : connectorInstalled && authenticated
      ? `${selectedTextProviderLabel()} 已登入`
      : "AI 尚未啟用";
  textProviderDetail.textContent = textProviderStatusMessage({ authenticated, connectorInstalled });
  enableOAuthProviderButton.disabled = !authenticated || oauthAiEnabled;
  enableOAuthProviderButton.textContent = oauthAiEnabled && authenticated ? "本場已啟用" : "啟用本場";
  loginTextProviderButton.textContent = providerConfig().loginLabel;
  loginTextProviderButton.hidden = !connectorInstalled || authenticated;
  if (openTextProviderInstallGuideButton) openTextProviderInstallGuideButton.hidden = connectorInstalled;
  if (copyTextProviderInstallCommandButton) copyTextProviderInstallCommandButton.hidden = connectorInstalled;
  if (refreshTextProviderStatusButton) refreshTextProviderStatusButton.hidden = connectorInstalled && authenticated;
  syncStartButtonAvailability();
  setupController.renderPrepSummary();
}

function textProviderStatusMessage({ authenticated, connectorInstalled }) {
  const providerLabel = selectedTextProviderLabel();
  const serviceLabel = providerConfig().serviceLabel;
  if (!connectorInstalled) {
    const command = textProviderStatus?.installCommand ?? providerConfig().installCommand;
    return `尚未安裝 ${providerLabel}。請依官方教學安裝，或複製指令到 Terminal 執行：${command}。安裝後按重新檢查。`;
  }
  if (oauthAiEnabled && authenticated) {
    return `本場會議已啟用；會議背景、即時提醒與會後整理會透過本機 ${providerLabel} 送到 ${serviceLabel}。Meeting Copilot 不會保存 CLI token。`;
  }
  if (authenticated) {
    return `已偵測到 ${providerLabel} 登入；按「啟用本場」後才會把本場會議內容送去 ${serviceLabel}。`;
  }
  return `${textProviderStatus?.statusLabel ?? `尚未登入 ${providerLabel}`}；登入由官方 CLI 處理，Meeting Copilot 不會保存 CLI token。`;
}

function canStartWithAi() {
  return Boolean(textProviderStatus?.authenticated && oauthAiEnabled);
}

function syncStartButtonAvailability() {
  if (document.body.dataset.state !== "setup") return;
  const ready = canStartWithAi() && canStartWithSelectedStt();
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
      : !canStartWithSelectedStt()
        ? "Whisper 尚未就緒"
      : textProviderStatus?.authenticated
        ? "請先按左側啟用 AI"
        : providerStartBlockedCopy();
  }
}

function canStartWithSelectedStt() {
  return Boolean(localSttStatus?.ready);
}

function setAiEnabledPreference(enabled) {
  oauthAiEnabled = enabled;
}

function clearLegacyAiEnabledPreference() {
  try {
    window.localStorage?.removeItem(LEGACY_AI_STORAGE_KEY);
  } catch {
    // Old builds persisted AI enablement; current builds require per-meeting enablement.
  }
}

function normalizeTextProviderId(providerId) {
  return Object.prototype.hasOwnProperty.call(TEXT_PROVIDERS, providerId) ? providerId : "codex-chatgpt-oauth";
}

function providerConfig(providerId = selectedTextProviderId) {
  return TEXT_PROVIDERS[normalizeTextProviderId(providerId)];
}

function selectedTextProviderLabel() {
  return providerConfig().label;
}

function providerMissingLoginCopy() {
  return providerConfig().missingLogin;
}

function providerStartBlockedCopy() {
  if (!nativeInvoke) return "請使用桌面 app";
  if (textProviderStatus?.connectorInstalled === false) return `請先安裝 ${selectedTextProviderLabel()}`;
  return providerMissingLoginCopy();
}

function readTextProviderPreference() {
  try {
    return normalizeTextProviderId(window.localStorage?.getItem(TEXT_PROVIDER_STORAGE_KEY));
  } catch {
    return "codex-chatgpt-oauth";
  }
}

function writeTextProviderPreference(providerId) {
  try {
    window.localStorage?.setItem(TEXT_PROVIDER_STORAGE_KEY, providerId);
  } catch {
    // Local storage only restores the user's selected AI provider.
  }
}

function syncTextProviderChoiceControls() {
  for (const choice of textProviderChoices) {
    choice.value = selectedTextProviderId;
  }
}

async function setSelectedTextProvider(providerId) {
  const nextProviderId = normalizeTextProviderId(providerId);
  if (nextProviderId === selectedTextProviderId) {
    syncTextProviderChoiceControls();
    return;
  }
  selectedTextProviderId = nextProviderId;
  writeTextProviderPreference(selectedTextProviderId);
  syncTextProviderChoiceControls();
  textProviderStatus = undefined;
  setAiEnabledPreference(false);
  renderTextProviderStatus();
  if (nativeInvoke && activeSessionId && document.body.dataset.state === "listening") {
    try {
      await nativeInvoke("set_session_text_provider", {
        sessionId: activeSessionId,
        providerId: selectedTextProviderId
      });
    } catch (error) {
      logAppError("ai.set_session_text_provider", error, { sessionId: activeSessionId, providerId: selectedTextProviderId }, "error");
    }
  }
  await refreshTextProviderStatus();
  if (nextProviderId !== selectedTextProviderId) return;
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
  await maybeRunLiveTranscriptRevision({ allowReview: true, force: true });
  if (!isCurrentReviewSession(sessionId)) return;
  const artifact = buildMeetingArtifact();
  if (!hasReviewContent(artifact)) {
    renderPostMeetingReview();
    reviewStatus.textContent = NO_REVIEW_AI_SKIP_MESSAGE;
    downloadState.textContent = "沒有可整理內容；可下載錯誤紀錄協助排查。";
    return;
  }
  downloadState.textContent = `正在用 ${selectedTextProviderLabel()} 產生 AI 整理。`;
  reviewStatus.textContent = `本機文件已可檢查；${selectedTextProviderLabel()} 正在更新 AI 整理。`;
  setAiActivity("post_meeting_summary", `${selectedTextProviderLabel()} 正在更新 AI 整理`);
  try {
    const response = await nativeInvoke("generate_ai_summary_oauth", {
      request: {
        textProviderId: selectedTextProviderId,
        title: artifact.title,
        sessionId: artifact.sessionId,
        generatedAt: artifact.generatedAt,
        prepContext: artifact.prepContext,
        localSummary: artifact.localSummary,
        transcript: artifact.transcript
      }
    });
    if (!isCurrentReviewSession(sessionId)) {
      clearAiActivity("post_meeting_summary");
      return;
    }
    aiSummaryOverride = response.summary;
    renderPostMeetingReview();
    reviewStatus.textContent = "AI 整理已更新，可以檢查文件並下載。";
    downloadState.textContent = "AI 整理已更新，可下載 Markdown、JSON 或 PDF。";
    clearAiActivity("post_meeting_summary", "AI 整理已更新，可以檢查文件並下載。");
  } catch (error) {
    logAppError("ai.post_meeting_summary", error, { sessionId }, "error");
    if (!isCurrentReviewSession(sessionId)) {
      clearAiActivity("post_meeting_summary");
      return;
    }
    aiSummaryOverride = undefined;
    renderPostMeetingReview();
    reviewStatus.textContent = `${selectedTextProviderLabel()} 整理暫時失敗；已保留本機整理，可以先下載。`;
    downloadState.textContent = `AI 整理暫時失敗，已保留本機整理：${formatError(error)}`;
    clearAiActivity("post_meeting_summary", `${selectedTextProviderLabel()} 整理暫時失敗；已保留本機整理。`);
  }
}

function formatRetryDelay(delayMs) {
  const seconds = Math.max(1, Math.ceil(delayMs / 1000));
  if (seconds < 60) return `${seconds} 秒`;
  return `${Math.ceil(seconds / 60)} 分鐘`;
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
  if (event?.id) transcriptRevisionMetadata(event.id);
  if (transcriptRevisionMeta.size > transcriptEvents.length + TRANSCRIPT_REVISION_META_PRUNE_THRESHOLD) {
    pruneTranscriptRevisionMetadata();
  }
}

function markTranscriptPersistence(id, status) {
  const event = transcriptEvents.find((item) => item.id === id);
  if (event) event.persistenceStatus = status;
}

function pruneTranscriptRevisionMetadata() {
  const liveIds = new Set(transcriptEvents.map((event) => event.id));
  for (const id of transcriptRevisionMeta.keys()) {
    if (!liveIds.has(id)) transcriptRevisionMeta.delete(id);
  }
}


function renderTranscriptDrawer() {
  renderTranscriptDrawerView({
    elements: { listeningCurtain, liveTranscript, transcriptDrawerToggle, transcriptDrawerCount, transcriptPreview, transcriptFull },
    transcriptDrawerOpen,
    transcriptEvents: displayedTranscriptEvents(),
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
  const series = selectedMeetingSeries();
  const context = setupController.combinedPrepContext();
  const contextLine = context ? `會議背景：${context.slice(0, 1400)}` : "未提供會議背景，會議中只依照即時內容判斷。";
  const title = series?.title ?? DEFAULT_MEETING_TITLE;
  return {
    sessionId: makeClientId("native"),
    projectId: "live_default_project",
    meetingType: "live_decision_copilot",
    title,
    goal: series
      ? `延續「${series.title}」追蹤本場會議決策，確認舊脈絡是否仍成立`
      : context
        ? `依據會議背景追蹤會議決策：${context.slice(0, 160)}`
      : "即時追蹤會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope",
    mustConfirm: ["owner", "deadline", "驗收標準", "rollback plan"],
    risks: ["未定義 owner/deadline 就做承諾", "demo scope 和正式版 scope 混在一起"],
    constraints: [
      "先確認決策條件再承諾交付",
      series ? `本場選用既有會議脈絡：${series.title}` : "本場未選用既有會議脈絡",
      contextLine
    ],
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
