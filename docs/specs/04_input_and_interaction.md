# 04. 輸入系統、互動選取與剪貼簿規格 (Input & Interaction)

## 1. 鍵盤編碼系統 (Keyboard Encoding)
- **標準 ANSI / VT100 / XTerm 模式**：
  - 方向鍵、功能鍵（F1–F12）、Home/End、PageUp/PageDown 的 CSI 跳脫碼生成。
  - **Backtab / 反向 Tab 鍵 (`Shift + Tab`)**：依標準發送 `\x1b[Z`（CSI Z / Cursor Backward Tabulation），完整支援 Shell/TUI 反向補全與選項切換。
  - **複合修飾鍵跳脫序列 (xterm Modifiers)**：支援 `Shift` (2)、`Alt` (3)、`Ctrl` (5) 及其組合修飾鍵對方向鍵與導航鍵的編碼（例如 `Shift+Up` -> `\x1b[1;2A`、`Ctrl+Left` -> `\x1b[1;5D` 跳詞）。
  - Backspace (ASCII 0x7F / 0x08) 與 Delete (CSI 3 ~) 處理。
- **Kitty Keyboard Protocol 支援**：
  - 支援無歧義按鍵編碼、按鍵釋放（Key Release）與重複事件捕捉。
  - 完整修飾鍵支援（Ctrl, Alt, Shift, Super / Cmd）。

---

## 2. 滑鼠與互動選取 (Mouse & Selection)
- **選取模式**：
  - 單擊拖曳：字元級別選取。
  - 雙擊：單詞選取（Word selection）。
  - 三擊：整行選取（Line selection）。
  - `Alt` + 拖曳：矩形區塊選取（Block selection）。
- **剪貼簿整合**：
  - 透過 `arboard` 實現跨平台原生剪貼簿讀寫。
  - 支援 Bracketed Paste Mode (`ESC [ ? 2004 h`)，避免黏貼程式碼時意外觸發終端自動執行。
- **OSC 8 超連結**：
  - 滑鼠懸停（Hover）顯示下劃線與連結網址提示。
  - `Cmd / Ctrl + Click` 自動調用系統預設瀏覽器開啟網址。

---

## 3. 滑鼠滾輪與 macOS 系統捲動連動 (Mouse Wheel & macOS Natural Scrolling Sync)
- **系統方向自動連動 (Natural Scrolling Sync)**：
  - `ScreenBuffer::scroll_display` 與 `winit::event::MouseScrollDelta` 遵循 macOS AppKit / 系統「自然捲動 (Natural Scrolling)」設定：`delta > 0` 向上捲動檢視歷史紀錄 (`scrollback`)，`delta < 0` 向下捲動回到目前提示字元。
- **觸控板與 Magic Mouse 像素級平滑累積 (`scroll_pixel_accumulator`)**：
  - 針對 macOS 觸控板高頻發送的微幅 `MouseScrollDelta::PixelDelta` 事件，於 `TerminalSession` 累積未滿單一字元高度 (`cell_height`) 的像素位移，消除慢速與平滑滑動時因整數截斷造成的失靈問題。
- **Alternate Screen 與 Mouse Reporting 自動轉發**：
  - 當 TUI 程式（如 `vim`、`tmux`、`htop`）啟用滑鼠回報協定 (`MouseProtocol::Normal` / `ButtonEvent` / `AnyEvent`) 時，自動將滾輪事件編碼為 SGR (`\x1b[<64;col;rowM` / `\x1b[<65;col;rowM`) 或 X10 格式發送至 PTY。
  - 當處於 Alternate Screen 且未開啟滑鼠回報（如 `less`、`man`）時，自動將滾輪事件轉換為上下方向鍵（`\x1b[A`/`\x1b[B` 或 `\x1bOA`/`\x1bOB`）傳送至 PTY。
