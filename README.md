# skTerm ⚡

<div align="center">

**Next-Generation High-Performance GPU-Accelerated Terminal Emulator in Rust**

[![CI](https://github.com/ericsk/skTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/ericsk/skTerm/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()

</div>

---

## 🌟 Key Features

- 🚀 **GPU-Accelerated Rendering**:
  - Cross-platform native hardware acceleration powered by `wgpu 24.0` (macOS Metal / Windows DirectX 12 & Vulkan / Linux Vulkan).
  - Dynamic 2D Glyph Texture Atlas caching with ultra-fast cold startup (< 15ms) and benchmarked throughput exceeding **1,111,225 lines/sec (102.57 MB/s)**.
- 🖥️ **Full VT100 / ANSI / xterm / 24-bit TrueColor Compatibility**:
  - Dual-buffer architecture (Primary Buffer with 10,000+ lines of Scrollback + Alternate Screen Buffer).
  - Full support for 16-color, 256-color, and 24-bit TrueColor RGB palettes.
  - Supports DECSTBM custom scroll regions, cursor shape switching (Block `█` / Beam `❘` / Underline `_`), and OSC 8 clickable hyperlinks.
- 🌏 **First-Class CJK & IME Support**:
  - Smart multi-fallback font engine (automatically discovers fonts in `~/Library/Fonts`, `/Library/Fonts`, `/usr/share/fonts`, etc.).
  - Full Input Method Editor (IME) support for Zhuyin, Pinyin, Cangjie, Japanese, Korean, and more, with candidate windows accurately tracking the cursor position.
  - Pixel-perfect alignment for CJK double-width characters and Emojis.
- 🪟 **Tabs & Split Panes Multiplexing**:
  - Built-in Binary Space Partitioning (BSP) tree supporting arbitrary-depth vertical (left/right) and horizontal (top/bottom) dynamic pane splitting.
  - Interactive mouse splitter dragging, keyboard shortcuts for fine-tuning pane ratios, one-key pane equalization, and full-screen pane zooming (`[Z]`).
  - Multi-tab bar with dynamic tab reordering (Move Tab Left/Right) and isolated PTY instances per session.
  - Smart session detection: typing `exit` or pressing `Ctrl+D` automatically closes the active pane or tab, and cleanly exits the application when the last session closes.
- 🔗 **Clickable Hyperlinks & URL Auto-Detection**:
  - Supports standard **OSC 8** escape sequence hyperlinks alongside real-time plain-text URL auto-detection (`https://`, `http://`, `www.`, etc.).
  - Hold `Cmd` (macOS) or `Ctrl` (Linux/Windows) while hovering to display a pointer cursor, and click to open links directly in your default browser.
- 🖼️ **Advanced Terminal Graphics Protocols (Sixel & Kitty Graphics)**:
  - **DEC Sixel**: Full bitmap raster decoding and palette support.
  - **Kitty Graphics Protocol**: Direct GPU texture pipeline rendering for 32-bit RGBA, 24-bit RGB, and PNG images.
- ⚙️ **Preferences GUI & Hot Reload**:
  - Built-in native GPU-rendered Preferences Modal (`Cmd+,` or `Ctrl+Shift+,`) for interactive visual configuration.
  - Live configuration of fonts, color themes, shell programs, and cursor styles with one-click save & apply, plus quick access to open the configuration in your external TOML editor.
  - Automatic hot-reloading applies configuration file changes immediately upon saving without restarting the terminal.

---

## ⌨️ Keyboard Shortcuts

| Shortcut (macOS) | Shortcut (Linux/Windows) | Description |
| :--- | :--- | :--- |
| `Cmd + ,` | `Ctrl + Shift + ,` | **Open Preferences GUI Modal** |
| `Cmd + =` / `Cmd + +` | `Ctrl + =` | **Zoom In Font Size** |
| `Cmd + -` | `Ctrl + -` | **Zoom Out Font Size** |
| `Cmd + 0` | `Ctrl + 0` | **Reset Font Size** |
| `Cmd + T` | `Ctrl + Shift + T` | **New Tab** |
| `Cmd + W` | `Ctrl + Shift + W` | **Close Current Pane / Tab** |
| `Cmd + D` | `Ctrl + Shift + D` | **Split Pane Vertically (Left / Right)** |
| `Cmd + Shift + D` | `Ctrl + Shift + H` | **Split Pane Horizontally (Top / Bottom)** |
| `Cmd + Shift + Enter` / `Z` | `Ctrl + Shift + Enter` | **Toggle Pane Maximize / Zoom** |
| `Cmd + Option + =` | `Ctrl + Alt + =` | **Equalize All Pane Sizes** |
| `Cmd + Option + Arrow Keys` | `Ctrl + Shift + Alt + Arrow Keys` | **Resize Current Pane Ratio** |
| `Cmd + Option + [` / `]` | `Ctrl + Shift + Alt + [` / `]` | **Move Current Tab Left / Right** |
| `Cmd + ]` / `Cmd + [` | `Alt + ]` / `Alt + [` | **Focus Next / Previous Pane** |
| `Cmd + Shift + ]` / `[` | `Ctrl + Shift + ]` / `[` (or `Ctrl + Tab` / `Ctrl + Shift + Tab`) | **Switch to Next / Previous Tab** |
| `Cmd + 1` ~ `9` | `Ctrl + 1` ~ `9` | **Switch Directly to Tab 1 ~ 9** |
| `Cmd + C` | `Ctrl + Shift + C` | **Copy Selected Text to Clipboard** |
| `Cmd + V` | `Ctrl + Shift + V` | **Paste Clipboard Text (Supports Bracketed Paste)** |
| `Cmd + Left Click` | `Ctrl + Left Click` | **Open Hyperlink (OSC 8 / URL) in Default Browser** |
| `Mouse Drag Splitter` | `Mouse Drag Splitter` | **Drag Split Dividers to Resize Panes** |
| `Ctrl + D` | `Ctrl + D` | **Send EOF Signal (Logout / Terminate Session)** |
| `Ctrl + C` | `Ctrl + C` | **Send SIGINT Signal (Interrupt Process)** |
| `Mouse Scroll Wheel` | `Mouse Scroll Wheel` | **Smooth Scroll Through Scrollback Buffer** |
| `Mouse Left Drag` | `Mouse Left Drag` | **Select Text (Double-Click for Word, Triple-Click for Line)** |
| `Mouse Right Click` | `Mouse Right Click` | **Quick Paste Clipboard Content** |

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
# macOS App Bundle (outputs dist/macos/skTerm.app)
./scripts/package_macos.sh

# Linux .deb Package (outputs dist/linux/skterm_0.1.0_amd64.deb)
./scripts/package_linux.sh

# Windows Portable Archive (PowerShell)
.\scripts\package_windows.ps1
```

---

## ⚙️ Configuration Example (`~/.config/skterm/config.toml`)

```toml
[font]
# Primary monospace font family (supports comma-separated fallback chains)
family = "NotoSansMNerdFontMono, NotoSansTC, Menlo"
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

[colors]
# Built-in color themes: "catppuccin-mocha", "tokyo-night", "dracula", "solarized-dark", "nord", "default-dark"
theme = "catppuccin-mocha"
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
├── crates/skterm-core    # VT100 state machine, character grid, PTY management, dual buffer & graphics decoding
├── crates/skterm-render  # wgpu 24.0 pipeline, 2D glyph texture atlas, Sixel/Kitty GPU rendering
├── crates/skterm-gui     # winit event loop, BSP split panes, IME handling & egui preferences UI
└── crates/skterm-app     # Application binary entry point & performance benchmarks
```

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
