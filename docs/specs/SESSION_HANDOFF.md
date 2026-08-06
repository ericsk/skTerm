# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p5-graphics-protocol`
* **當前階段**：**Phase 5 (進階終端圖像協定 Sixel & Kitty Graphics Protocols)**
* **已完成項目**：
  - [x] `Phase 0`: Workspace 結構、4 個核心 crates、`docs/specs/` 全部 9 份規格文件建立並已合併至 `main`。
  - [x] `Phase 1`: PTY 抽象、VT100/xterm 狀態機、ScreenGrid 雙緩衝與單元測試建立並已合併至 `main`。
  - [x] `Phase 2`: wgpu GPU 渲染管線、Glyph Atlas 與原生開窗顯示建立並已合併至 `main`。
  - [x] `Phase 3`: 輸入系統、選取、剪貼簿、TOML 設定熱重載、CJK 字型回退與 Retina 16pt HiDPI 縮放建立並已合併至 `main`。
  - [x] `Phase 4`: 多分頁與 BSP 窗格分割、快捷鍵切換、Session exit 自動關閉建立並已合併至 `main`。
  - [x] `Phase 5`:
    - `skterm-core::graphics`:
      - **DEC Sixel 圖像解碼器 (`SixelDecoder`)**：支援調色盤定義（RGB/HLS）、重複像素計數 `!N`、六像素縱向位元帶解析與 RGBA 點陣圖解碼。
      - **Kitty Graphics 圖像協定解析器 (`KittyGraphicsParser`)**：支援 APC `G...;payload` 控制參數、多塊分段傳輸（`m=0/1`）、32-bit RGBA、24-bit RGB 與 PNG 格式解碼。
      - **終端影像實體 (`GraphicImage`)**：支援圖像錨定於單元格 `(row, col)`，並在螢幕滾動時自動同步偏移。
    - `skterm-render::renderer` & `pipeline`:
      - WGSL 圖像著色器（`ImageInstance` / `vs_image` / `fs_image`）。
      - 動態 GPU Image Texture 快取與 BindGroup 管理，每張終端圖形直接由 GPU 紋理管線渲染。
    - 提供測試範例：
      - `cargo run -p skterm-app --example sixel_image`
      - `cargo run -p skterm-app --example kitty_image`
  - [x] 單元測試：全部 **20/20 PASS**。

---

## 2. 下一步行動 (Next Steps - Phase 6)
在使用者驗收並確認 Phase 5 之後：
1. Commit `feat/p5-graphics-protocol` 並合併至 `main` 分支。
2. 切換至新分支 `feat/p6-packaging-and-bench`。
3. 實作 Phase 6：終端標準相容性驗證、效能基準測試與跨平台原生發行封裝：
   - 整合標準終端測試套件（`esctest` / `vttest`）與 ANSI/VT100 邊界測試。
   - 效能優化與高吞吐量壓力測試（百萬行文字湧入吞吐量最佳化）。
   - 跨平台打包建置腳本（macOS `.app` / `dmg`，Linux `.deb` / AppImage，Windows `.msi` / zip）。

---

## 3. 跨 Session 驗證指令
```bash
# 執行所有單元測試（包含 Sixel 與 Kitty 解碼測試）
cargo test --workspace

# 啟動終端應用程式
cargo run -p skterm-app

# 在終端內部或測試執行 Sixel 與 Kitty 圖像輸出
cargo run -p skterm-app --example sixel_image
cargo run -p skterm-app --example kitty_image
```
