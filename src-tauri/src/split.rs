//! 宫格拆切与缩放。
//!
//! 拆切：按 cols×rows 等分，但模型偶尔会画出细分隔线、或格线不在正中——在每条内部格线附近
//! 一个窄带里找「整列 / 整行近似纯色」的线，找到就把它从两侧格子里裁掉；再对内部边各内缩一点点
//! 去掉接缝处的过渡。不再按目标比例裁切：每格比例由宫格等分决定。
//! 没有分隔线时，再在同一窄带里找「相邻两行 / 列差异最大」的位置：模型画的格线常常偏离等分位置
//! 几个到十几个像素（实测 GPT 1×3 竖排第二条缝偏了 10 像素），明显突变处才是真正的接缝。
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
/// 内容接缝：相邻两行 / 列的平均 RGB 差至少这么大（0–255），且是带内中位数的这么多倍
const SEAM_MIN_DIFF: f64 = 12.0;
const SEAM_MIN_RATIO: f64 = 3.0;

/// 两个像素 RGB 三通道的平均差（色相不同、亮度相近的接缝也能看出来）
fn pixel_diff(a: image::Rgba<u8>, b: image::Rgba<u8>) -> f64 {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs() as f64).sum::<f64>() / 3.0
}

/// 第 y 行与第 y-1 行在 [x0, x1) 上的平均差（隔一个像素取样）
fn row_diff(img: &DynamicImage, y: u32, x0: u32, x1: u32) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for x in (x0..x1).step_by(2) {
        sum += pixel_diff(img.get_pixel(x, y), img.get_pixel(x, y - 1));
        n += 1.0;
    }
    if n > 0.0 { sum / n } else { 0.0 }
}

fn col_diff(img: &DynamicImage, x: u32, y0: u32, y1: u32) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in (y0..y1).step_by(2) {
        sum += pixel_diff(img.get_pixel(x, y), img.get_pixel(x - 1, y));
        n += 1.0;
    }
    if n > 0.0 { sum / n } else { 0.0 }
}

/// 在 `nominal` 附近 ±band 内找接缝，返回 (前一格的结束, 后一格的开始)：
/// 先找分隔线簇（整列 / 行近似纯色），没有时找明显的内容突变（`diff(i)` 是第 i 与 i-1 行 / 列的差），
/// 都没有时两者都是 nominal。
fn find_seam(nominal: u32, band: u32, limit: u32, is_line: impl Fn(u32) -> bool, diff: impl Fn(u32) -> f64) -> (u32, u32) {
    let lo = nominal.saturating_sub(band);
    let hi = (nominal + band).min(limit.saturating_sub(1));
    let mut lines: Vec<u32> = (lo..=hi).filter(|&i| is_line(i)).collect();
    // 真正的分隔线是窄带里的一条细线；近似纯色的行 / 列一直连到窄带边缘（大片天空、纯色背景）时不算，
    // 否则会把格线两侧整段正常画面当成分隔线裁掉
    if let Some((start, end)) = cluster_near(&lines, nominal, lo, hi)
        && (start == lo || end == hi)
    {
        lines.clear();
    }
    if lines.is_empty() {
        let scores: Vec<(u32, f64)> = (lo.max(1)..=hi).map(|i| (i, diff(i))).collect();
        if scores.len() >= 3 {
            let (best_i, best) = scores.iter().copied().fold((nominal, 0.0), |a, b| if b.1 > a.1 { b } else { a });
            let mut values: Vec<f64> = scores.iter().map(|s| s.1).collect();
            values.sort_by(|a, b| a.total_cmp(b));
            let median = values[values.len() / 2];
            if best >= SEAM_MIN_DIFF && best >= median * SEAM_MIN_RATIO {
                return (best_i, best_i);
            }
        }
        return (nominal, nominal);
    }
    let (start, end) = cluster_near(&lines, nominal, lo, hi).expect("非空");
    (start, end + 1)
}

/// 取离 nominal 最近的一条，再向两侧扩成连续的一簇，返回 [start, end]
fn cluster_near(lines: &[u32], nominal: u32, lo: u32, hi: u32) -> Option<(u32, u32)> {
    let center = *lines.iter().min_by_key(|&&i| (i as i64 - nominal as i64).abs())?;
    let (mut start, mut end) = (center, center);
    while start > lo && lines.contains(&(start - 1)) {
        start -= 1;
    }
    while end < hi && lines.contains(&(end + 1)) {
        end += 1;
    }
    Some((start, end))
}

/// 计算某一方向的各格区间 [start, end)
fn spans(total: u32, parts: u32, band: u32, inset: u32, is_line: impl Fn(u32) -> bool, diff: impl Fn(u32) -> f64) -> Vec<(u32, u32)> {
    let mut cuts = vec![(0u32, 0u32)];
    for i in 1..parts {
        let nominal = total * i / parts;
        cuts.push(find_seam(nominal, band, total, &is_line, &diff));
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

/// 按宫格等分拆切（检测并裁掉分隔线）。返回从左到右、从上到下的 N 张。
pub fn split_grid(img: &DynamicImage, cols: u32, rows: u32) -> Vec<DynamicImage> {
    let (w, h) = img.dimensions();
    let (cols, rows) = (cols.max(1), rows.max(1));
    let cell_w = w / cols;
    let cell_h = h / rows;
    // 找线 / 接缝的窄带：格宽 / 高的 4%，至少 3 像素；内缩：0.4%，至少 1 像素
    let band_x = (cell_w * 4 / 100).max(3);
    let band_y = (cell_h * 4 / 100).max(3);
    let inset_x = (cell_w * 4 / 1000).max(1);
    let inset_y = (cell_h * 4 / 1000).max(1);
    let xs = spans(w, cols, band_x, inset_x, |x| column_is_line(img, x, 0, h), |x| col_diff(img, x, 0, h));
    let ys = spans(h, rows, band_y, inset_y, |y| row_is_line(img, y, 0, w), |y| row_diff(img, y, 0, w));
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for &(y0, y1) in &ys {
        for &(x0, x1) in &xs {
            out.push(img.crop_imm(x0, y0, x1 - x0, y1 - y0));
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
        let parts = split_grid(&img, 2, 2);
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
        let parts = split_grid(&img, 2, 2);
        assert_eq!(parts.len(), 4);
        for p in &parts {
            assert!(!has_white_edge(p), "切出的格子边上不应带分隔线");
            // 格线偏了 9 像素，两侧格子不再等大；只裁掉线，不另裁比例
            let (w, h) = p.dimensions();
            assert!((575..=615).contains(&w) && (375..=420).contains(&h), "{w}x{h}");
        }
    }

    #[test]
    fn three_in_a_row_keeps_full_cells() {
        // 21:9 横排 3 张：每格 7:9，等分不裁比例
        let img = synth(2100, 900, 3, 1, 6, 0);
        let parts = split_grid(&img, 3, 1);
        assert_eq!(parts.len(), 3);
        for p in &parts {
            let (w, h) = p.dimensions();
            assert_eq!(h, 900);
            assert!((680..=700).contains(&w), "{w}");
            assert!(!has_white_edge(p));
        }
    }

    /// 平滑渐变的格子（相邻行 / 列几乎没差别），格线偏离等分位置、没有分隔线
    fn smooth_offset(w: u32, h: u32, rows: u32, offset: i32) -> DynamicImage {
        let seams: Vec<i32> = (1..rows).map(|i| (h * i / rows) as i32 + offset).collect();
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            let cell = seams.iter().filter(|&&s| y as i32 >= s).count() as u32;
            for x in 0..w {
                let shade = (x * 60 / w) as u8;
                let base = [[200u8, 120, 60], [60, 160, 90], [90, 110, 220]][cell as usize % 3];
                img.put_pixel(x, y, Rgb([base[0].saturating_sub(shade), base[1], base[2].saturating_sub(shade / 2)]));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn follows_offset_content_seam_without_lines() {
        // 1×3 竖排，第二条缝偏了 10 像素（同实测 GPT 9:16 三张竖排）
        let img = smooth_offset(941, 1672, 3, -10);
        let parts = split_grid(&img, 1, 3);
        assert_eq!(parts.len(), 3);
        for (i, p) in parts.iter().enumerate() {
            let (w, h) = p.dimensions();
            let top = p.get_pixel(w / 2, 0);
            let bottom = p.get_pixel(w / 2, h - 1);
            assert_eq!(top, bottom, "第 {} 张上下边来自同一格，不应混进相邻格", i + 1);
        }
        // 没有突变的地方仍按等分切
        let even = smooth_offset(900, 900, 3, 0);
        let parts = split_grid(&even, 1, 3);
        let heights: Vec<u32> = parts.iter().map(|p| p.height()).collect();
        assert!(heights.iter().all(|h| (295..=300).contains(h)), "{heights:?}");
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
