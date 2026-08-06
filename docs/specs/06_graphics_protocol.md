# 06. 終端圖像協定規格 (Graphics Protocol: Sixel & Kitty)

## 1. 協定範疇與支援
- **Sixel Graphics Protocol**：
  - 傳統 DEC Sixel 圖像標準，支援 256 色調色盤與點陣圖解碼。
  - 將 Sixel 位元流即時解碼為 RGBA 點陣圖並綁定為 GPU 紋理。
- **Kitty Graphics Protocol**：
  - 現代高速二進位圖像傳輸協定（支援 direct transmission、f=24/32、z-index、尺寸縮放）。

---

## 2. GPU 紋理生命週期與網格對齊 (Grid Alignment)
- 將圖像錨定至特定的終端單元格座標 `(row, col)`。
- 當終端滾動時，圖像隨文字行同步向上平移；若滾出 Scrollback 則自動釋放 GPU 紋理資源。
