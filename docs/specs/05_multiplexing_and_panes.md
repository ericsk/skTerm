# 05. 多分頁與 BSP 窗格切分規格 (Multiplexing & Panes)

## 1. 二元空間分割樹 (Binary Space Partitioning Tree)
窗格切分採用 BSP 樹狀資料結構表示：

```rust
pub enum PaneNode {
    /// 包含終端實例的葉節點
    Leaf {
        id: PaneId,
        terminal_id: TerminalId,
        is_focused: bool,
    },
    /// 包含分割子節點的內部節點
    Split {
        direction: SplitDirection, // Horizontal / Vertical
        ratio: f32,                // 分割比例 (例如 0.5)
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}
```

---

## 2. 佈局計算、窗格互動與事件調度
- **動態尺寸計算**：根據父視窗的像素尺寸與 Padding，遞迴遍歷 BSP 樹計算各個 Pane 網格的實際像素邊界 (Rect)。
- **獨立 PTY/Screen 實例**：每個 Pane 擁有獨立的 `PtyProcess` 與 `ScreenGrid`，彼此不受阻塞。
- **窗格分隔線拖曳調整 (Splitter Dragging Resize)**：
  - 滑鼠移至 BSP 窗格間的分隔線時自動變換游標為雙向調整箭頭（`CursorIcon::ColResize` 或 `CursorIcon::RowResize`），按住左鍵拖曳可平滑調整 `Split::ratio`（限制範圍 `0.1..=0.9`），並即時觸發子窗格 PTY `resize` 與緩衝區 Reflow。
- **單一窗格最大化縮放 (Pane Zoom / Maximize)**：
  - 支援 `Cmd + Shift + Enter`（macOS）或 `Ctrl + Shift + Enter`（Windows/Linux）將當前焦點窗格暫時全螢幕放大至整個主工作區，再次按下則無損還原原本的多窗格切分比例。
- **跨平台快捷鍵規範分流**：
  - **macOS**：採用 `Cmd` 觸發視窗與窗格管理，保留 `Ctrl+D` 忠實作為 Unix 標準 EOF 傳入 Shell：
    - `Cmd + D`：左右分割窗格 (Vertical Split)。
    - `Cmd + Shift + D`：上下分割窗格 (Horizontal Split)。
    - `Cmd + Shift + Enter`：切換當前窗格最大化/還原 (Toggle Pane Zoom)。
    - `Cmd + W`：關閉當前焦點窗格（若為最後一個窗格則關閉分頁）。
    - `Cmd + [` / `]`：切換相鄰窗格焦點。
    - `Cmd + T`：新增分頁 (New Tab，自動繼承當前工作目錄)。
    - `Cmd + 1~9`：直達第 1~9 個分頁。
    - `Cmd + F` / `Cmd + K`：開啟搜尋列 / 清空終端畫面與歷史紀錄。
  - **Linux / Windows**：採用 `Ctrl + Shift + <Key>` 與 `Ctrl + 1~9` 組合鍵，避免與終端基本控制碼衝突：
    - `Ctrl + Shift + D` / `Ctrl + Shift + E`：分割窗格。
    - `Ctrl + Shift + Enter`：切換窗格最大化/還原。
    - `Ctrl + Shift + W`：關閉窗格。
    - `Ctrl + Shift + T`：新增分頁。
    - `Ctrl + 1~9`：直達第 1~9 個分頁。

---

## 3. 分頁標籤列顯示策略 (Tab Bar Style)
- `tab_bar_style = "auto"`：當視窗內存在多於 1 個分頁時自動顯示標籤列，單一分頁時自動隱藏以最大化終端工作空間。
- `tab_bar_style = "always"`：恆常顯示分頁標籤列。
- `tab_bar_style = "never"`：恆常隱藏分頁標籤列。

---

## 4. 視窗與分頁佈局階層 (Window & Tab Layout Hierarchy)

- **頂層左右分割（主工作區 vs 全域右側邊欄）**：
  - 整個視窗首先在水平方向分割為**左側主工作區**（寬度為 `screen_w - reserved_sidebar_w`）與**右側全域 AI Agents 面板**（寬度為 `reserved_sidebar_w`）。
  - 分頁標籤列（Tab List）僅於左側主工作區頂端展開，**絕不橫跨至右側邊欄上方**，因此在概念與視覺上，右側 AI Agents 面板的上緣與分頁標籤列的頂部齊高，清楚表達「側邊欄層級高於分頁、跨分頁運作」的語意。
- **跨平台一致的懸浮膠囊分頁列與智慧標題 (`tab_title`)**：
  - macOS 視窗停用系統全寬 `NSWindow` 原生分頁列（`NSWindowTabbingModeDisallowed`），改由內部 `Workspace::tabs` 統一管理分頁（`Cmd + T` 新增分頁、`Cmd + W` 關閉窗格/分頁、`Cmd + N` 開啟新視窗）。
  - **智慧分頁標題**：當 Shell 閒置時僅顯示縮短後的工作目錄（如 `~/P/skTerm`、隱藏目錄保留首字元如 `~/.c/nvim`）；當有前景程式執行時自動顯示 `程式名稱: 縮短路徑`（如 `claude: ~/P/skTerm`、`cargo: ~/P/skTerm`）。
  - **分頁拖曳排序與背景活動提示**：支援按住分頁膠囊左右拖曳重新排序；背景分頁有新 PTY 輸出時於標題前顯示 `• ` 圓點提示，並同步更新視窗置中標題列。

---

## 5. 全域 AI Agent 面板架構 (Global AI Agent Panel)
- **層級高於分頁與窗格 (`App::sidebar_open`)**：
  - 面板開關狀態隸屬於最頂層 `App` 結構（而非單一 `Workspace`），確保開啟新分頁 (`Cmd + T`) 時自動繼承顯示狀態，且在任何分頁切換開關時（`Cmd + Shift + A`、標題列按鈕或面板關閉鈕）同步更新所有分頁佈局與窗格尺寸。
- **版面與分頁列頂部齊高 (`skterm_render::agent_panel`)**：
  - 面板停靠於視窗**右側**全高區域（`y = PANEL_MARGIN .. screen_h - PANEL_MARGIN`），上緣與左側分頁標籤列頂部齊高；切換分頁時僅左側主工作區切換，右側面板保持固定不動。
  - 標頭顯示 `ALL AGENTS (n)` 與範圍說明 `Global · across N tabs`，明示清單涵蓋所有分頁。
  - 幾何由 `AgentPanelLayout` 單一來源提供，renderer 繪製與 GUI 點擊判定共用同一組座標，避免兩者漂移。
- **開關入口位於標題列，而非分頁列或終端內容之上**：
  - 以 `NSTitlebarAccessoryViewController` 承載一顆原生 `AI` 按鈕（`crate::macos_titlebar`），靠齊 macOS 原生標題列尾端，不佔用任何終端或分頁空間。
  - 按鈕為 PushOnPushOff 型態，其按下狀態由 `App::toggle_sidebar` 同步至**所有**視窗，反映面板的全域性。
  - AppKit 的動作回呼不經由 winit 事件流，故以單調遞增的點擊計數器傳遞，`about_to_wait` 每輪比對計數差值，避免兩次輪詢間的點擊遺失。
  - 鍵盤入口維持 `Cmd + Shift + A`。
- **跨分頁 Agent 自動發現與導覽 (`App::scan_all_ai_agents`)**：
  - 透過 `ProcessTreeCache` 跨所有 `TerminalSession`、分頁與窗格掃描正在執行的 AI Agent 實例（如 Antigravity CLI、Claude Code、Gemini CLI、Jetski），並於每張卡片標示歸屬徽章（如 `Tab 1 · #1`、`Tab 2 · #1`）。
  - 點擊面板 Agent 卡片時，自動切換至目標分頁並聚焦該窗格。

