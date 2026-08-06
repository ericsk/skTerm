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
- **快捷鍵支援**：
  - `Cmd/Ctrl + Shift + D`：水平分割窗格。
  - `Cmd/Ctrl + D`：垂直分割窗格。
  - `Cmd/Ctrl + W`：關閉當前焦點窗格。
  - `Cmd/Ctrl + [` / `]`：切換相鄰窗格焦點。
  - `Cmd/Ctrl + T`：新增分頁 (New Tab)。
