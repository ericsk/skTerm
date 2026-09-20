//! Procedural gap-free rendering for Unicode Block Elements (U+2580..=U+259F)
//! and Box Drawing characters (U+2500..=U+257F), matching Ghostty/Kitty/Alacritty behavior.

use crate::pipeline::BgInstance;

/// Attempt to render a Unicode Block Element or Box Drawing character as exact
/// cell-aligned geometric rectangles (`BgInstance`s) without font glyph gaps/seams.
///
/// Returns `true` if the character was procedurally drawn, `false` if it should
/// fall back to standard font glyph rasterization.
pub fn draw_custom_block_or_box_char(
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) -> bool {
    let code = c as u32;

    if (0x2580..=0x259F).contains(&code) {
        draw_block_element(code, x, y, w, h, color, instances);
        return true;
    }

    if (0x2500..=0x257F).contains(&code)
        && draw_box_drawing(code, x, y, w, h, color, instances) {
            return true;
        }

    false
}

fn push_rect(instances: &mut Vec<BgInstance>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    if w > 0.0 && h > 0.0 {
        instances.push(BgInstance {
            rect: [x, y, w, h],
            color,
        });
    }
}

fn draw_block_element(
    code: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let w_left = (w * 0.5).round();
    let w_right = w - w_left;
    let h_top = (h * 0.5).round();
    let h_bot = h - h_top;

    match code {
        // U+2580: Upper Half Block (▀)
        0x2580 => push_rect(instances, x, y, w, h_top, color),

        // U+2581..=U+2587: Lower N/8 Blocks (▁ ▂ ▃ ▄ ▅ ▆ ▇)
        0x2581..=0x2587 => {
            let n = (code - 0x2580) as f32;
            let bh = if code == 0x2584 {
                h_bot
            } else {
                (h * (n / 8.0)).round().clamp(1.0, h)
            };
            push_rect(instances, x, y + h - bh, w, bh, color);
        }

        // U+2588: Full Block (█)
        0x2588 => push_rect(instances, x, y, w, h, color),

        // U+2589..=U+258F: Left N/8 Blocks (▉ ▊ ▋ ▌ ▍ ▎ ▏)
        0x2589..=0x258F => {
            let n = (8 - (code - 0x2588)) as f32;
            let bw = if code == 0x258C {
                w_left
            } else {
                (w * (n / 8.0)).round().clamp(1.0, w)
            };
            push_rect(instances, x, y, bw, h, color);
        }

        // U+2590: Right Half Block (▐)
        0x2590 => push_rect(instances, x + w_left, y, w_right, h, color),

        // U+2591: Light Shade (░)
        0x2591 => {
            let mut c = color;
            c[3] *= 0.25;
            push_rect(instances, x, y, w, h, c);
        }

        // U+2592: Medium Shade (▒)
        0x2592 => {
            let mut c = color;
            c[3] *= 0.50;
            push_rect(instances, x, y, w, h, c);
        }

        // U+2593: Dark Shade (▓)
        0x2593 => {
            let mut c = color;
            c[3] *= 0.75;
            push_rect(instances, x, y, w, h, c);
        }

        // U+2594: Upper One Eighth Block (▔)
        0x2594 => {
            let bh = (h / 8.0).round().clamp(1.0, h);
            push_rect(instances, x, y, w, bh, color);
        }

        // U+2595: Right One Eighth Block (▕)
        0x2595 => {
            let bw = (w / 8.0).round().clamp(1.0, w);
            push_rect(instances, x + w - bw, y, bw, h, color);
        }

        // U+2596..=U+259F: Quadrant Blocks
        // Bitmask of quadrants: [UL, UR, LL, LR]
        0x2596..=0x259F => {
            let mask: u8 = match code {
                0x2596 => 0b0010, // ▖ LL
                0x2597 => 0b0001, // ▗ LR
                0x2598 => 0b1000, // ▘ UL
                0x2599 => 0b1011, // ▙ UL + LL + LR
                0x259A => 0b1001, // ▞ UL + LR
                0x259B => 0b1110, // ▛ UL + UR + LL
                0x259C => 0b1101, // ▜ UL + UR + LR
                0x259D => 0b0100, // ▝ UR
                0x259E => 0b0110, // ▞ UR + LL
                0x259F => 0b0111, // ▟ UR + LL + LR
                _ => 0,
            };

            // Optimize full halves if both quadrants in a row/col are active
            if mask == 0b1111 {
                push_rect(instances, x, y, w, h, color);
            } else {
                if (mask & 0b1100) == 0b1100 {
                    push_rect(instances, x, y, w, h_top, color);
                } else {
                    if (mask & 0b1000) != 0 {
                        push_rect(instances, x, y, w_left, h_top, color);
                    }
                    if (mask & 0b0100) != 0 {
                        push_rect(instances, x + w_left, y, w_right, h_top, color);
                    }
                }

                if (mask & 0b0011) == 0b0011 {
                    push_rect(instances, x, y + h_top, w, h_bot, color);
                } else {
                    if (mask & 0b0010) != 0 {
                        push_rect(instances, x, y + h_top, w_left, h_bot, color);
                    }
                    if (mask & 0b0001) != 0 {
                        push_rect(instances, x + w_left, y + h_top, w_right, h_bot, color);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Draw standard Box Drawing characters (U+2500..=U+257F) using cell-aligned strokes.
fn draw_box_drawing(
    code: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) -> bool {
    let thin = (w / 7.0).round().clamp(1.0, 3.0);
    let heavy = (thin * 2.0).max(2.0);

    // Arms: (left, right, up, down) where 0 = none, 1 = thin, 2 = heavy
    let arms: Option<(u8, u8, u8, u8)> = match code {
        0x2500 | 0x2504 | 0x2508 | 0x254C => Some((1, 1, 0, 0)), // ─
        0x2501 | 0x2505 | 0x2509 | 0x254D => Some((2, 2, 0, 0)), // ━
        0x2502 | 0x2506 | 0x250A | 0x254E => Some((0, 0, 1, 1)), // │
        0x2503 | 0x2507 | 0x250B | 0x254F => Some((0, 0, 2, 2)), // ┃
        0x250C | 0x256D => Some((0, 1, 0, 1)),                   // ┌ ╭
        0x250F => Some((0, 2, 0, 2)),                            // ┏
        0x2510 | 0x256E => Some((1, 0, 0, 1)),                   // ┐ ╮
        0x2513 => Some((2, 0, 0, 2)),                            // ┓
        0x2514 | 0x2570 => Some((0, 1, 1, 0)),                   // └ ╰
        0x2517 => Some((0, 2, 2, 0)),                            // ┗
        0x2518 | 0x256F => Some((1, 0, 1, 0)),                   // ┘ ╯
        0x251B => Some((2, 0, 2, 0)),                            // ┛
        0x251C => Some((0, 1, 1, 1)),                            // ├
        0x2523 => Some((0, 2, 2, 2)),                            // ┣
        0x2524 => Some((1, 0, 1, 1)),                            // ┤
        0x252B => Some((2, 0, 2, 2)),                            // ┫
        0x252C => Some((1, 1, 0, 1)),                            // ┬
        0x2533 => Some((2, 2, 0, 2)),                            // ┳
        0x2534 => Some((1, 1, 1, 0)),                            // ┴
        0x253B => Some((2, 2, 2, 0)),                            // ┻
        0x253C => Some((1, 1, 1, 1)),                            // ┼
        0x254B => Some((2, 2, 2, 2)),                            // ╋
        0x2574 => Some((1, 0, 0, 0)),                            // ╴
        0x2575 => Some((0, 0, 1, 0)),                            // ╵
        0x2576 => Some((0, 1, 0, 0)),                            // ╶
        0x2577 => Some((0, 0, 0, 1)),                            // ╷
        0x2578 => Some((2, 0, 0, 0)),                            // ╸
        0x2579 => Some((0, 0, 2, 0)),                            // ╹
        0x257A => Some((0, 2, 0, 0)),                            // ╺
        0x257B => Some((0, 0, 0, 2)),                            // ╻
        0x2550 => Some((2, 2, 0, 0)),                            // ═ (approx heavy/double)
        0x2551 => Some((0, 0, 2, 2)),                            // ║
        _ => None,
    };

    if let Some((l, r, u, d)) = arms {
        let stroke_t = |level: u8| -> f32 {
            match level {
                2 => heavy,
                _ => thin,
            }
        };

        let max_h_stroke = stroke_t(l.max(r));
        let max_v_stroke = stroke_t(u.max(d));

        let cx = x + ((w - max_v_stroke) * 0.5).round();
        let cy = y + ((h - max_h_stroke) * 0.5).round();

        if l > 0 {
            let st = stroke_t(l);
            let arm_y = y + ((h - st) * 0.5).round();
            push_rect(instances, x, arm_y, cx + max_v_stroke - x, st, color);
        }
        if r > 0 {
            let st = stroke_t(r);
            let arm_y = y + ((h - st) * 0.5).round();
            push_rect(instances, cx, arm_y, x + w - cx, st, color);
        }
        if u > 0 {
            let st = stroke_t(u);
            let arm_x = x + ((w - st) * 0.5).round();
            push_rect(instances, arm_x, y, st, cy + max_h_stroke - y, color);
        }
        if d > 0 {
            let st = stroke_t(d);
            let arm_x = x + ((w - st) * 0.5).round();
            push_rect(instances, arm_x, cy, st, y + h - cy, color);
        }
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_half_blocks_seamless() {
        let mut instances = Vec::new();
        let x = 10.0;
        let y = 20.0;
        let w = 9.0;
        let h = 19.0; // odd height to test rounding
        let color = [1.0, 1.0, 1.0, 1.0];

        // Upper half block '▀' (U+2580)
        assert!(draw_custom_block_or_box_char('▀', x, y, w, h, color, &mut instances));
        // Lower half block '▄' (U+2584)
        assert!(draw_custom_block_or_box_char('▄', x, y, w, h, color, &mut instances));

        assert_eq!(instances.len(), 2);
        let top = instances[0].rect;
        let bot = instances[1].rect;

        // Top and bottom must meet at exact same Y coordinate with zero gap
        assert_eq!(top[0], x);
        assert_eq!(top[2], w);
        assert_eq!(bot[0], x);
        assert_eq!(bot[2], w);
        assert_eq!(top[1] + top[3], bot[1]);
        assert_eq!(top[3] + bot[3], h);
    }
}
