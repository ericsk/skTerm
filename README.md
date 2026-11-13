# skTerm ⚡

<div align="center">

**High-Performance GPU-Accelerated Terminal Emulator in Rust (`wgpu`)**

[![CI](https://github.com/ericsk/skTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/ericsk/skTerm/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()

</div>

---

## 🌟 Key Features

- 🚀 **GPU-Accelerated Rendering**:
  - Cross-platform native hardware acceleration powered by `wgpu 30.0` (macOS Metal / Windows DirectX 12 & Vulkan / Linux Vulkan).
  - Dynamic 2D Glyph Texture Atlas caching with fast startup and benchmarked VT/grid throughput of **1,199,159 lines/sec (110.69 MB/s)** (`cargo run --release --example benchmark -p skterm-app`).
- 🖥️ **Full VT100 / ANSI / xterm / 24-bit TrueColor Compatibility**:
  - Dual-buffer architecture (Primary Buffer with 10,000+ lines of Scrollback + Alternate Screen Buffer).
  - Full support for 16-color, 256-color, and 24-bit TrueColor RGB palettes.
  - Supports DECSTBM custom scroll regions, cursor shape switching (Block `█` / Beam `❘` / Underline `_`), scrollback search (`Find`), and OSC 8 clickable hyperlinks.
- 🌏 **First-Class CJK & Inline IME Support**:
  - Progressive multi-fallback font engine with OpenType variable font weight support and centered wide-glyph / Emoji / Powerline cell metrics.
  - Native inline inverse-video Input Method Editor (IME) preedit rendering for Zhuyin, Pinyin, Cangjie, Japanese, Korean, and more, accurately tracking the cursor position.
- 🪟 **Tabs & BSP Split Panes Multiplexing**:
  - Built-in Binary Space Partitioning (BSP) tree supporting arbitrary-depth horizontal (left/right) and vertical (top/bottom) dynamic pane splitting.
  - Interactive mouse splitter dragging, keyboard shortcuts for fine-tuning pane ratios, one-key pane equalization, and full-screen pane zooming.
  - Multi-tab and multi-window support with dynamic tab reordering, window position persistence, and isolated PTY instances per pane.
  - Smart session exit: closing the last pane closes the tab/window cleanly.
- 🤖 **AI Agents Sidebar & `OSC 7501` Protocol**:
  - Global floating AI Agents panel (`Cmd+Shift+A` / `Ctrl+Shift+A`) tracking running AI CLI sessions across all tabs and windows.
  - Inline task label renaming (`Cmd+Shift+R` / `Ctrl+Shift+R`) and live lifecycle status integration via the `OSC 7501` protocol.
- 🖼️ **Advanced Terminal Graphics Protocols (Kitty & DEC Sixel)**:
  - **Kitty Graphics Protocol**: Direct GPU texture pipeline rendering for 32-bit RGBA, 24-bit RGB, and PNG images.
  - **DEC Sixel**: Full bitmap raster decoding and palette support.
- 🔗 **Clickable Hyperlinks & URL Auto-Detection**:
  - Supports standard **OSC 8** escape sequence hyperlinks alongside real-time plain-text URL auto-detection (`https://`, `http://`, `www.`, etc.).
  - Hold `Cmd` (macOS) or `Ctrl` (Linux/Windows) while hovering to display a pointer cursor, and click to open links directly in your default browser.
- ⚙️ **Preferences GUI, Hot Reload, i18n & OTA Updates**:
  - Native GPU-rendered Preferences window (`egui 0.37`, `Cmd+,` / `Ctrl+,`) plus native macOS menu bar integration.
  - Built-in themes (`pure-black`, `catppuccin-mocha`, `tokyo-night`, `dracula`, `solarized-dark`, `nord`, `default-dark`) and automatic TOML configuration hot-reloading.
  - Multi-language UI localization for 8 languages (`en`, `zh-TW`, `zh-CN`, `ja`, `ko`, `es`, `fr`, `de`) with automatic OS locale detection.
  - Built-in GitHub Releases OTA updater with SHA-256 verification and atomic in-place updates.

---

## ⌨️ Keyboard Shortcuts

| Shortcut (macOS) | Shortcut (Linux/Windows) | Description |
| :--- | :--- | :--- |
| `Cmd + ,` | `Ctrl + ,` | **Open Preferences GUI** |
| `Cmd + =` / `Cmd + +` | `Ctrl + =` | **Zoom In Font Size** |
| `Cmd + -` | `Ctrl + -` | **Zoom Out Font Size** |
| `Cmd + 0` | `Ctrl + 0` | **Reset Font Size** |
| `Cmd + N` | `Ctrl + Shift + N` | **New Window** |
| `Cmd + T` | `Ctrl + Shift + T` | **New Tab** |
| `Cmd + W` | — | **Close Current Pane / Tab** |
| `Cmd + Shift + W` | `Ctrl + Shift + W` | **Close Window** |
| `Cmd + D` | `Ctrl + Shift + D` | **Split Pane Left / Right** |
| `Cmd + Shift + D` | `Ctrl + Shift + H` | **Split Pane Top / Bottom** |
| `Cmd + Shift + A` | `Ctrl + Shift + A` | **Toggle AI Agents Sidebar** |
| `Cmd + Shift + R` | `Ctrl + Shift + R` | **Rename Active Agent / Pane Label** |
| `Cmd + Shift + Enter` / `Cmd + Shift + Z` | `Ctrl + Shift + Enter` / `Ctrl + Shift + Z` | **Toggle Pane Maximize / Zoom** |
| `Cmd + Option + =` | `Ctrl + Alt + =` | **Equalize All Pane Sizes** |
| `Cmd + Option + Arrow Keys` | `Ctrl + Shift + Alt + Arrow Keys` | **Resize Current Pane Ratio** |
| `Cmd + Option + [` / `]` | `Ctrl + Shift + Alt + [` / `]` | **Move Current Tab Left / Right** |
| `Cmd + ]` / `Cmd + [` | `Alt + ]` / `Alt + [` | **Focus Next / Previous Pane** |
| `Cmd + Shift + ]` / `[` | `Ctrl + Shift + ]` / `[` (or `Ctrl + Tab` / `Ctrl + Shift + Tab`) | **Switch to Next / Previous Tab** |
| `Cmd + 1` ~ `9` | `Ctrl + 1` ~ `9` | **Switch Directly to Tab 1 ~ 9** |
| `Cmd + F` | `Ctrl + Shift + F` | **Find in Scrollback** |
| `Cmd + K` | `Ctrl + Shift + K` | **Clear Scrollback & Screen** |
| `Cmd + C` / `Cmd + V` | `Ctrl + Shift + C` / `Ctrl + Shift + V` | **Copy / Paste (Supports Bracketed Paste)** |
| `Cmd + A` | — | **Select All** |
| `Cmd + Left Click` | `Ctrl + Left Click` | **Open Hyperlink (OSC 8 / URL) in Default Browser** |
| `Mouse Drag Splitter` | `Mouse Drag Splitter` | **Drag Split Dividers to Resize Panes** |
| `Mouse Scroll Wheel` | `Mouse Scroll Wheel` | **Smooth Scroll Through Scrollback Buffer** |
| `Mouse Left Drag` | `Mouse Left Drag` | **Select Text (Double-Click for Word, Triple-Click for Line)** |
| `Mouse Right Click` | `Mouse Right Click` | **Context Menu or Quick Paste (Configurable)** |

---

## 🛠️ Build & Run

### Prerequisites
- **Rust Toolchain**: 1.80+ (`cargo`, `rustc`)

### Development & Testing
```bash
# Run all workspace unit tests and compatibility test suites
cargo test --workspace

# Launch the skTerm application
cargo run -p skterm-app

# Run throughput performance benchmark
cargo run --release --example benchmark -p skterm-app
```

### Cross-Platform Packaging
```bash
# macOS App Bundle & DMG Installer (outputs under dist/macos/)
./scripts/package_macos.sh

# Linux .deb Package & Portable Tarball (outputs under dist/linux/)
./scripts/package_linux.sh

# Windows Portable Archive & Installer (PowerShell, outputs under dist\windows\)
.\scripts\package_windows.ps1
```

---

## ⚙️ Configuration Example

Configuration file location:
- **macOS / Linux**: `~/.config/skterm/config.toml`
- **Windows**: `%APPDATA%\skTerm\config.toml`

```toml
[font]
# Primary monospace font family (supports comma-separated fallback chains)
family = "Menlo"
# Font size in points (pt)
size = 16.0
# Line height multiplier
line_height = 1.2
# Custom CJK fallback font search list (optional)
fallback_families = ["Hiragino Sans GB", "PingFang TC", "Noto Sans CJK TC"]

[window]
# Initial terminal geometry (columns and rows)
columns = 80
rows = 24
# Window inner padding in points (pt)
padding_x = 8.0
padding_y = 8.0
# Background opacity (0.1 ~ 1.0)
opacity = 1.0
# Startup window mode ("normal", "maximized", "fullscreen")
startup_mode = "normal"
# Window decorations and title bar style ("full", "transparent", "none")
decorations = "full"
# Tab bar display policy ("auto", "always", "never")
tab_bar_style = "auto"
# UI language ("auto", "en", "zh-TW", "zh-CN", "ja", "ko", "es", "fr", "de")
language = "auto"
# Automatically check GitHub Releases for OTA updates
auto_check_updates = true

[terminal]
# Terminal emulator TERM environment variable (default: "xterm-256color")
term = "xterm-256color"
# Custom default shell program and arguments (optional)
# shell = { program = "/bin/zsh", args = ["--login"] }
# Initial working directory (optional)
# working_directory = "~"
# Scrollback history buffer size in lines
scrollback_lines = 10000
# Custom environment variables map (optional)
# [terminal.env]
# COLORTERM = "truecolor"
# EDITOR = "vim"
# macOS Option key behavior ("both", "only_left", "only_right", "none")
option_as_alt = "both"
# Terminal bell notification mode ("visual", "sound", "none")
bell = "visual"
# Right-click mouse behavior ("menu" or "paste")
right_click_action = "menu"

[colors]
# Built-in themes: "pure-black", "catppuccin-mocha", "tokyo-night", "dracula", "solarized-dark", "nord", "default-dark"
theme = "pure-black"
# Custom color overrides in hex format (optional)
# foreground = "#cdd6f4"
# background = "#1e1e2e"
# cursor     = "#f5e0dc"
# selection  = "#585b70"

[cursor]
# Cursor shape: "block" (█), "beam" (❘), "underline" (_)
shape = "block"
# Toggle cursor blinking
blink = true
# Cursor blink interval in milliseconds
blink_interval_ms = 500
```

---

## 🏛️ Architecture

```
skTerm Workspace
├── crates/skterm-core    # VT100 state machine, character grid, PTY management, OSC 7501, Sixel & Kitty graphics decoding
├── crates/skterm-render  # wgpu 30.0 pipeline, 2D glyph texture atlas, AI agents panel & Sixel/Kitty GPU rendering
├── crates/skterm-gui     # winit event loop, BSP split panes, inline IME, i18n, OTA updater & egui 0.37 preferences UI
└── crates/skterm-app     # Application binary entry point & examples (benchmark, kitty_image, sixel_image)
```

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
