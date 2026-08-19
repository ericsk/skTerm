# 08. 相容性測試與效能驗證規格 (Testing & Compatibility)

## 1. 自動化測試體系
- **單元測試 (`cargo test`)**：
  - `ScreenGrid` 雙緩衝切換、歷史滾動、字元寫入與 Reflow。
  - `vte` 解析器各跳脫序列（CSI, OSC, SGR, DECSTBM）之狀態變更驗證。
  - VT100 延遲換行 (`pending_wrap`)、`\r` Carriage Return 與 zsh `PROMPT_SP` 清空畫面相容性測試 (`test_vt_zsh_prompt_sp_clear`)。
  - Unicode / CJK 雙寬字元與零寬字元之寬度演算法。
- **標準測試套件整合 (`esctest` & `vttest`)**：
  - 整合 Python `esctest` 自動化驗證終端狀態與標準 ANSI 規格的一致性。
  - 全數 32/32 單元與相容性測試 100% 通過。

---

## 2. 效能基準標準 (Benchmarks)
- **Throughput Benchmark**：
  - 目標：處理 `cat` 巨量文字串流時，每秒解析吞吐量 > 500 MB/s。
- **Input-to-Screen Latency**：
  - 目標：端到端按鍵輸入至 GPU Present 延遲 < 5ms。
