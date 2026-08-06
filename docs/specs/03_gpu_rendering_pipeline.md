# 03. wgpu GPU 渲染管線與文字排版規格 (GPU Rendering Pipeline)

## 1. 渲染架構與目標
- **後端支援**：macOS (Metal)、Windows (DirectX 12 / Vulkan)、Linux (Vulkan / OpenGL)。
- **目標幀率**：支援 60Hz / 120Hz+ ProMotion / G-Sync。
- **零閒置耗電 (Damage Tracking)**：僅在畫面網格存在 Dirty lines、游標閃爍或動畫時提交 GPU RenderPass。

---

## 2. 渲染階段劃分 (Render Pipeline Passes)

渲染分為三層主要的繪圖流程：

```mermaid
graph TD
    subgraph "Pass 1: Background & Selection Quads"
        BG["Cell Background Quad Pipeline (Instanced)"]
        SEL["Selection Highlight Quad Pipeline"]
    end

    subgraph "Pass 2: Glyph Text & CJK Atlas"
        ATLAS["Dynamic Glyph Texture Atlas (cosmic-text / fontdue)"]
        TEXT["Text Quad Pipeline (Subpixel / Greyscale / RGBA Emoji)"]
    end

    subgraph "Pass 3: Cursor & Decoration Lines"
        CUR["Cursor Quad Pipeline (Block / Beam / Underline)"]
        DEC["Underline & Strikethrough Geometry"]
    end

    BG --> TEXT
    SEL --> TEXT
    ATLAS --> TEXT
    TEXT --> CUR
    CUR --> DEC
```

---

## 3. 動態 Glyph Atlas 設計
- 維護一張尺寸為 2048x2048 的動態 GPU 紋理（`wgpu::TextureFormat::R8Unorm` 為灰階文字，`Rgba8UnormSrgb` 為彩色 Emoji）。
- 利用 LRU（Least Recently Used）演算法管理字元快取。
- 支援 CJK 字元動態回退（Fallback Fonts）與字形連字（Font Ligatures）。
