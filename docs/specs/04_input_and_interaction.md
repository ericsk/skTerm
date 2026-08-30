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
