# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p1-terminal-core`
* **當前階段**：**Phase 1 (PTY 抽象、VT 狀態機與 Screen Grid 核心)**
* **已完成項目**：
  - [x] `Phase 0`: Workspace 結構、4 個核心 crates、`docs/specs/` 全部 9 份規格文件建立並已合併至 `main`。
  - [x] `skterm-core::grid`:
    - `Color`（支援 Default, 16 Named, 256 Indexed, 24-bit TrueColor RGB）。
    - `CellFlags`（Bold, Dim, Italic, Underline, Blink, Inverse, Hidden, Strikethrough, Wide Char, Spacer, Wrapped）。
    - `Cell` & `Row`（支援 CJK 雙寬字元標記、Damage Tracking / Dirty Lines 標記）。
    - `ScreenBuffer`（Primary 支援 Scrollback 歷史行緩衝、Alternate 專供全螢幕 TUI、DECSTBM 滾動區域、Line/Char Insert/Delete、Erase Display/Line）。
    - `ScreenGrid`（雙緩衝管理、活動緩衝區切換）。
  - [x] `skterm-core::vt`:
    - 完整實作 `vte::Perform` trait。
    - 游標定位（CUP, HVP, CUU, CUD, CUF, CUB, CNL, CPL, CHA, VPA）。
    - 清除指令（ED, EL, ECH, DCH, ICH, DL, IL）。
    - SGR 樣式與色彩解析（16 色、256 色、24-bit TrueColor RGB 支援 `;` 與 `:` 參數格式）。
    - DEC 私有模式（Alternate Screen `?1049h/l`、光標顯示 `?25h/l`、Bracketed Paste `?2004h/l`、滑鼠回報協定）。
    - ESC 序列（DECSC/DECRC 游標儲存/復原、RI、IND、NEL、RIS 重設）。
    - OSC 序列（OSC 0/2 視窗標題、OSC 8 點擊超連結）。
  - [x] `skterm-core::pty`:
    - 跨平台 PTY 子行程管理（Windows ConPTY + Unix POSIX openpty）。
    - 預設 Shell 自動探測（`$SHELL` / `/bin/zsh` / `/bin/bash` / `powershell.exe`）。
    - 背景非同步讀取執行緒與 Crossbeam MPSC 通道。
    - 執行緒安全寫入器與 PTY Master 尺寸即時同步。
  - [x] `skterm-core::terminal`:
    - 整合 Terminal 高階實例（結合 PTY, VTE Parser, ScreenGrid）。
    - 支援 Headless 模式（便於自動化測試）。
  - [x] 單元測試：7 項核心單元測試 + 2 項 Workspace 測試全部 PASS。
  - [x] 整合範例：`cargo run --example headless_pty -p skterm-core` 實際啟動 Shell 並成功寫入/解析/渲染至 Grid。

---

## 2. 下一步行動 (Next Steps - Phase 2)
在使用者驗收並確認 Phase 1 之後：
1. Commit `feat/p1-terminal-core` 並合併至 `main` 分支。
2. 切換至新分支 `feat/p2-wgpu-renderer`。
3. 實作 `skterm-render` 與視窗渲染管線：
   - 建立 `winit` 視窗並初始化 `wgpu`（支援 Metal / DX12 / Vulkan）。
   - 實作 Glyph Texture Atlas 與字型排版（支援等寬字、CJK 與 Emoji）。
   - 實作 WGSL 著色器（背景色塊 Instanced Quads、文字頂點與紋理採樣、光標與選取高亮）。
   - 結合 `skterm-core::Terminal` 達成 60/120 FPS 原生終端開窗顯示。

---

## 3. 跨 Session 驗證指令
```bash
# 執行所有單元測試
cargo test --workspace

# 執行真實 PTY Shell 互動輸出測試
cargo run --example headless_pty -p skterm-core
```
