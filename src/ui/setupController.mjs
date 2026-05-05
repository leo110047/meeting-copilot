export function createSetupController({
  elements,
  nativeInvoke,
  nativeListen,
  canStartWithAi,
  syncStartButtonAvailability,
  textProviderAuthenticated,
  selectedTextProviderId,
  selectedTextProviderLabel,
  logAppError,
  formatError,
  escapeHtml
}) {
  let droppedFileNames = [];
  let droppedContextChunks = [];
  let droppedFileErrors = [];
  let prepDictating = false;
  let prepSummaryTimer;
  let prepSummaryRequestId = 0;
  let prepSummaryInFlight = false;
  let prepSummaryQueued = false;

  const {
    setupContext,
    setupDropZone,
    setupContextMeta,
    droppedFileCount,
    prepDictationButton,
    prepSummary
  } = elements;

  function install() {
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
      const errors = [];
      for (const file of files) {
        if (!isSupportedTextContextFileName(file.name)) {
          const message = "只支援文字檔：txt、md、csv、json、log、srt、vtt";
          errors.push({ name: file.name, error: message });
          recordDroppedFileError(file.name, message);
          logAppError("files.browser_drop_file", message, { name: file.name, size: file.size, type: file.type }, "warning");
          continue;
        }
        const loadedFile = await readBrowserDroppedFile(file);
        if (loadedFile?.text) {
          loaded.push(loadedFile);
        } else if (loadedFile?.error) {
          errors.push({ name: file.name, error: loadedFile.error });
          recordDroppedFileError(file.name, loadedFile.error);
        }
      }
      appendDroppedContext(loaded.filter(Boolean));
      droppedFileNames.push(...loaded.map((file) => file.name));
      const loadedNames = loaded.map((file) => file.name).join("、");
      setupContextMeta.textContent = loaded.length > 0
        ? errors.length > 0
          ? `已加入檔案：${loadedNames}；部分檔案未讀取：${formatFileErrors(errors)}`
          : `已加入檔案：${loadedNames}。`
        : errors.length > 0
          ? `檔案未加入：${formatFileErrors(errors)}`
          : "檔案讀取失敗，尚未加入會議背景。";
    });
    prepDictationButton.addEventListener("click", togglePrepDictation);
    installNativeDropListeners().catch((error) => {
      logAppError("ui.install_native_drop_listeners", error, {}, "error");
      setupContextMeta.textContent = `檔案拖拉尚未啟用：${formatError(error)}`;
    });
    installPrepDictationListeners().catch((error) => {
      logAppError("ui.install_prep_dictation_listeners", error, {}, "error");
      setupContextMeta.textContent = `語音輸入尚未啟用：${formatError(error)}`;
    });
  }

  async function togglePrepDictation() {
    if (!nativeInvoke) {
      logAppError("prep.dictation_unavailable", "Prep dictation requires the desktop app", {}, "warning");
      setupContextMeta.textContent = "語音輸入需要使用桌面 app。";
      return;
    }
    if (!prepDictating && !canStartWithAi()) {
      logAppError("prep.dictation_blocked_without_ai", "Prep dictation was requested before AI was enabled", { authenticated: textProviderAuthenticated() }, "warning");
      syncStartButtonAvailability();
      setupContextMeta.textContent = textProviderAuthenticated()
        ? "請先啟用 AI，才能使用語音輸入。"
        : `請先登入 ${selectedTextProviderLabel()}，才能使用語音輸入。`;
      return;
    }
    prepDictationButton.disabled = true;
    try {
      if (prepDictating) {
        await nativeInvoke("stop_prep_dictation");
        setPrepDictating(false);
        setupContextMeta.textContent = "語音輸入已停止。";
      } else {
        await nativeInvoke("start_prep_dictation", { providerId: selectedTextProviderId() });
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
  }

  async function stopPrepDictationBeforeMeeting() {
    if (!prepDictating || !nativeInvoke) return;
    await nativeInvoke("stop_prep_dictation").catch((error) => {
      logAppError("prep.dictation_stop_before_meeting", error, {}, "warning");
    });
    setPrepDictating(false);
  }

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
        recordDroppedFileError(file.name, file.error);
        logAppError("files.native_drop_file", file.error, { name: file.name }, "warning");
      }
      droppedFileNames.push(...loaded.map((file) => file.name));
      const loadedNames = loaded.map((file) => file.name).join("、");
      setupContextMeta.textContent = errors.length > 0
        ? loaded.length > 0
          ? `已加入檔案：${loadedNames}；部分檔案未讀取：${errors.slice(0, 2).join("；")}`
          : `部分檔案未讀取：${errors.slice(0, 2).join("；")}`
        : `已加入檔案：${loadedNames}。啟用 AI 後會送出檔案文字內容。`;
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

  function recordDroppedFileError(name, error) {
    droppedFileErrors.push({ name, error });
    if (droppedFileErrors.length > 50) droppedFileErrors = droppedFileErrors.slice(-50);
  }

  function readBrowserDroppedFile(file) {
    return new Promise((resolve) => {
      const reader = new FileReader();
      reader.onload = () => resolve({ name: file.name, text: String(reader.result).slice(0, 8000), truncated: false });
      reader.onerror = () => {
        logAppError("files.browser_drop_file", reader.error ?? "browser FileReader failed", { name: file.name, size: file.size, type: file.type }, "warning");
        resolve({ name: file.name, text: "", truncated: false, error: reader.error?.message ?? "browser FileReader failed" });
      };
      reader.readAsText(file);
    });
  }

  function isSupportedTextContextFileName(name) {
    // Keep in sync with read_dropped_context_file in src-tauri/src/shell_storage.inc.rs.
    return /\.(txt|md|markdown|csv|json|log|srt|vtt)$/i.test(name);
  }

  function formatFileErrors(errors) {
    return errors
      .slice(0, 2)
      .map((file) => `${file.name}: ${file.error}`)
      .join("；");
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

  function contextDiagnostics() {
    return {
      fileCount: droppedContextChunks.length,
      failedFiles: droppedFileErrors.slice(-5)
    };
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

  function resetPrepSummaryQueue() {
    clearTimeout(prepSummaryTimer);
    prepSummaryTimer = undefined;
    prepSummaryRequestId += 1;
    prepSummaryInFlight = false;
    prepSummaryQueued = false;
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
          textProviderId: selectedTextProviderId(),
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
      if (requestId !== prepSummaryRequestId) return;
      prepSummaryInFlight = false;
      if (prepSummaryQueued) {
        prepSummaryQueued = false;
        schedulePrepSummaryGeneration();
      }
    }
  }

  return {
    install,
    combinedPrepContext,
    contextDiagnostics,
    renderPrepSummary,
    resetPrepSummaryQueue,
    schedulePrepSummaryGeneration,
    stopPrepDictationBeforeMeeting
  };
}
