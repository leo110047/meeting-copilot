# Meeting Copilot

Meeting Copilot 是一個 native desktop app，用來在會議中即時輔助決策。
它會記錄會議內容、追蹤決策背景，並在 owner、scope、deadline、驗收標準還不清楚時，提示使用者先補問或暫停承諾。

它透過會議歷史、專案記憶、deterministic reducers 與可 replay 的 fixtures，降低會議中做出不健康承諾的機率。

## 環境需求

- Node.js 20+
- npm
- Rust toolchain
- 目前 OS 對應的 Tauri build prerequisites
- 從原始碼建置本機 Whisper sidecar 時需要 CMake 與 C/C++ toolchain（macOS: Xcode Command Line Tools；Windows: Visual Studio Build Tools）。
- macOS native transcription 需要授權 `Meeting Copilot` 的 Speech Recognition、Microphone，以及螢幕與系統錄音權限。
- Windows system audio transcription 需要 Windows audio endpoint 可用；本機 Whisper 模式會使用 WASAPI loopback capture。
- AI 功能需要透過本機官方 CLI 連接器登入訂閱帳號：ChatGPT 走 Codex CLI，Claude 走 Claude Code CLI。未安裝時 app 只會提供官方安裝教學與指令，不會自行安裝全域 CLI。
- Claude Code connector 目前以 Claude Code CLI `2.1.128` 驗證；需要支援 `claude auth status`、`claude auth login` 與 `claude -p --output-format json --tools "" --no-session-persistence --no-chrome`。

## 安裝

```bash
npm install
```

## 啟動 Desktop App

開發模式：

```bash
npm run tauri:dev
```

建立 debug native app：

```bash
npm run native:build
```

macOS debug bundle 會產生在：

```text
target/debug/bundle/macos/Meeting Copilot.app
```

macOS TCC 權限會綁定 code-signing identity。engineering-only build 可用 `Apple Development` 或 ad-hoc 簽章；這可以讓熟悉 macOS Gatekeeper 的測試者手動開啟，但不應視為正式對外發佈。要交給一般使用者測試或發佈，必須使用 `Developer ID Application` identity 簽署並 notarize。

本機 debug 若要固定簽章，可用 identity 的 SHA-1 hash，避免 Keychain 裡有同名憑證時 `codesign` ambiguous：

```bash
MEETING_COPILOT_CODESIGN_IDENTITY="CERTIFICATE_SHA1_HASH" \
MEETING_COPILOT_CODESIGN_KEYCHAIN="$HOME/Library/Keychains/login.keychain-db" \
npm run native:build:mac
```

若要診斷麥克風是否真的有進到 native bridge，可暫時加上 `MEETING_COPILOT_AUDIO_DIAGNOSTICS=1`；這會在 app error log 寫入 `native_transcription.bridge_diagnostic` 與 `rms` / `peak` 音量值，平常不應開啟。

若沒有可用的 Apple signing identity，macOS build 會失敗，而不是靜默改成 ad-hoc。只有一次性本機試跑才應明確開啟：

```bash
MEETING_COPILOT_ALLOW_ADHOC_SIGNING=1 npm run native:build:mac
```

ad-hoc 簽章沒有穩定 TeamIdentifier，不適合測試螢幕與系統錄音權限流程。

macOS distribution build 會拒絕 `Apple Development` 與本機自簽憑證，只接受有效的 `Developer ID Application` identity：

```bash
MEETING_COPILOT_CODESIGN_IDENTITY="DEVELOPER_ID_APPLICATION_SHA1_HASH" \
MEETING_COPILOT_CODESIGN_KEYCHAIN="$HOME/Library/Keychains/login.keychain-db" \
npm run native:build:mac:release
```

產出的 app/dmg 仍需完成 Apple notarization 與 stapling 後再交付外部使用者。

Windows debug installer / bundle 使用：

```bash
npm run native:build:windows
```

## 常用命令

```bash
npm run build          # 建置 shared frontend 並驗證 fixtures
npm test               # 執行 JavaScript test suite
npm run rust:test      # 執行 Rust tests
npm run migrate        # 建立或更新本機 SQLite database
npm run replay         # replay 預設 golden fixture
npm run manual         # 執行 CLI transcript loop，僅供測試
npm run stt:bakeoff    # 驗證 STT provider evaluation shape
npm run verify         # 執行完整本機驗證流程
```

## 產品流程

1. **會前準備**：登入並啟用 AI，輸入會議背景，也可以拖拉文字檔。支援的拖曳檔案會讀取文字內容，啟用 AI 後作為會議背景送給 AI。
2. **會議中**：開始會議、選擇音訊來源、調整視窗透明度，需要時展開逐字稿抽屜。macOS 與 Windows 都提供 `麥克風 + 系統音訊`、`我的麥克風` 與 `系統音訊`；混合來源會同時啟動 mic 與 system capture，並保留 transcript source。預設本機 STT 走 bundled Whisper runner；若音訊或 STT 出錯，錯誤會進 app error log。
3. **會後整理**：檢查兩份文件：AI 整理與逐字稿。主要匯出 Markdown/TXT，JSON/PDF 作為次要格式；需要回報 bug 時，可在其他格式中下載錯誤紀錄 JSON。

## 架構地圖

```text
src/domain        domain contracts 與 reducers
src/core          event bus、compiler、retrieval、policy、coach engine
src/storage       SQLite schema 與 repositories
src/providers     STT 與 text model provider interfaces
src/replay        replay harness 與 fixture evaluation
src/ui            shared desktop frontend
src-tauri         native shell、commands、tray/status item、SQLite bridge
native            platform speech helpers
tests             contract、replay、storage、UI、native shell tests
fixtures          replay 與 golden transcript fixtures
docs              設計與實作補充文件
```

## AI 與隱私邊界

- STT provider 和 text decision provider 是分開的。
- 使用者在 app 內明確啟用本場會議 AI 前，不會開始使用 AI 功能；切換 CLI 連接器或開始下一場會議後需要重新啟用。
- CLI 認證由 Codex CLI / Claude Code CLI 自行處理，Meeting Copilot 只檢查登入狀態，不讀取或保存 CLI token。
- Live AI extraction 只能輸出 patch，不能重寫完整 meeting state。
- Shared artifacts 不包含 private copilot state。
- `PoliticalSignal` 不得直接綁定姓名或 participant ID。
- 本機 runtime data 會存在 `.data/`，不應提交到版本控制。
- 錯誤紀錄會保存 stage、source、severity、message 與診斷 detail；不會把音訊 payload 寫入 log。

## 相關文件

- [DESIGN.md](./DESIGN.md)：UI source of truth。
- [docs/desktop-shell.md](./docs/desktop-shell.md)：desktop shell 補充說明。
- [docs/native-audio-stt.md](./docs/native-audio-stt.md)：native audio 與 STT 補充說明。
- [docs/implementation-understanding.md](./docs/implementation-understanding.md)：實作摘要。
