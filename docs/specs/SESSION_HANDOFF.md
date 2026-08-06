# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p3-input-and-config`
* **當前階段**：**Phase 3 (輸入系統、互動選取、剪貼簿與設定檔熱重載)**
* **已完成項目**：
  - [x] `Phase 0`: Workspace 結構、4 個核心 crates、`docs/specs/` 全部 9 份規格文件建立並已合併至 `main`。
  - [x] `Phase 1`: PTY 抽象、VT100/xterm 狀態機、ScreenGrid 雙緩衝與單元測試建立並已合併至 `main`。
  - [x] `Phase 2`: wgpu GPU 渲染管線、Glyph Atlas 與原生開窗顯示建立並已合併至 `main`。
  - [x] `skterm-core::grid`:
    - `Selection` & `SelectionMode`（支援 Character / Word / Line / Block 選取模式）。
    - 支援 `extract_selected_text()`（智慧換行折疊與去除空白）。
    - 支援 `scroll_offset` 與 `get_visible_row`（流暢歷史滾動瀏覽）。
  - [x] `skterm-render::renderer`:
    - `RenderTheme` 結構，支援自訂配色主題覆蓋。
    - 渲染選取高亮矩形（Semi-transparent selection highlight quads）。
    - 支援歷史滾動行文字與背景渲染。
  - [x] `skterm-gui::config`:
    - `Config` TOML 解析（支援 `font`, `window`, `colors`, `cursor`）。
    - 內建 6 款熱門色彩主題（Catppuccin Mocha, Dracula, Solarized Dark, Tokyo Night, Nord, Default Dark）。
    - `ConfigManager` + `notify` 背景檔案監聽與無重啟即時熱重載（Hot Reload）。
  - [x] `skterm-gui::app_window`:
    - 滑鼠左鍵拖曳選取文字、雙擊選詞、三擊選行。
    - 滑鼠滾輪即時滾動歷史緩衝區（Scrollback）。
    - 跨平台原生剪貼簿整合（`Cmd+C` / `Ctrl+Shift+C` 複製，`Cmd+V` / `Ctrl+Shift+V` 貼上，支援 Bracketed Paste 協定）。
    - 右鍵快速貼上。
  - [x] 單元測試：全部 **16/16 PASS**。

---

## 2. 下一步行動 (Next Steps - Phase 4)
在使用者驗收並確認 Phase 3 之後：
1. Commit `feat/p3-input-and-config` 並合併至 `main` 分支。
2. 切換至新分支 `feat/p4-tabs-and-panes`。
3. 實作 Phase 4：多分頁與 BSP 窗格切分（Multiplexing）：
   - 實作二元空間分割樹（Binary Space Partitioning Tree，支援水平/垂直動態分割）。
   - 支援多分頁（Tabs）標籤列與切換。
   - 多個獨立 PTY 與 Screen Grid 實例，各窗格獨立渲染與事件路由。
   - 快捷鍵：`Cmd/Ctrl + Shift + D` 水平分割、`Cmd/Ctrl + D` 垂直分割、`Cmd/Ctrl + T` 新增分頁、`Cmd/Ctrl + W` 關閉窗格。

---

## 3. 跨 Session 驗證指令
```bash
# 執行所有單元測試
cargo test --workspace

# 執行真實原生 GPU 終端應用程式（支援滑鼠選取、複製貼上、滾輪滾動、設定檔熱重載）
cargo run -p skterm-app
```
