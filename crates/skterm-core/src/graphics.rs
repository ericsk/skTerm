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

    /// Decode raw Sixel byte stream into RGBA pixels
    pub fn decode(&mut self, data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        let mut i = 0;
        let len = data.len();

        let mut max_x = 0;
        let mut max_y = 0;

        let mut temp_pixels: HashMap<(usize, usize), [u8; 4]> = HashMap::new();

        while i < len {
            let b = data[i];
            match b {
                b'#' => {
                    // Color definition or selection
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
                        // RGB definition: #reg;2;r;g;b (percentage 0..100 or 0..255)
                        let reg = nums[0] as u16;
                        let r = if nums[2] <= 100 { (nums[2] * 255 / 100) as u8 } else { nums[2] as u8 };
                        let g = if nums[3] <= 100 { (nums[3] * 255 / 100) as u8 } else { nums[3] as u8 };
                        let b_val = if nums[4] <= 100 { (nums[4] * 255 / 100) as u8 } else { nums[4] as u8 };
                        self.color_table.insert(reg, [r, g, b_val, 255]);
                        self.current_color = reg;
                    }
                    continue;
                }
                b'!' => {
                    // Repeat count: !<count><char>
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
                            let color = self.color_table.get(&self.current_color).copied().unwrap_or([255, 255, 255, 255]);
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
                    // Line feed: advance 6 pixels down and return to start
                    self.cur_x = 0;
                    self.cur_y += 6;
                    i += 1;
                    continue;
                }
                b'$' => {
                    // Carriage return: return to x = 0
                    self.cur_x = 0;
                    i += 1;
                    continue;
                }
                b'"' => {
                    // Raster attributes: "Pan;Pad;Ph;Pv (skip attribute header)
                    i += 1;
                    while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                        i += 1;
                    }
                    continue;
                }
                63..=126 => {
                    // Sixel data character
                    let sixel_val = b - 63;
                    let color = self.color_table.get(&self.current_color).copied().unwrap_or([255, 255, 255, 255]);
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
                self.current_params.insert(kv[0].to_string(), kv[1].to_string());
            }
        }

        if !payload_str.is_empty() {
            if let Ok(decoded_bytes) = BASE64_STANDARD.decode(payload_str) {
                self.chunk_buffer.extend_from_slice(&decoded_bytes);
            }
        }

        let is_more = self.current_params.get("m").map(|v| v == "1").unwrap_or(false);
        if is_more {
            return None; // Wait for further chunks
        }

        let format = self.current_params.get("f").cloned().unwrap_or_else(|| "32".to_string());
        let width = self.current_params.get("s").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let height = self.current_params.get("v").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let z_index = self.current_params.get("z").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);

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
                    for chunk in data.chunks_exact(3) {
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
