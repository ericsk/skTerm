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

## 4. 平台原生分頁架構 (Platform-Native Tab Architecture)

### 4.1 macOS 系統級 `NSWindow` 原生分頁 (路徑一)
- **架構模型 (`TerminalSession`)**：
  - 視窗架構全面重構為 `TerminalSession` 多視窗會話模型，每個 Session 擁有獨立的 `Window`、`TermRenderer`、`Workspace` 與 PTY。
  - macOS 視窗配置相同的系統級標識符 `with_tabbing_identifier("skterm.tabs")`。
  - 視窗啟動時透過 Cocoa Objective-C runtime 發送 `[NSWindow setTabbingMode:NSWindowTabbingModePreferred]`。
  - 當建立第二個及後續分頁時，自動調用 `[parent addTabbedWindow:new_window ordered:NSWindowAbove]` 將新視窗合併入系統標籤列。
- **100% 系統原生體驗**：
  - 分頁列由 macOS 系統 WindowServer 渲染，具備原生毛玻璃透明效果（Vibrancy）與流暢切換動畫。
  - **分頁自由拖曳抽離/合併**：支援使用滑鼠將分頁拖出獨立成為新視窗，或拖回吸附合併。
  - **系統手勢與檢視**：支援 macOS 原生「顯示所有分頁 (Show All Tabs, `Cmd + Shift + \`)」縮圖檢視與右鍵選單。
  - **純淨終端繪圖高度**：在 macOS 原生分頁模式下，畫布內部的 `get_tab_bar_height()` 回傳 `0.0`，終端網格百分之百佔據視窗所有可用像素，無需自繪避讓或刻印按鈕。

### 4.2 Windows 平台原生準備架構
- **雙模式分頁架構 (Dual-Mode Tab Model)**：
  - 保留 `tab_bar_style` 與內建渲染支援，在 Windows / Linux 上提供符合現代視覺（如 Windows 11 Fluent Design）的內嵌懸浮膠囊分頁列。
  - `App` 的多 Session 管理機制預留對接 Windows 原生 **WinUI 3 XAML Islands `TabView`** 或 **Windows Terminal Tab Control**，未來可直接透過 HWND 嵌入 Windows 原生標籤列。
