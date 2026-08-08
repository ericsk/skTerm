# skTerm ⚡

<div align="center">

**跨平台原生高效能 GPU 加速終端機**  
*Next-Generation High-Performance GPU-Accelerated Terminal Emulator in Rust*

[![CI](https://github.com/ericsk/skTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/ericsk/skTerm/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()

</div>

---

## 🌟 核心特色 (Key Features)

- 🚀 **極致效能與 GPU 渲染 (GPU-Accelerated Rendering)**：
  - 基於 `wgpu 24.0` 跨平台原生硬體加速（macOS Metal / Windows DirectX 12 & Vulkan / Linux Vulkan）。
  - 動態 2D Glyph Texture Atlas 快取與極速冷啟動（< 15ms），實測吞吐量超過 **1,111,225 行/秒 (102.57 MB/s)**。
- 🖥️ **完整 VT100 / ANSI / xterm / 24-bit TrueColor 相容**：
  - 雙緩衝架構（Primary Buffer with 10,000+ 行 Scrollback + Alternate Screen Buffer）。
  - 完整支援 16 色、256 色與 24-bit TrueColor RGB 配色。
  - 支援 DECSTBM 自訂滾動區域、游標形狀切換（Block █ / Beam ❘ / Underline  ）與 OSC 8 點擊超連結。
- 🌏 **完善 CJK 與國際化支援 (First-Class CJK & IME Support)**：
  - 智慧多重回退字型引擎（自動搜尋 `~/Library/Fonts`, `/Library/Fonts`, `/usr/share/fonts` 等）。
  - 支援注音、拼音、倉頡等原生中文輸入法（IME），候選字浮動視窗精準跟隨游標。
  - CJK 雙寬字元（Double-Width）與 Emoji 完美對齊排版。
- 🪟 **多分頁與 BSP 空間分割窗格 (Tabs & Split Panes Multiplexing)**：
  - 內建二元空間分割樹（BSP Tree），支援任意深度的左右（垂直）與上下（水平）窗格動態切分。
  - 多分頁標籤列（Tab Bar），多窗格焦點邊框高亮與獨立 PTY 執行個體。
  - 智慧 Session 偵測：輸入 `exit` 或按下 `Ctrl+D` 自動關閉窗格/分頁，全數關閉時安全結束程式。
- 🖼️ **進階終端圖像協定 (Sixel & Kitty Graphics)**：
  - **DEC Sixel**：完整點陣光柵解碼與調色盤支援。
  - **Kitty Graphics Protocol**：支援 32-bit RGBA、24-bit RGB 與 PNG 圖像直接由 GPU 紋理管線渲染。
- ⚙️ **圖形化偏好設定 GUI 介面與即時熱重載 (Preferences GUI & Hot Reload)**：
  - 內建原生 GPU 渲染偏好設定視窗（Preferences Modal），按 `Cmd+,`（`Ctrl+Shift+,`）即可直接呼叫 GUI 介面進行視覺化設定。
  - 支援外觀字型、色彩主題、模擬器 Shell、游標動態即時設定並一鍵儲存套用，同步支援 TOML 文字編輯器開啟。
  - 修改設定檔無需重啟，存檔即時熱更新外觀。

---

## ⌨️ 快捷鍵指南 (Keyboard Shortcuts)

| 快捷鍵 (macOS) | 快捷鍵 (Linux/Windows) | 說明 |
| :--- | :--- | :--- |
| `Cmd + ,` | `Ctrl + Shift + ,` | **開啟偏好設定 GUI 視窗 (Open Preferences GUI)** |
| `Cmd + =` / `Cmd + +` | `Ctrl + =` | **放大字型大小 (Zoom In Font Size)** |
| `Cmd + -` | `Ctrl + -` | **縮小字型大小 (Zoom Out Font Size)** |
| `Cmd + 0` | `Ctrl + 0` | **重設字型大小 (Reset Font Size)** |
| `Cmd + T` | `Ctrl + Shift + T` | **新增分頁 (New Tab)** |
| `Cmd + W` | `Ctrl + Shift + W` | **關閉當前窗格/分頁 (Close Pane/Tab)** |
| `Cmd + D` | `Ctrl + Shift + D` | **垂直切分窗格 (左右分割)** |
| `Cmd + Shift + D` | `Ctrl + Shift + H` | **水平切分窗格 (上下分割)** |
| `Cmd + ]` / `Cmd + [` | `Ctrl + ]` / `Ctrl + [` | **切換焦點窗格 (Next/Prev Pane)** |
| `Cmd + Shift + ]` / `[` | `Ctrl + Tab` / `Ctrl + Shift + Tab` | **切換分頁 (Next/Prev Tab)** |
| `Cmd + 1` ~ `9` | `Ctrl + 1` ~ `9` | **直接切換至第 1 ~ 9 個分頁** |
| `Cmd + C` | `Ctrl + Shift + C` | **複製選取文字至剪貼簿** |
| `Cmd + V` | `Ctrl + Shift + V` | **貼上剪貼簿文字 (支援 Bracketed Paste)** |
| `Ctrl + D` | `Ctrl + D` | **發送 EOF 訊號 (登出/結束程式)** |
| `Ctrl + C` | `Ctrl + C` | **發送 SIGINT 訊號 (中斷程式)** |
| `滑鼠滾輪` | `滑鼠滾輪` | **流暢滾動檢視歷史緩衝區 (Scrollback)** |
| `滑鼠左鍵拖曳` | `滑鼠左鍵拖曳` | **文字反白選取 (雙擊選詞，三擊選行)** |
| `滑鼠右鍵點擊` | `滑鼠右鍵點擊` | **快速貼上剪貼簿內容** |

---

## 🛠️ 建置與執行 (Build & Run)

### 系統依賴要求
- **Rust Toolchain**: 1.80+ (`cargo`, `rustc`)

### 開發與測試
```bash
# 執行全部 30 項單元測試與相容性測試
cargo test --workspace

# 啟動 skTerm 終端機
cargo run -p skterm-app

# 執行吞吐量效能測試基準 (Benchmark)
cargo run --release --example benchmark -p skterm-app
```

### 跨平台發行打包 (Packaging)
```bash
# macOS App Bundle (產出 dist/macos/skTerm.app)
./scripts/package_macos.sh

# Linux .deb 安裝包 (產出 dist/linux/skterm_0.1.0_amd64.deb)
./scripts/package_linux.sh

# Windows 可攜式壓縮包 (PowerShell)
.\scripts\package_windows.ps1
```

---

## ⚙️ 設定檔範例 (`~/.config/skterm/config.toml`)

```toml
[font]
# 主要等寬字型家族名稱 (支援逗號分隔複合字型鏈)
family = "NotoSansMNerdFontMono, NotoSansTC, Menlo"
# 字型大小 (pt 單位)
size = 16.0
# 行高倍率
line_height = 1.2
# 自訂 CJK 多重回退字型搜尋清單 (選填)
fallback_families = ["Hiragino Sans GB", "PingFang TC", "Noto Sans CJK TC"]

[window]
# 初始終端幾何大小 (欄數與行數)
columns = 80
rows = 24
# 視窗邊距 (單位: pt)
padding_x = 8.0
padding_y = 8.0
# 視窗背景不透明度 (0.1 ~ 1.0)
opacity = 1.0
# 啟動視窗模式 ("normal", "maximized", "fullscreen")
startup_mode = "normal"
# 標題列與視窗外觀 ("full", "transparent", "none")
decorations = "full"
# 分頁標籤列顯示策略 ("auto", "always", "never")
tab_bar_style = "auto"

[terminal]
# 終端模擬器 TERM 環境變數 (預設: "xterm-256color")
term = "xterm-256color"
# 自訂預設 Shell 與啟動參數 (選填)
# shell = { program = "/bin/zsh", args = ["--login"] }
# 初始工作目錄 (選填)
# working_directory = "~"
# 歷史滾動緩衝區行數
scrollback_lines = 10000
# 自訂環境變數表 (選填)
# [terminal.env]
# COLORTERM = "truecolor"
# EDITOR = "vim"
# macOS Option 鍵行為 ("both", "only_left", "only_right", "none")
option_as_alt = "both"
# 終端鈴聲模式 ("visual", "sound", "none")
bell = "visual"

[colors]
# 內建色彩主題: "catppuccin-mocha", "tokyo-night", "dracula", "solarized-dark", "nord", "default-dark"
theme = "catppuccin-mocha"
# 自訂色彩覆蓋 (選填 Hex 碼)
# foreground = "#cdd6f4"
# background = "#1e1e2e"
# cursor     = "#f5e0dc"
# selection  = "#585b70"

[cursor]
# 游標形狀: "block" (█), "beam" (❘), "underline" (_)
shape = "block"
# 游標閃爍開關
blink = true
# 閃爍頻率 (毫秒)
blink_interval_ms = 500
```

---

## 🏛️ 架構設計 (Architecture)

```
skTerm Workspace
├── crates/skterm-core    # VT100 狀態機、字元網格、PTY 管理、雙緩衝與圖像解碼
├── crates/skterm-render  # wgpu 24.0 管線、2D 字型圖集、Sixel/Kitty GPU 渲染
├── crates/skterm-gui     # winit 事件迴圈、BSP 窗格切分、IME 處理與 egui 偏好設定
└── crates/skterm-app     # 應用程式進入點與效能測試基準 (Benchmark)
```

---

## 📄 開源授權 (License)

本專案採用雙重開源授權 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)。
