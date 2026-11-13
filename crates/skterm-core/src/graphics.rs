//! Terminal Graphics Protocols: DEC Sixel and Kitty Graphics Protocol Decoders.

use base64::engine::general_purpose::{
    STANDARD as BASE64_STANDARD, STANDARD_NO_PAD as BASE64_NO_PAD,
};
use base64::Engine;
use flate2::read::ZlibDecoder;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

/// Unicode Private Use Area codepoint used by the Kitty Graphics Protocol for text-grid
/// image placeholders (`U+10EEEE`).
pub const KITTY_PLACEHOLDER_CHAR: char = '\u{10EEEE}';

/// High bit tagging a `Cell::hyperlink_id` (`NonZeroU32`) as packed Kitty Unicode
/// placeholder tile coordinates rather than an OSC 8 hyperlink table entry.
pub const PLACEHOLDER_META_TAG: u32 = 0x8000_0000;

/// Canonical 297 combining marks (`gen/rowcolumn-diacritics.txt` from the Kitty Graphics
/// Protocol specification, derived from Unicode 6.0 CCC=230 non-decomposing marks).
pub const KITTY_DIACRITICS: [u32; 297] = [
    0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D, 0x033E, 0x033F, 0x0346, 0x034A, 0x034B, 0x034C,
    0x0350, 0x0351, 0x0352, 0x0357, 0x035B, 0x0363, 0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
    0x036A, 0x036B, 0x036C, 0x036D, 0x036E, 0x036F, 0x0483, 0x0484, 0x0485, 0x0486, 0x0487, 0x0592,
    0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599, 0x059C, 0x059D, 0x059E, 0x059F, 0x05A0, 0x05A1,
    0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AF, 0x05C4, 0x0610, 0x0611, 0x0612, 0x0613, 0x0614, 0x0615,
    0x0616, 0x0617, 0x0657, 0x0658, 0x0659, 0x065A, 0x065B, 0x065D, 0x065E, 0x06D6, 0x06D7, 0x06D8,
    0x06D9, 0x06DA, 0x06DB, 0x06DC, 0x06DF, 0x06E0, 0x06E1, 0x06E2, 0x06E4, 0x06E7, 0x06E8, 0x06EB,
    0x06EC, 0x0730, 0x0732, 0x0733, 0x0735, 0x0736, 0x073A, 0x073D, 0x073F, 0x0740, 0x0741, 0x0743,
    0x0745, 0x0747, 0x0749, 0x074A, 0x07EB, 0x07EC, 0x07ED, 0x07EE, 0x07EF, 0x07F0, 0x07F1, 0x07F3,
    0x0816, 0x0817, 0x0818, 0x0819, 0x081B, 0x081C, 0x081D, 0x081E, 0x081F, 0x0820, 0x0821, 0x0822,
    0x0823, 0x0825, 0x0826, 0x0827, 0x0829, 0x082A, 0x082B, 0x082C, 0x082D, 0x0951, 0x0953, 0x0954,
    0x0F82, 0x0F83, 0x0F86, 0x0F87, 0x135D, 0x135E, 0x135F, 0x17DD, 0x193A, 0x1A17, 0x1A75, 0x1A76,
    0x1A77, 0x1A78, 0x1A79, 0x1A7A, 0x1A7B, 0x1A7C, 0x1B6B, 0x1B6D, 0x1B6E, 0x1B6F, 0x1B70, 0x1B71,
    0x1B72, 0x1B73, 0x1CD0, 0x1CD1, 0x1CD2, 0x1CDA, 0x1CDB, 0x1CE0, 0x1DC0, 0x1DC1, 0x1DC3, 0x1DC4,
    0x1DC5, 0x1DC6, 0x1DC7, 0x1DC8, 0x1DC9, 0x1DCB, 0x1DCC, 0x1DD1, 0x1DD2, 0x1DD3, 0x1DD4, 0x1DD5,
    0x1DD6, 0x1DD7, 0x1DD8, 0x1DD9, 0x1DDA, 0x1DDB, 0x1DDC, 0x1DDD, 0x1DDE, 0x1DDF, 0x1DE0, 0x1DE1,
    0x1DE2, 0x1DE3, 0x1DE4, 0x1DE5, 0x1DE6, 0x1DFE, 0x20D0, 0x20D1, 0x20D4, 0x20D5, 0x20D6, 0x20D7,
    0x20DB, 0x20DC, 0x20E1, 0x20E7, 0x20E9, 0x20F0, 0x2CEF, 0x2CF0, 0x2CF1, 0x2DE0, 0x2DE1, 0x2DE2,
    0x2DE3, 0x2DE4, 0x2DE5, 0x2DE6, 0x2DE7, 0x2DE8, 0x2DE9, 0x2DEA, 0x2DEB, 0x2DEC, 0x2DED, 0x2DEE,
    0x2DEF, 0x2DF0, 0x2DF1, 0x2DF2, 0x2DF3, 0x2DF4, 0x2DF5, 0x2DF6, 0x2DF7, 0x2DF8, 0x2DF9, 0x2DFA,
    0x2DFB, 0x2DFC, 0x2DFD, 0x2DFE, 0x2DFF, 0xA66F, 0xA67C, 0xA67D, 0xA6F0, 0xA6F1, 0xA8E0, 0xA8E1,
    0xA8E2, 0xA8E3, 0xA8E4, 0xA8E5, 0xA8E6, 0xA8E7, 0xA8E8, 0xA8E9, 0xA8EA, 0xA8EB, 0xA8EC, 0xA8ED,
    0xA8EE, 0xA8EF, 0xA8F0, 0xA8F1, 0xAAB0, 0xAAB2, 0xAAB3, 0xAAB7, 0xAAB8, 0xAABE, 0xAABF, 0xAAC1,
    0xFE20, 0xFE21, 0xFE22, 0xFE23, 0xFE24, 0xFE25, 0xFE26, 0x10A0F, 0x10A38, 0x1D185, 0x1D186,
    0x1D187, 0x1D188, 0x1D189, 0x1D1AA, 0x1D1AB, 0x1D1AC, 0x1D1AD, 0x1D242, 0x1D243, 0x1D244,
];

/// Map a combining diacritic character to its 0-based index in [`KITTY_DIACRITICS`].
#[inline]
pub fn kitty_diacritic_index(c: char) -> Option<u16> {
    KITTY_DIACRITICS
        .binary_search(&(c as u32))
        .ok()
        .map(|idx| idx as u16)
}

/// Tile coordinate metadata packed into `Cell::hyperlink_id` (`NonZeroU32`) for a
/// `U+10EEEE` Kitty Unicode placeholder cell, keeping `Cell` at 20 bytes with zero heap
/// allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaceholderCellMeta {
    /// Number of combining diacritics attached to this placeholder cell (`0..=3`).
    pub diacritic_count: u8,
    /// Decoded row tile index (`0..297`) when `diacritic_count >= 1`.
    pub row_idx: u16,
    /// Decoded column tile index (`0..297`) when `diacritic_count >= 2`.
    pub col_idx: u16,
    /// Most-significant byte (bits 24..31) of the 32-bit `image_id` when `diacritic_count >= 3`.
    pub id_msb: u8,
}

impl PlaceholderCellMeta {
    #[inline]
    pub const fn empty() -> Self {
        Self {
            diacritic_count: 0,
            row_idx: 0,
            col_idx: 0,
            id_msb: 0,
        }
    }

    #[inline]
    pub const fn pack(self) -> NonZeroU32 {
        let raw = PLACEHOLDER_META_TAG
            | (((self.diacritic_count as u32) & 0x3) << 28)
            | ((self.id_msb as u32) << 20)
            | (((self.col_idx as u32) & 0x3FF) << 10)
            | ((self.row_idx as u32) & 0x3FF);
        // SAFETY: `PLACEHOLDER_META_TAG` sets bit 31, so `raw` is always non-zero.
        unsafe { NonZeroU32::new_unchecked(raw) }
    }

    #[inline]
    pub const fn unpack(id: NonZeroU32) -> Option<Self> {
        let raw = id.get();
        if (raw & PLACEHOLDER_META_TAG) == 0 {
            return None;
        }
        Some(Self {
            diacritic_count: ((raw >> 28) & 0x3) as u8,
            id_msb: ((raw >> 20) & 0xFF) as u8,
            col_idx: ((raw >> 10) & 0x3FF) as u16,
            row_idx: (raw & 0x3FF) as u16,
        })
    }

    #[inline]
    pub fn push_diacritic(&mut self, idx: u16) {
        match self.diacritic_count {
            0 => {
                self.row_idx = idx;
                self.diacritic_count = 1;
            }
            1 => {
                self.col_idx = idx;
                self.diacritic_count = 2;
            }
            2 => {
                self.id_msb = (idx & 0xFF) as u8;
                self.diacritic_count = 3;
            }
            _ => {}
        }
    }
}

/// Decoded Graphic Image placement to be rendered in the Terminal Grid.
#[derive(Debug, Clone)]
pub struct GraphicImage {
    /// Unique GPU texture identifier (`StoredImage::texture_id` for Kitty, or monotonic ID for Sixel).
    pub id: u32,
    /// Kitty protocol image ID (`i=`), or `0` for Sixel.
    pub image_id: u32,
    /// Kitty protocol image number (`I=`), or `0` when unspecified.
    pub image_number: u32,
    /// Kitty protocol placement ID (`p=`), or `0` when unnamed.
    pub placement_id: u32,
    /// Anchor row (`>= 0` indexes live `grid[row]`; `< 0` indexes `-row` lines into `scrollback` from newest).
    pub row: isize,
    /// Anchor column (`0..cols`).
    pub col: usize,
    /// Full texture width in pixels.
    pub width_px: u32,
    /// Full texture height in pixels.
    pub height_px: u32,
    /// Source crop rectangle `[x, y, w, h]` in texture pixels.
    pub src_rect: [u32; 4],
    /// Sub-cell pixel offset `[X, Y]` within the anchor cell.
    pub cell_offset: [u32; 2],
    /// Explicit target display width in columns (`c=`), if specified.
    pub display_cols: Option<u32>,
    /// Explicit target display height in rows (`r=`), if specified.
    pub display_rows: Option<u32>,
    /// Occupied width in grid cells.
    pub width_cells: usize,
    /// Occupied height in grid cells.
    pub height_cells: usize,
    /// Shared RGBA8 pixel data (`width_px * height_px * 4` bytes).
    pub rgba_data: Arc<Vec<u8>>,
    /// Layering index (`< 0` behind text, `>= 0` above text).
    pub z_index: i32,
}

impl GraphicImage {
    /// Compute the rendered `[offset_x, offset_y, display_w_px, display_h_px]` relative to the
    /// anchor cell's top-left corner `(col * cell_w, row * cell_h)`, preserving aspect ratio
    /// per the Kitty Graphics Protocol specification.
    pub fn compute_display_box(&self, cell_w: f32, cell_h: f32) -> [f32; 4] {
        let cell_w = cell_w.max(1.0);
        let cell_h = cell_h.max(1.0);
        let src_w = self.src_rect[2].max(1) as f32;
        let src_h = self.src_rect[3].max(1) as f32;
        let off_x = (self.cell_offset[0] as f32).min(cell_w - 1.0).max(0.0);
        let off_y = (self.cell_offset[1] as f32).min(cell_h - 1.0).max(0.0);

        match (self.display_cols, self.display_rows) {
            (None, None) => [off_x, off_y, src_w, src_h],
            (Some(c), None) => {
                let w = c.max(1) as f32 * cell_w;
                let h = src_h * (w / src_w);
                [off_x, off_y, w, h]
            }
            (None, Some(r)) => {
                let h = r.max(1) as f32 * cell_h;
                let w = src_w * (h / src_h);
                [off_x, off_y, w, h]
            }
            (Some(c), Some(r)) => {
                let box_w = c.max(1) as f32 * cell_w;
                let box_h = r.max(1) as f32 * cell_h;
                let scale = (box_w / src_w).min(box_h / src_h);
                let w = src_w * scale;
                let h = src_h * scale;
                let pad_x = (box_w - w) * 0.5;
                let pad_y = (box_h - h) * 0.5;
                [off_x + pad_x, off_y + pad_y, w, h]
            }
        }
    }

    /// Compute normalized texture UV bounds `[u0, v0, u1, v1]` for `src_rect`.
    #[inline]
    pub fn uv_bounds(&self) -> [f32; 4] {
        let tw = self.width_px.max(1) as f32;
        let th = self.height_px.max(1) as f32;
        let sx = (self.src_rect[0] as f32).clamp(0.0, tw);
        let sy = (self.src_rect[1] as f32).clamp(0.0, th);
        let sw = (self.src_rect[2] as f32).clamp(0.0, tw - sx);
        let sh = (self.src_rect[3] as f32).clamp(0.0, th - sy);
        [sx / tw, sy / th, (sx + sw) / tw, (sy + sh) / th]
    }

    /// Returns true if this placement covers 0-based live grid cell `(target_row, target_col)`.
    #[inline]
    pub fn intersects_cell(&self, target_row: isize, target_col: usize) -> bool {
        let r_end = self.row + self.height_cells.max(1) as isize;
        let c_end = self.col + self.width_cells.max(1);
        target_row >= self.row && target_row < r_end && target_col >= self.col && target_col < c_end
    }
}

/// Stored animation frame within a [`StoredImage`].
#[derive(Debug, Clone)]
pub struct AnimationFrame {
    pub frame_number: u32,
    pub rgba_data: Arc<Vec<u8>>,
    pub gap_ms: u32,
}

/// Uploaded Kitty image stored in terminal memory, reusable across multiple placements
/// (`a=p`) and virtual Unicode placeholder placements (`U=1`).
#[derive(Debug, Clone)]
pub struct StoredImage {
    pub image_id: u32,
    pub image_number: u32,
    pub texture_id: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub rgba_data: Arc<Vec<u8>>,
    pub frames: Vec<AnimationFrame>,
    pub active_frame: usize,
    pub seq: u64,
}

impl StoredImage {
    pub fn byte_len(&self) -> usize {
        let base = self.rgba_data.len();
        let extra: usize = self.frames.iter().skip(1).map(|f| f.rgba_data.len()).sum();
        base.saturating_add(extra)
    }
}

/// Virtual placement prototype registered via `a=p,U=1` for `U+10EEEE` Unicode placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualPlacement {
    pub image_id: u32,
    pub placement_id: u32,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub src_rect: [u32; 4],
}

/// Sixel Graphics Decoder
#[derive(Debug, Clone)]
pub struct SixelDecoder {
    color_table: HashMap<u16, [u8; 4]>,
    current_color: u16,
    cur_x: usize,
    cur_y: usize,
}

impl Default for SixelDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SixelDecoder {
    pub fn new() -> Self {
        let mut color_table = HashMap::new();
        let default_palette: [[u8; 3]; 16] = [
            [0, 0, 0],       // 0: Black
            [51, 102, 204],  // 1: Blue
            [204, 0, 0],     // 2: Red
            [51, 204, 51],   // 3: Green
            [204, 0, 204],   // 4: Magenta
            [51, 204, 204],  // 5: Cyan
            [204, 204, 0],   // 6: Yellow
            [204, 204, 204], // 7: Gray
            [102, 102, 102], // 8: Dark Gray
            [102, 153, 255], // 9: Light Blue
            [255, 102, 102], // 10: Light Red
            [102, 255, 102], // 11: Light Green
            [255, 102, 255], // 12: Light Magenta
            [102, 255, 255], // 13: Light Cyan
            [255, 255, 102], // 14: Light Yellow
            [255, 255, 255], // 15: White
        ];

        for (i, rgb) in default_palette.iter().enumerate() {
            color_table.insert(i as u16, [rgb[0], rgb[1], rgb[2], 255]);
        }

        Self {
            color_table,
            current_color: 0,
            cur_x: 0,
            cur_y: 0,
        }
    }

    /// Decode raw Sixel byte stream into RGBA pixels.
    ///
    /// Sixel data is written strictly left to right within a six-pixel-tall
    /// band, so the output can be produced by contiguous runs into a single
    /// flat RGBA allocation. The image extents are not known until the stream
    /// has been parsed, so the stream is parsed twice: once with a sink that
    /// only records extents, then once into the sized buffer. Both passes go
    /// through the same parser ([`SixelDecoder::run`]), so they cannot drift
    /// apart, and the second pass starts from the same decoder state as the
    /// first.
    pub fn decode(&mut self, data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        let start_state = self.clone();

        // Pass 1: measure. This leaves `self` in its correct post-decode
        // state, which callers rely on across chunked payloads.
        let mut extents = ExtentSink::default();
        self.run(data, &mut extents);

        let (width, height) = (extents.max_x, extents.max_y);
        if width == 0 || height == 0 {
            return None;
        }
        if width * height > MAX_SIXEL_PIXELS {
            warn!(
                "Sixel image {}x{} exceeds the decoder pixel budget; dropping",
                width, height
            );
            return None;
        }

        // Pass 2: fill. Pixels that the stream never touches stay at
        // (0, 0, 0, 0), i.e. transparent, exactly as before.
        let mut rgba_buf = vec![0u8; width * height * 4];
        *self = start_state;
        let mut sink = BufferSink {
            buf: &mut rgba_buf,
            width,
        };
        self.run(data, &mut sink);

        Some((width as u32, height as u32, rgba_buf))
    }

    /// Parse `data`, emitting every painted pixel run to `sink`.
    ///
    /// This is the single source of truth for Sixel semantics; `decode` runs
    /// it once per pass.
    fn run<S: SixelSink>(&mut self, data: &[u8], sink: &mut S) {
        let len = data.len();
        let mut i = 0;

        // Resolving the selected register is a hash lookup, so it is cached
        // here and refreshed only when the selection or the palette changes,
        // rather than once per sixel character.
        let mut color = self.active_color();

        while i < len {
            let b = data[i];
            match b {
                b'#' => {
                    // Color definition or selection
                    i += 1;
                    // Only the first five parameters are meaningful
                    // (`Pc;Pu;Px;Py;Pz`), but the total count still decides
                    // between a bare selection (`#1`) and a definition.
                    let mut nums = [0u32; 5];
                    let mut num_count = 0usize;
                    let mut curr_num: u32 = 0;
                    let mut has_num = false;

                    while i < len {
                        let c = data[i];
                        if c.is_ascii_digit() {
                            // Saturating so that an absurd digit run cannot
                            // overflow-panic on a debug build.
                            curr_num = curr_num
                                .saturating_mul(10)
                                .saturating_add((c - b'0') as u32);
                            has_num = true;
                            i += 1;
                        } else if c == b';' {
                            if has_num {
                                if num_count < nums.len() {
                                    nums[num_count] = curr_num;
                                }
                                num_count += 1;
                                curr_num = 0;
                                has_num = false;
                            }
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    if has_num {
                        if num_count < nums.len() {
                            nums[num_count] = curr_num;
                        }
                        num_count += 1;
                    }

                    if num_count == 1 {
                        self.current_color = nums[0] as u16;
                        color = self.active_color();
                    } else if num_count >= 5 && nums[1] == 2 {
                        // RGB definition: #reg;2;r;g;b (percentage 0..100 or 0..255).
                        // Other color spaces (HLS, `;1;`) are not implemented
                        // and are ignored, leaving the selection untouched.
                        let reg = nums[0] as u16;
                        let rgba = [
                            scale_color_component(nums[2]),
                            scale_color_component(nums[3]),
                            scale_color_component(nums[4]),
                            255,
                        ];
                        self.color_table.insert(reg, rgba);
                        self.current_color = reg;
                        color = rgba;
                    }
                }
                b'!' => {
                    // Repeat count: !<count><char>
                    i += 1;
                    let mut count = 0usize;
                    while i < len && data[i].is_ascii_digit() {
                        count = count
                            .saturating_mul(10)
                            .saturating_add((data[i] - b'0') as usize);
                        i += 1;
                    }
                    if i < len {
                        // The byte after the count is always consumed, even
                        // when it is not sixel data.
                        let char_byte = data[i];
                        i += 1;
                        if (63..=126).contains(&char_byte) {
                            self.put_sixel(char_byte - 63, count, color, sink);
                        }
                    }
                }
                b'-' => {
                    // Line feed: advance 6 pixels down and return to start
                    self.cur_x = 0;
                    self.cur_y = self.cur_y.saturating_add(6).min(MAX_SIXEL_DIM);
                    i += 1;
                }
                b'$' => {
                    // Carriage return: return to x = 0
                    self.cur_x = 0;
                    i += 1;
                }
                b'"' => {
                    // Raster attributes: "Pan;Pad;Ph;Pv (skip attribute header)
                    i += 1;
                    while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                        i += 1;
                    }
                }
                63..=126 => {
                    // Sixel data character
                    self.put_sixel(b - 63, 1, color, sink);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    /// Paint `count` adjacent columns of the six-pixel pattern `sixel_val` at
    /// the cursor, then advance the cursor past them.
    fn put_sixel<S: SixelSink>(
        &mut self,
        sixel_val: u8,
        count: usize,
        color: [u8; 4],
        sink: &mut S,
    ) {
        let x = self.cur_x;
        if sixel_val != 0 && x < MAX_SIXEL_DIM {
            let run_len = count.min(MAX_SIXEL_DIM - x);
            if run_len > 0 {
                for bit in 0..6 {
                    if (sixel_val & (1 << bit)) != 0 {
                        let y = self.cur_y + bit;
                        if y < MAX_SIXEL_DIM {
                            sink.put_run(x, y, run_len, color);
                        }
                    }
                }
            }
        }
        self.cur_x = x.saturating_add(count).min(MAX_SIXEL_DIM);
    }

    /// The RGBA value of the selected register, white if it is undefined.
    fn active_color(&self) -> [u8; 4] {
        self.color_table
            .get(&self.current_color)
            .copied()
            .unwrap_or([255, 255, 255, 255])
    }
}

/// Upper bound on either Sixel axis.
///
/// Real images are orders of magnitude smaller; this only exists so that a
/// corrupt or hostile stream (`!99999999~`, megabytes of `-`) cannot make the
/// decoder allocate unbounded memory or spin. Pixels beyond the bound are
/// dropped, so a well-formed image is never rescaled or clipped.
const MAX_SIXEL_DIM: usize = 16_384;

/// Upper bound on the decoded pixel count, i.e. 256 MiB of RGBA.
const MAX_SIXEL_PIXELS: usize = 64_000_000;

/// Sixel color components are percentages (0..=100) per the DEC spec, but
/// several encoders emit 0..=255 directly, so larger values pass through.
fn scale_color_component(value: u32) -> u8 {
    if value <= 100 {
        (value * 255 / 100) as u8
    } else {
        value as u8
    }
}

/// Destination for one horizontal run of identically colored pixels.
///
/// Lets the measuring pass and the filling pass share a single parser.
trait SixelSink {
    /// Paint `len` pixels starting at `(x, y)`, running right along row `y`.
    fn put_run(&mut self, x: usize, y: usize, len: usize, color: [u8; 4]);
}

/// Sink for the measuring pass: records the image extents and nothing else.
#[derive(Default)]
struct ExtentSink {
    max_x: usize,
    max_y: usize,
}

impl SixelSink for ExtentSink {
    fn put_run(&mut self, x: usize, y: usize, len: usize, _color: [u8; 4]) {
        self.max_x = self.max_x.max(x + len);
        self.max_y = self.max_y.max(y + 1);
    }
}

/// Sink for the filling pass: writes runs straight into the RGBA buffer.
struct BufferSink<'a> {
    buf: &'a mut [u8],
    width: usize,
}

impl SixelSink for BufferSink<'_> {
    fn put_run(&mut self, x: usize, y: usize, len: usize, color: [u8; 4]) {
        // The measuring pass sized the buffer from the very same runs, so
        // these bounds always hold; they are re-checked to keep a future
        // mismatch from panicking on a user-supplied byte stream.
        if x >= self.width {
            return;
        }
        let len = len.min(self.width - x);
        let start = (y * self.width + x) * 4;
        let Some(row) = self.buf.get_mut(start..start + len * 4) else {
            return;
        };
        for pixel in row.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&color);
        }
    }
}

/// Maximum dimension (width or height) in pixels for a single Kitty image.
pub const MAX_KITTY_DIM: u32 = 16_384;

/// Maximum pixel count for a single Kitty image (64 MP = 256 MiB RGBA).
pub const MAX_KITTY_PIXELS: usize = 64_000_000;

/// Maximum raw/decompressed payload size for a single Kitty command (256 MiB).
pub const MAX_KITTY_RAW_BYTES: usize = MAX_KITTY_PIXELS * 4;

/// Default per-terminal Kitty image storage quota (320 MiB, matching Kitty's default).
pub const MAX_IMAGE_STORAGE_BYTES: usize = 320 * 1024 * 1024;

/// Parsed control parameters for a Kitty Graphics Protocol command (`_G<control>;<payload>`).
#[derive(Debug, Clone)]
pub struct KittyControlParams {
    /// `a`: Action (`'t'`, `'T'`, `'p'`, `'d'`, `'q'`, `'f'`, `'a'`, `'c'`). Default `'t'`.
    pub action: char,
    /// `f`: Pixel/data format (`24` = RGB, `32` = RGBA, `100` = PNG). Default `32`.
    pub format: u32,
    /// `t`: Transmission medium (`'d'` = direct, `'f'` = file, `'t'` = temp file, `'s'` = shm). Default `'d'`.
    pub medium: char,
    /// `o`: Compression (`Some('z')` = zlib deflate).
    pub compression: Option<char>,
    /// `m`: More chunks follow (`true` when `m=1`, `false` when `m=0`).
    pub more: bool,
    /// `q`: Response suppression (`0` = normal, `1` = errors only, `2` = silent).
    pub quiet: u8,
    /// `i`: Explicit image ID (`0` if unspecified).
    pub image_id: u32,
    /// `I`: Image number (`0` if unspecified).
    pub image_number: u32,
    /// `p`: Placement ID (`0` if unspecified).
    pub placement_id: u32,
    /// `s`: Source pixel width (`0` if unspecified).
    pub data_width: u32,
    /// `v`: Source pixel height (`0` if unspecified).
    pub data_height: u32,
    /// `S`: Byte count to read from file/SHM (`0` = read until end).
    pub read_size: usize,
    /// `O`: Byte offset to seek before reading from file/SHM.
    pub read_offset: u64,
    /// `x`: Source crop X (or range start ID for `a=d,d=r`, or frame X offset for `a=f`).
    pub x: u32,
    /// `y`: Source crop Y (or range end ID for `a=d,d=r`, or frame Y offset for `a=f`).
    pub y: u32,
    /// `w`: Source crop width.
    pub w: u32,
    /// `h`: Source crop height.
    pub h: u32,
    /// `X`: Sub-cell X pixel offset (or composition mode for `a=f`, or dest X for `a=c`).
    pub cell_x_offset: u32,
    /// `Y`: Sub-cell Y pixel offset (or RGBA fill color for `a=f`, or dest Y for `a=c`).
    pub cell_y_offset: u32,
    /// `c`: Display width in columns (or target 1-based frame index for `a=f` / `a=a` / `a=c`).
    pub cols: u32,
    /// `r`: Display height in rows (or source 1-based frame index for `a=f` / `a=a` / `a=c`).
    pub rows: u32,
    /// `C`: Cursor movement policy (`0` = move cursor, `1` = keep cursor fixed).
    pub cursor_policy: u8,
    /// `U`: Create a virtual placement (`1`) for `U+10EEEE` Unicode placeholders.
    pub unicode_placeholder: bool,
    /// `z`: Z-index for placement (`< 0` behind text, `>= 0` above text; or frame gap ms in `a=f`).
    pub z_index: i32,
    /// `d`: Deletion target selector for `a=d` (default `'a'`).
    pub delete_target: char,
    /// `P`: Parent image ID for relative placement (`0` if none).
    pub parent_id: u32,
    /// `Q`: Parent placement ID for relative placement (`0` if none).
    pub parent_placement_id: u32,
    /// `H`: Horizontal offset in cells from parent placement.
    pub parent_offset_h: i32,
    /// `V`: Vertical offset in cells from parent placement.
    pub parent_offset_v: i32,
}

impl Default for KittyControlParams {
    fn default() -> Self {
        Self {
            action: 't',
            format: 32,
            medium: 'd',
            compression: None,
            more: false,
            quiet: 0,
            image_id: 0,
            image_number: 0,
            placement_id: 0,
            data_width: 0,
            data_height: 0,
            read_size: 0,
            read_offset: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            cell_x_offset: 0,
            cell_y_offset: 0,
            cols: 0,
            rows: 0,
            cursor_policy: 0,
            unicode_placeholder: false,
            z_index: 0,
            delete_target: 'a',
            parent_id: 0,
            parent_placement_id: 0,
            parent_offset_h: 0,
            parent_offset_v: 0,
        }
    }
}

impl KittyControlParams {
    pub fn apply_header(&mut self, control_str: &str) {
        self.more = false;
        for pair in control_str.split(',') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim();
            match k {
                "a" => {
                    if let Some(ch) = v.chars().next() {
                        self.action = ch;
                    }
                }
                "f" => {
                    if let Ok(fmt) = v.parse::<u32>() {
                        self.format = fmt;
                    }
                }
                "t" => {
                    if let Some(ch) = v.chars().next() {
                        self.medium = ch;
                    }
                }
                "o" => {
                    self.compression = v.chars().next();
                }
                "m" => {
                    self.more = v == "1";
                }
                "q" => {
                    if let Ok(q) = v.parse::<u8>() {
                        self.quiet = q;
                    }
                }
                "i" => {
                    if let Ok(id) = v.parse::<u32>() {
                        self.image_id = id;
                    }
                }
                "I" => {
                    if let Ok(num) = v.parse::<u32>() {
                        self.image_number = num;
                    }
                }
                "p" => {
                    if let Ok(pid) = v.parse::<u32>() {
                        self.placement_id = pid;
                    }
                }
                "s" => {
                    if let Ok(w) = v.parse::<u32>() {
                        self.data_width = w;
                    }
                }
                "v" => {
                    if let Ok(h) = v.parse::<u32>() {
                        self.data_height = h;
                    }
                }
                "S" => {
                    if let Ok(sz) = v.parse::<usize>() {
                        self.read_size = sz;
                    }
                }
                "O" => {
                    if let Ok(off) = v.parse::<u64>() {
                        self.read_offset = off;
                    }
                }
                "x" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.x = val;
                    }
                }
                "y" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.y = val;
                    }
                }
                "w" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.w = val;
                    }
                }
                "h" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.h = val;
                    }
                }
                "X" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.cell_x_offset = val;
                    }
                }
                "Y" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.cell_y_offset = val;
                    }
                }
                "c" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.cols = val;
                    }
                }
                "r" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.rows = val;
                    }
                }
                "C" => {
                    if let Ok(val) = v.parse::<u8>() {
                        self.cursor_policy = val;
                    }
                }
                "U" => {
                    self.unicode_placeholder = v == "1";
                }
                "z" => {
                    if let Ok(val) = v.parse::<i32>() {
                        self.z_index = val;
                    }
                }
                "d" => {
                    if let Some(ch) = v.chars().next() {
                        self.delete_target = ch;
                    }
                }
                "P" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.parent_id = val;
                    }
                }
                "Q" => {
                    if let Ok(val) = v.parse::<u32>() {
                        self.parent_placement_id = val;
                    }
                }
                "H" => {
                    if let Ok(val) = v.parse::<i32>() {
                        self.parent_offset_h = val;
                    }
                }
                "V" => {
                    if let Ok(val) = v.parse::<i32>() {
                        self.parent_offset_v = val;
                    }
                }
                _ => {}
            }
        }
    }

    /// Format a Kitty protocol response `\x1b_G...;<msg>\x1b\\` if allowed by `quiet` and ID rules.
    pub fn format_response(
        &self,
        resolved_id: Option<u32>,
        is_ok: bool,
        msg: &str,
    ) -> Option<Vec<u8>> {
        if self.quiet >= 2 {
            return None;
        }
        if is_ok && self.quiet >= 1 {
            return None;
        }

        // Delete commands never emit OK responses per Kitty spec.
        if self.action == 'd' && is_ok {
            return None;
        }

        // Non-query commands only respond when the client supplied an image ID (`i`) or number (`I`).
        if self.action != 'q' && self.image_id == 0 && self.image_number == 0 {
            return None;
        }

        let mut keys = Vec::with_capacity(3);
        let id_to_report = if self.image_id != 0 {
            Some(self.image_id)
        } else {
            resolved_id.filter(|&id| id != 0)
        };
        if let Some(id) = id_to_report {
            keys.push(format!("i={id}"));
        }
        if self.image_number != 0 {
            keys.push(format!("I={}", self.image_number));
        }
        if self.placement_id != 0 {
            keys.push(format!("p={}", self.placement_id));
        }

        let resp = format!("\x1b_G{};{}\x1b\\", keys.join(","), msg);
        Some(resp.into_bytes())
    }
}

/// Result of feeding a single Kitty Graphics chunk into [`KittyGraphicsParser::feed_chunk`].
#[derive(Debug)]
pub enum KittyParseStep {
    /// Waiting for additional `m=1` chunks before executing the command.
    NeedMoreChunks,
    /// A complete command (control parameters + reassembled Base64-decoded payload) is ready.
    Complete {
        params: KittyControlParams,
        payload: Vec<u8>,
    },
    /// Base64 or size error while reassembling chunks.
    Error {
        params: KittyControlParams,
        code: &'static str,
        message: &'static str,
    },
}

/// Kitty Graphics Protocol Chunk Assembler & Parser
#[derive(Debug, Clone)]
pub struct KittyGraphicsParser {
    chunk_buffer: Vec<u8>,
    b64_rem: [u8; 4],
    b64_rem_len: usize,
    in_chunked: bool,
    overflowed: bool,
    current_params: KittyControlParams,
}

impl Default for KittyGraphicsParser {
    fn default() -> Self {
        Self::new()
    }
}

impl KittyGraphicsParser {
    pub fn new() -> Self {
        Self {
            chunk_buffer: Vec::new(),
            b64_rem: [0; 4],
            b64_rem_len: 0,
            in_chunked: false,
            overflowed: false,
            current_params: KittyControlParams::default(),
        }
    }

    pub fn reset(&mut self) {
        self.chunk_buffer.clear();
        self.b64_rem = [0; 4];
        self.b64_rem_len = 0;
        self.in_chunked = false;
        self.overflowed = false;
        self.current_params = KittyControlParams::default();
    }

    /// Feed a single Kitty Graphics command body (with leading `'G'` already stripped):
    /// `<control_kv>;<base64_payload>`.
    pub fn feed_chunk(&mut self, cmd: &[u8]) -> KittyParseStep {
        let (control_bytes, payload_bytes) = match cmd.iter().position(|&b| b == b';') {
            Some(pos) => (&cmd[..pos], &cmd[pos + 1..]),
            None => (cmd, &[][..]),
        };

        let control_str = std::str::from_utf8(control_bytes).unwrap_or("");

        // If a previous chunked transfer was left unfinished (e.g. an aborted client) and a
        // standalone non-transmission action (`a=d`, `a=p`, `a=a`, `a=c`) arrives, discard the
        // stale partial state so the new command executes cleanly.
        if self.in_chunked {
            for pair in control_str.split(',') {
                if let Some(("a", v)) = pair.split_once('=') {
                    if matches!(v.trim(), "d" | "p" | "a" | "c") {
                        self.reset();
                    }
                    break;
                }
            }
        }

        if !self.in_chunked {
            self.current_params = KittyControlParams::default();
        }
        self.current_params.apply_header(control_str);

        if !payload_bytes.is_empty() && !self.overflowed {
            if let Err((code, message)) =
                self.append_base64_chunk(payload_bytes, !self.current_params.more)
            {
                let params = self.current_params.clone();
                self.reset();
                return KittyParseStep::Error {
                    params,
                    code,
                    message,
                };
            }
        } else if !self.current_params.more && self.b64_rem_len > 0 && !self.overflowed {
            if let Err((code, message)) = self.append_base64_chunk(&[], true) {
                let params = self.current_params.clone();
                self.reset();
                return KittyParseStep::Error {
                    params,
                    code,
                    message,
                };
            }
        }

        if self.current_params.more {
            self.in_chunked = true;
            return KittyParseStep::NeedMoreChunks;
        }

        let was_overflowed = self.overflowed;
        let params = std::mem::take(&mut self.current_params);
        let payload = std::mem::take(&mut self.chunk_buffer);
        self.b64_rem = [0; 4];
        self.b64_rem_len = 0;
        self.in_chunked = false;
        self.overflowed = false;

        if was_overflowed {
            return KittyParseStep::Error {
                params,
                code: "ENOMEM",
                message: "Image payload exceeds maximum size",
            };
        }

        KittyParseStep::Complete { params, payload }
    }

    fn append_base64_chunk(
        &mut self,
        payload_bytes: &[u8],
        is_final: bool,
    ) -> Result<(), (&'static str, &'static str)> {
        // Strip ASCII whitespace while prepending any 1..3 leftover Base64 chars from the prior chunk.
        let mut clean = Vec::with_capacity(self.b64_rem_len + payload_bytes.len());
        clean.extend_from_slice(&self.b64_rem[..self.b64_rem_len]);
        self.b64_rem_len = 0;

        for &b in payload_bytes {
            if !b.is_ascii_whitespace() {
                clean.push(b);
            }
        }

        if clean.is_empty() {
            return Ok(());
        }

        let to_decode = if is_final {
            &clean[..]
        } else {
            let aligned = clean.len() & !3;
            let rem = clean.len() - aligned;
            if rem > 0 {
                self.b64_rem[..rem].copy_from_slice(&clean[aligned..]);
                self.b64_rem_len = rem;
            }
            &clean[..aligned]
        };

        if to_decode.is_empty() {
            return Ok(());
        }

        let decoded = BASE64_STANDARD
            .decode(to_decode)
            .or_else(|_| {
                let stripped = to_decode
                    .iter()
                    .copied()
                    .take_while(|&b| b != b'=')
                    .collect::<Vec<u8>>();
                BASE64_NO_PAD.decode(&stripped)
            })
            .map_err(|_| ("EINVAL", "Invalid base64 payload"))?;

        if self.chunk_buffer.len().saturating_add(decoded.len()) > MAX_KITTY_RAW_BYTES {
            self.overflowed = true;
            self.chunk_buffer.clear();
            return Ok(());
        }

        self.chunk_buffer.extend_from_slice(&decoded);
        Ok(())
    }

    /// Parse Kitty Graphics payload: `<keys>;<base64_payload>` into `(width, height, rgba, z_index)`.
    /// Preserved as a convenience API for direct unit tests and standalone decoding.
    pub fn parse_command(&mut self, cmd: &str) -> Option<(u32, u32, Vec<u8>, i32)> {
        match self.feed_chunk(cmd.as_bytes()) {
            KittyParseStep::Complete { params, payload } => {
                let raw = load_transmission_payload(&params, payload).ok()?;
                let (w, h, rgba) = decode_kitty_pixels(&params, raw).ok()?;
                Some((w, h, rgba, params.z_index))
            }
            KittyParseStep::NeedMoreChunks | KittyParseStep::Error { .. } => None,
        }
    }
}

/// Resolve the transmission medium (`t=d`, `t=f`, `t=t`, `t=s`) and optional compression (`o=z`)
/// into raw uncompressed image/frame bytes.
pub fn load_transmission_payload(
    params: &KittyControlParams,
    payload: Vec<u8>,
) -> Result<Vec<u8>, (&'static str, &'static str)> {
    let raw_compressed = match params.medium {
        'd' => payload,
        'f' | 't' => {
            let path = decode_payload_path(&payload)?;
            let data = read_bounded_regular_file(&path, params.read_offset, params.read_size)?;
            if params.medium == 't' {
                safe_remove_kitty_temp_file(&path);
            }
            data
        }
        's' => read_shared_memory_payload(&payload, params.read_offset, params.read_size)?,
        _ => return Err(("EINVAL", "Unknown transmission medium")),
    };

    match params.compression {
        None => Ok(raw_compressed),
        Some('z') => decompress_zlib_bounded(&raw_compressed),
        Some(_) => Err(("EINVAL", "Unsupported compression format")),
    }
}

fn decode_payload_path(payload: &[u8]) -> Result<PathBuf, (&'static str, &'static str)> {
    if payload.is_empty() || payload.contains(&0) {
        return Err(("EINVAL", "Invalid file path in payload"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(payload)))
    }
    #[cfg(not(unix))]
    {
        let s = std::str::from_utf8(payload).map_err(|_| ("EINVAL", "Non-UTF8 file path"))?;
        Ok(PathBuf::from(s))
    }
}

fn read_bounded_regular_file(
    path: &Path,
    offset: u64,
    size: usize,
) -> Result<Vec<u8>, (&'static str, &'static str)> {
    let mut file =
        std::fs::File::open(path).map_err(|_| ("ENOENT", "Failed to open image file"))?;
    let meta = file
        .metadata()
        .map_err(|_| ("ENOENT", "Failed to stat image file"))?;
    if !meta.file_type().is_file() {
        return Err(("EINVAL", "Path does not refer to a regular file"));
    }

    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| ("EINVAL", "Failed to seek image file"))?;
    }

    let max_to_read = if size > 0 {
        if size > MAX_KITTY_RAW_BYTES {
            return Err(("ENOMEM", "Requested read size exceeds maximum limit"));
        }
        size
    } else {
        let remaining = meta.len().saturating_sub(offset);
        if remaining > MAX_KITTY_RAW_BYTES as u64 {
            return Err(("ENOMEM", "Image file exceeds maximum size limit"));
        }
        MAX_KITTY_RAW_BYTES
    };

    let limit = (max_to_read as u64).saturating_add(1);
    let mut buf = Vec::new();
    file.take(limit)
        .read_to_end(&mut buf)
        .map_err(|_| ("EIO", "Failed to read image file"))?;

    if size > 0 {
        if buf.len() > size {
            buf.truncate(size);
        }
    } else if buf.len() > MAX_KITTY_RAW_BYTES {
        return Err(("ENOMEM", "Image file exceeds maximum size limit"));
    }

    Ok(buf)
}

/// Per the Kitty Graphics Protocol security specification, temporary files (`t=t`) are only
/// deleted if the path contains `tty-graphics-protocol`, is not a symbolic link, and resides
/// inside a recognized system temporary directory.
fn safe_remove_kitty_temp_file(path: &Path) {
    let path_str = path.to_string_lossy();
    if !path_str.contains("tty-graphics-protocol") {
        return;
    }

    let Ok(sym_meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if sym_meta.file_type().is_symlink() || !sym_meta.file_type().is_file() {
        return;
    }

    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(canon_parent) = std::fs::canonicalize(parent) else {
        return;
    };

    let mut allowed_roots: Vec<PathBuf> = Vec::with_capacity(8);
    let env_tmp = std::env::temp_dir();
    if let Ok(canon_tmp) = std::fs::canonicalize(&env_tmp) {
        allowed_roots.push(canon_tmp);
    }
    allowed_roots.push(env_tmp);
    #[cfg(unix)]
    {
        for root in [
            "/tmp",
            "/private/tmp",
            "/dev/shm",
            "/var/tmp",
            "/private/var/tmp",
            "/var/folders",
            "/private/var/folders",
        ] {
            allowed_roots.push(PathBuf::from(root));
        }
    }

    if allowed_roots
        .iter()
        .any(|root| canon_parent.starts_with(root))
    {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn read_shared_memory_payload(
    payload: &[u8],
    offset: u64,
    size: usize,
) -> Result<Vec<u8>, (&'static str, &'static str)> {
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

    let raw_name = std::str::from_utf8(payload)
        .map_err(|_| ("EINVAL", "Invalid UTF-8 shared memory name"))?
        .trim_matches('\0');
    if raw_name.is_empty() || raw_name.len() > 250 || raw_name.contains("..") {
        return Err(("EINVAL", "Invalid shared memory object name"));
    }

    let stripped = raw_name.strip_prefix('/').unwrap_or(raw_name);
    if stripped.is_empty() || stripped.contains('/') {
        return Err((
            "EINVAL",
            "Shared memory object name must not contain path separators",
        ));
    }

    let normalized = format!("/{stripped}");
    let c_name =
        std::ffi::CString::new(normalized).map_err(|_| ("EINVAL", "Invalid shared memory name"))?;

    let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return Err(("ENOENT", "Failed to open POSIX shared memory object"));
    }

    // Always unlink immediately after opening, as required by the Kitty Graphics Protocol.
    unsafe {
        let _ = libc::shm_unlink(c_name.as_ptr());
    }

    // RAII guard ensures `close(fd)` runs on every return path.
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(owned_fd.as_raw_fd(), &mut stat_buf) } != 0 || stat_buf.st_size <= 0 {
        return Err(("EIO", "Failed to stat POSIX shared memory object"));
    }

    let total_len = stat_buf.st_size as u64;
    if offset >= total_len {
        return Err(("EINVAL", "Read offset exceeds shared memory size"));
    }

    let avail = total_len - offset;
    let copy_len_u64 = if size > 0 {
        if size > MAX_KITTY_RAW_BYTES {
            return Err(("ENOMEM", "Requested shared memory size exceeds limit"));
        }
        (size as u64).min(avail)
    } else {
        if avail > MAX_KITTY_RAW_BYTES as u64 {
            return Err(("ENOMEM", "Shared memory object exceeds maximum size limit"));
        }
        avail
    };

    let map_len = total_len as usize;
    let copy_len = copy_len_u64 as usize;

    // Use `mmap` rather than `read(2)`: on macOS (XNU), POSIX `shm_open` descriptors do not
    // implement `read`/`write` vnode ops and must be memory-mapped.
    let map_ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            owned_fd.as_raw_fd(),
            0,
        )
    };
    if map_ptr == libc::MAP_FAILED {
        return Err(("EIO", "Failed to mmap shared memory object"));
    }

    let mut buf = vec![0u8; copy_len];
    unsafe {
        std::ptr::copy_nonoverlapping(
            (map_ptr as *const u8).add(offset as usize),
            buf.as_mut_ptr(),
            copy_len,
        );
        let _ = libc::munmap(map_ptr, map_len);
    }

    Ok(buf)
}

#[cfg(not(unix))]
fn read_shared_memory_payload(
    _payload: &[u8],
    _offset: u64,
    _size: usize,
) -> Result<Vec<u8>, (&'static str, &'static str)> {
    Err((
        "ENOSYS",
        "POSIX shared memory transmission is not supported on this platform",
    ))
}

fn decompress_zlib_bounded(compressed: &[u8]) -> Result<Vec<u8>, (&'static str, &'static str)> {
    let decoder = ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .take((MAX_KITTY_RAW_BYTES as u64).saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|_| ("EINVAL", "Failed to decompress zlib payload"))?;
    if out.len() > MAX_KITTY_RAW_BYTES {
        return Err((
            "ENOMEM",
            "Decompressed zlib payload exceeds maximum size limit",
        ));
    }
    Ok(out)
}

/// Decode raw pixel or PNG/JPEG bytes into `(width_px, height_px, rgba_bytes)`.
pub fn decode_kitty_pixels(
    params: &KittyControlParams,
    mut data: Vec<u8>,
) -> Result<(u32, u32, Vec<u8>), (&'static str, &'static str)> {
    let width = params.data_width;
    let height = params.data_height;

    match params.format {
        100 => {
            let reader = image::ImageReader::new(std::io::Cursor::new(&data))
                .with_guessed_format()
                .map_err(|_| ("EINVAL", "Failed to inspect encoded image header"))?;
            let (w, h) = reader
                .into_dimensions()
                .map_err(|_| ("EINVAL", "Failed to read encoded image dimensions"))?;
            validate_kitty_dimensions(w, h)?;

            let img = image::load_from_memory(&data).map_err(|_| {
                warn!("Failed to decode Kitty PNG/compressed image from memory");
                ("EINVAL", "Failed to decode PNG image")
            })?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            validate_kitty_dimensions(w, h)?;
            info!("Decoded Kitty PNG image: {}x{} px", w, h);
            Ok((w, h, rgba.into_raw()))
        }
        24 => {
            validate_kitty_dimensions(width, height)?;
            let pixel_count = (width as usize) * (height as usize);
            let needed = pixel_count * 3;
            if data.len() < needed {
                return Err((
                    "EINVAL",
                    "Insufficient RGB pixel data for specified dimensions",
                ));
            }
            let mut rgba = Vec::with_capacity(pixel_count * 4);
            for chunk in data[..needed].as_chunks::<3>().0 {
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
                rgba.push(chunk[2]);
                rgba.push(255);
            }
            Ok((width, height, rgba))
        }
        32 => {
            if width > 0 && height > 0 {
                validate_kitty_dimensions(width, height)?;
                let needed = (width as usize) * (height as usize) * 4;
                if data.len() < needed {
                    return Err((
                        "EINVAL",
                        "Insufficient RGBA pixel data for specified dimensions",
                    ));
                }
                data.truncate(needed);
                Ok((width, height, data))
            } else if let Ok(img) = image::load_from_memory(&data) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                validate_kitty_dimensions(w, h)?;
                Ok((w, h, rgba.into_raw()))
            } else {
                Err(("EINVAL", "Missing width/height for raw RGBA image"))
            }
        }
        _ => Err(("EINVAL", "Unsupported Kitty pixel format")),
    }
}

#[inline]
pub fn validate_kitty_dimensions(
    width: u32,
    height: u32,
) -> Result<(), (&'static str, &'static str)> {
    if width == 0 || height == 0 {
        return Err(("EINVAL", "Image dimensions must be non-zero"));
    }
    if width > MAX_KITTY_DIM || height > MAX_KITTY_DIM {
        return Err(("EINVAL", "Image dimensions exceed maximum texture bounds"));
    }
    if (width as usize).saturating_mul(height as usize) > MAX_KITTY_PIXELS {
        return Err(("ENOMEM", "Image exceeds maximum pixel budget"));
    }
    Ok(())
}

/// Composite or overwrite an RGBA patch `(src_w, src_h)` onto `dst` `(dst_w, dst_h)` at `(dst_x, dst_y)`.
#[allow(clippy::too_many_arguments)]
pub fn composite_rgba_patch(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_x: u32,
    dst_y: u32,
    overwrite: bool,
) {
    if dst_x >= dst_w || dst_y >= dst_h {
        return;
    }
    let copy_w = src_w.min(dst_w - dst_x) as usize;
    let copy_h = src_h.min(dst_h - dst_y) as usize;
    let dst_stride = dst_w as usize * 4;
    let src_stride = src_w as usize * 4;

    for row in 0..copy_h {
        let d_off = (dst_y as usize + row) * dst_stride + (dst_x as usize) * 4;
        let s_off = row * src_stride;
        if overwrite {
            dst[d_off..d_off + copy_w * 4].copy_from_slice(&src[s_off..s_off + copy_w * 4]);
        } else {
            for col in 0..copy_w {
                let sp = &src[s_off + col * 4..s_off + col * 4 + 4];
                let dp = &mut dst[d_off + col * 4..d_off + col * 4 + 4];
                let sa = sp[3] as u32;
                if sa == 255 {
                    dp.copy_from_slice(sp);
                } else if sa > 0 {
                    let inv_sa = 255 - sa;
                    let da = dp[3] as u32;
                    let out_a = sa + (da * inv_sa + 127) / 255;
                    if out_a > 0 {
                        dp[0] = ((sp[0] as u32 * sa * 255 + dp[0] as u32 * da * inv_sa)
                            / (out_a * 255))
                            .min(255) as u8;
                        dp[1] = ((sp[1] as u32 * sa * 255 + dp[1] as u32 * da * inv_sa)
                            / (out_a * 255))
                            .min(255) as u8;
                        dp[2] = ((sp[2] as u32 * sa * 255 + dp[2] as u32 * da * inv_sa)
                            / (out_a * 255))
                            .min(255) as u8;
                        dp[3] = out_a.min(255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // Reference decoder: a verbatim copy of the original `HashMap<(x, y)>`
    // based implementation. It exists purely as a correctness oracle for the
    // flat-buffer decoder; the differential test below feeds both the same
    // byte streams and requires byte-identical output.
    // ---------------------------------------------------------------------
    #[derive(Debug, Clone)]
    struct RefDecoder {
        color_table: HashMap<u16, [u8; 4]>,
        current_color: u16,
        cur_x: usize,
        cur_y: usize,
    }

    impl RefDecoder {
        fn new() -> Self {
            // Borrow the palette from the real decoder so the two can never
            // drift apart on the default colour table.
            let seed = SixelDecoder::new();
            Self {
                color_table: seed.color_table.clone(),
                current_color: seed.current_color,
                cur_x: seed.cur_x,
                cur_y: seed.cur_y,
            }
        }

        fn decode(&mut self, data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
            let mut i = 0;
            let len = data.len();

            let mut max_x = 0;
            let mut max_y = 0;

            let mut temp_pixels: HashMap<(usize, usize), [u8; 4]> = HashMap::new();

            while i < len {
                let b = data[i];
                match b {
                    b'#' => {
                        i += 1;
                        let mut nums = Vec::new();
                        let mut curr_num = 0;
                        let mut has_num = false;

                        while i < len {
                            let c = data[i];
                            if c.is_ascii_digit() {
                                curr_num = curr_num * 10 + (c - b'0') as u32;
                                has_num = true;
                                i += 1;
                            } else if c == b';' {
                                if has_num {
                                    nums.push(curr_num);
                                    curr_num = 0;
                                    has_num = false;
                                }
                                i += 1;
                            } else {
                                break;
                            }
                        }
                        if has_num {
                            nums.push(curr_num);
                        }

                        if nums.len() == 1 {
                            self.current_color = nums[0] as u16;
                        } else if nums.len() >= 5 && nums[1] == 2 {
                            let reg = nums[0] as u16;
                            let r = if nums[2] <= 100 {
                                (nums[2] * 255 / 100) as u8
                            } else {
                                nums[2] as u8
                            };
                            let g = if nums[3] <= 100 {
                                (nums[3] * 255 / 100) as u8
                            } else {
                                nums[3] as u8
                            };
                            let b_val = if nums[4] <= 100 {
                                (nums[4] * 255 / 100) as u8
                            } else {
                                nums[4] as u8
                            };
                            self.color_table.insert(reg, [r, g, b_val, 255]);
                            self.current_color = reg;
                        }
                        continue;
                    }
                    b'!' => {
                        i += 1;
                        let mut count = 0;
                        while i < len && data[i].is_ascii_digit() {
                            count = count * 10 + (data[i] - b'0') as usize;
                            i += 1;
                        }
                        if i < len {
                            let char_byte = data[i];
                            i += 1;
                            if (63..=126).contains(&char_byte) {
                                let sixel_val = char_byte - 63;
                                let color = self
                                    .color_table
                                    .get(&self.current_color)
                                    .copied()
                                    .unwrap_or([255, 255, 255, 255]);
                                for _ in 0..count {
                                    for bit in 0..6 {
                                        if (sixel_val & (1 << bit)) != 0 {
                                            let px = self.cur_x;
                                            let py = self.cur_y + bit;
                                            temp_pixels.insert((px, py), color);
                                            max_x = max_x.max(px + 1);
                                            max_y = max_y.max(py + 1);
                                        }
                                    }
                                    self.cur_x += 1;
                                }
                            }
                        }
                        continue;
                    }
                    b'-' => {
                        self.cur_x = 0;
                        self.cur_y += 6;
                        i += 1;
                        continue;
                    }
                    b'$' => {
                        self.cur_x = 0;
                        i += 1;
                        continue;
                    }
                    b'"' => {
                        i += 1;
                        while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                            i += 1;
                        }
                        continue;
                    }
                    63..=126 => {
                        let sixel_val = b - 63;
                        let color = self
                            .color_table
                            .get(&self.current_color)
                            .copied()
                            .unwrap_or([255, 255, 255, 255]);
                        for bit in 0..6 {
                            if (sixel_val & (1 << bit)) != 0 {
                                let px = self.cur_x;
                                let py = self.cur_y + bit;
                                temp_pixels.insert((px, py), color);
                                max_x = max_x.max(px + 1);
                                max_y = max_y.max(py + 1);
                            }
                        }
                        self.cur_x += 1;
                        i += 1;
                        continue;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            if max_x == 0 || max_y == 0 {
                return None;
            }

            let mut rgba_buf = vec![0u8; max_x * max_y * 4];
            for ((x, y), color) in temp_pixels {
                if x < max_x && y < max_y {
                    let offset = (y * max_x + x) * 4;
                    rgba_buf[offset] = color[0];
                    rgba_buf[offset + 1] = color[1];
                    rgba_buf[offset + 2] = color[2];
                    rgba_buf[offset + 3] = color[3];
                }
            }

            Some((max_x as u32, max_y as u32, rgba_buf))
        }
    }

    // ---------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------

    const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE_1: [u8; 4] = [51, 102, 204, 255]; // default palette register 1
    const RED_2: [u8; 4] = [204, 0, 0, 255]; // default palette register 2
    const GREEN_3: [u8; 4] = [51, 204, 51, 255]; // default palette register 3
    const BLACK_0: [u8; 4] = [0, 0, 0, 255]; // default palette register 0
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn decode(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        SixelDecoder::new().decode(data)
    }

    /// Flatten a row-major grid of pixels into the expected RGBA buffer.
    fn expected(pixels: &[&[[u8; 4]]]) -> Vec<u8> {
        pixels
            .iter()
            .flat_map(|row| row.iter().flat_map(|px| px.iter().copied()))
            .collect()
    }

    /// Read pixel (x, y) out of a decoded buffer.
    fn px(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let off = ((y * width + x) * 4) as usize;
        [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]
    }

    // ---------------------------------------------------------------------
    // 1. Exact, hand-computed RGBA output
    // ---------------------------------------------------------------------

    /// `~` (0x7E) is sixel value 63: all six bits set, so one full column.
    /// `#0;2;100;0;0` redefines register 0 as pure red (100% -> 255).
    #[test]
    fn single_full_column_has_exact_rgba_bytes() {
        let (w, h, buf) = decode(b"#0;2;100;0;0~").expect("decodes");
        assert_eq!((w, h), (1, 6));
        assert_eq!(
            buf,
            expected(&[&[RED], &[RED], &[RED], &[RED], &[RED], &[RED]])
        );
    }

    /// Hand-computed 3x2 image exercising unset bits and unset pixels.
    /// `@` = 0x40 -> value 1 -> bit 0 only. `?` = 0x3F -> value 0 -> nothing.
    /// `A` = 0x41 -> value 2 -> bit 1 only.
    #[test]
    fn sparse_bits_leave_untouched_pixels_transparent() {
        let (w, h, buf) = decode(b"#1@?A").expect("decodes");
        assert_eq!((w, h), (3, 2));
        assert_eq!(
            buf,
            expected(&[
                &[BLUE_1, TRANSPARENT, TRANSPARENT],
                &[TRANSPARENT, TRANSPARENT, BLUE_1],
            ])
        );
    }

    /// A zero-valued sixel still advances the cursor but never contributes to
    /// the image extents, so an all-`?` payload decodes to nothing.
    #[test]
    fn payload_without_any_set_bit_decodes_to_none() {
        assert!(decode(b"?????").is_none());
        assert!(decode(b"").is_none());
        assert!(decode(b"\"1;1;10;20").is_none());
    }

    // ---------------------------------------------------------------------
    // 2. `!` repeat introducer
    // ---------------------------------------------------------------------

    #[test]
    fn repeat_introducer_replicates_the_column() {
        let (w, h, buf) = decode(b"#2!4~").expect("decodes");
        assert_eq!((w, h), (4, 6));
        assert_eq!(buf, [RED_2; 24].concat());
    }

    #[test]
    fn repeat_introducer_respects_partial_bit_patterns() {
        // `B` = 0x42 -> value 3 -> bits 0 and 1.
        let (w, h, buf) = decode(b"#1!3B").expect("decodes");
        assert_eq!((w, h), (3, 2));
        assert_eq!(buf, expected(&[&[BLUE_1; 3], &[BLUE_1; 3]]));
    }

    #[test]
    fn repeat_count_zero_emits_nothing_and_does_not_advance() {
        // `!0~` writes nothing and leaves the cursor at x = 0, so the
        // following `@` lands on column 0.
        let (w, h, buf) = decode(b"#1!0~@").expect("decodes");
        assert_eq!((w, h), (1, 1));
        assert_eq!(buf, expected(&[&[BLUE_1]]));
    }

    #[test]
    fn repeat_introducer_without_digits_is_a_zero_count() {
        // `!~` -> count 0: the `~` is consumed but paints nothing.
        assert!(decode(b"#1!~").is_none());
    }

    #[test]
    fn repeat_introducer_swallows_its_following_byte() {
        // The byte after the count is always consumed, even when it is a
        // control character: the `-` here is eaten, so there is no newline.
        let (w, h, _buf) = decode(b"#1!2-@").expect("decodes");
        assert_eq!((w, h), (1, 1));
    }

    // ---------------------------------------------------------------------
    // 3. `$` carriage return and `-` newline
    // ---------------------------------------------------------------------

    #[test]
    fn carriage_return_and_newline_place_bands_correctly() {
        // `@` at (0,0) blue, `$` back to x=0, `A` at (0,1) red, `-` down a
        // band, `@` at (0,6) green.
        let (w, h, buf) = decode(b"#1@$#2A-#3@").expect("decodes");
        assert_eq!((w, h), (1, 7));
        assert_eq!(
            buf,
            expected(&[
                &[BLUE_1],
                &[RED_2],
                &[TRANSPARENT],
                &[TRANSPARENT],
                &[TRANSPARENT],
                &[TRANSPARENT],
                &[GREEN_3],
            ])
        );
    }

    #[test]
    fn newline_resets_x_and_advances_six_rows() {
        let (w, h, buf) = decode(b"#1@@-#2@").expect("decodes");
        assert_eq!((w, h), (2, 7));
        assert_eq!(px(&buf, w, 0, 0), BLUE_1);
        assert_eq!(px(&buf, w, 1, 0), BLUE_1);
        assert_eq!(px(&buf, w, 0, 6), RED_2);
        assert_eq!(px(&buf, w, 1, 6), TRANSPARENT);
    }

    #[test]
    fn overlapping_writes_keep_the_last_colour() {
        // Same pixel painted twice; the later colour wins.
        let (w, h, buf) = decode(b"#1@$#2@").expect("decodes");
        assert_eq!((w, h), (1, 1));
        assert_eq!(buf, expected(&[&[RED_2]]));
    }

    // ---------------------------------------------------------------------
    // 4. Colour definition and selection
    // ---------------------------------------------------------------------

    #[test]
    fn colour_definition_uses_percentage_scaling_below_101() {
        // 50% -> 50 * 255 / 100 = 127 (integer division).
        let (_, _, buf) = decode(b"#9;2;50;100;0@").expect("decodes");
        assert_eq!(&buf[..4], &[127, 255, 0, 255]);
    }

    #[test]
    fn colour_definition_passes_through_values_above_100() {
        let (_, _, buf) = decode(b"#9;2;200;101;255@").expect("decodes");
        assert_eq!(&buf[..4], &[200, 101, 255, 255]);
    }

    #[test]
    fn colour_definition_also_selects_the_register() {
        // Define register 5, then paint without re-selecting it.
        let (_, _, buf) = decode(b"#5;2;0;0;100@").expect("decodes");
        assert_eq!(&buf[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn colour_selection_switches_between_registers() {
        let (w, _, buf) = decode(b"#1@#2@#3@").expect("decodes");
        assert_eq!(px(&buf, w, 0, 0), BLUE_1);
        assert_eq!(px(&buf, w, 1, 0), RED_2);
        assert_eq!(px(&buf, w, 2, 0), GREEN_3);
    }

    #[test]
    fn selecting_an_undefined_register_falls_back_to_white() {
        let (_, _, buf) = decode(b"#250@").expect("decodes");
        assert_eq!(&buf[..4], &WHITE);
    }

    #[test]
    fn non_rgb_colour_introducer_is_ignored() {
        // `;1;` is HLS, which this decoder does not implement: the register is
        // neither defined nor selected, so register 0 stays current.
        let (_, _, buf) = decode(b"#4;1;120;50;100@").expect("decodes");
        assert_eq!(&buf[..4], &BLACK_0);
    }

    #[test]
    fn raster_attributes_are_skipped() {
        let (w, h, buf) = decode(b"\"1;1;10;20@").expect("decodes");
        assert_eq!((w, h), (1, 1));
        assert_eq!(buf, expected(&[&[BLACK_0]]));
    }

    #[test]
    fn decoder_state_persists_across_calls() {
        let mut dec = SixelDecoder::new();
        let (w1, h1, _) = dec.decode(b"#9;2;100;0;0@").expect("first decode");
        assert_eq!((w1, h1), (1, 1));

        // The cursor is still at x = 1 and register 9 is still selected.
        let (w2, h2, buf2) = dec.decode(b"@").expect("second decode");
        assert_eq!((w2, h2), (2, 1));
        assert_eq!(px(&buf2, w2, 0, 0), TRANSPARENT);
        assert_eq!(px(&buf2, w2, 1, 0), RED);
    }

    // ---------------------------------------------------------------------
    // 5. Malformed / truncated payloads must not panic or hang
    // ---------------------------------------------------------------------

    #[test]
    fn malformed_payloads_do_not_panic_or_hang() {
        let cases: &[&[u8]] = &[
            b"#",
            b"!",
            b"$",
            b"-",
            b"\"",
            b"#;",
            b"#;;;;;",
            b"##",
            b"#0;2;100",
            b"#0;2;100;0",
            b"#0;2;",
            b"!5",
            b"!5#",
            b"!!!!",
            b"\"1;1;",
            b"\x00\x01\x02\xff\xfe",
            b"~\x80\x90~",
            b"#1@-$-$-!3~",
            b"#0;2;100;0;0",
            &[b'-'; 64],
            &[b'!'; 64],
            &[b'#'; 64],
        ];
        for case in cases {
            // Must return, and must not panic, whatever it decides to return.
            let _ = SixelDecoder::new().decode(case);
        }
    }

    #[test]
    fn truncated_prefixes_of_a_valid_payload_never_panic() {
        let full: &[u8] = b"\"1;1;8;12#0;2;100;0;0!4~$#1;2;0;100;0!2@-#2!3B$~";
        for n in 0..=full.len() {
            let _ = SixelDecoder::new().decode(&full[..n]);
        }
    }

    /// The old decoder looped ~10^12 times on this and overflowed its `usize`
    /// counter on long digit runs (a panic in debug builds).
    #[test]
    fn absurd_counts_are_bounded_instead_of_hanging() {
        let (w, h, buf) = decode(b"#1!999999999~").expect("decodes");
        assert_eq!((w, h), (MAX_SIXEL_DIM as u32, 6));
        assert_eq!(buf.len(), MAX_SIXEL_DIM * 6 * 4);

        let mut repeat = b"#1!".to_vec();
        repeat.extend_from_slice(&[b'9'; 64]);
        repeat.push(b'~');
        assert!(decode(&repeat).is_some());

        let mut register = b"#".to_vec();
        register.extend_from_slice(&[b'9'; 64]);
        register.push(b'@');
        assert!(decode(&register).is_some());
    }

    /// An image past the pixel budget is dropped rather than allocated.
    #[test]
    fn oversized_images_are_rejected() {
        let mut payload = b"#1!20000~".to_vec();
        payload.extend_from_slice(&[b'-'; 700]);
        payload.extend_from_slice(b"#1@");
        // 16384 x 4201 = ~68.8M pixels, over the 64M budget.
        assert!(decode(&payload).is_none());
    }

    // ---------------------------------------------------------------------
    // 6. Differential test against the reference implementation
    // ---------------------------------------------------------------------

    /// Deterministic xorshift so failures are reproducible.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Build a pseudo-random but structurally plausible sixel payload. Counts
    /// and lengths are kept small so the reference implementation stays cheap.
    fn random_payload(rng: &mut Rng) -> Vec<u8> {
        let mut out = Vec::new();
        let tokens = 4 + rng.below(56);
        for _ in 0..tokens {
            match rng.below(10) {
                0..=3 => out.push(63 + rng.below(64) as u8),
                4 => {
                    out.extend_from_slice(format!("!{}", rng.below(5)).as_bytes());
                    out.push(63 + rng.below(64) as u8);
                }
                5 => out.push(b'$'),
                6 => out.push(b'-'),
                7 => out.extend_from_slice(format!("#{}", rng.below(24)).as_bytes()),
                8 => {
                    let s = format!(
                        "#{};2;{};{};{}",
                        rng.below(24),
                        rng.below(256),
                        rng.below(256),
                        rng.below(256)
                    );
                    out.extend_from_slice(s.as_bytes());
                }
                _ => {
                    // Garbage: anything outside the sixel data range, plus the
                    // odd stray introducer.
                    out.push(rng.below(63) as u8);
                }
            }
        }
        out
    }

    #[test]
    fn matches_reference_implementation_on_random_payloads() {
        let mut rng = Rng(0x5eed_1234_9abc_def1);
        for iteration in 0..512 {
            let payload = random_payload(&mut rng);
            // Truncate half the payloads to exercise partial tokens.
            let payload = if iteration % 2 == 0 {
                let cut = payload.len() - (rng.below(payload.len() as u64 + 1) as usize);
                payload[..cut].to_vec()
            } else {
                payload
            };

            let got = SixelDecoder::new().decode(&payload);
            let want = RefDecoder::new().decode(&payload);
            assert_eq!(
                got,
                want,
                "iteration {iteration} mismatch for payload {:?}",
                String::from_utf8_lossy(&payload)
            );
        }
    }

    #[test]
    fn matches_reference_implementation_across_sequential_calls() {
        // The decoder keeps cursor and palette state between calls; make sure
        // the refactor preserves that too.
        let mut rng = Rng(0x1234_5678_9abc_def0);
        let mut got_dec = SixelDecoder::new();
        let mut want_dec = RefDecoder::new();
        for iteration in 0..128 {
            let payload = random_payload(&mut rng);
            let got = got_dec.decode(&payload);
            let want = want_dec.decode(&payload);
            assert_eq!(got, want, "iteration {iteration} mismatch");
            assert_eq!(got_dec.cur_x, want_dec.cur_x, "cur_x drifted");
            assert_eq!(got_dec.cur_y, want_dec.cur_y, "cur_y drifted");
            assert_eq!(
                got_dec.current_color, want_dec.current_color,
                "current_color drifted"
            );
            assert_eq!(got_dec.color_table, want_dec.color_table, "palette drifted");

            // Keep the coordinates bounded across 128 rounds.
            if iteration % 8 == 7 {
                got_dec = SixelDecoder::new();
                want_dec = RefDecoder::new();
            }
        }
    }
}
