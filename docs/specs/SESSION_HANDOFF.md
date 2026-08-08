# Session Handoff & 開發進度總表

## 1. 當前專案狀態 (Current Status)
* **當前分支**：`main` (已完全整合並通過所有測試)
* **當前階段**：**偏好設定系統 (Preferences & Configuration System)**
* **已完成項目**：
  - [x] `Phase 0 ~ Phase 6`: 核心終端機、GPU 渲染管線、輸入與 IME、多窗格分頁、Sixel/Kitty 圖像協定與發行打包已全數合併至 `main`。
  - [x] **偏好設定系統 (Preferences & Configuration)**：
    - `[font]`：主字型家族、字型大小 (pt)、行高、粗體/斜體自訂與 CJK 多重回退字型搜尋清單。
    - `[window]`：初始欄數 (`columns = 80`)、行數 (`rows = 24`)、邊距 (`padding_x`, `padding_y`)、背景不透明度 (`opacity`)、啟動模式 (`startup_mode`)、標題列 (`decorations`) 與分頁列顯示策略 (`tab_bar_style`)。
    - `[terminal]`：`term` (`TERM` 變數)、`shell` (自訂程式與參數)、初始工作目錄 (`working_directory`)、`scrollback_lines` 歷史行數、自訂環境變數表 (`env`)、macOS Option 鍵模式 (`option_as_alt`)、鈴聲模式 (`bell`)。
    - `[colors]`：6 款內建主題、自訂 Hex 覆蓋值 (`foreground`, `background`, `cursor`, `selection`) 與自訂 16 色 ANSI 調色盤 (`palette`)。
    - `[cursor]`：游標形狀 (`shape`)、閃爍開關 (`blink`) 與閃爍間隔毫秒數 (`blink_interval_ms`)。
    - `[keybindings]`：自訂快捷鍵對應。
    - **圖形化偏好設定視窗 (Preferences GUI)**：在終端機內按下 `Cmd+,`（macOS）或 `Ctrl+Shift+,`（Linux/Windows）直接呼叫內建 GUI 介面（Appearance、Colors、Terminal、Cursor 四大分頁），並提供一鍵儲存、重設預設值與開啟 TOML 檔案。
    - **原生字型搜尋與解析引擎**：自動搜尋使用者與系統字型庫，支援複合 Nerd Fonts / CJK 混合鏈與即時動態更換字型。
    - **快捷鍵規範分流**：macOS 採用 `Cmd` 觸發視窗與窗格分割（`Cmd+D` 左右分割、`Cmd+Shift+D` 上下分割），`Ctrl+D` 忠實作為 EOF 傳入 Shell；Linux/Windows 採用 `Ctrl+Shift+<Key>`，完全避免熱鍵衝突。
    - **字型動態縮放**：支援 `Cmd+=`（放大）、`Cmd+-`（縮小）、`Cmd+0`（還原）。
    - **即時熱重載 (Hot-Reload)**：背景檔案監聽，存檔後即時無重啟套用外觀與主題色彩。
  - [x] 單元測試：全部 **30/30 PASS**（100% 成功，0 Warning，0 Error）。

---

## 2. 驗證指令
```bash
# 執行所有單元測試與相容性測試
cargo test --workspace

# 啟動原生終端並測試偏好設定與熱重載
cargo run -p skterm-app

# 測試打包 macOS App Bundle
./scripts/package_macos.sh
```
