# Session Handoff & 開發進度總表 (Development Progress Summary)

## 1. 當前專案狀態 (Current Project Status)
* **當前分支**：`main`（已完全整合所有模組並通過全數自動化測試）
* **最新進度階段**：**全域 AI Agent 面板階層重構、macOS 原生標題列配件、零配置滾動與 VSCode 多字體回退系統 (Global AI Panel Hierarchy, Native Titlebar Accessory, Zero-Alloc Grid & Multi-Font Engine)**
* **自動化測試狀態**：全 Workspace **105 / 105 PASS**（100% 通過，0 Warning，0 Error）
  - `skterm-core`：27 項單元測試 + 12 項 VT 相容性測試
  - `skterm-gui`：35 項單元測試
  - `skterm-render`：30 項單元測試 + 1 項 CJK 字型光柵化整合測試

---

## 2. 已完成開發里程碑總覽 (Completed Milestones)

### A. 視窗佈局階層與全域 AI Agent 監控面板 (`skterm-gui`, `skterm-render`)
- [x] **頂層左右分割佈局（主工作區 vs 右側全域 AI 面板）**：
  - 視窗水平分割為左側主工作區（`screen_w - reserved_sidebar_w`）與右側全域 AI Agents 面板（`reserved_sidebar_w`）。
  - 分頁標籤列（Tab List）僅於左側主工作區頂端展開，**絕不橫跨至右側邊欄上方**；右側 AI 面板上緣與左側分頁列頂部齊高（`y = PANEL_MARGIN .. screen_h - PANEL_MARGIN`），直覺表達「面板層級高於分頁、跨分頁運作」之語意。
- [x] **單一來源幾何結構 (`skterm_render::agent_panel::AgentPanelLayout`)**：
  - 渲染器繪圖與 GUI 點擊判定（Hit-testing）共用同一組 `AgentPanelLayout` 像素座標，消除繪圖與點擊區域漂移，並涵蓋 16 項幾何單元測試。
- [x] **macOS 原生標題列 `AI` 按鈕配件 (`crates/skterm-gui/src/macos_titlebar.rs`)**：
  - 透過 `NSTitlebarAccessoryViewController` 於 macOS 原生標題列右側嵌入 PushOnPushOff 樣式的 `AI` 開關按鈕，不佔用分頁或終端機顯示空間，點擊狀態跨視窗同步且透過原子計數器 (`CLICK_COUNT`) 零配置傳遞至事件迴圈。
- [x] **跨分頁 AI Agent 自動發現與導覽 (`App::scan_all_ai_agents`)**：
  - 透過 `ProcessTreeCache` 跨分頁與窗格自動偵測正在執行的 AI 代理（Antigravity CLI、Claude Code、Gemini CLI、Jetski 等），標示所屬分頁徽章（`Tab 1 · #1`）並支援一鍵點擊切換分頁與聚焦窗格。
- [x] **macOS 原生 `NSTextField` 卡片標題內嵌編輯 (`macos_textfield.rs`)**：
  - 雙擊/右鍵點擊卡片或按 `Cmd+Shift+R` 喚出原生 `NSTextField` 自訂任務名稱，100% 支援 macOS 原生輸入法組字、候選字視窗與選取快捷鍵。

### B. 分頁管理、智慧標題與 BSP 窗格切分 (`skterm-gui`, `skterm-core`)
- [x] **跨平台一致懸浮膠囊分頁列與智慧標題 (`tab_title.rs`)**：
  - 停用 macOS 全寬 `NSWindow` 原生分頁列（`NSWindowTabbingModeDisallowed`），統一由內部 `Workspace::tabs` 管理，解決標題列重疊並確保右側全域 AI 面板完整置頂。
  - **前景行程與縮短路徑標題**：閒置 Shell 顯示縮短路徑（如 `~/P/skTerm`、隱藏目錄保留首字元如 `~/.c/nvim`）；執行程式時顯示 `程式名稱: 縮短路徑`（如 `claude: ~/P/skTerm`）。
  - **背景活動圓點與排序**：背景分頁有新輸出時顯示 `• ` 提示；支援滑鼠左右拖曳調整分頁順序，並支援 `Cmd + 1~9`（macOS）/ `Ctrl + 1~9`（Windows/Linux）直達分頁。
- [x] **BSP 二元空間分割窗格與進階操作 (`layout.rs`)**：
  - 支援無限層級垂直/水平分割（`Cmd+D` / `Cmd+Shift+D`）、滑鼠拖曳分隔線無段調整比例（`CursorIcon::ColResize` / `RowResize`）、單一窗格最大化縮放（`Cmd+Shift+Enter` Toggle Pane Zoom）。
  - 新開分頁或分割窗格自動繼承當前窗格的工作目錄（透過 OSC 7 或系統行程表查詢）。

### C. GPU 渲染引擎與字型排版系統 (`skterm-render`)
- [x] **VSCode 風格多字體回退串列与智慧字重匹配 (`atlas.rs`)**：
  - 支援逗號分隔的多字體設定（如 `"JetBrains Mono, Menlo, PingFang TC"`），於偏好設定視窗提供即時字體解析預覽（Live Resolution Preview）。
  - 智慧解析多變體字型家族：自動剝離樣式後綴、優先選擇標準寬度（Regular Width），並根據粗體/斜體旗標精準匹配字面。
- [x] **程序化無縫框線與區塊元素 (`box_drawing.rs`)**：
  - 針對 Unicode Box Drawing (`U+2500..U+257F`) 與 Block Elements (`U+2580..U+259F`) 直接以 GPU 幾何頂點程序化繪製，徹底消除行高調整導致的 TUI 表格斷線與縫隙。
- [x] **`bold_is_bright` 粗體高亮與零重建即時變色**：
  - 粗體文字自動映射至高亮 ANSI 色盤（0..7 -> 8..15）或純白前景；切換主題透過 Uniform 緩衝區更新實現 0 毫秒無閃爍變色。

### D. 終端核心效能、搜尋與互動體驗 (`skterm-core`, `skterm-gui`)
- [x] **零配置滾動行回收池 (`RowPool` / `recycled_rows`)**：
  - 滾動（`scroll_up`、`DECSTBM`、`IL`/`DL`）時循環重用被淘汰的 `Row` 記憶體緩衝區，達成大量文字輸出時零 Heap 分配。
- [x] **零閒置耗電事件迴圈 (`ControlFlow::Wait` / `WaitUntil`) 與 Release 最佳化**：
  - 終端靜止時完全休眠（0.0% CPU 佔用）；`Cargo.toml` Release Profile 啟用 `lto = "thin"`、`codegen-units = 1`、`panic = "abort"` 與 `strip = true`。
- [x] **全緩衝區即時搜尋 (`Cmd+F`)、清空畫面 (`Cmd+K`) 與互動式捲軸**：
  - 支援跨 Scrollback 與當前畫面的關鍵字高亮搜尋與上下跳轉；`Cmd+K` 清空歷史與畫面；右側提供可拖曳捲軸並在打字時自動回到底部。
- [x] **剪貼簿圖片直接貼上轉路徑 & 純文字 URL 自動偵測**：
  - `Cmd+V` 貼上剪貼簿圖片時自動存為 `/tmp/skterm_paste_*.png` 並插入跳脫路徑；同時支援 OSC 8 與純文字網址 Hover 底線與 `Cmd/Ctrl + Click` 開啟瀏覽器。
- [x] **原子化設定檔儲存 (`config_persistence`)**：
  - 透過暫存檔寫入後原子級 `rename` 覆蓋 `config.toml`，防止並發存檔或熱重載讀取時發生檔案損毀。

---

## 3. 常用開發與驗證指令
```bash
# 執行全 Workspace 105 項自動化單元與相容性測試
cargo test --workspace

# 啟動開發版 skTerm 終端模擬器
cargo run -p skterm-app

# 編譯最佳化 Release 版本
cargo build --release

# 打包 macOS App Bundle (skTerm.app 含 AppIcon.icns 與 ad-hoc 簽署)
./scripts/package_macos.sh
```
