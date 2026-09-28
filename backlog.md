# skTerm VT100 / xterm / xterm-256color 相容性體檢與開發順序 (Backlog)

本文件記錄 `skTerm` 針對 VT100、xterm 與 `xterm-256color` 控制碼的完整體檢結果與開發優先順序。

---

## ✅ 已完成項目 (Completed)

- [x] **SGR `58` / `59` 底線色彩與 ITU T.416 冒號子參數解析 (`crates/skterm-core/src/vt.rs`)**
  - 支援 `58;2;r;g;b`、`58;5;idx`、`58:2::r:g:b`、`38:2::r:g:b`、`48:2::r:g:b`、`4:0..5`，避免子參數外洩誤觸 `DIM` 或高亮度前景／背景色。
- [x] **文字裝飾渲染與 sRGB Gamma 修正 (`crates/skterm-render/src/renderer.rs`)**
  - 支援 `UNDERLINE`、`STRIKETHROUGH`、`DIM`、`HIDDEN` 渲染（含空白字元底線），並選用非 sRGB surface format 避免重複 Gamma 編碼導致色彩泛白。
- [x] **CSI SGR Intermediates 過濾 (`crates/skterm-core/src/vt.rs`)**
  - 避免 `xterm` 的 `modifyOtherKeys` 控制序列（`\x1b[>4;2m` / `\x1b[>4m`）在進入或離開 TUI 時誤開 `UNDERLINE` 屬性。

---

## 🔴 第一階段：高優先級 (High Priority — 立即實作)

現代 CLI / TUI（如 `jetski`、`neovim`、`tmux`、`fzf`、`htop`、`fish`、`zsh`）最常觸發、影響啟動速度與版面正確性的核心控制碼：

1. [x] **終端機狀態與屬性查詢回報機制 (Terminal Device Queries & Responses)**
   - 在 `TerminalState` 建立 `pending_responses: Vec<u8>` 回應佇列，並於 `Terminal::poll_pty` 自動將回應寫回子行程 PTY（同時提供 `Terminal::take_pending_responses` 供 headless 測試驗證）。
   - **DA1 (`CSI c` / `CSI 0 c`)**：回傳 `\x1b[?62;4;22c`（VT220 Level 2、支援 Sixel 與 ANSI 色彩）。
   - **DA2 (`CSI > c` / `CSI > 0 c`)**：回傳 `\x1b[>41;350;0c`（xterm 次設備屬性）。
   - **DSR (`CSI 5 n`)**：回傳 `\x1b[0n`（Terminal OK）。
   - **CPR (`CSI 6 n` / `CSI ? 6 n`)**：回傳 `\x1b[{row};{col}R` / `\x1b[?{row};{col}R`（1-indexed 游標座標）。
   - **DECRQM (`CSI ? Ps $ p` / `CSI Ps $ p`)**：回傳 `\x1b[?{Ps};{Pm}$y` / `\x1b[{Ps};{Pm}$y`（回報 `?1`、`?7`、`?25`、`?47`/`?1049`、`?1000`、`?1002`、`?1003`、`?1006`、`?2004`、`?2026` 與 ANSI mode `4` 的啟用狀態）。
   - **OSC 10 / 11 / 12 (`\x1b]10;?\x07` 等)**：回報預設前景色、背景色與游標色彩（`rgb:RRRR/GGGG/BBBB`），供 TUI 自動偵測深淺色主題。
   - **Kitty Graphics Query (`a=q`)**：支援查詢指令不污染畫面圖片列表，並於帶有 `i=<id>` 時回傳 `\x1b_Gi=<id>;OK\x1b\\`。

2. [x] **CSI `b` (REP — Repeat Preceding Graphic Character)**
   - 記錄最後一次 `print` 的字元 `last_printed_char`，收到 `CSI Ps b` 時重複輸出該字元 `Ps` 次（符合 `xterm-256color` terminfo `rep=\E[%p1%db`，解決 `ncurses` / `tmux` 畫連續空白或框線缺字問題）。

3. [x] **全面 CSI Intermediates 防護與常用游標／Tab 定位控制碼補齊**
   - 為 `csi_dispatch` 所有標準 CSI 指令（`H`, `f`, `A`..`G`, `d`, `@`, `P`, `X`, `L`, `M`, `S`, `T`, `J`, `K`, `I`, `Z`, `` ` ``, `a`, `e`, `b`）加上 `intermediates.is_empty()` 檢查，徹底杜絕私有序列（`>`、`?`、`=`、`$`）誤觸標準指令。
   - 補齊 `CSI Ps I` (CHT 向前跳 Tab)、`CSI Ps Z` (CBT 向後跳 Tab)、``CSI Ps ` `` (HPA 水平絕對定位)、`CSI Ps a` (HPR 水平相對移動)、`CSI Ps e` (VPR 垂直相對移動)。
   - 修正 `CSI E` (CNL) 與 `CSI F` (CPL) 重設 `pending_wrap = false`。

4. [x] **捲動時的 BCE (Background Color Erase) 背景色支援**
   - 更新 `scroll_up`、`scroll_down`、`linefeed`、`newline` 與 `ESC D` / `ESC E` / `ESC M`，使新捲入的空白行填入當前 SGR 背景色 `self.bg`（而非固定 `Color::DefaultBg`），符合 `xterm-256color` 的 `bce` 規範。

---

## 🟡 第二階段：中優先級 (Medium Priority — 後續迭代)

5. [x] **DEC Private Modes 擴充 (`?7` DECAWM 自動換行、`?2026` Synchronized Output)**
   - `?7` (DECAWM)：控制行尾是否設定 `pending_wrap` 自動換行（支援 TUI 安全繪製右下角最後一格）。
   - `?2026` (Synchronized Output)：追蹤批次畫面更新狀態並支援 DECRQM `?2026$p` 查詢。
6. [x] **VT100 特殊繪圖字元集 (DEC Special Graphics `ESC ( 0` / `ESC ( B`)**
   - 支援 G0/G1 字元集切換與 `0x0E` (SO) / `0x0F` (SI)，將 DEC Special Graphics 模式的 `0x60..=0x7E` 映射為對應的 Unicode 框線與符號字元（解決傳統 `ncurses` / `dialog` / `tmux` ACS 框線顯示為 `qqqqxxxx` 的問題）。
7. [x] **獨立底線色彩儲存與進階底線樣式 (`4:2` 雙底線、`4:3` 波浪底線、`4:4` 點狀底線、`4:5` 虛線底線、SGR `53`/`55` Overline)**
   - 利用 `Cell` 內原有的 2 bytes 對齊空間儲存 `underline_color_id: Option<NonZeroU16>`（維持 `size_of::<Cell>() == 20` 零額外記憶體開銷），並於 `skterm-render` 實作雙底線、LSP 波浪底線、點狀／虛線底線、Overline 與獨立底線色彩渲染。
8. [x] **DEC Origin Mode (`?6` DECOM) 與 Focus Event (`?1004`)**
   - 支援 `?6h`/`?6l`（使 `CSI H`/`f`/`d` 與 `CPR` 相對於 `scroll_region` 定位並限制於捲動區內），以及 `?1004h`/`?1004l` 視窗焦點切換時發送 `\x1b[I` / `\x1b[O`。

---

## 🟢 第三階段：低優先級 (Low Priority — 進階擴充協定)

9. [x] **進階鍵盤協定 (Kitty Keyboard Protocol `CSI > Ps u` / `CSI < u` / `CSI = u` / `CSI ? u` & xterm `modifyOtherKeys`)**
   - 支援無歧義組合鍵編碼（如 `Shift+Enter`、`Ctrl+Shift+Letter`、`Ctrl+;`、Kitty `CSI u` 與 xterm `27;mod;code~`），並在離開 Alternate Screen 時自動清理堆疊狀態。
10. [x] **動態調色盤 OSC 控制碼 (`OSC 4`、`OSC 104`、`OSC 10`～`12` 設定與 `OSC 110`～`112` 重設)**
    - 支援執行期動態修改、查詢（`?`）與重設 ANSI 256 色盤及預設前景／背景／游標色彩（支援 `#RRGGBB` 與 `rgb:RRRR/GGGG/BBBB` 格式）。
11. [x] **自訂 Tab 停駐點 (`ESC H` HTS、`CSI g` TBC)**
    - 支援 `ESC H` 於當前欄設定水平 Tab 停駐點、`CSI 0 g` 清除當前欄停駐點、`CSI 3 g` 清除全部停駐點，並與 `\t`、`CSI I` (CHT)、`CSI Z` (CBT) 完整整合。

---

## 🟣 第四階段：端到端渲染整合與 `xterm-256color` 深度相容 (Phase 4 — End-to-End Integration & Extended xterm-256color)

12. [x] **動態調色盤與主題色 (`OSC 4/104/10..12/110..112`) GPU 渲染端到端串接 (`skterm-render` & `skterm-gui`)**
    - 將 `TerminalState` 的 `dynamic_palette`、`dynamic_fg`、`dynamic_bg`、`dynamic_cursor` 傳遞至 `PaneRenderItem` 與 `TermRenderer`，使執行期透過 OSC 動態修改的色盤、背景色、前景色與游標色彩即時反映於 GPU 畫面渲染。
13. [x] **Synchronized Output (`?2026h` / `?2026l`) 事件迴圈批次繪圖延遲與安全逾時 (`skterm-core` & `skterm-gui`)**
    - 記錄 `synchronized_output_since` 時間戳記並設定 100ms 安全逾時上限；當活動窗格處於 `?2026h` 批次更新期間時暫緩中間幀重繪，待收到 `?2026l` 或逾時到達後立即觸發完整幀渲染，消除大型 TUI 畫面撕裂。
14. [x] **DCS 進階狀態與 Terminfo 能力查詢 (`XTGETTCAP` `DCS + q`、`DECRQSS` `DCS $ q`) 及 Sixel DCS 隔離**
    - 修正 `hook` 僅在 `intermediates.is_empty()` 時進入 Sixel 模式，避免 `DCS + q` 與 `DCS $ q` 誤觸 Sixel 解碼。
    - 實作 `XTGETTCAP` (`\x1bP+q<hex>\x1b\\`) 回報 `TN`/`name` (`xterm-256color`)、`Tc`/`RGB`、`colors`/`Co` (`256`)、`Smulx`、`Setulc`、`sync` 等能力，未知能力回傳 `0+r`。
    - 實作 `DECRQSS` (`\x1bP$q...\x1b\\`) 支援 `m` (SGR)、`r` (DECSTBM)、` q` (DECSCUSR) 狀態回報。
15. [x] **`DECSTR` 軟重設 (`CSI ! p`)、`XTVERSION` (`CSI > q`) 與 `XTWINOPS` 視窗操作 (`CSI Ps t`)**
    - 實作 `xterm-256color` `is2`/`rs2` 必備的 `CSI ! p` (DECSTR 軟重設)。
    - 實作 `CSI > q` (`XTVERSION`) 回報 `\x1b>|skTerm(0.1.0)\x1b\\`。
    - 實作 `CSI 18 t`（字元行列數回報）、`CSI 14 t` / `CSI 16 t`（像素尺寸回報）及 `CSI 22 t` / `CSI 23 t`（視窗標題堆疊 Push/Pop）。
16. [x] **`xterm-256color` 完整按鍵編碼補齊 (`skterm-gui/src/input.rs`)**
    - 在 `?1h` (`DECCKM` Application Cursor Keys) 模式下，`Home` / `End` 發送 `\x1bOH` / `\x1bOF`（符合 `xterm-256color` 的 `khome=\EOH`、`kend=\EOF`）。
    - 支援 `F1..F12` 搭配 `Shift` / `Alt` / `Ctrl` 修飾鍵編碼（`\x1b[1;{m}P..S` 與 `\x1b[15..24;{m}~`，對應 `kf13..kf63`）。
    - 支援 `Ctrl+Space` / `Ctrl+@` (`0x00`)、`Ctrl+/` / `Ctrl+_` (`0x1f`)、`Ctrl+[` (`0x1b`)、`Ctrl+\` (`0x1c`)、`Ctrl+]` (`0x1d`)、`Ctrl+^` (`0x1e`)、`Ctrl+?` (`0x7f`) 等常用控制字元組合。

