# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p4-tabs-and-panes`
* **當前階段**：**Phase 4 (多分頁與 BSP 二元空間分割窗格 Multiplexing)**
* **已完成項目**：
  - [x] `Phase 0`: Workspace 結構、4 個核心 crates、`docs/specs/` 全部 9 份規格文件建立並已合併至 `main`。
  - [x] `Phase 1`: PTY 抽象、VT100/xterm 狀態機、ScreenGrid 雙緩衝與單元測試建立並已合併至 `main`。
  - [x] `Phase 2`: wgpu GPU 渲染管線、Glyph Atlas 與原生開窗顯示建立並已合併至 `main`。
  - [x] `Phase 3`: 輸入系統、選取、剪貼簿、TOML 設定熱重載、CJK 字型回退與 Retina 16pt HiDPI 縮放建立並已合併至 `main`。
  - [x] `Phase 4`:
    - `skterm-gui::layout`:
      - 二元空間分割樹（Binary Space Partitioning Tree，`PaneNode::Leaf` / `PaneNode::Split`）。
      - 多分頁管理（`Workspace` & `Tab`，支援無限層級水平/垂直分割）。
      - 視窗幾何動態切分演算法（`compute_layout` & `sync_pane_sizes`）。
      - **自動偵測終止 Session**（`cleanup_dead_sessions()`：當 Shell 輸入 `exit` 或 `Ctrl+D` 時自動關閉 Pane/Tab，所有 Tab 關閉時自動安全結束程式）。
    - `skterm-render::renderer`:
      - 支援多分頁標籤列渲染（頂部 Tab Bar，動態高亮當前分頁、分隔線、標題）。
      - 支援多窗格邊框（Separator borders）與焦點窗格高亮（Focused border highlight）。
      - 每個窗格獨立持有 PTY 與 ScreenGrid，GPU 批次 Instanced 著色渲染。
    - 快捷鍵與滑鼠路由支援：
      - `Cmd+T` / `Ctrl+Shift+T`：新增分頁。
      - `Cmd+D`：垂直分割窗格（左右分割）。
      - `Cmd+Shift+D`：水平分割窗格（上下分割）。
      - `Cmd+W` / `Ctrl+Shift+W`：關閉當前焦點窗格（最後一個窗格關閉時關閉分頁）。
      - `Cmd+]` / `Cmd+[`：在焦點窗格之間切換（或加 Shift 切換分頁）。
      - `Cmd+1` ~ `Cmd+9`：直接切換至第 1 ~ 9 個分頁。
      - 滑鼠點選分頁列即時切換 Tab；點選任意窗格立即聚焦並可獨立反白選取文字。
  - [x] 單元測試：全部 **18/18 PASS**。

---

## 2. 下一步行動 (Next Steps - Phase 5)
在使用者驗收並確認 Phase 4 之後：
1. Commit `feat/p4-tabs-and-panes` 並合併至 `main` 分支。
2. 切換至新分支 `feat/p5-graphics-protocol`。
3. 實作 Phase 5：進階終端圖像協定（Sixel & Kitty Graphics Protocols）：
   - Sixel 協定解碼器（DCS `q` / Sixel 點陣圖像解析並上傳至 GPU 紋理）。
   - Kitty Graphics 協定解碼器（APC `G...;payload` 支援傳輸 RGBA / PNG 圖像並錨定在 Terminal Grid 單元格）。
   - 支援 `timg`, `chafa`, `icat`, `neofetch` 等圖形終端工具輸出圖像。

---

## 3. 跨 Session 驗證指令
```bash
# 執行所有單元測試
cargo test --workspace

# 執行真實原生 GPU 終端應用程式（支援 Tabs 多分頁、BSP 窗格切割、Shell exit 自動關閉、CJK 輸入、設定熱重載）
cargo run -p skterm-app
```
