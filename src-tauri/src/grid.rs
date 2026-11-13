//! 多合一出图：一次请求让模型在一张图里按宫格画出 N 个方案，下载后按宫格拆成 N 张。
//!
//! - 布局：每格比例 = 用户选的 a:b；cols×rows 的合成比例 = (cols·a):(rows·b)。
//!   对 N 枚举可行布局，选合成比例最接近该模型「支持的比例列表」的一个；不精确命中时，
//!   拆切时每格按 a:b 居中裁切。
//! - 尺寸：预览里的尺寸是估算（界面标「约」）：GPT 实际输出约 1.57MP（1:1 请求 1024x1024
//!   实测返回 1254×1254，其余比例与接口像素表一致）；Nano 用 Gemini 3 的 1K 尺寸表（实测 4:3 =
//!   1200×896、16:9 = 1376×768 对得上），2K / 4K 依次翻倍。真正拆切与放大时以下载到的
//!   合成图实际像素为准：同档位下单张与合图总像素相同（见 `target_from_composite`）。
//! - 提示词：在用户提示词外包一层宫格指令。

use crate::api::{GenParams, Model, gpt_image};
use serde::{Deserialize, Serialize};

/// GPT 支持的比例（auto 除外），与 gpt_image::SIZES 一致
pub fn supported_ratios(model: Model) -> Vec<&'static str> {
    match model {
        Model::GptImage => gpt_image::SIZES.iter().map(|(r, _)| *r).collect(),
        Model::NanoBanana => vec!["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "5:4", "4:5", "21:9", "1:4", "4:1", "1:8", "8:1"],
    }
}

/// 解析 "a:b"
pub fn parse_ratio(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once(':')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && b > 0).then_some((a, b))
}

/// N 张时可选的布局（cols, rows），靠前的在打平时优先
pub fn layouts(count: u8) -> &'static [(u8, u8)] {
    match count {
        2 => &[(2, 1), (1, 2)],
        3 => &[(3, 1), (1, 3)],
        4 => &[(2, 2), (4, 1), (1, 4)],
        _ => &[(1, 1)],
    }
}

/// 比例差距：对数距离，与方向无关
fn distance(x: f64, y: f64) -> f64 {
    (x.ln() - y.ln()).abs()
}

/// 精确命中的容差（21:9 与 2.333… 之类的取整误差）
const EXACT_TOLERANCE: f64 = 0.005;

/// 选布局：返回 (cols, rows, 请求比例, 是否精确命中)
pub fn choose_layout(model: Model, cell: (u32, u32), count: u8) -> (u8, u8, &'static str, bool) {
    let supported = supported_ratios(model);
    let mut best: Option<(f64, u8, u8, &'static str)> = None;
    for &(cols, rows) in layouts(count) {
        let comp = (cols as f64 * cell.0 as f64) / (rows as f64 * cell.1 as f64);
        let (d, ratio) = supported
            .iter()
            .map(|r| {
                let (a, b) = parse_ratio(r).expect("内置比例");
                (distance(comp, a as f64 / b as f64), *r)
            })
            .fold((f64::MAX, ""), |acc, x| if x.0 < acc.0 - 1e-9 { x } else { acc });
        if best.is_none_or(|b| d < b.0 - 1e-9) {
            best = Some((d, cols, rows, ratio));
        }
    }
    let (d, cols, rows, ratio) = best.expect("至少一个布局");
    (cols, rows, ratio, d < EXACT_TOLERANCE)
}

/// Nano 1K 档的总像素（无表比例的兜底估算用）
const NANO_1K_AREA: f64 = 1200.0 * 896.0;
/// GPT 输出的总像素（实测 1:1 = 1254×1254，接口像素表里其余比例也都约 1.57MP）
const GPT_AREA: f64 = 1254.0 * 1254.0;
/// Gemini 3 图像模型 1K 尺寸表
const NANO_1K: &[(&str, [u32; 2])] = &[
    ("1:1", [1024, 1024]),
    ("2:3", [848, 1264]),
    ("3:2", [1264, 848]),
    ("3:4", [896, 1200]),
    ("4:3", [1200, 896]),
    ("4:5", [928, 1152]),
    ("5:4", [1152, 928]),
    ("9:16", [768, 1376]),
    ("16:9", [1376, 768]),
    ("21:9", [1584, 672]),
];

fn round16(v: f64) -> u32 {
    ((v / 16.0).round() * 16.0).max(16.0) as u32
}

/// Nano 某档位、某比例单张的估算尺寸
pub fn nano_size(tier: &str, ratio: (u32, u32)) -> [u32; 2] {
    let key = format!("{}:{}", ratio.0, ratio.1);
    let k = tier_scale(tier).sqrt() as u32;
    if let Some((_, [w, h])) = NANO_1K.iter().find(|(r, _)| *r == key) {
        return [w * k, h * k];
    }
    let area = NANO_1K_AREA * tier_scale(tier);
    let r = ratio.0 as f64 / ratio.1 as f64;
    [round16((area * r).sqrt()), round16((area / r).sqrt())]
}

/// 给定总像素与比例，求宽高（四舍五入）
pub fn size_for_area(area: f64, ratio: (u32, u32)) -> [u32; 2] {
    let r = ratio.0 as f64 / ratio.1 as f64;
    let w = (area * r).sqrt().round().max(1.0);
    [w as u32, ((w / r).round().max(1.0)) as u32]
}

/// 拆切后的目标尺寸（单张原尺寸）：以该模型自己的单张尺寸为准，再按「实际合成图面积 /
/// 预期合成图面积」等比修正——模型实际输出与估算不一致时也能跟上。
pub fn target_from_composite(composite: (u32, u32), plan: &GridPlan) -> Option<[u32; 2]> {
    let [sw, sh] = plan.single_size;
    let [rw, rh] = plan.request_size;
    if sw == 0 || rw == 0 {
        return None;
    }
    let scale = ((composite.0 as f64 * composite.1 as f64) / (rw as f64 * rh as f64)).sqrt();
    Some([((sw as f64 * scale).round() as u32).max(1), ((sh as f64 * scale).round() as u32).max(1)])
}

fn tier_scale(tier: &str) -> f64 {
    match tier {
        "2K" => 4.0,
        "4K" => 16.0,
        _ => 1.0,
    }
}

/// 高一档分辨率；已是 4K 时不变
pub fn higher_tier(tier: &str) -> &'static str {
    match tier {
        "2K" | "4K" => "4K",
        _ => "2K",
    }
}

/// 某模型、某比例、某档位单独出一张时的尺寸；auto 无法预知返回 None
pub fn single_size(model: Model, ratio: &str, tier: &str) -> Option<[u32; 2]> {
    match model {
        Model::GptImage => parse_ratio(ratio).map(|r| size_for_area(GPT_AREA, r)),
        Model::NanoBanana => parse_ratio(ratio).map(|r| nano_size(tier, r)),
    }
}

/// 宽 × 高的格子裁到 a:b（居中），返回裁切后的尺寸
pub fn fit_ratio(w: u32, h: u32, ratio: (u32, u32)) -> [u32; 2] {
    let target = ratio.0 as f64 / ratio.1 as f64;
    if w as f64 / h as f64 > target {
        [((h as f64 * target).round() as u32).clamp(1, w), h]
    } else {
        [w, ((w as f64 / target).round() as u32).clamp(1, h)]
    }
}

/// 一次出图的宫格方案（N=1 时就是普通单张）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridPlan {
    pub count: u8,
    pub cols: u8,
    pub rows: u8,
    /// 用户选的（每格）比例
    pub cell_ratio: String,
    /// 实际请求的（合成）比例
    pub request_ratio: String,
    /// 合成比例是否精确等于 cols·a : rows·b（否则拆切时每格居中裁切）
    pub exact: bool,
    /// Nano 请求的分辨率档位；GPT 为空
    pub request_tier: String,
    /// 用户选的分辨率档位（Nano）；GPT 为空
    #[serde(default)]
    pub tier: String,
    /// 请求尺寸（合成图）；比例为 auto 时为 [0, 0]
    pub request_size: [u32; 2],
    /// 拆切（含居中裁切）后每张的尺寸
    pub cell_size: [u32; 2],
    /// 该模型单独出一张时的尺寸
    pub single_size: [u32; 2],
    /// 最终每张的输出尺寸（开了「放大到单张原尺寸」时等于 single_size）
    pub output_size: [u32; 2],
    /// 是否放大 / 缩放到单张原尺寸
    pub upscale: bool,
    /// 尺寸是否为估算（Nano）
    pub estimated: bool,
}

/// 计算宫格方案。`upscale` 来自设置「拆切后放大到单张原尺寸」。
pub fn plan(model: Model, params: &GenParams, count: u8, upscale: bool) -> Result<GridPlan, String> {
    let count = count.clamp(1, 4);
    let tier = if params.image_size.is_empty() { "1K" } else { params.image_size.as_str() };
    let ratio_str = if params.aspect_ratio.is_empty() { "auto" } else { params.aspect_ratio.as_str() };
    let Some(cell) = parse_ratio(ratio_str) else {
        if count > 1 {
            return Err("比例为「自动」时无法多合一：需要先确定每张的比例，才能算出宫格与拆切尺寸".into());
        }
        return Ok(GridPlan {
            count: 1,
            cols: 1,
            rows: 1,
            cell_ratio: "auto".into(),
            request_ratio: "auto".into(),
            exact: true,
            request_tier: if model == Model::NanoBanana { tier.into() } else { String::new() },
            tier: if model == Model::NanoBanana { tier.into() } else { String::new() },
            request_size: [0, 0],
            cell_size: [0, 0],
            single_size: [0, 0],
            output_size: [0, 0],
            upscale: false,
            estimated: true,
        });
    };
    if count > 1 && model == Model::GptImage && matches!(params.output_format.as_str(), "gif" | "svg") {
        return Err("GIF / SVG 输出无法按宫格拆切，多合一请用 PNG".into());
    }
    let (cols, rows, request_ratio, exact) = if count == 1 { (1, 1, ratio_str, true) } else { choose_layout(model, cell, count) };
    let request_ratio: &str = if count == 1 { ratio_str } else { request_ratio };
    let request_tier = match model {
        Model::NanoBanana if count > 1 && params.higher_res => higher_tier(tier),
        Model::NanoBanana => tier,
        Model::GptImage => "",
    };
    let request_size = single_size(model, request_ratio, request_tier).unwrap_or([0, 0]);
    let single = single_size(model, ratio_str, tier).unwrap_or([0, 0]);
    let raw_cell = [request_size[0] / cols as u32, request_size[1] / rows as u32];
    let cell_size = if count == 1 { request_size } else { fit_ratio(raw_cell[0].max(1), raw_cell[1].max(1), cell) };
    let upscale = upscale && count > 1;
    Ok(GridPlan {
        count,
        cols,
        rows,
        cell_ratio: ratio_str.into(),
        request_ratio: request_ratio.into(),
        exact,
        request_tier: request_tier.into(),
        tier: if model == Model::NanoBanana { tier.into() } else { String::new() },
        request_size,
        cell_size,
        single_size: single,
        output_size: if upscale { single } else { cell_size },
        upscale,
        estimated: true,
    })
}

/// 在用户提示词外包一层宫格指令；N=1 原样返回。
pub fn wrap_prompt(prompt: &str, plan: &GridPlan) -> String {
    if plan.count <= 1 {
        return prompt.to_string();
    }
    format!(
        "请在一张图中以 {cols} 列 × {rows} 行的宫格排列，画出 {n} 个不同的方案，每格一个方案。\
要求：整张图恰好被均分成 {n} 个格子，每格画面比例为 {ratio}；各格互相独立、各自完整构图，主体不跨格；\
格子之间不要任何边框、分隔线或留白，画面直接相接；不要出现任何文字、编号或标注；{n} 个方案在构图、角度或细节上要有明显区别。\n\n\
每个方案的内容：{prompt}",
        cols = plan.cols,
        rows = plan.rows,
        n = plan.count,
        ratio = plan.cell_ratio,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(ratio: &str, tier: &str) -> GenParams {
        GenParams { aspect_ratio: ratio.into(), image_size: tier.into(), ..GenParams::default() }
    }

    /// 两个模型 × 各比例 × N=2/3/4：布局合法、请求比例在支持列表里、精确时合成比例正确、尺寸合理
    #[test]
    fn every_model_ratio_and_count() {
        for model in [Model::GptImage, Model::NanoBanana] {
            let supported = supported_ratios(model);
            for ratio in &supported {
                let cell = parse_ratio(ratio).unwrap();
                for n in 2..=4u8 {
                    let p = plan(model, &params(ratio, "1K"), n, true).unwrap();
                    assert_eq!(p.cols as u32 * p.rows as u32, n as u32, "{model:?} {ratio} {n}");
                    assert!(supported.contains(&p.request_ratio.as_str()), "{model:?} {ratio} {n} -> {}", p.request_ratio);
                    let req = parse_ratio(&p.request_ratio).unwrap();
                    let comp = (p.cols as f64 * cell.0 as f64) / (p.rows as f64 * cell.1 as f64);
                    if p.exact {
                        assert!(distance(comp, req.0 as f64 / req.1 as f64) < EXACT_TOLERANCE);
                    }
                    // 没有任何布局比选中的更接近
                    for &(c, r) in layouts(n) {
                        let other = (c as f64 * cell.0 as f64) / (r as f64 * cell.1 as f64);
                        let best_other = supported
                            .iter()
                            .map(|s| {
                                let (a, b) = parse_ratio(s).unwrap();
                                distance(other, a as f64 / b as f64)
                            })
                            .fold(f64::MAX, f64::min);
                        assert!(distance(comp, req.0 as f64 / req.1 as f64) <= best_other + 1e-9);
                    }
                    // 拆切后每格比例 ≈ 用户比例
                    let [cw, ch] = p.cell_size;
                    assert!(cw > 0 && ch > 0);
                    assert!(distance(cw as f64 / ch as f64, cell.0 as f64 / cell.1 as f64) < 0.03, "{model:?} {ratio} {n} cell {cw}x{ch}");
                    assert_eq!(p.output_size, p.single_size);
                }
            }
        }
    }

    #[test]
    fn known_layouts() {
        let p = plan(Model::GptImage, &params("1:1", ""), 4, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str(), p.exact), (2, 2, "1:1", true));
        assert_eq!(p.request_size, [1254, 1254], "GPT 1:1 实测返回 1254×1254");
        assert_eq!(p.cell_size, [627, 627]);
        assert_eq!(p.output_size, [1254, 1254]);

        let p = plan(Model::GptImage, &params("1:1", ""), 2, false).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str()), (2, 1, "2:1"));
        assert_eq!(p.output_size, p.cell_size, "关闭放大时输出等于拆切尺寸");

        let p = plan(Model::NanoBanana, &params("16:9", "1K"), 4, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str()), (2, 2, "16:9"));

        // 1:4 竖图 × 4 张：2×2（1:4）与 4×1（1:1）都精确，按顺序优先更方正的 2×2
        let p = plan(Model::NanoBanana, &params("1:4", "1K"), 4, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str(), p.exact), (2, 2, "1:4", true));
        // 1:1 × 3 张：3:1 不受支持，GPT 最近的是 21:9 → 每格居中裁切
        let p = plan(Model::GptImage, &params("1:1", ""), 3, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str(), p.exact), (3, 1, "21:9", false));
        assert_eq!(p.cell_size[0], p.cell_size[1]);
    }

    #[test]
    fn nano_higher_res_and_sizes() {
        assert_eq!(nano_size("1K", (4, 3)), [1200, 896]);
        assert_eq!(nano_size("1K", (16, 9)), [1376, 768]);
        assert_eq!(nano_size("2K", (16, 9)), [2752, 1536]);
        let mut p = params("1:1", "1K");
        p.higher_res = true;
        let plan_hi = plan(Model::NanoBanana, &p, 4, true).unwrap();
        assert_eq!(plan_hi.request_tier, "2K");
        // 2K 合图切 4 格 ≈ 原生 1K 单张
        let single = plan_hi.single_size;
        assert!((plan_hi.cell_size[0] as i32 - single[0] as i32).abs() <= 16);
        // 单张时不升档
        assert_eq!(plan(Model::NanoBanana, &p, 1, true).unwrap().request_tier, "1K");
        assert_eq!(higher_tier("4K"), "4K");
    }

    #[test]
    fn target_size_from_real_composite() {
        // 实测：GPT 4 合 1 合成图 1254×1254 → 单张 1254×1254
        let p = plan(Model::GptImage, &params("1:1", ""), 4, true).unwrap();
        assert_eq!(target_from_composite((1254, 1254), &p), Some([1254, 1254]));
        // 实测：Nano 16:9 1K 4 合 1 合成图 1376×768 → 单张 1376×768
        let p = plan(Model::NanoBanana, &params("16:9", "1K"), 4, true).unwrap();
        assert_eq!(target_from_composite((1376, 768), &p), Some([1376, 768]));
        // 实际输出比估算大一圈时等比跟上
        assert_eq!(target_from_composite((2752, 1536), &p), Some([2752, 1536]));
        // 升一档：2K 合成图 2752×1536 → 原生 1K 单张
        let mut hp = params("16:9", "1K");
        hp.higher_res = true;
        let p = plan(Model::NanoBanana, &hp, 4, true).unwrap();
        assert_eq!(target_from_composite((2752, 1536), &p), Some([1376, 768]));
    }

    #[test]
    fn auto_and_unsupported_formats() {
        assert!(plan(Model::NanoBanana, &params("auto", "1K"), 2, true).is_err());
        let single = plan(Model::NanoBanana, &params("auto", "1K"), 1, true).unwrap();
        assert_eq!(single.request_ratio, "auto");
        let mut svg = params("1:1", "");
        svg.output_format = "svg".into();
        assert!(plan(Model::GptImage, &svg, 4, true).is_err());
        assert!(plan(Model::GptImage, &svg, 1, true).is_ok());
    }

    #[test]
    fn prompt_wrapping() {
        let p = plan(Model::GptImage, &params("1:1", ""), 4, true).unwrap();
        let text = wrap_prompt("一只猫", &p);
        assert!(text.contains("2 列 × 2 行") && text.contains("4 个不同的方案") && text.contains("1:1"));
        assert!(text.contains("不要任何边框") && text.contains("不要出现任何文字"));
        assert!(text.ends_with("一只猫"));
        let one = plan(Model::GptImage, &params("1:1", ""), 1, true).unwrap();
        assert_eq!(wrap_prompt("一只猫", &one), "一只猫");
    }

    /// 打印布局选择表：cargo test print_layout_table -- --ignored --nocapture
    #[test]
    #[ignore]
    fn print_layout_table() {
        for model in [Model::GptImage, Model::NanoBanana] {
            println!("\n### {}\n\n| 每张比例 | N=2 | N=3 | N=4 |\n|---|---|---|---|", model.id());
            for ratio in supported_ratios(model) {
                let cells: Vec<String> = (2..=4u8)
                    .map(|n| {
                        let p = plan(model, &params(ratio, "1K"), n, true).unwrap();
                        format!("{}×{} → {}{}", p.cols, p.rows, p.request_ratio, if p.exact { "" } else { "（裁切）" })
                    })
                    .collect();
                println!("| {ratio} | {} |", cells.join(" | "));
            }
        }
    }
}
