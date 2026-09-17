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
- **零配置行回收池 (`recycled_rows` / Zero-Allocation Row Recycling)**：
  - 當發生向上捲動 (`scroll_up`)、區域捲動 (`DECSTBM`) 或插入/刪除行 (`IL` / `DL`) 時，被擠出畫面或 Scrollback 上限的 `Row` 緩衝區 (`Vec<Cell>`) 會被回收至 `recycled_rows` 池中重複利用（清空內容並重設為當前背景色），徹底消除高吞吐量文字輸出（如 `cat` 大檔案或編譯日誌）時的 Heap 記憶體頻繁配置與釋放。
- **游標狀態 (Cursor)**：
  - `point: (usize, usize)`：當前 (row, col)。
  - `shape: CursorShape`（Block, Beam, Underline）。
  - `visible: bool`：游標可見性。
  - `saved_cursor: SavedCursor`：ANSI `ESC 7` / `ESC 8` 儲存與復原之游標狀態。
- **滾動區域 (DECSTBM)**：
  - `scroll_region: Range<usize>`：限制特定行區間滾動（用於 status bar 等固定區域）。

---

## 2. Reflow（視窗尺寸調整動態換行）演算法
當使用者調整視窗尺寸或分割窗格導致欄寬（Cols）變更時：
1. 遍歷包含 `is_wrapped == true` 的邏輯連續行並重組。
2. **保留 Hard Prompt 行（Prompt Line Preservation）**：明確區分因終端寬度折行的軟換行 (`is_wrapped == true`) 與使用者按下 Enter 或 Shell 輸出的硬換行 (`is_wrapped == false`)，確保 Shell Prompt 與獨立指令行在視窗縮放時不會被錯誤合併或吞字。
3. **窗格分割邊界保護**：分割窗格縮減寬度時不會無故將 Scrollback 歷史行拉回可視區覆蓋當前提示字元。
4. 雙寬字元（CJK/Emoji）不可跨邊界截斷；若位於行末最後一格則自動換至下一行開頭。
5. 動態重新計算游標所在的新 (row, col) 座標與 Scrollback 位移。

---

## 3. VT100 延遲自動換行機制 (Deferred Auto-Wrap / Wrap-at-EOL)
為完美支援 `zsh` 的 `PROMPT_SP` 檢查機制與 Unix shell 終端互動：
- **行尾等待換行狀態 (`pending_wrap`)**：當文字剛好填滿一行的最後一欄時，游標停留在最後一欄，並將 `pending_wrap` 設為 `true`。只有當**下一個可列印文字字元**寫入時才觸發真正的換行 (`newline`)。
- **重置旗標條件**：當收到 `\r` (Carriage Return)、游標絕對/相對移動指令 (`H`, `f`, `A`, `B`, `C`, `D`, `G`, `d`)、或是螢幕/行清除指令 (`\x1b[2J`, `\x1b[3J`, `\x1b[K`) 時，**無條件重置 `pending_wrap = false`**，避免提示字元換行錯誤並確保 `clear` 指令後不殘留 `%` 符號。

---

## 4. 全緩衝區搜尋 (`Cmd+F`) 與畫面清除 (`Cmd+K`)
- **全緩衝區即時檢索 (`search`)**：
  - 支援跨 Scrollback 歷史緩衝區與當前可視 ScreenGrid 進行不分大小寫或精確字串搜尋，回傳所有匹配項的絕對行號與欄位區間 `(abs_row, start_col..end_col)`，供渲染器繪製高亮標記並支援上一個/下一個跳轉。
- **清空緩衝區與畫面 (`clear_scrollback_and_screen`)**：
  - 對應 `Cmd + K` 操作，一鍵清空 Scrollback 歷史紀錄並將當前游標所在行移至畫面頂端，保持當前 Shell Prompt 完整可用。

---

## 5. 前景行程偵測與工作目錄追蹤 (Foreground Process & OSC 7 CWD)
- **OSC 7 工作目錄解析**：即時解析 Shell 發送的 `OSC 7 ; file://hostname/path ST` 序列，記錄當前終端工作目錄 (`working_directory`)。
- **系統原生行程樹與前景行程查詢 (`foreground_process`)**：
  - 透過作業系統底層 API（macOS `libproc` / `proc_pidinfo`、Linux `/proc/<pid>/stat` & `cwd`）查詢 PTY 領導行程及其前景子行程名稱與當前 CWD。
  - 供分頁膠囊動態顯示「正在執行的程式 + 縮短路徑」（如 `claude: ~/P/skTerm`）以及新分頁/窗格自動繼承當前工作目錄。
