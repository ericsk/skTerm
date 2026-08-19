# 07. 設定檔系統與配色主題規格 (Configuration & Theming)

## 1. 設定檔格式與路徑 (TOML)
- **路徑**：
  - macOS: `~/Library/Application Support/skTerm/config.toml` 或 `~/.config/skterm/config.toml`
  - Linux: `~/.config/skterm/config.toml`
  - Windows: `%APPDATA%\skTerm\config.toml`

### 設定項範例 (`config.toml`)
```toml
[font]
family = "JetBrains Mono"
size = 14.0
line_height = 1.2

[window]
opacity = 0.95
blur = true
padding = { x = 8.0, y = 8.0 }

[colors]
theme = "pure-black" # "pure-black" | "catppuccin-mocha" | "tokyo-night" | "dracula" | "solarized-dark" | "nord" | "default-dark"

[cursor]
shape = "block" # "block" | "beam" | "underline"
blink = true
```

---

## 2. macOS Sonoma/Sequoia HIG 偏好設定視窗
- **架構設計**：
  - 獨立視窗與 WGPU 繪圖管線，按下 `Cmd + ,` 或系統選單呼叫。
  - 左側 `190pt` 分類導航欄（字型與外觀、色彩主題、Shell 與終端、游標樣式、快捷鍵），整合連線狀態圓點與設定檔存取。
  - 右側採用 Inset Grouped Table Cards 樣式，包含 macOS 原生感翠綠色 Toggle Switch (`#30D158`)、Segmented Control 分段切換按鈕、Keyboard Badges 膠囊徽章。
- **一鍵主題卡片畫廊 (1-Click Theme Gallery)**：
  - 直接展示 6 大精緻主題預覽卡片，每張卡片附帶主題名稱、代表色 5 色圓點、微光 Hover 與 macOS 系統藍高亮選取邊框 (`#0A84FF`)，一鍵點擊 0 毫秒即時套用。
- **純黑預設主題 (Pure Black OLED)**：
  - 預設主題改為 `"pure-black"`，底色為純黑 `#000000`，搭配亮白 `#F5F5F7` 前景字體與專屬鮮豔 16 色 ANSI 調色盤。
- **自訂背景與文字顏色覆蓋**：
  - 支援全功能調色盤選色器、Hex 碼輸入，以及快速一鍵色點（純黑、深墨黑、亮白、駭客綠、琥珀黃等）。

---

## 3. 視窗尺寸自動記住 (Debounced Geometry Auto-Save)
- 使用者拖拉視窗改變大小 (`WindowEvent::Resized`) 時，系統即時重新計算行列數 (`columns` × `rows`)。
- 拖拉停止後 400ms 或視窗關閉時，在背景執行緒自動存入 `config.toml`。
- 下次啟動應用程式時，精準維持前一次調整好的視窗實體大小。

---

## 4. 熱重載 (Hot Reload) 與零延遲即時同步 (Zero-Latency Live Sync)
- 透過 `notify` crate 背景監聽設定檔變更。
- **回聲事件過濾 (Echo Hot-Reload Filter)**：自動存檔時比對設定快照 (`old_cfg != *reloaded_cfg`) 與字型變更 (`font_changed`)，避免切換主題時誤觸發 WGPU Shader 與字形快取重建，維持 60 FPS 流暢切換。
