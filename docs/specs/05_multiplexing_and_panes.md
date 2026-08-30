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

## 2. 佈局計算與事件調度
- **動態尺寸計算**：根據父視窗的像素尺寸與 Padding，遞迴遍歷 BSP 樹計算各個 Pane 網格的實際像素邊界 (Rect)。
- **獨立 PTY/Screen 實例**：每個 Pane 擁有獨立的 `PtyProcess` 與 `ScreenGrid`，彼此不受阻塞。
- **跨平台快捷鍵規範分流**：
  - **macOS**：採用 `Cmd` 觸發視窗與窗格管理，保留 `Ctrl+D` 忠實作為 Unix 標準 EOF 傳入 Shell：
    - `Cmd + D`：左右分割窗格 (Vertical Split)。
    - `Cmd + Shift + D`：上下分割窗格 (Horizontal Split)。
    - `Cmd + W`：關閉當前焦點窗格。
    - `Cmd + [` / `]`：切換相鄰窗格焦點。
    - `Cmd + T`：新增分頁 (New Tab)。
    - `Cmd + 1~9`：直達第 N 個分頁。
  - **Linux / Windows**：採用 `Ctrl + Shift + <Key>` 組合鍵，避免與終端基本控制碼衝突：
    - `Ctrl + Shift + D` / `Ctrl + Shift + E`：分割窗格。
    - `Ctrl + Shift + W`：關閉窗格。
    - `Ctrl + Shift + T`：新增分頁。

---

## 3. 分頁標籤列顯示策略 (Tab Bar Style)
- `tab_bar_style = "auto"`：當視窗內存在多於 1 個分頁時自動顯示標籤列，單一分頁時自動隱藏以最大化終端工作空間。
- `tab_bar_style = "always"`：恆常顯示分頁標籤列。
- `tab_bar_style = "never"`：恆常隱藏分頁標籤列。

---

## 4. macOS Sonoma / Sequoia 原生感分頁列設計 (Native HIG Tab Bar)
- **佈局幾何 (Geometry)**：
  - 高度由 `28pt` 升級為標準 `36pt`，提供舒適的垂直排版與卡片膠囊呼吸感。
  - **避讓 Traffic Lights (紅綠燈)**：macOS 下分頁自動自 `x = 78pt` 起算，保留左側安全邊距，徹底杜絕分頁與視窗控制按鈕重疊。
  - **自適應分段懸浮膠囊 (Floating Segmented Tab Capsules)**：各分頁呈懸浮卡片質感，上下預留 4pt 外距，左右預留 4pt 間隔。
- **視覺階層 (Visual Hierarchy)**：
  - **Active Tab**：深雅卡片底色 + 頂部/側邊微光邊框 + 底部 2px macOS 活力藍指示條 (`#0A84FF`) + 亮白字體。
  - **Inactive Tab**：半透明沉浸底色 + 次要灰文字。
  - **底部分隔線**：1px 微光半透明細線區隔終端工作區。
- **互動控制 (Interactivity)**：
  - **關閉分頁 (`×` Close Button)**：各分頁右側配置精緻微型 `×` 按鈕，點選即時關閉。
  - **新增分頁 (`+` New Tab Button)**：末端配置獨立圓角 `+` 按鈕，點選即時建立新分頁。
  - **雙擊空白處**：雙擊 Tab Bar 空白區域快速建立新分頁。
