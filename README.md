# skTerm ⚡

> **跨平台原生高效能 GPU 加速終端機 (High-Performance GPU-Accelerated Terminal Emulator in Rust)**

skTerm 是一款以 Rust 全新打造的跨平台原生終端機，兼具極致吞吐效能、現代化多工排版與先進圖像協定支援。

---

## 🌟 核心特色 (Key Features)

- 🚀 **極致效能與 GPU 渲染**：
  - 基於 `wgpu 24.0` 跨平台硬體加速渲染（macOS Metal / Windows DX12 & Vulkan / Linux Vulkan）。
  - 動態 2D Glyph Texture Atlas 快取，實測超過 **110 萬行/秒 (100+ MB/s)** 吞吐量。
- 🖥️ **完整 VT100 / xterm / 24-bit TrueColor 相容**：
  - 雙緩衝架構（Primary Buffer with 10,000 行 Scrollback + Alternate Screen Buffer）。
  - 完整支援 16 色、256 色與 24-bit TrueColor RGB 配色。
  - 支援 DECSTBM 自訂滾動區域、游標形狀切換（Block/Underline/Beam）與 OSC 8 超連結。
- 🌏 **完善 CJK 與國際化支援**：
  - 自動偵測系統字型並提供 CJK Fallback 鏈（PingFang / 冬青黑體 / 華黑 / Noto Sans CJK / 微軟雅黑）。
  - 支援注音、拼音、倉頡等原生中文輸入法（IME），候選字浮動視窗精準跟隨游標。
  - CJK 雙寬字元（Double-Width）與 Emoji 完美對齊排版。
- 🪟 **多分頁與 BSP 空間分割窗格 (Tabs & Split Panes Multiplexing)**：
  - 內建二元空間分割樹（BSP Tree），支援任意深度的左右（垂直）與上下（水平）窗格動態切分。
  - 多分頁標籤列（Tab Bar），多窗格邊框高亮與獨立 PTY 執行個體。
  - 智慧 Session 偵測：輸入 `exit` 自動關閉窗格/分頁，全數關閉時安全結束程式。
- 🖼️ **進階終端圖像協定**：
  - **DEC Sixel**：完整點陣光柵解碼與調色盤支援。
  - **Kitty Graphics Protocol**：支援 32-bit RGBA、24-bit RGB 與 PNG 圖像直接由 GPU 紋理管線渲染。
- 🎨 **TOML 設定檔即時熱重載 (Hot Reload)**：
  - 設定檔位置：`~/.config/skterm/config.toml`。
  - 內建 6 款熱門主題：Catppuccin Mocha、Tokyo Night、Dracula、Solarized Dark、Nord、Default Dark。
  - 修改設定檔無需重啟，即時熱更新主題色彩與邊距。

---

## ⌨️ 快捷鍵指南 (Keyboard Shortcuts)

| 快捷鍵 (macOS) | 快捷鍵 (Linux/Windows) | 說明 |
| :--- | :--- | :--- |
| `Cmd + T` | `Ctrl + Shift + T` | **新增分頁 (New Tab)** |
| `Cmd + W` | `Ctrl + Shift + W` | **關閉當前窗格/分頁 (Close Pane/Tab)** |
| `Cmd + D` | `Ctrl + Shift + O` | **垂直切分窗格 (左右分割)** |
| `Cmd + Shift + D` | `Ctrl + Shift + E` | **水平切分窗格 (上下分割)** |
| `Cmd + ]` / `Cmd + [` | `Ctrl + ]` / `Ctrl + [` | **切換焦點窗格 (Next/Prev Pane)** |
| `Cmd + Shift + ]` / `[` | `Ctrl + Tab` / `Ctrl + Shift + Tab` | **切換分頁 (Next/Prev Tab)** |
| `Cmd + 1` ~ `9` | `Ctrl + 1` ~ `9` | **直接切換至第 1 ~ 9 個分頁** |
| `Cmd + C` | `Ctrl + Shift + C` | **複製選取文字至剪貼簿** |
| `Cmd + V` | `Ctrl + Shift + V` | **貼上剪貼簿文字 (支援 Bracketed Paste)** |
| `滑鼠滾輪` | `滑鼠滾輪` | **流暢滾動檢視歷史緩衝區 (Scrollback)** |
| `滑鼠左鍵拖曳` | `滑鼠左鍵拖曳` | **文字反白選取 (雙擊選詞，三擊選行)** |
| `滑鼠右鍵點擊` | `滑鼠右鍵點擊` | **快速貼上剪貼簿內容** |

---

## 🛠️ 建置與執行 (Build & Run)

### 系統依賴要求
- **Rust Toolchain**: 1.80+ (`cargo`, `rustc`)

### 開發與測試
```bash
# 執行全部 20+ 項單元測試與相容性測試
cargo test --workspace

# 啟動開發模式
cargo run -p skterm-app

# 執行 10 萬行吞吐量效能測試
cargo run --release --example benchmark -p skterm-app
```

### 跨平台打包 (Packaging)
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
family = "Menlo"
size = 16.0
line_height = 1.2

[window]
opacity = 1.0
padding_x = 8.0
padding_y = 8.0

[colors]
# 支援: "catppuccin-mocha", "tokyo-night", "dracula", "solarized-dark", "nord", "default-dark"
theme = "catppuccin-mocha"

[cursor]
shape = "block" # "block", "beam", "underline"
blink = true
```

---

## 🏛️ 架構設計 (Architecture)

```mermaid
graph TD
    App[skterm-app (Entry & App Bundle)] --> GUI[skterm-gui (winit Window, Tabs & Panes Layout, Config)]
    GUI --> Core[skterm-core (vte DEC Parser, ScreenGrid, PTY, Sixel & Kitty Decoders)]
    GUI --> Render[skterm-render (wgpu Pipeline, Glyph Atlas, Shaders, Textures)]
    Render --> Core
```

---

## 📄 授權 (License)

MIT OR Apache-2.0 © skTerm Developers
