//! Procedural gap-free rendering for Unicode Block Elements (`U+2580..=U+259F`),
//! full Box Drawing (`U+2500..=U+257F`, including rounded corners, double lines,
//! dashed lines, mixed light/heavy junctions, and diagonals), Braille Patterns
//! (`U+2800..=U+28FF`), and Symbols for Legacy Computing Sextants (`U+1FB00..=U+1FB3B`),
//! matching Ghostty/Kitty/Alacritty geometric crispness.

use crate::pipeline::BgInstance;

/// Attempt to render a geometric Unicode character (Block Element, Box Drawing,
/// Braille Pattern, or Legacy Computing Sextant) as exact cell-aligned rectangles
/// (`BgInstance`s) without font glyph gaps or fallback seams.
///
/// Returns `true` if the character was procedurally handled (including blank
/// Braille `U+2800`), or `false` if it should fall back to standard font glyph
/// rasterization.
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

    if (0x2500..=0x257F).contains(&code) {
        return draw_box_drawing(code, x, y, w, h, color, instances);
    }

    if (0x2800..=0x28FF).contains(&code) {
        draw_braille(code, x, y, w, h, color, instances);
        return true;
    }

    if (0x1FB00..=0x1FB3B).contains(&code) {
        draw_sextant(code, x, y, w, h, color, instances);
        return true;
    }

    false
}

#[inline]
fn push_rect(instances: &mut Vec<BgInstance>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    if w > 0.0 && h > 0.0 {
        instances.push(BgInstance {
            rect: [x, y, w, h],
            color,
        });
    }
}

#[inline]
fn push_span(instances: &mut Vec<BgInstance>, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]) {
    let w = x1 - x0;
    let h = y1 - y0;
    if w > 0.0 && h > 0.0 {
        instances.push(BgInstance {
            rect: [x0, y0, w, h],
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
                0x259A => 0b1001, // ▚ UL + LR
                0x259B => 0b1110, // ▛ UL + UR + LL
                0x259C => 0b1101, // ▜ UL + UR + LR
                0x259D => 0b0100, // ▝ UR
                0x259E => 0b0110, // ▞ UR + LL
                0x259F => 0b0111, // ▟ UR + LL + LR
                _ => 0,
            };

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arm {
    None,
    Light,
    Heavy,
    Double,
}

/// Draw all 128 Unicode Box Drawing characters (`U+2500..=U+257F`) using cell-aligned geometry.
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
    let heavy = (thin * 2.0).max(2.0).min(w.min(h));

    // 1. Dashed horizontal & vertical lines (2-, 3-, and 4-dash)
    match code {
        0x2504 => {
            draw_dashed_horiz(x, y, w, h, thin, 3, color, instances);
            return true;
        }
        0x2505 => {
            draw_dashed_horiz(x, y, w, h, heavy, 3, color, instances);
            return true;
        }
        0x2506 => {
            draw_dashed_vert(x, y, w, h, thin, 3, color, instances);
            return true;
        }
        0x2507 => {
            draw_dashed_vert(x, y, w, h, heavy, 3, color, instances);
            return true;
        }
        0x2508 => {
            draw_dashed_horiz(x, y, w, h, thin, 4, color, instances);
            return true;
        }
        0x2509 => {
            draw_dashed_horiz(x, y, w, h, heavy, 4, color, instances);
            return true;
        }
        0x250A => {
            draw_dashed_vert(x, y, w, h, thin, 4, color, instances);
            return true;
        }
        0x250B => {
            draw_dashed_vert(x, y, w, h, heavy, 4, color, instances);
            return true;
        }
        0x254C => {
            draw_dashed_horiz(x, y, w, h, thin, 2, color, instances);
            return true;
        }
        0x254D => {
            draw_dashed_horiz(x, y, w, h, heavy, 2, color, instances);
            return true;
        }
        0x254E => {
            draw_dashed_vert(x, y, w, h, thin, 2, color, instances);
            return true;
        }
        0x254F => {
            draw_dashed_vert(x, y, w, h, heavy, 2, color, instances);
            return true;
        }
        // 2. Rounded Corners (╭ ╮ ╯ ╰)
        0x256D..=0x2570 => {
            draw_rounded_corner(code, x, y, w, h, thin, color, instances);
            return true;
        }
        // 3. Diagonals (╱ ╲ ╳)
        0x2571 => {
            draw_diagonal(x, y, w, h, thin, true, color, instances);
            return true;
        }
        0x2572 => {
            draw_diagonal(x, y, w, h, thin, false, color, instances);
            return true;
        }
        0x2573 => {
            draw_diagonal(x, y, w, h, thin, true, color, instances);
            draw_diagonal(x, y, w, h, thin, false, color, instances);
            return true;
        }
        _ => {}
    }

    use Arm::{Double as D, Heavy as H, Light as L, None as N};

    // 4. Complete arm table for every remaining Box Drawing codepoint: (left, right, up, down)
    let arms: Option<(Arm, Arm, Arm, Arm)> = match code {
        // Basic straight lines
        0x2500 => Some((L, L, N, N)), // ─
        0x2501 => Some((H, H, N, N)), // ━
        0x2502 => Some((N, N, L, L)), // │
        0x2503 => Some((N, N, H, H)), // ┃
        // Corners (Light / Heavy / Mixed)
        0x250C => Some((N, L, N, L)), // ┌
        0x250D => Some((N, H, N, L)), // ┍
        0x250E => Some((N, L, N, H)), // ┎
        0x250F => Some((N, H, N, H)), // ┏
        0x2510 => Some((L, N, N, L)), // ┐
        0x2511 => Some((H, N, N, L)), // ┑
        0x2512 => Some((L, N, N, H)), // ┒
        0x2513 => Some((H, N, N, H)), // ┓
        0x2514 => Some((N, L, L, N)), // └
        0x2515 => Some((N, H, L, N)), // ┕
        0x2516 => Some((N, L, H, N)), // ┖
        0x2517 => Some((N, H, H, N)), // ┗
        0x2518 => Some((L, N, L, N)), // ┘
        0x2519 => Some((H, N, L, N)), // ┙
        0x251A => Some((L, N, H, N)), // ┚
        0x251B => Some((H, N, H, N)), // ┛
        // Vertical and Right (├ .. ┣)
        0x251C => Some((N, L, L, L)), // ├
        0x251D => Some((N, H, L, L)), // ┝
        0x251E => Some((N, L, H, L)), // ┞
        0x251F => Some((N, L, L, H)), // ┟
        0x2520 => Some((N, L, H, H)), // ┠
        0x2521 => Some((N, H, H, L)), // ┡
        0x2522 => Some((N, H, L, H)), // ┢
        0x2523 => Some((N, H, H, H)), // ┣
        // Vertical and Left (┤ .. ┫)
        0x2524 => Some((L, N, L, L)), // ┤
        0x2525 => Some((H, N, L, L)), // ┥
        0x2526 => Some((L, N, H, L)), // ┦
        0x2527 => Some((L, N, L, H)), // ┧
        0x2528 => Some((L, N, H, H)), // ┨
        0x2529 => Some((H, N, H, L)), // ┩
        0x252A => Some((H, N, L, H)), // ┪
        0x252B => Some((H, N, H, H)), // ┫
        // Down and Horizontal (┬ .. ┳)
        0x252C => Some((L, L, N, L)), // ┬
        0x252D => Some((H, L, N, L)), // ┭
        0x252E => Some((L, H, N, L)), // ┮
        0x252F => Some((H, H, N, L)), // ┯
        0x2530 => Some((L, L, N, H)), // ┰
        0x2531 => Some((H, L, N, H)), // ┱
        0x2532 => Some((L, H, N, H)), // ┲
        0x2533 => Some((H, H, N, H)), // ┳
        // Up and Horizontal (┴ .. ┻)
        0x2534 => Some((L, L, L, N)), // ┴
        0x2535 => Some((H, L, L, N)), // ┵
        0x2536 => Some((L, H, L, N)), // ┶
        0x2537 => Some((H, H, L, N)), // ┷
        0x2538 => Some((L, L, H, N)), // ┸
        0x2539 => Some((H, L, H, N)), // ┹
        0x253A => Some((L, H, H, N)), // ┺
        0x253B => Some((H, H, H, N)), // ┻
        // Vertical and Horizontal Crosses (┼ .. ╋)
        0x253C => Some((L, L, L, L)), // ┼
        0x253D => Some((H, L, L, L)), // ┽
        0x253E => Some((L, H, L, L)), // ┾
        0x253F => Some((H, H, L, L)), // ┿
        0x2540 => Some((L, L, H, L)), // ╀
        0x2541 => Some((L, L, L, H)), // ╁
        0x2542 => Some((L, L, H, H)), // ╂
        0x2543 => Some((H, L, H, L)), // ╃
        0x2544 => Some((L, H, H, L)), // ╄
        0x2545 => Some((H, L, L, H)), // ╅
        0x2546 => Some((L, H, L, H)), // ╆
        0x2547 => Some((H, H, H, L)), // ╇
        0x2548 => Some((H, H, L, H)), // ╈
        0x2549 => Some((H, L, H, H)), // ╉
        0x254A => Some((L, H, H, H)), // ╊
        0x254B => Some((H, H, H, H)), // ╋
        // Double & Mixed-Double Box Drawing (═ .. ╬)
        0x2550 => Some((D, D, N, N)), // ═
        0x2551 => Some((N, N, D, D)), // ║
        0x2552 => Some((N, D, N, L)), // ╒
        0x2553 => Some((N, L, N, D)), // ╓
        0x2554 => Some((N, D, N, D)), // ╔
        0x2555 => Some((D, N, N, L)), // ╕
        0x2556 => Some((L, N, N, D)), // ╖
        0x2557 => Some((D, N, N, D)), // ╗
        0x2558 => Some((N, D, L, N)), // ╘
        0x2559 => Some((N, L, D, N)), // ╙
        0x255A => Some((N, D, D, N)), // ╚
        0x255B => Some((D, N, L, N)), // ╛
        0x255C => Some((L, N, D, N)), // ╜
        0x255D => Some((D, N, D, N)), // ╝
        0x255E => Some((N, D, L, L)), // ╞
        0x255F => Some((N, L, D, D)), // ╟
        0x2560 => Some((N, D, D, D)), // ╠
        0x2561 => Some((D, N, L, L)), // ╡
        0x2562 => Some((L, N, D, D)), // ╢
        0x2563 => Some((D, N, D, D)), // ╣
        0x2564 => Some((D, D, N, L)), // ╤
        0x2565 => Some((L, L, N, D)), // ╥
        0x2566 => Some((D, D, N, D)), // ╦
        0x2567 => Some((D, D, L, N)), // ╧
        0x2568 => Some((L, L, D, N)), // ╨
        0x2569 => Some((D, D, D, N)), // ╩
        0x256A => Some((D, D, L, L)), // ╪
        0x256B => Some((L, L, D, D)), // ╫
        0x256C => Some((D, D, D, D)), // ╬
        // Half lines & Light/Heavy transitions
        0x2574 => Some((L, N, N, N)), // ╴
        0x2575 => Some((N, N, L, N)), // ╵
        0x2576 => Some((N, L, N, N)), // ╶
        0x2577 => Some((N, N, N, L)), // ╷
        0x2578 => Some((H, N, N, N)), // ╸
        0x2579 => Some((N, N, H, N)), // ╹
        0x257A => Some((N, H, N, N)), // ╺
        0x257B => Some((N, N, N, H)), // ╻
        0x257C => Some((L, H, N, N)), // ╼
        0x257D => Some((N, N, L, H)), // ╽
        0x257E => Some((H, L, N, N)), // ╾
        0x257F => Some((N, N, H, L)), // ╿
        _ => None,
    };

    let Some((l, r, u, d)) = arms else {
        return false;
    };

    if matches!(l, D) || matches!(r, D) || matches!(u, D) || matches!(d, D) {
        draw_double_or_mixed_box(l, r, u, d, x, y, w, h, thin, color, instances);
        return true;
    }

    let stroke_t = |arm: Arm| -> f32 {
        match arm {
            Arm::Heavy => heavy,
            Arm::Light => thin,
            _ => 0.0,
        }
    };

    let max_h_stroke = stroke_t(l).max(stroke_t(r)).max(thin);
    let max_v_stroke = stroke_t(u).max(stroke_t(d)).max(thin);

    let cx = x + ((w - max_v_stroke) * 0.5).round();
    let cy = y + ((h - max_h_stroke) * 0.5).round();

    if l != Arm::None {
        let st = stroke_t(l);
        let arm_y = y + ((h - st) * 0.5).round();
        let end_x = if r != Arm::None {
            x + (w * 0.5).round()
        } else {
            cx + max_v_stroke
        };
        push_span(instances, x, arm_y, end_x, arm_y + st, color);
    }
    if r != Arm::None {
        let st = stroke_t(r);
        let arm_y = y + ((h - st) * 0.5).round();
        let start_x = if l != Arm::None {
            x + (w * 0.5).round()
        } else {
            cx
        };
        push_span(instances, start_x, arm_y, x + w, arm_y + st, color);
    }
    if u != Arm::None {
        let st = stroke_t(u);
        let arm_x = x + ((w - st) * 0.5).round();
        let end_y = if d != Arm::None {
            y + (h * 0.5).round()
        } else {
            cy + max_h_stroke
        };
        push_span(instances, arm_x, y, arm_x + st, end_y, color);
    }
    if d != Arm::None {
        let st = stroke_t(d);
        let arm_x = x + ((w - st) * 0.5).round();
        let start_y = if u != Arm::None {
            y + (h * 0.5).round()
        } else {
            cy
        };
        push_span(instances, arm_x, start_y, arm_x + st, y + h, color);
    }

    true
}

/// Draw parallel double-line and mixed light/double box-drawing characters (`U+2550..=U+256C`)
/// with clean inner and outer corner miters.
#[allow(clippy::too_many_arguments)]
fn draw_double_or_mixed_box(
    l: Arm,
    r: Arm,
    u: Arm,
    d: Arm,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thin: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    // Choose parallel line width and inter-line gap that fit comfortably inside the cell.
    let d_thin = if w < 5.0 || h < 5.0 { 1.0 } else { thin };
    let gap = d_thin.max(1.0).min((w - 2.0 * d_thin).max(1.0));
    let total = 2.0 * d_thin + gap;

    let cx_l = x + ((w - total) * 0.5).round();
    let cx_r = cx_l + d_thin + gap;
    let cx_m = x + ((w - d_thin) * 0.5).round();

    let cy_t = y + ((h - total) * 0.5).round();
    let cy_b = cy_t + d_thin + gap;
    let cy_m = y + ((h - d_thin) * 0.5).round();

    let x_end = x + w;
    let y_end = y + h;

    // Horizontal arms
    match l {
        Arm::Double => {
            let top_x1 = match (u, r, d) {
                (Arm::Double, _, _) => cx_l + d_thin,
                (Arm::Light, _, _) => cx_m + d_thin,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cx_r + d_thin,
                _ => cx_m + d_thin,
            };
            let bot_x1 = match (d, r, u) {
                (Arm::Double, _, _) => cx_l + d_thin,
                (Arm::Light, _, _) => cx_m + d_thin,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cx_r + d_thin,
                _ => cx_m + d_thin,
            };
            push_span(instances, x, cy_t, top_x1, cy_t + d_thin, color);
            push_span(instances, x, cy_b, bot_x1, cy_b + d_thin, color);
        }
        Arm::Light => {
            let x1 = if u == Arm::Double || d == Arm::Double {
                if r != Arm::None {
                    cx_l + d_thin
                } else {
                    cx_r + d_thin
                }
            } else {
                cx_m + d_thin
            };
            push_span(instances, x, cy_m, x1, cy_m + d_thin, color);
        }
        _ => {}
    }

    match r {
        Arm::Double => {
            let top_x0 = match (u, l, d) {
                (Arm::Double, _, _) => cx_r,
                (Arm::Light, _, _) => cx_m,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cx_l,
                _ => cx_m,
            };
            let bot_x0 = match (d, l, u) {
                (Arm::Double, _, _) => cx_r,
                (Arm::Light, _, _) => cx_m,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cx_l,
                _ => cx_m,
            };
            push_span(instances, top_x0, cy_t, x_end, cy_t + d_thin, color);
            push_span(instances, bot_x0, cy_b, x_end, cy_b + d_thin, color);
        }
        Arm::Light => {
            let x0 = if u == Arm::Double || d == Arm::Double {
                if l != Arm::None {
                    cx_r
                } else {
                    cx_l
                }
            } else {
                cx_m
            };
            push_span(instances, x0, cy_m, x_end, cy_m + d_thin, color);
        }
        _ => {}
    }

    // Vertical arms
    match u {
        Arm::Double => {
            let left_y1 = match (l, d, r) {
                (Arm::Double, _, _) => cy_t + d_thin,
                (Arm::Light, _, _) => cy_m + d_thin,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cy_b + d_thin,
                _ => cy_m + d_thin,
            };
            let right_y1 = match (r, d, l) {
                (Arm::Double, _, _) => cy_t + d_thin,
                (Arm::Light, _, _) => cy_m + d_thin,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cy_b + d_thin,
                _ => cy_m + d_thin,
            };
            push_span(instances, cx_l, y, cx_l + d_thin, left_y1, color);
            push_span(instances, cx_r, y, cx_r + d_thin, right_y1, color);
        }
        Arm::Light => {
            let y1 = if l == Arm::Double || r == Arm::Double {
                if d != Arm::None {
                    cy_t + d_thin
                } else {
                    cy_b + d_thin
                }
            } else {
                cy_m + d_thin
            };
            push_span(instances, cx_m, y, cx_m + d_thin, y1, color);
        }
        _ => {}
    }

    match d {
        Arm::Double => {
            let left_y0 = match (l, u, r) {
                (Arm::Double, _, _) => cy_b,
                (Arm::Light, _, _) => cy_m,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cy_t,
                _ => cy_m,
            };
            let right_y0 = match (r, u, l) {
                (Arm::Double, _, _) => cy_b,
                (Arm::Light, _, _) => cy_m,
                (Arm::None, Arm::Double, _) | (Arm::None, _, Arm::Double) => cy_t,
                _ => cy_m,
            };
            push_span(instances, cx_l, left_y0, cx_l + d_thin, y_end, color);
            push_span(instances, cx_r, right_y0, cx_r + d_thin, y_end, color);
        }
        Arm::Light => {
            let y0 = if l == Arm::Double || r == Arm::Double {
                if u != Arm::None {
                    cy_b
                } else {
                    cy_t
                }
            } else {
                cy_m
            };
            push_span(instances, cx_m, y0, cx_m + d_thin, y_end, color);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_dashed_horiz(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thickness: f32,
    count: usize,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let cy = y + ((h - thickness) * 0.5).round();
    let step = w / count as f32;
    let dash_w = (step * 0.6).round().clamp(1.0, (step - 1.0).max(1.0));
    let lead = ((step - dash_w) * 0.5).floor().max(0.0);
    for i in 0..count {
        let sx = (x + i as f32 * step + lead).min(x + w - 1.0);
        let sw = dash_w.min(x + w - sx);
        push_rect(instances, sx, cy, sw, thickness, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_dashed_vert(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thickness: f32,
    count: usize,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let cx = x + ((w - thickness) * 0.5).round();
    let step = h / count as f32;
    let dash_h = (step * 0.6).round().clamp(1.0, (step - 1.0).max(1.0));
    let lead = ((step - dash_h) * 0.5).floor().max(0.0);
    for i in 0..count {
        let sy = (y + i as f32 * step + lead).min(y + h - 1.0);
        let sh = dash_h.min(y + h - sy);
        push_rect(instances, cx, sy, thickness, sh, color);
    }
}

/// Draw smooth quarter-circle rounded box corners (`╭ ╮ ╯ ╰` = `U+256D..=U+2570`)
/// seamlessly meeting the horizontal and vertical cell edges.
#[allow(clippy::too_many_arguments)]
fn draw_rounded_corner(
    code: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thin: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let cx = x + ((w - thin) * 0.5).round();
    let cy = y + ((h - thin) * 0.5).round();
    let right_room = (x + w - (cx + thin)).max(0.0);
    let left_room = (cx - x).max(0.0);
    let down_room = (y + h - (cy + thin)).max(0.0);
    let up_room = (cy - y).max(0.0);

    let (goes_right, goes_down) = match code {
        0x256D => (true, true),   // ╭
        0x256E => (false, true),  // ╮
        0x256F => (false, false), // ╯
        0x2570 => (true, false),  // ╰
        _ => return,
    };

    let h_room = if goes_right { right_room } else { left_room };
    let v_room = if goes_down { down_room } else { up_room };
    // Outer radius of the quarter-circle measured from arc center to outer stroke edge.
    let inner_r = h_room.min(v_room).floor().max(1.0);
    let outer_r = inner_r + thin;

    // Straight tangent arms extending from the end of the arc to the cell edges.
    if goes_right {
        let arm_x0 = cx + outer_r;
        push_span(instances, arm_x0, cy, x + w, cy + thin, color);
    } else {
        let arm_x1 = cx + thin - outer_r;
        push_span(instances, x, cy, arm_x1, cy + thin, color);
    }

    if goes_down {
        let arm_y0 = cy + outer_r;
        push_span(instances, cx, arm_y0, cx + thin, y + h, color);
    } else {
        let arm_y1 = cy + thin - outer_r;
        push_span(instances, cx, y, cx + thin, arm_y1, color);
    }

    // Center of the quarter-circle in pixel coordinates.
    let arc_cx = if goes_right {
        cx + outer_r
    } else {
        cx + thin - outer_r
    };
    let arc_cy = if goes_down {
        cy + outer_r
    } else {
        cy + thin - outer_r
    };

    // Rasterize the quarter-circle annulus (inner_r .. outer_r) in 1px horizontal slices.
    let rows = outer_r.ceil() as i32;
    let r_out_sq = outer_r * outer_r;
    let r_in_sq = inner_r * inner_r;
    let min_span = thin.max(1.0);

    for step in 0..rows {
        let dy_near = step as f32;
        let dy_far = (step as f32 + 1.0).min(outer_r);
        let dy_mid = (dy_near + dy_far) * 0.5;

        let x_outer = (r_out_sq - dy_mid * dy_mid).max(0.0).sqrt();
        let x_inner = if dy_mid < inner_r {
            (r_in_sq - dy_mid * dy_mid).max(0.0).sqrt()
        } else {
            0.0
        };

        // Ensure thin diagonal steps remain at least 1px wide and connect with neighbors.
        let dx_outer = x_outer.round().clamp(1.0, outer_r);
        let mut dx_inner = x_inner.floor().clamp(0.0, outer_r);
        if dx_outer - dx_inner < min_span && dy_mid < inner_r {
            dx_inner = (dx_outer - min_span).max(0.0);
        }

        let slice_y = if goes_down {
            arc_cy - dy_far
        } else {
            arc_cy + dy_near
        };
        let slice_h = dy_far - dy_near;

        let (sx0, sx1) = if goes_right {
            (arc_cx - dx_outer, arc_cx - dx_inner)
        } else {
            (arc_cx + dx_inner, arc_cx + dx_outer)
        };

        push_span(
            instances,
            sx0.max(x),
            slice_y.max(y),
            sx1.min(x + w),
            (slice_y + slice_h).min(y + h),
            color,
        );
    }
}

/// Draw cell-spanning diagonal lines (`╱` = bottom-left to top-right, `╲` = top-left to bottom-right).
#[allow(clippy::too_many_arguments)]
fn draw_diagonal(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thin: f32,
    ascending: bool,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let steps = h.ceil().max(1.0) as i32;
    let half_w = (thin * 0.75).max(0.75);
    for row in 0..steps {
        let t = (row as f32 + 0.5) / steps as f32;
        let cx = if ascending {
            x + (1.0 - t) * w
        } else {
            x + t * w
        };
        let x0 = (cx - half_w).floor().max(x);
        let x1 = (cx + half_w).ceil().min(x + w);
        let y0 = y + row as f32;
        let y1 = (y0 + 1.0).min(y + h);
        push_span(instances, x0, y0, x1, y1, color);
    }
}

/// Draw Unicode Braille Patterns (`U+2800..=U+28FF`) as a uniform 2x4 pixel-crisp dot matrix.
///
/// Inter-cell dot pitch matches intra-cell dot pitch so tiled Braille charts (`btop`, `chafa`,
/// TUI animations) render seamlessly without uneven gaps.
fn draw_braille(
    code: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    let bits = (code - 0x2800) as u8;
    if bits == 0 {
        return;
    }

    let col_w = w / 2.0;
    let row_h = h / 4.0;
    let dot_w = (col_w * 0.52).round().clamp(1.0, (col_w - 1.0).max(1.0));
    let dot_h = (row_h * 0.52).round().clamp(1.0, (row_h - 1.0).max(1.0));
    let dot_s = dot_w.min(dot_h).max(1.0);

    // Standard Unicode Braille dot bit-to-(col, row) layout:
    //   bit 0 (0x01): (0, 0)    bit 3 (0x08): (1, 0)
    //   bit 1 (0x02): (0, 1)    bit 4 (0x10): (1, 1)
    //   bit 2 (0x04): (0, 2)    bit 5 (0x20): (1, 2)
    //   bit 6 (0x40): (0, 3)    bit 7 (0x80): (1, 3)
    const DOT_POS: [(u8, usize, usize); 8] = [
        (0x01, 0, 0),
        (0x02, 0, 1),
        (0x04, 0, 2),
        (0x40, 0, 3),
        (0x08, 1, 0),
        (0x10, 1, 1),
        (0x20, 1, 2),
        (0x80, 1, 3),
    ];

    let col_x = [
        x + (0.5 * col_w - dot_s * 0.5).round(),
        x + (1.5 * col_w - dot_s * 0.5).round(),
    ];
    let row_y = [
        y + (0.5 * row_h - dot_s * 0.5).round(),
        y + (1.5 * row_h - dot_s * 0.5).round(),
        y + (2.5 * row_h - dot_s * 0.5).round(),
        y + (3.5 * row_h - dot_s * 0.5).round(),
    ];

    for &(mask, col, row) in &DOT_POS {
        if (bits & mask) != 0 {
            let dx = col_x[col].clamp(x, (x + w - dot_s).max(x));
            let dy = row_y[row].clamp(y, (y + h - dot_s).max(y));
            push_rect(instances, dx, dy, dot_s, dot_s, color);
        }
    }
}

/// Draw Symbols for Legacy Computing Sextants (`U+1FB00..=U+1FB3B`) as a seamless 2x3 sub-cell
/// block mosaic. Adjacent cells in the same row are coalesced into a single full-width rect.
fn draw_sextant(
    code: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    instances: &mut Vec<BgInstance>,
) {
    // 6-bit mask: bit 0=TL, bit 1=TR, bit 2=ML, bit 3=MR, bit 4=BL, bit 5=BR.
    // Unicode skips 0 (space), 21 (0b010101 = U+258C ▌), 42 (0b101010 = U+2590 ▐),
    // and 63 (0b111111 = U+2588 █), mapping the remaining 60 patterns in ascending order.
    let idx = code - 0x1FB00;
    let mask: u8 = if idx < 20 {
        (idx + 1) as u8
    } else if idx < 40 {
        (idx + 2) as u8
    } else {
        (idx + 3) as u8
    };

    let x_mid = x + (w * 0.5).round();
    let y1 = y + (h / 3.0).round();
    let y2 = y + (h * 2.0 / 3.0).round();

    let rows = [(y, y1, 0), (y1, y2, 2), (y2, y + h, 4)];
    for (ry0, ry1, shift) in rows {
        let left = (mask & (1 << shift)) != 0;
        let right = (mask & (1 << (shift + 1))) != 0;
        match (left, right) {
            (true, true) => push_span(instances, x, ry0, x + w, ry1, color),
            (true, false) => push_span(instances, x, ry0, x_mid, ry1, color),
            (false, true) => push_span(instances, x_mid, ry0, x + w, ry1, color),
            (false, false) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    #[test]
    fn test_half_blocks_seamless() {
        let mut instances = Vec::new();
        let x = 10.0;
        let y = 20.0;
        let w = 9.0;
        let h = 19.0; // odd height to test rounding

        assert!(draw_custom_block_or_box_char(
            '▀',
            x,
            y,
            w,
            h,
            WHITE,
            &mut instances
        ));
        assert!(draw_custom_block_or_box_char(
            '▄',
            x,
            y,
            w,
            h,
            WHITE,
            &mut instances
        ));

        assert_eq!(instances.len(), 2);
        let top = instances[0].rect;
        let bot = instances[1].rect;

        assert_eq!(top[0], x);
        assert_eq!(top[2], w);
        assert_eq!(bot[0], x);
        assert_eq!(bot[2], w);
        assert_eq!(top[1] + top[3], bot[1]);
        assert_eq!(top[3] + bot[3], h);
    }

    #[test]
    fn test_all_box_drawing_block_braille_sextant_codepoints_handled() {
        let mut instances = Vec::new();
        // All 128 Box Drawing characters (U+2500..=U+257F)
        for cp in 0x2500u32..=0x257Fu32 {
            let c = char::from_u32(cp).unwrap();
            instances.clear();
            assert!(
                draw_custom_block_or_box_char(c, 0.0, 0.0, 18.0, 36.0, WHITE, &mut instances),
                "Box drawing U+{cp:04X} ({c}) must be handled procedurally"
            );
            assert!(
                !instances.is_empty(),
                "Box drawing U+{cp:04X} ({c}) must emit geometry"
            );
            for inst in &instances {
                assert!(
                    inst.rect[0] >= -0.01 && inst.rect[0] + inst.rect[2] <= 18.01,
                    "U+{cp:04X} rect X out of cell bounds: {:?}",
                    inst.rect
                );
                assert!(
                    inst.rect[1] >= -0.01 && inst.rect[1] + inst.rect[3] <= 36.01,
                    "U+{cp:04X} rect Y out of cell bounds: {:?}",
                    inst.rect
                );
            }
        }

        // All 32 Block Elements (U+2580..=U+259F)
        for cp in 0x2580u32..=0x259Fu32 {
            let c = char::from_u32(cp).unwrap();
            instances.clear();
            assert!(draw_custom_block_or_box_char(
                c,
                0.0,
                0.0,
                18.0,
                36.0,
                WHITE,
                &mut instances
            ));
            assert!(!instances.is_empty());
        }

        // All 256 Braille Patterns (U+2800..=U+28FF)
        for cp in 0x2800u32..=0x28FFu32 {
            let c = char::from_u32(cp).unwrap();
            instances.clear();
            assert!(draw_custom_block_or_box_char(
                c,
                0.0,
                0.0,
                18.0,
                36.0,
                WHITE,
                &mut instances
            ));
            let expected_dots = ((cp - 0x2800) as u8).count_ones() as usize;
            assert_eq!(instances.len(), expected_dots);
        }

        // All 60 Sextants (U+1FB00..=U+1FB3B)
        for cp in 0x1FB00u32..=0x1FB3Bu32 {
            let c = char::from_u32(cp).unwrap();
            instances.clear();
            assert!(draw_custom_block_or_box_char(
                c,
                0.0,
                0.0,
                18.0,
                36.0,
                WHITE,
                &mut instances
            ));
            assert!(!instances.is_empty());
            for inst in &instances {
                assert!(inst.rect[0] >= -0.01 && inst.rect[0] + inst.rect[2] <= 18.01);
                assert!(inst.rect[1] >= -0.01 && inst.rect[1] + inst.rect[3] <= 36.01);
            }
        }

        // Regular ASCII should NOT be handled procedurally
        instances.clear();
        assert!(!draw_custom_block_or_box_char(
            'A',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut instances
        ));
    }

    #[test]
    fn test_double_lines_have_parallel_separated_strokes() {
        let mut h_inst = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '═',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut h_inst
        ));
        // Double horizontal line has top & bottom strokes separated by a clear vertical gap.
        let ys: Vec<f32> = h_inst.iter().map(|i| i.rect[1]).collect();
        let min_y = ys.iter().copied().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_y > min_y + 2.0,
            "═ must render two vertically separated strokes, got {:?}",
            h_inst
        );

        // Double corner ╔ emits 4 distinct arms (2 horizontal + 2 vertical) meeting cleanly.
        let mut corner = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '╔',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut corner
        ));
        assert_eq!(corner.len(), 4);
        // Rightmost edge of horizontal strokes reaches cell right edge (18.0)
        assert!(corner
            .iter()
            .any(|i| (i.rect[0] + i.rect[2] - 18.0).abs() < 0.01));
        // Bottommost edge of vertical strokes reaches cell bottom edge (36.0)
        assert!(corner
            .iter()
            .any(|i| (i.rect[1] + i.rect[3] - 36.0).abs() < 0.01));
    }

    #[test]
    fn test_rounded_corners_are_curved_and_edge_aligned() {
        for (ch, expect_right, expect_down) in [
            ('╭', true, true),
            ('╮', false, true),
            ('╯', false, false),
            ('╰', true, false),
        ] {
            let mut instances = Vec::new();
            assert!(draw_custom_block_or_box_char(
                ch,
                0.0,
                0.0,
                18.0,
                36.0,
                WHITE,
                &mut instances
            ));
            // Must emit more than 2 rects (straight arms + curved arc slices)
            assert!(
                instances.len() >= 5,
                "Rounded corner {ch} should render curved arc slices, got {} rects",
                instances.len()
            );
            if expect_right {
                assert!(instances
                    .iter()
                    .any(|i| (i.rect[0] + i.rect[2] - 18.0).abs() < 0.01));
            } else {
                assert!(instances.iter().any(|i| i.rect[0].abs() < 0.01));
            }
            if expect_down {
                assert!(instances
                    .iter()
                    .any(|i| (i.rect[1] + i.rect[3] - 36.0).abs() < 0.01));
            } else {
                assert!(instances.iter().any(|i| i.rect[1].abs() < 0.01));
            }
        }
    }

    #[test]
    fn test_dashed_lines_emit_expected_segment_counts() {
        for (ch, expected) in [
            ('╌', 2),
            ('╍', 2),
            ('╎', 2),
            ('╏', 2),
            ('┄', 3),
            ('┅', 3),
            ('┆', 3),
            ('┇', 3),
            ('┈', 4),
            ('┉', 4),
            ('┊', 4),
            ('┋', 4),
        ] {
            let mut instances = Vec::new();
            assert!(draw_custom_block_or_box_char(
                ch,
                0.0,
                0.0,
                20.0,
                40.0,
                WHITE,
                &mut instances
            ));
            assert_eq!(
                instances.len(),
                expected,
                "Dashed char {ch} should emit {expected} segments"
            );
        }
    }

    #[test]
    fn test_braille_full_cell_non_overlapping_2x4_grid() {
        let mut instances = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '⣿',
            10.0,
            20.0,
            18.0,
            36.0,
            WHITE,
            &mut instances
        ));
        assert_eq!(instances.len(), 8);
        // No two dots overlap
        for i in 0..instances.len() {
            for j in (i + 1)..instances.len() {
                let a = instances[i].rect;
                let b = instances[j].rect;
                let overlap_x = a[0] < b[0] + b[2] && b[0] < a[0] + a[2];
                let overlap_y = a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
                assert!(
                    !(overlap_x && overlap_y),
                    "Braille dots {i} and {j} overlap: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn test_sextant_bitmask_and_row_coalescing() {
        // U+1FB00: TL only -> 1 rect in top-left sixth
        let mut tl = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '\u{1FB00}',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut tl
        ));
        assert_eq!(tl.len(), 1);
        assert_eq!(tl[0].rect, [0.0, 0.0, 9.0, 12.0]);

        // U+1FB02: TL+TR -> coalesced into 1 full-width top-third rect
        let mut top_row = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '\u{1FB02}',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut top_row
        ));
        assert_eq!(top_row.len(), 1);
        assert_eq!(top_row[0].rect, [0.0, 0.0, 18.0, 12.0]);

        // U+1FB14 (idx=20 -> mask=22 = 0b010110 = TR + ML + BL)
        let mut skip21 = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '\u{1FB14}',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut skip21
        ));
        assert_eq!(skip21.len(), 3);
        assert_eq!(skip21[0].rect, [9.0, 0.0, 9.0, 12.0]); // TR
        assert_eq!(skip21[1].rect, [0.0, 12.0, 9.0, 12.0]); // ML
        assert_eq!(skip21[2].rect, [0.0, 24.0, 9.0, 12.0]); // BL

        // U+1FB3B (idx=59 -> mask=62 = 0b111110 = TR + middle full + bottom full)
        let mut last = Vec::new();
        assert!(draw_custom_block_or_box_char(
            '\u{1FB3B}',
            0.0,
            0.0,
            18.0,
            36.0,
            WHITE,
            &mut last
        ));
        assert_eq!(last.len(), 3);
        assert_eq!(last[0].rect, [9.0, 0.0, 9.0, 12.0]); // TR
        assert_eq!(last[1].rect, [0.0, 12.0, 18.0, 12.0]); // ML+MR coalesced
        assert_eq!(last[2].rect, [0.0, 24.0, 18.0, 12.0]); // BL+BR coalesced
    }
}
