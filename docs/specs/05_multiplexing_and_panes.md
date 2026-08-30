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
