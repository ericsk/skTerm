# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p2-wgpu-renderer`
* **當前階段**：**Phase 2 (wgpu GPU 渲染管線、Glyph Atlas 與原生開窗顯示)**
* **已完成項目**：
  - [x] `Phase 0`: Workspace 結構、4 個核心 crates、`docs/specs/` 全部 9 份規格文件建立並已合併至 `main`。
  - [x] `Phase 1`: PTY 抽象、VT100/xterm 狀態機、ScreenGrid 雙緩衝與單元測試建立並已合併至 `main`。
  - [x] `skterm-render::atlas`:
    - 基於 `fontdue` 的極速字型光柵化引擎。
    - 2048x2048 2D 動態 Glyph Texture Atlas 與 Shelf Packing 演算法。
    - 支援 macOS (`Menlo.ttc`, `Monaco.ttf`), Linux (`DejaVuSansMono.ttf`), Windows (`consola.ttf`) 系統等寬字型自動載入。
    - 即時單元格尺寸與 Baseline 運算。
  - [x] `skterm-render::shader`:
    - 撰寫高效 WGSL 著色器，支援 Instanced Quads（背景色塊與文字頂點）。
    - 支援 Alpha Blending、反白樣式與 24-bit TrueColor/256 色/16 色即時色彩映射。
  - [x] `skterm-render::pipeline`:
    - `TerminalPipeline` 實作（背景 Quad 管線、文字 Glyph 採樣管線、Uniform 緩衝區與 Instance 緩衝區）。
  - [x] `skterm-render::renderer`:
    - `TermRenderer` 封裝 `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface<'static>`，支援自動 VSync 與 Damage Tracking。
  - [x] `skterm-gui::input`:
    - 鍵盤事件編碼器（Enter, Backspace, Tab, Escape, 方向鍵, Home/End, F1-F12, Ctrl+Letter 組合鍵, 常用文字與 IME 輸入）。
  - [x] `skterm-gui::app_window`:
    - 基於 `winit 0.30` 的 `ApplicationHandler` 視窗生命週期調度。
    - 視窗縮放自動計算 Grid 網格維度（Cols, Rows）並同步至 PTY Master。
    - 幀率調度與事件迴圈：PTY 有新輸出或 Grid Dirty 時觸發 Redraw。
  - [x] `skterm-app`:
    - 整合主入口，直接啟動原生 GPU 加速終端應用程式開窗運行。
  - [x] 單元測試：全部 **12/12 PASS**。

---

## 2. 下一步行動 (Next Steps - Phase 3)
在使用者驗收並確認 Phase 2 之後：
1. Commit `feat/p2-wgpu-renderer` 並合併至 `main` 分支。
2. 切換至新分支 `feat/p3-input-and-config`。
3. 實作 Phase 3：現代互動與設定系統：
   - 支援滑鼠反白選取文字（字元/單詞/整行）與邊界高亮渲染。
   - 整合 `arboard` 實作跨平台剪貼簿複製 (`Cmd/Ctrl+C`) 與貼上 (`Cmd/Ctrl+V`, Bracketed Paste)。
   - 支援 Kitty Keyboard Protocol（無歧義按鍵與釋放事件）。
   - 實作 TOML 設定檔系統（`config.toml`）與 `notify` 熱重載（即時變更字型大小、主題色彩與透明度）。

---

## 3. 跨 Session 驗證指令
```bash
# 執行所有單元測試
cargo test --workspace

# 執行真實原生 GPU 終端開窗應用程式（可在 macOS/Windows/Linux 開窗互動並輸入指令）
cargo run -p skterm-app
```
