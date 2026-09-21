# 08. 相容性測試與效能驗證規格 (Testing & Compatibility)

## 1. 自動化測試體系 (`cargo test --workspace`)
- **全 Workspace 共 105 項自動化測試（100% 綠燈通過）**：
  - **`skterm-core`（27 單元測試 + 12 VT 相容性整合測試）**：
    - `ScreenGrid` 雙緩衝切換、歷史滾動、字元寫入、動態 Reflow（保留 Hard Prompt 行與窗格分割保護）。
    - **零配置行回收池 (`row_recycling`)**：驗證 `scroll_up`、`DECSTBM` 區域滾動、`IL`/`DL` 插入刪除行及無 scrollback 模式下的 `Row` 緩衝區循環重用與背景色填充。
    - `vte` 解析器各跳脫序列（CSI, OSC, SGR, DECSTBM, DEC Private Modes）與 zsh `PROMPT_SP` 清空畫面相容性 (`test_vt_zsh_prompt_sp_clear`)。
    - 全緩衝區搜尋 (`search_scrollback_and_screen`)、`Cmd+K` 清空 (`clear_scrollback_and_screen`)、OSC 8 與純文字 URL 自動偵測、前景行程與 OSC 7 CWD 查詢 (`foreground_process`)。
  - **`skterm-gui`（35 單元測試）**：
    - BSP 窗格切分、最大化縮放 (`Pane Zoom`)、分隔線拖曳調整 (`Splitter Drag`)、分頁拖曳排序與直達索引 (`Cmd/Ctrl + 1~9`)。
    - **智慧分頁標題 (`tab_title`)**：驗證前景程式顯示、路徑縮寫、隱藏目錄首字元保留與家目錄 `~` 邊界處理。
    - **原子化設定檔並發寫入 (`config_persistence`)**：多執行緒高頻並發寫入與讀取驗證，確保絕不出現半成品檔案或損毀 TOML。
    - 跨平台視窗圖示與 Linux Wayland/X11 `app_id`/`WM_CLASS` 設定、捲軸拖曳互動。
  - **`skterm-render`（30 單元測試 + 1 CJK 整合測試）**：
    - **全域 AI 面板幾何與點擊判定 (`agent_panel`)**：16 項針對 `AgentPanelLayout` 單一來源座標、頂部與分頁列齊高、拖曳把手、按鈕與卡片點擊區域的嚴密測試。
    - **智慧字體與字重選擇 (`atlas`)**：驗證多樣式字型家族中的 Bold/Italic/Regular 精準匹配與寬度變體過濾。
    - 程序化無縫框線與半高方塊 (`box_drawing`)、捲軸幾何 (`ScrollbarMetrics`) 與 CJK 字型光柵化測試 (`test_cjk_font`)。
- **跨平台 CI 自動化驗證 (GitHub Actions)**：
  - Matrix 矩陣涵蓋 **macOS (`macos-latest`)**、**Ubuntu Linux (`ubuntu-latest`)** 與 **Windows (`windows-latest`)**。

---

## 2. 效能基準與 Release 編譯最佳化 (Benchmarks & Release Profile)
- **Zero-Allocation Scrolling & Idle-Power Loop**：
  - 滾動時零 Heap 記憶體配置；閒置時 `ControlFlow::Wait` 達成 0.0% CPU 佔用率。
- **Release Profile 編譯最佳化 (`Cargo.toml`)**：
  - 啟用 `lto = "thin"` 跨模組連結時最佳化、`codegen-units = 1`、`panic = "abort"` 與 `strip = true`，大幅縮減執行檔體積並提升指令吞吐量。
- **Throughput & Latency 目標**：
  - 處理 `cat` 巨量文字串流時，每秒解析吞吐量 > 500 MB/s；端到端按鍵輸入至 GPU Present 延遲 < 5ms。
