//! 宫格拆切与缩放。
//!
//! 拆切：按 cols×rows 等分，但模型偶尔会画出细分隔线、或格线不在正中——在每条内部格线附近
//! 一个窄带里找「整列 / 整行近似纯色」的线，找到就把它从两侧格子里裁掉；再对内部边各内缩一点点
//! 去掉接缝处的过渡；最后把每格居中裁到目标比例。
//! 缩放：统一用 fast_image_resize 的 Lanczos3（运行时自动启用 SSE4.1 / AVX2 / NEON）。

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, GenericImageView, RgbImage, RgbaImage};

/// 用 Lanczos3 缩放到 w×h（不保持比例，调用方先裁好比例）。
pub fn resize(img: &DynamicImage, w: u32, h: u32) -> Result<DynamicImage, String> {
    if img.width() == w && img.height() == h {
        return Ok(img.clone());
    }
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    let mut resizer = Resizer::new();
    if img.color().has_alpha() {
        let src = DynamicImage::ImageRgba8(img.to_rgba8());
        let mut dst = RgbaImage::new(w, h);
        resizer.resize(&src, &mut dst, &options).map_err(|e| format!("缩放失败：{e}"))?;
        Ok(DynamicImage::ImageRgba8(dst))
    } else {
        let src = DynamicImage::ImageRgb8(img.to_rgb8());
        let mut dst = RgbImage::new(w, h);
        resizer.resize(&src, &mut dst, &options).map_err(|e| format!("缩放失败：{e}"))?;
        Ok(DynamicImage::ImageRgb8(dst))
    }
}

/// 等比缩小到长边不超过 max_side（参考图预压缩用）。
pub fn shrink_to(img: &DynamicImage, max_side: u32) -> Result<DynamicImage, String> {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max_side {
        return Ok(img.clone());
    }
    let scale = max_side as f64 / long as f64;
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    resize(img, nw, nh)
}

fn luma(p: image::Rgba<u8>) -> i32 {
    (p[0] as i32 * 299 + p[1] as i32 * 587 + p[2] as i32 * 114) / 1000
}

/// 第 x 列在 [y0, y1) 范围内是否近似纯色（分隔线）
fn column_is_line(img: &DynamicImage, x: u32, y0: u32, y1: u32) -> bool {
    let (mut lo, mut hi) = (255, 0);
    for y in y0..y1 {
        let l = luma(img.get_pixel(x, y));
        lo = lo.min(l);
        hi = hi.max(l);
        if hi - lo > LINE_TOLERANCE {
            return false;
        }
    }
    true
}

fn row_is_line(img: &DynamicImage, y: u32, x0: u32, x1: u32) -> bool {
    let (mut lo, mut hi) = (255, 0);
    for x in x0..x1 {
        let l = luma(img.get_pixel(x, y));
        lo = lo.min(l);
        hi = hi.max(l);
        if hi - lo > LINE_TOLERANCE {
            return false;
        }
    }
    true
}

/// 一整列 / 行的亮度极差不超过它，才算分隔线
const LINE_TOLERANCE: i32 = 24;

/// 在 `nominal` 附近 ±band 内找分隔线簇，返回 (前一格的结束, 后一格的开始)。
/// 找不到时两者都是 nominal。
fn find_seam(nominal: u32, band: u32, limit: u32, is_line: impl Fn(u32) -> bool) -> (u32, u32) {
    let lo = nominal.saturating_sub(band);
    let hi = (nominal + band).min(limit.saturating_sub(1));
    let lines: Vec<u32> = (lo..=hi).filter(|&i| is_line(i)).collect();
    if lines.is_empty() {
        return (nominal, nominal);
    }
    // 取离 nominal 最近的一条，再向两侧扩成连续的一簇
    let center = *lines.iter().min_by_key(|&&i| (i as i64 - nominal as i64).abs()).unwrap();
    let (mut start, mut end) = (center, center);
    while start > lo && lines.contains(&(start - 1)) {
        start -= 1;
    }
    while end < hi && lines.contains(&(end + 1)) {
        end += 1;
    }
    (start, end + 1)
}

/// 计算某一方向的各格区间 [start, end)
fn spans(total: u32, parts: u32, band: u32, inset: u32, is_line: impl Fn(u32) -> bool) -> Vec<(u32, u32)> {
    let mut cuts = vec![(0u32, 0u32)];
    for i in 1..parts {
        let nominal = total * i / parts;
        cuts.push(find_seam(nominal, band, total, &is_line));
    }
    cuts.push((total, total));
    (0..parts as usize)
        .map(|i| {
            let mut start = cuts[i].1;
            let mut end = cuts[i + 1].0;
            if i > 0 {
                start += inset;
            }
            if i + 1 < parts as usize {
                end = end.saturating_sub(inset);
            }
            if end <= start {
                // 极端情况：退回等分
                (total * i as u32 / parts, total * (i as u32 + 1) / parts)
            } else {
                (start, end)
            }
        })
        .collect()
}

/// 按宫格拆切，每格居中裁到 ratio。返回从左到右、从上到下的 N 张。
pub fn split_grid(img: &DynamicImage, cols: u32, rows: u32, ratio: (u32, u32)) -> Vec<DynamicImage> {
    let (w, h) = img.dimensions();
    let (cols, rows) = (cols.max(1), rows.max(1));
    let cell_w = w / cols;
    let cell_h = h / rows;
    // 找线的窄带：格宽 / 高的 3%，至少 2 像素；内缩：0.4%，至少 1 像素
    let band_x = (cell_w * 3 / 100).max(2);
    let band_y = (cell_h * 3 / 100).max(2);
    let inset_x = (cell_w * 4 / 1000).max(1);
    let inset_y = (cell_h * 4 / 1000).max(1);
    let xs = spans(w, cols, band_x, inset_x, |x| column_is_line(img, x, 0, h));
    let ys = spans(h, rows, band_y, inset_y, |y| row_is_line(img, y, 0, w));
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for &(y0, y1) in &ys {
        for &(x0, x1) in &xs {
            let (cw, ch) = (x1 - x0, y1 - y0);
            let [fw, fh] = crate::grid::fit_ratio(cw, ch, ratio);
            let cx = x0 + (cw - fw) / 2;
            let cy = y0 + (ch - fh) / 2;
            out.push(img.crop_imm(cx, cy, fw, fh));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    /// 合成一张 cols×rows 宫格图：每格是不同色相的渐变（保证整列 / 整行都不是纯色），
    /// 格线位置可偏移、可画白色分隔线。
    fn synth(w: u32, h: u32, cols: u32, rows: u32, line: u32, shift: i32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let c = (x * cols / w).min(cols - 1);
                let r = (y * rows / h).min(rows - 1);
                let base = [(40 + c * 90) as u8, (60 + r * 80) as u8, 120];
                let tex = ((x * 7 + y * 13) % 60) as u8;
                img.put_pixel(x, y, Rgb([base[0].saturating_add(tex), base[1].saturating_add(tex / 2), base[2].saturating_add(tex)]));
            }
        }
        if line > 0 {
            for i in 1..cols {
                let x0 = (w * i / cols) as i32 + shift - line as i32 / 2;
                for x in x0..x0 + line as i32 {
                    for y in 0..h {
                        img.put_pixel(x as u32, y, Rgb([250, 250, 250]));
                    }
                }
            }
            for i in 1..rows {
                let y0 = (h * i / rows) as i32 + shift - line as i32 / 2;
                for y in y0..y0 + line as i32 {
                    for x in 0..w {
                        img.put_pixel(x, y as u32, Rgb([250, 250, 250]));
                    }
                }
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    fn has_white_edge(img: &DynamicImage) -> bool {
        let (w, h) = img.dimensions();
        let white = |x, y| luma(img.get_pixel(x, y)) > 240;
        (0..h).any(|y| white(0, y) || white(w - 1, y)) || (0..w).any(|x| white(x, 0) || white(x, h - 1))
    }

    #[test]
    fn plain_grid_splits_evenly() {
        let img = synth(1000, 1000, 2, 2, 0, 0);
        let parts = split_grid(&img, 2, 2, (1, 1));
        assert_eq!(parts.len(), 4);
        for p in &parts {
            let (w, h) = p.dimensions();
            assert!((490..=500).contains(&w) && w == h, "{w}x{h}");
        }
        // 左上与右下格颜色来自不同格
        assert_ne!(parts[0].get_pixel(10, 10), parts[3].get_pixel(10, 10));
    }

    #[test]
    fn removes_separator_lines_even_when_offset() {
        let img = synth(1200, 800, 2, 2, 8, 9);
        let parts = split_grid(&img, 2, 2, (3, 2));
        assert_eq!(parts.len(), 4);
        for p in &parts {
            assert!(!has_white_edge(p), "切出的格子边上不应带分隔线");
            let (w, h) = p.dimensions();
            assert!((w as f64 / h as f64 - 1.5).abs() < 0.02, "{w}x{h}");
        }
    }

    #[test]
    fn three_in_a_row_with_center_crop() {
        // 3×1 合成比例 21:9（每格 7:9），目标 1:1 → 每格居中裁成方形
        let img = synth(2100, 900, 3, 1, 6, 0);
        let parts = split_grid(&img, 3, 1, (1, 1));
        assert_eq!(parts.len(), 3);
        for p in &parts {
            let (w, h) = p.dimensions();
            assert_eq!(w, h);
            assert!(!has_white_edge(p));
        }
    }

    #[test]
    fn lanczos_resize_exact_size() {
        let img = synth(300, 200, 1, 1, 0, 0);
        let up = resize(&img, 900, 600).unwrap();
        assert_eq!(up.dimensions(), (900, 600));
        let down = shrink_to(&up, 450).unwrap();
        assert_eq!(down.dimensions(), (450, 300));
        let rgba = DynamicImage::ImageRgba8(img.to_rgba8());
        assert!(resize(&rgba, 30, 20).unwrap().color().has_alpha());
    }
}
