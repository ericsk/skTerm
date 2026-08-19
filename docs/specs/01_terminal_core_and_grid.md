# 01. 終端核心與 Screen Grid 規格 (Terminal Core & Grid)

## 1. 資料結構設計

### Cell (單元格)
每個 Cell 代表終端螢幕網格上的一個字元單元：
- `c: char`：儲存的 Unicode 字元（如 `'a'`, `'中'`, `'🚀'`）。雙寬字元的後半部標記為虛擬延展格。
- `fg: Color`：前景色（支援 Default, Named(16), Indexed(256), Rgb(r,g,b)）。
- `bg: Color`：背景色。
- `flags: CellFlags`：樣式旗標位元組（Bold, Dim, Italic, Underline, Blink, Inverse, Hidden, Strikethrough）。
- `width: u8`：顯示寬度（0: 連接字/Zero-width, 1: 標準英數半形, 2: CJK 雙寬/Emoji）。
- `hyperlink: Option<Arc<Hyperlink>>`：OSC 8 超連結參照。

### Row (行緩衝區)
- `cells: Vec<Cell>`：固定長度（等於 cols）的單元格陣列。
- `is_wrapped: bool`：標記此行是否因視窗邊界自動折行（用於 Reflow 與文字反白跨行複製）。
- `is_dirty: bool`：標記此行自上次渲染後是否有被修改（Damage Tracking）。

### ScreenGrid (螢幕網格)
- **雙緩衝設計**：
  - `primary`: 支援歷史滾動（Scrollback Buffer，預設 10,000 行）的主畫面。
  - `alternate`: 專供全螢幕 TUI 程式（`vim`, `tmux`, `htop`, `less`）使用的獨立備用畫面（無 scrollback）。
- **游標狀態 (Cursor)**：
  - `point: (usize, usize)`：當前 (row, col)。
  - `shape: CursorShape`（Block, Beam, Underline）。
  - `visible: bool`：游標可見性。
  - `saved_cursor: SavedCursor`：ANSI `ESC 7` / `ESC 8` 儲存與復原之游標狀態。
- **滾動區域 (DECSTBM)**：
  - `scroll_region: Range<usize>`：限制特定行區間滾動（用於 status bar 等固定區域）。

---

## 2. Reflow（視窗尺寸調整換行）演算法
當使用者調整視窗尺寸（Cols 變更）：
1. 遍歷包含 `is_wrapped == true` 的邏輯行。
2. 根據新的 `new_cols` 重新計算字元折行點。
3. 雙寬字元（CJK/Emoji）不可跨邊界截斷；若位於行末最後一格則自動換至下一行開頭。
4. 動態調整游標與 Scrollback 歷史行位置。

---

## 3. VT100 延遲自動換行機制 (Deferred Auto-Wrap / Wrap-at-EOL)
為完美支援 `zsh` 的 `PROMPT_SP` 檢查機制與 Unix shell 終端互動：
- **行尾等待換行狀態 (`pending_wrap`)**：當文字剛好填滿一行的最後一欄時，游標停留在最後一欄，並將 `pending_wrap` 設為 `true`。只有當**下一個可列印文字字元**寫入時才觸發真正的換行 (`newline`)。
- **重置旗標條件**：當收到 `\r` (Carriage Return)、游標絕對/相對移動指令 (`H`, `f`, `A`, `B`, `C`, `D`, `G`, `d`)、或是螢幕/行清除指令 (`\x1b[2J`, `\x1b[3J`, `\x1b[K`) 時，**無條件重置 `pending_wrap = false`**，避免提示字元換行錯誤並確保 `clear` 指令後不殘留 `%` 符號。
