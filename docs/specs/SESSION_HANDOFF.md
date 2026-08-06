# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`feat/p0-init-workspace`
* **當前階段**：**Phase 0 (專案基底與 Spec 體系初始化)**
* **已完成項目**：
  - [x] 初始化 Git 儲存庫與 `.gitignore`。
  - [x] 建立 Cargo Workspace 結構（`skterm-core`, `skterm-render`, `skterm-gui`, `skterm-app`）。
  - [x] 建立並編寫 `docs/specs/` 全部 9 份架構與規格文件。
  - [x] 驗證 `cargo check --workspace` 編譯通過。
  - [x] 驗證 `cargo test --workspace` 通過。

---

## 2. 下一步行動 (Next Steps - Phase 1)
在使用者驗收並確認 Phase 0 之後：
1. 合併 `feat/p0-init-workspace` 至 `main` 分支。
2. 切換至新分支 `feat/p1-terminal-core`。
3. 開始實作 `skterm-core` 核心：
   - `pty.rs`：封裝 `portable-pty` 實現跨平台 PTY 子行程抽象與非同步讀寫通道。
   - `grid.rs`：實作 `Cell`、`Row`、`ScreenGrid`（雙緩衝、Scrollback、CJK 雙寬字支援、Damage Tracking）。
   - `vt.rs`：實作 `vte::Perform` 解碼器（游標控制、SGR 16/256/TrueColor、清屏、滾動區）。

---

## 3. 跨 Session 驗證指令
```bash
# 驗證所有模組編譯與依賴解析
cargo check --workspace

# 執行所有單元測試
cargo test --workspace

# 執行應用程式基本輸出
cargo run -p skterm-app
```
