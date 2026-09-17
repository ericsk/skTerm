# 07. 設定檔系統與配色主題規格 (Configuration & Theming)

## 1. 設定檔格式與路徑 (TOML)
- **路徑**：
  - macOS: `~/Library/Application Support/skTerm/config.toml` 或 `~/.config/skterm/config.toml`
  - Linux: `~/.config/skterm/config.toml`
  - Windows: `%APPDATA%\skTerm\config.toml`

### 設定項範例 (`config.toml`)
```toml
[font]
family = "JetBrains Mono, Menlo, PingFang TC"   # 支援 VSCode 風格逗號分隔字體回退串列
size = 16.0
line_height = 1.2

[window]
columns = 80
rows = 24
padding_x = 8.0
padding_y = 8.0
opacity = 1.0
startup_mode = "normal"      # "normal" | "maximized" | "fullscreen"
decorations = "full"         # "full" | "transparent" | "none"
tab_bar_style = "auto"       # "auto" | "always" | "never"

[terminal]
term = "xterm-256color"
scrollback_lines = 10000
option_as_alt = "both"       # "both" | "only_left" | "only_right" | "none"
bell = "visual"              # "visual" | "sound" | "none"

[colors]
theme = "pure-black"         # "pure-black" | "catppuccin-mocha" | "tokyo-night" | "dracula" | "solarized-dark" | "nord" | "default-dark"
bold_is_bright = true        # 粗體文字自動提升為高亮色 (ANSI 0..7 -> 8..15，預設前景 -> 純白)
# foreground = "#F5F5F7"     # 自訂覆蓋值 (選填)
# background = "#000000"

[cursor]
shape = "block"              # "block" | "beam" | "underline"
blink = true
blink_interval_ms = 500
```

---

## 2. macOS Sonoma/Sequoia HIG 偏好設定視窗
- **架構設計**：
  - 獨立視窗與 WGPU 繪圖管線，按下 `Cmd + ,` 或系統選單呼叫。
  - 左側 `190pt` 分類導航欄（字型與外觀、色彩主題、Shell 與終端、游標樣式、快捷鍵），整合連線狀態圓點與設定檔存取。
  - 右側採用 Inset Grouped Table Cards 樣式，包含 macOS 原生感翠綠色 Toggle Switch (`#30D158`)、Segmented Control 分段切換按鈕、Keyboard Badges 膠囊徽章。
- **VSCode 風格字體串列與即時解析預覽 (Font Family List & Live Preview)**：
  - 支援輸入逗號分隔的多字體回退清單，下方即時顯示系統實際匹配到的各順位字型檔案名稱（Live Resolution Preview）與系統已安裝字型自動補全選單。
- **一鍵主題卡片畫廊 (1-Click Theme Gallery)**：
  - 直接展示 6 大精緻主題預覽卡片，每張卡片附帶主題名稱、代表色 5 色圓點、微光 Hover 與 macOS 系統藍高亮選取邊框 (`#0A84FF`)，一鍵點擊 0 毫秒即時套用。
- **純黑預設主題 (Pure Black OLED) 與 `bold_is_bright`**：
  - 預設主題為 `"pure-black"`，底色為純黑 `#000000`，搭配亮白 `#F5F5F7` 前景字體、鮮豔 16 色 ANSI 調色盤與 `bold_is_bright` 粗體自動高亮對比強化。
- **自訂背景與文字顏色覆蓋**：
  - 支援全功能調色盤選色器、Hex 碼輸入，以及快速一鍵色點（純黑、深墨黑、亮白、駭客綠、琥珀黃等）。

---

## 3. 視窗尺寸自動記住與原子化存檔 (Debounced Geometry Auto-Save & Atomic Write)
- 使用者拖拉視窗改變大小 (`WindowEvent::Resized`) 時，系統即時重新計算行列數 (`columns` × `rows`)。
- 拖拉停止後 400ms 或視窗關閉時，在背景執行緒自動存入 `config.toml`。
- **原子化檔案寫入 (Atomic File Persistence)**：
  - 所有設定檔寫入均先寫入同目錄下的唯一暫存檔（`config.toml.tmp.<pid>.<counter>`）並執行 `sync_all()`，再透過作業系統原子級 `std::fs::rename` 覆蓋 `config.toml`。
  - 徹底杜絕多執行緒並發存檔（如視窗縮放防抖與偏好設定同時存檔）或熱重載讀取時讀到截斷的半成品檔案而損毀設定。

---

## 4. 熱重載 (Hot Reload) 與零延遲即時同步 (Zero-Latency Live Sync)
- 透過 `notify` crate 背景監聽設定檔變更。
- **回聲事件過濾 (Echo Hot-Reload Filter)**：自動存檔時比對設定快照 (`old_cfg != *reloaded_cfg`) 與字型變更 (`font_changed`)，避免切換主題時誤觸發 WGPU Shader 與字形快取重建，維持 60 FPS 流暢切換。
