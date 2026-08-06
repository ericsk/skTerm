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
theme = "catppuccin-mocha"

[cursor]
shape = "block" # "block" | "beam" | "underline"
blink = true
```

---

## 2. 熱重載 (Hot Reload)
- 透過 `notify` crate 背景監聽設定檔變更。
- 檔案更新時自動重新解析並透過通道發送至 UI / 渲染管線，無需重啟即可即時套用配色、字型大小與透明度。
