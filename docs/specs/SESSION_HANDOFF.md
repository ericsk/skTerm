# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`main` (已完全整合並通過所有測試)
* **當前階段**：**macOS HIG 偏好設定視窗重構、即時主題同步與純黑預設外觀 (macOS Sonoma HIG & Live Theming)**
* **已完成項目**：
  - [x] `Phase 0 ~ Phase 6`: 核心終端機、GPU 渲染管線、輸入與 IME、多窗格分頁、Sixel/Kitty 圖像協定與發行打包已全數合併至 `main`。
  - [x] **macOS Sonoma/Sequoia HIG 獨立偏好設定視窗 (`PreferencesWindow`)**：
    - `190pt` 左側導航欄，整合狀態指示點與檔案快捷存取。
    - 右側 Inset Grouped Table Cards 樣式卡片。
    - 翠綠色 Toggle Switch (`#30D158`)、Segmented Control 分段切換按鈕、Keyboard Badges 膠囊徽章。
  - [x] **一鍵主題卡片畫廊 (1-Click Theme Gallery)**：
    - 6 款主題卡片一鍵切換，支援即時 Hover 微光與 macOS 系統藍高亮邊框 (`#0A84FF`)。
  - [x] **純黑預設主題 (Pure Black OLED #000000)**：
    - 預設背景純黑 `#000000` + 亮白 `#F5F5F7` 文字 + 鮮明 16 色 ANSI 調色盤。
    - 自訂色彩覆蓋區支援調色盤選色器、Hex 輸入與快速一鍵色點（純黑、深墨黑、亮白、駭客綠、琥珀黃等）。
  - [x] **視窗尺寸自動記住 (Debounced Geometry Auto-Save)**：
    - 拖拉視窗改變大小後 400ms 背景防抖存檔，下次啟動時精準維持前一次拉好的視窗大小。
  - [x] **零延遲即時同步與效能優化 (Zero-Latency Live Sync)**：
    - 消除純切換主題時的 WGPU Shader 與字形 Atlas 重建卡頓。
    - 實作背景執行緒寫入 `config.toml`，消除磁碟 I/O 阻塞。
    - 加入回聲熱重載過濾 (`Echo Hot-Reload Filter`)，避免自動存檔引發連鎖卡頓。
  - [x] **平台原生分頁架構升級 (Platform-Native Tab Architecture)**：
    - **macOS（路徑一）**：全套重構為 `TerminalSession` 多視窗會話模型，透過 Cocoa Objective-C runtime 深度整合 macOS 系統原生 **`NSWindow` Tabbing**（`NSWindowTabbingModePreferred` 與 `addTabbedWindow:ordered:`），享有 100% macOS 系統級毛玻璃、分頁拖曳抽離成獨立視窗/拖回合夥、縮圖檢視 (`Cmd+Shift+\`)，畫布內無需自繪避讓，終端保持 100% 純淨顯示高度。
    - **Windows / Linux 準備**：保留內嵌分頁與多 Session 抽象層，預留未來透過 HWND 嵌入 WinUI 3 XAML Islands `TabView` 的擴充插槽。
  - [x] **鍵盤編碼系統擴充 (Keyboard Encoding Enhancements)**：
    - 完整支援 **`Shift + Tab`（Backtab）** 發送標準 ANSI/xterm 序列 **`\x1b[Z`**（CSI Z），支援 Shell/TUI 反向補全。
    - 支援 `Shift`/`Alt`/`Ctrl` 複合修飾鍵對方向鍵與導航鍵的 xterm 參數編碼（如 `Ctrl + Left/Right` 跳詞）。
  - [x] **macOS 系統捲動連動與觸控板平滑捲動 (macOS Natural Scrolling & Smooth Trackpad Scroll)**：
    - 修正 `ScreenBuffer::scroll_display` 方向邏輯，與 macOS「自然捲動 (Natural Scrolling)」設定一致。
    - 加入 `scroll_pixel_accumulator` 累積 `MouseScrollDelta::PixelDelta` 次像素位移，支援觸控板與 Magic Mouse 平滑捲動。
    - 支援 Alternate Screen 與 Mouse Reporting（`vim`、`tmux`、`less`、`htop`）滾輪事件自動轉發。
  - [x] **全域 AI Agent 側邊欄架構 (Global AI Agent Sidebar)**：
    - 側邊欄開關狀態 (`App::sidebar_open`) 提升至視窗群組層級（高於分頁與窗格），開新分頁自動同步顯示且支援跨分頁掃描與點擊跳轉。
  - [x] 單元測試與相容性測試：全部 **45/45 PASS**（100% 成功，0 Warning，0 Error）。

---

## 2. 驗證指令
```bash
# 執行所有單元測試與相容性測試
cargo test --workspace

# 啟動原生終端並測試偏好設定與熱重載
cargo run -p skterm-app

# 測試打包 macOS App Bundle
./scripts/package_macos.sh
```
