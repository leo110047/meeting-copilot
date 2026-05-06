export function labelCaptureSource(source) {
  if (source === "mixed") return "麥克風 + 系統音訊";
  if (source === "mic") return "我的麥克風";
  if (source === "system") return "系統音訊";
  return source ?? "未知音訊";
}

export function labelLanguage(language) {
  if (!language) return "自動語言";
  if (/zh/i.test(language)) return "中文";
  if (/en/i.test(language)) return "英文";
  return language;
}

export function formatAudioMonitorMessage(message, code = "") {
  if (message.includes("stopped from tray")) return "已從系統列結束會議。";
  if (code === "no_speech_detected" || /No speech detected|未偵測到語音|未检测到语音/i.test(message)) return "尚未偵測到可轉成文字的語音；請確認音訊來源正在播放或有人說話。";
  if (code === "recognition_request_canceled" || /Recognition request (was )?cancell?ed/i.test(message)) return "語音辨識正在自動恢復。";
  if (isScreenRecordingPermissionMessage(message)) return "沒有系統音訊權限。請到 macOS 設定的螢幕與系統錄音開啟 Meeting Copilot，然後重新開始會議。";
  if (/system native speech helper exited/i.test(message)) return "系統音訊轉錄已停止；請確認螢幕與系統錄音已開啟 Meeting Copilot。";
  if (/exited before Stop Listening/i.test(message)) return "語音轉錄已停止，請結束後重新開始會議。";
  if (/permission|denied/i.test(message)) return "沒有麥克風或系統音訊權限。";
  if (/read-only file system/i.test(message)) return "目前無法啟動音訊，請重新開啟 app 後再試。";
  return `語音狀態：${message}`;
}

export function isScreenRecordingPermissionMessage(message) {
  return /TCC|ScreenCaptureKit|Screen Recording|system audio permission|screenCaptureReady=false|screenSystemAudioPreflight=false|視窗、顯示器擷取|螢幕錄製|螢幕與系統錄音|擷取/i.test(String(message ?? ""));
}

export function clamp(value, min, max) {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function detectUiLanguage(text) {
  if (/[\u4e00-\u9fff]/.test(text) && /[a-z]/i.test(text)) return "mixed";
  if (/[\u4e00-\u9fff]/.test(text)) return "zh";
  return "en";
}

export function formatError(error) {
  if (typeof error === "string") return error;
  return error?.message ?? JSON.stringify(error);
}

export function makeClientId(prefix) {
  return `${prefix}_${crypto.randomUUID?.() ?? `${Date.now()}_${Math.random().toString(16).slice(2)}`}`;
}
