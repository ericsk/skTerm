# 06. 終端圖像協定規格 (Graphics Protocol: Sixel & Kitty)

## 1. 協定範疇與支援
- **Sixel Graphics Protocol**：
  - 傳統 DEC Sixel 圖像標準，支援 256 色調色盤與點陣圖解碼。
  - 將 Sixel 位元流即時解碼為 RGBA 點陣圖並綁定為 GPU 紋理。
- **Kitty Graphics Protocol**：
  - 完整攔截標準 **APC (`ESC _ G ... ST`)** 與相容 **OSC (`ESC ] G ... ST`)** 控制序列（支援跨讀取緩衝區串流切分）。
  - **傳輸媒介與壓縮**：支援直傳 (`t=d`)、本機一般檔案 (`t=f`)、安全暫存檔 (`t=t`，嚴格驗證暫存目錄與 `tty-graphics-protocol` 命名)、POSIX 共享記憶體 (`t=s`)、分塊重組 (`m=1`/`m=0`)、偏移量與長度讀取 (`O=`/`S=`) 及 Zlib 解壓縮 (`o=z`)。
  - **像素格式**：支援 32-bit RGBA (`f=32`)、24-bit RGB (`f=24`) 與 PNG/JPEG (`f=100`)。
  - **指令動作**：
    - `a=q`：能力探測與錯誤回報（遵循 `q=0`/`q=1`/`q=2` 靜音旗標）。
    - `a=t` / `a=T` / `a=p`：分離圖像儲存庫（`StoredImage`，具備 320 MiB LRU 配額）與畫面擺放實例（`GraphicImage`），支援 ID (`i=`)、編號 (`I=`)、Placement ID (`p=`)、裁切區域 (`x,y,w,h`)、子格偏移 (`X,Y`)、欄列等比縮放 (`c,r`)、游標策略 (`C=0`/`C=1`) 及相對父圖定位 (`P,Q,H,V`)。
    - `a=d`：支援 `a/A`、`i/I`、`n/N`、`c/C`、`p/P`、`q/Q`、`r/R`、`x/X`、`y/Y`、`z/Z`、`f/F` 等小寫（僅移除畫面位置）與大寫（連同釋放未引用紋理）刪除選擇器。
    - `a=f` / `a=c` / `a=a`：支援動畫影格建立、Alpha 合成/覆寫與目標影格切換。
    - **Unicode Placeholders (`U=1`, `U+10EEEE`)**：支援 297 個標準組合變音符號（`KITTY_DIACRITICS`）編碼之列/欄/MSB 瓦片座標與省略延續模式，相容 `tmux`、`yazi`、`snacks.nvim` 等 TUI 工具。

---

## 2. GPU 紋理生命週期、分層與網格對齊 (Grid Alignment & Layering)
- **網格與像素同步**：視窗與分割窗格縮放時同步更新 `TIOCSWINSZ` (`ws_xpixel`, `ws_ypixel`) 及 `XTWINOPS` (`CSI 14 t` / `CSI 16 t`)。
- **捲動與窗格裁切**：圖像座標支援負列索引進入 `scrollback`，隨捲軸 (`scroll_offset`) 平滑捲動並透過 WGSL UV 邊界裁切避免超出分割窗格；超出 `max_scrollback` 時自動淘汰，GPU `image_cache` 亦具備 256 MiB LRU 回收機制。
- **Z-index 分層渲染**：`z < 0` 繪製於終端文字下方（背景之上），`z >= 0` 與 Unicode Placeholders 繪製於終端文字上方、浮動選單與搜尋列下方。
