# Meeting Copilot

Meeting Copilot 是一個 native desktop app，用來在會議中即時輔助決策。
它會記錄會議內容、追蹤決策背景，並在 owner、scope、deadline、驗收標準還不清楚時，提示使用者先補問或暫停承諾。

這不是單場會議摘要工具。產品目標是 Layer 3 決策副駕：透過會議歷史、專案記憶、deterministic reducers 與可 replay 的 fixtures，降低會議中做出不健康承諾的機率。

## 環境需求

- Node.js 20+
- npm
- Rust toolchain
- 目前 OS 對應的 Tauri build prerequisites
- macOS native microphone transcription 需要 Speech Recognition / Microphone 權限；system audio 需要 Screen Recording 權限。
- Windows system audio transcription 需要 Windows audio endpoint 可用，且會使用 WASAPI loopback 與 Windows SpeechRecognition。
- AI 功能需要透過本機 subscription/OAuth connector 登入 ChatGPT

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
2. **會議中**：開始會議、選擇音訊來源、調整視窗透明度，需要時展開逐字稿抽屜。macOS 與 Windows 都提供 `我的麥克風` 與 `系統音訊`；Windows 系統音訊走 WASAPI loopback，再交給本機 SpeechRecognition。若音訊或 STT 出錯，錯誤會進 app error log。
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
- 使用者在 app 內明確啟用 AI 前，不會開始使用 AI 功能。
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
