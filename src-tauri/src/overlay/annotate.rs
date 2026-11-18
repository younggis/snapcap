//! 标注合成（§9.3）
//!
//! 把 UI 传回的 `AnnotationOp` 列表绘制到 BGRA 位图上，落盘与剪贴板使用同一
//! 合成结果。矩形 / 箭头 / 文字 / 马赛克(box blur) / 序号全部手写实现，无外部
//! 绘图依赖。颜色约定为 `0xAARRGGBB`（与前端 canvas 常用格式一致）。

use crate::models::AnnotationOp;
use anyhow::Result;

/// 把全部标注按顺序合成到 BGRA 缓冲（原地修改）。
pub fn apply_ops(bgra: &mut Vec<u8>, width: u32, height: u32, ops: &[AnnotationOp]) -> Result<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    for op in ops {
        match op {
            AnnotationOp::Rect {
                x,
                y,
                w,
                h,
                color,
                width: stroke,
            } => {
                draw_rect(bgra, width, height, *x, *y, *w, *h, *color, *stroke);
            }
            AnnotationOp::Arrow {
                from,
                to,
                color,
                width: stroke,
            } => {
                draw_arrow(
                    bgra,
                    width,
                    height,
                    from.0,
                    from.1,
                    to.0,
                    to.1,
                    *color,
                    *stroke,
                );
            }
            AnnotationOp::Text {
                x,
                y,
                text,
                color,
                size,
            } => {
                draw_text(bgra, width, height, *x, *y, text, *color, *size);
            }
            AnnotationOp::Blur {
                x,
                y,
                w,
                h,
                radius,
            } => {
                box_blur(bgra, width, height, *x, *y, *w, *h, *radius);
            }
            AnnotationOp::Step { x, y, number } => {
                draw_step(bgra, width, height, *x, *y, *number);
            }
        }
    }
    Ok(())
}

/// 解包 0xAARRGGBB 为 BGRA 四元组。
fn split_color(color: u32) -> [u8; 4] {
    let a = ((color >> 24) & 0xff) as u8;
    let r = ((color >> 16) & 0xff) as u8;
    let g = ((color >> 8) & 0xff) as u8;
    let b = (color & 0xff) as u8;
    let a = if a == 0 { 255 } else { a };
    [b, g, r, a]
}

fn set_pixel(bgra: &mut [u8], w: u32, h: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let idx = (y as usize * w as usize + x as usize) * 4;
    if idx + 3 >= bgra.len() {
        return;
    }
    let a = color[3] as f32 / 255.0;
    let inv = 1.0 - a;
    for c in 0..3 {
        let blended = color[c] as f32 * a + bgra[idx + c] as f32 * inv;
        bgra[idx + c] = blended.round().clamp(0.0, 255.0) as u8;
    }
    bgra[idx + 3] = 255;
}

/// Bresenham 直线，带线宽（以像素方块加粗）。
fn draw_line(
    bgra: &mut [u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    thickness: u32,
) {
    let rgba = split_color(color);
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let t = thickness.max(1) as i32;
    let half = t / 2;
    loop {
        for oy in -half..=half {
            for ox in -half..=half {
                set_pixel(bgra, w, h, x + ox, y + oy, rgba);
            }
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

fn draw_rect(
    bgra: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: u32,
    rh: u32,
    color: u32,
    thickness: u32,
) {
    let x2 = x + rw as i32;
    let y2 = y + rh as i32;
    draw_line(bgra, w, h, x, y, x2, y, color, thickness);
    draw_line(bgra, w, h, x2, y, x2, y2, color, thickness);
    draw_line(bgra, w, h, x2, y2, x, y2, color, thickness);
    draw_line(bgra, w, h, x, y2, x, y, color, thickness);
}

fn draw_arrow(
    bgra: &mut [u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    thickness: u32,
) {
    draw_line(bgra, w, h, x0, y0, x1, y1, color, thickness);
    // 箭头头部：沿反方向旋转 ±30° 的两条短线。
    let dx = (x1 - x0) as f64;
    let dy = (y1 - y0) as f64;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let head = (8.0 + thickness as f64 * 2.0).min(len);
    let angle = 0.5f64; // ~28.6°
    for sign in [-1.0f64, 1.0] {
        let rx = ux * angle.cos() - uy * angle.sin() * sign;
        let ry = ux * angle.sin() * sign + uy * angle.cos();
        let hx = (x1 as f64 - rx * head).round() as i32;
        let hy = (y1 as f64 - ry * head).round() as i32;
        draw_line(bgra, w, h, x1, y1, hx, hy, color, thickness);
    }
}

/// 手写 5x7 点阵字体（仅 ASCII；非 ASCII 画占位方框）。
fn glyph(ch: char) -> Option<[u8; 7]> {
    let ch = ch.to_ascii_uppercase();
    let table: &[(char, [u8; 7])] = &[
        ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
        ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
        ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
        ('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]),
        ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
        ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
        ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
        ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
        ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
        ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
        ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
        ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
        ('C', [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111]),
        ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
        ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
        ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
        ('G', [0b01111, 0b10000, 0b10000, 0b10011, 0b10001, 0b10001, 0b01111]),
        ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
        ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
        ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
        ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
        ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
        ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
        ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
        ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
        ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
        ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
        ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
        ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
        ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
        ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
        ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
        ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
        ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
        ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
        ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
        ('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
        (',', [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b00100, 0b01000]),
        ('-', [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
        (' ', [0; 7]),
    ];
    table.iter().find(|(c, _)| *c == ch).map(|(_, g)| *g)
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    bgra: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    text: &str,
    color: u32,
    size: u32,
) {
    let rgba = split_color(color);
    let scale = (size as i32 / 8).max(1);
    let cell = 5 * scale + scale; // 字宽 + 字距
    for (ci, ch) in text.chars().enumerate() {
        let base_x = x + ci as i32 * cell;
        match glyph(ch) {
            Some(rows) => {
                for (ry, bits) in rows.iter().enumerate() {
                    for cx in 0..5 {
                        if (bits >> (4 - cx)) & 1 == 1 {
                            for sy in 0..scale {
                                for sx in 0..scale {
                                    set_pixel(
                                        bgra,
                                        w,
                                        h,
                                        base_x + cx as i32 * scale + sx as i32,
                                        y + ry as i32 * scale + sy as i32,
                                        rgba,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            None => {
                // 非 ASCII（如中文）画一个占位方框，UI 侧仍可预览文字。
                for ry in 0..7 * scale {
                    for cx in 0..5 * scale {
                        if ry == 0 || ry == 7 * scale - 1 || cx == 0 || cx == 5 * scale - 1 {
                            set_pixel(bgra, w, h, base_x + cx as i32, y + ry as i32, rgba);
                        }
                    }
                }
            }
        }
    }
}

/// 区域 box blur（马赛克）。radius 为采样盒半径，0 表示 1px 盒。
fn box_blur(bgra: &mut [u8], w: u32, h: u32, x: i32, y: i32, rw: u32, rh: u32, radius: u32) {
    if rw == 0 || rh == 0 || bgra.is_empty() {
        return;
    }
    let r = radius as i32;
    let x1 = x.max(0);
    let y1 = y.max(0);
    let x2 = (x + rw as i32).min(w as i32);
    let y2 = (y + rh as i32).min(h as i32);
    if x2 <= x1 || y2 <= y1 {
        return;
    }
    let src = bgra.to_vec();
    for py in y1..y2 {
        for px in x1..x2 {
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            let sy0 = (py - r).max(0);
            let sy1 = (py + r).min(h as i32 - 1);
            let sx0 = (px - r).max(0);
            let sx1 = (px + r).min(w as i32 - 1);
            for sy in sy0..=sy1 {
                for sx in sx0..=sx1 {
                    let idx = (sy as usize * w as usize + sx as usize) * 4;
                    if idx + 2 < src.len() {
                        sum[0] += src[idx] as u32;
                        sum[1] += src[idx + 1] as u32;
                        sum[2] += src[idx + 2] as u32;
                        count += 1;
                    }
                }
            }
            if count > 0 {
                let idx = (py as usize * w as usize + px as usize) * 4;
                for c in 0..3 {
                    bgra[idx + c] = (sum[c] / count) as u8;
                }
                bgra[idx + 3] = 255;
            }
        }
    }
}

/// 序号：蓝色实心圆 + 白色数字。
fn draw_step(bgra: &mut [u8], w: u32, h: u32, x: i32, y: i32, number: u32) {
    let radius = 14i32;
    let blue = split_color(0xFF2D7FF9);
    let white = split_color(0xFFFFFFFF);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(bgra, w, h, x + dx, y + dy, blue);
            }
        }
    }
    let label = number.to_string();
    let scale = 2i32;
    let total_w = label.len() as i32 * 6 * scale;
    let start_x = x - total_w / 2;
    let start_y = y - 4 * scale;
    for (ci, ch) in label.chars().enumerate() {
        if let Some(rows) = glyph(ch) {
            for (ry, bits) in rows.iter().enumerate() {
                for cx in 0..5 {
                    if (bits >> (4 - cx)) & 1 == 1 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                set_pixel(
                                    bgra,
                                    w,
                                    h,
                                    start_x + ci as i32 * 6 * scale + cx as i32 * scale + sx,
                                    start_y + ry as i32 * scale + sy,
                                    white,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_annotation_draws_outline() {
        let mut bgra = vec![0u8; 8 * 8 * 4];
        apply_ops(
            &mut bgra,
            8,
            8,
            &[AnnotationOp::Rect { x: 1, y: 1, w: 4, h: 4, color: 0x0000ff, width: 1 }],
        )
        .unwrap();
        let idx = (1 * 8 + 1) * 4;
        assert_eq!(&bgra[idx..idx + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn step_annotation_writes_pixels() {
        let mut bgra = vec![0u8; 16 * 16 * 4];
        apply_ops(&mut bgra, 16, 16, &[AnnotationOp::Step { x: 8, y: 8, number: 1 }]).unwrap();
        assert!(bgra.iter().any(|&v| v != 0));
    }

    #[test]
    fn blur_annotation_is_bounded() {
        let mut bgra = vec![120u8; 16 * 16 * 4];
        apply_ops(
            &mut bgra,
            16,
            16,
            &[AnnotationOp::Blur { x: 2, y: 2, w: 8, h: 8, radius: 2 }],
        )
        .unwrap();
        assert_eq!(bgra.len(), 16 * 16 * 4);
    }
}
