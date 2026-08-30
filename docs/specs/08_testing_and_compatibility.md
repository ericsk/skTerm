# 08. 相容性測試與效能驗證規格 (Testing & Compatibility)

## 1. 自動化測試體系
- **單元測試 (`cargo test`)**：
  - `ScreenGrid` 雙緩衝切換、歷史滾動、字元寫入與 Reflow。
  - `vte` 解析器各跳脫序列（CSI, OSC, SGR, DECSTBM）之狀態變更驗證。
  - VT100 延遲換行 (`pending_wrap`)、`\r` Carriage Return 與 zsh `PROMPT_SP` 清空畫面相容性測試 (`test_vt_zsh_prompt_sp_clear`)。
  - Unicode / CJK 雙寬字元與零寬字元之寬度演算法。
- **跨平台 CI 自動化驗證 (GitHub Actions)**：
  - Matrix 矩陣涵蓋 **macOS (`macos-latest`)**、**Ubuntu Linux (`ubuntu-latest`)** 與 **Windows (`windows-latest`)**。
  - 整合字型光柵化跨平台路徑測試 (`test_cjk_rasterize`) 與 headless 容錯。
  - 包含 Shift+Tab (Backtab `\x1b[Z`) 與修飾鍵跳脫碼驗證，全數 34/34 單元與相容性測試於三大平台 100% 綠燈通過。

---

## 2. 效能基準標準 (Benchmarks)
- **Throughput Benchmark**：
  - 目標：處理 `cat` 巨量文字串流時，每秒解析吞吐量 > 500 MB/s。
- **Input-to-Screen Latency**：
  - 目標：端到端按鍵輸入至 GPU Present 延遲 < 5ms。
