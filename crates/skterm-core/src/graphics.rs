//! Terminal Graphics Protocols: DEC Sixel and Kitty Graphics Protocol Decoders.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::collections::HashMap;
use tracing::{info, warn};

/// Decoded Graphic Image to be placed and rendered in the Terminal Grid.
#[derive(Debug, Clone)]
pub struct GraphicImage {
    pub id: u32,
    pub row: usize,
    pub col: usize,
    pub width_px: u32,
    pub height_px: u32,
    pub width_cells: usize,
    pub height_cells: usize,
    pub rgba_data: Vec<u8>,
    pub z_index: i32,
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

/// Kitty Graphics Protocol Chunk Assembler & Parser
#[derive(Debug, Clone)]
pub struct KittyGraphicsParser {
    chunk_buffer: Vec<u8>,
    current_params: HashMap<String, String>,
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
            current_params: HashMap::new(),
        }
    }

    /// Parse Kitty Graphics payload: `G<keys>;<base64_payload>`
    pub fn parse_command(&mut self, cmd: &str) -> Option<(u32, u32, Vec<u8>, i32)> {
        let parts: Vec<&str> = cmd.splitn(2, ';').collect();
        let control_str = parts[0];
        let payload_str = if parts.len() > 1 { parts[1].trim() } else { "" };

        let pairs = control_str.split(',');
        for pair in pairs {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() == 2 {
                self.current_params
                    .insert(kv[0].to_string(), kv[1].to_string());
            }
        }

        if !payload_str.is_empty() {
            if let Ok(decoded_bytes) = BASE64_STANDARD.decode(payload_str) {
                self.chunk_buffer.extend_from_slice(&decoded_bytes);
            }
        }

        let is_more = self
            .current_params
            .get("m")
            .map(|v| v == "1")
            .unwrap_or(false);
        if is_more {
            return None; // Wait for further chunks
        }

        let format = self
            .current_params
            .get("f")
            .cloned()
            .unwrap_or_else(|| "32".to_string());
        let width = self
            .current_params
            .get("s")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let height = self
            .current_params
            .get("v")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let z_index = self
            .current_params
            .get("z")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);

        let data = std::mem::take(&mut self.chunk_buffer);
        self.current_params.clear();

        match format.as_str() {
            "100" => {
                // PNG Format
                if let Ok(img) = image::load_from_memory(&data) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    info!("Decoded Kitty PNG image: {}x{} px", w, h);
                    Some((w, h, rgba.into_raw(), z_index))
                } else {
                    warn!("Failed to decode Kitty PNG image from memory");
                    None
                }
            }
            "24" => {
                // RGB Raw Format
                if width > 0 && height > 0 && data.len() >= (width * height * 3) as usize {
                    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                    for chunk in data.as_chunks::<3>().0 {
                        rgba.push(chunk[0]);
                        rgba.push(chunk[1]);
                        rgba.push(chunk[2]);
                        rgba.push(255);
                    }
                    Some((width, height, rgba, z_index))
                } else {
                    None
                }
            }
            _ => {
                // 32-bit RGBA Raw Format (default f=32)
                if width > 0 && height > 0 && data.len() >= (width * height * 4) as usize {
                    Some((width, height, data, z_index))
                } else if let Ok(img) = image::load_from_memory(&data) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    Some((w, h, rgba.into_raw(), z_index))
                } else {
                    None
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
