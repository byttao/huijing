//! 多合一出图：一次请求让模型在一张图里按宫格画出 N 个方案，下载后按宫格等分拆成 N 张。
//!
//! - 用户选的比例就是实际请求、实际出图（整张图）的比例，不再反推合成比例。
//! - 布局：4 张固定 2×2；2 张、3 张可横排（N×1）或竖排（1×N）。每格比例 = (a/cols):(b/rows)，
//!   即 (a·rows):(b·cols) 约分。横竖两种排法下每格与整张的比例差距相同（都是 N 倍），
//!   所以默认取「每格更接近方形」的那种：横图并排、竖图上下叠。比例为自动时默认横排。
//! - 尺寸：预览里的是估算（界面标「约」）。GPT 实际输出约 1.57MP（1:1 请求 1024x1024 实测返回
//!   1254×1254）；Nano 用 Gemini 3 的 1K 尺寸表（实测 4:3 = 1200×896、16:9 = 1376×768），2K / 4K 翻倍。
//! - 放大：开了「拆切后放大」时，每张等比放大到「整张图的长边」——以下载到的合成图实际像素为准，
//!   合图升了一档时取原档位的长边（即合成图长边的一半）。

use crate::api::{GenParams, Model};
use serde::{Deserialize, Serialize};

/// 解析 "a:b"
pub fn parse_ratio(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once(':')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && b > 0).then_some((a, b))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// 每格比例 (a/cols):(b/rows) = (a·rows):(b·cols)，约分
pub fn cell_ratio(ratio: (u32, u32), cols: u8, rows: u8) -> (u32, u32) {
    let (w, h) = (ratio.0 * rows as u32, ratio.1 * cols as u32);
    let g = gcd(w, h).max(1);
    (w / g, h / g)
}

/// 布局方向
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// 横排：N 列 × 1 行
    Row,
    /// 竖排：1 列 × N 行
    Column,
}

/// 默认方向：每格更接近方形的那种；比例为自动时横排
pub fn default_orientation(ratio: Option<(u32, u32)>, count: u8) -> Orientation {
    let Some((a, b)) = ratio else { return Orientation::Row };
    let squareness = |(w, h): (u32, u32)| ((w as f64) / (h as f64)).ln().abs();
    let row = squareness(cell_ratio((a, b), count, 1));
    let col = squareness(cell_ratio((a, b), 1, count));
    if col < row - 1e-9 { Orientation::Column } else { Orientation::Row }
}

/// 布局 (cols, rows)。`layout`："row" / "column" 指定方向，其他值用默认方向；4 张固定 2×2。
pub fn layout(ratio: Option<(u32, u32)>, count: u8, layout: &str) -> (u8, u8) {
    match count {
        0 | 1 => (1, 1),
        4 => (2, 2),
        n => {
            let o = match layout {
                "row" => Orientation::Row,
                "column" => Orientation::Column,
                _ => default_orientation(ratio, n),
            };
            if o == Orientation::Row { (n, 1) } else { (1, n) }
        }
    }
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

/// 某模型、某比例、某档位整张图的估算尺寸；auto 无法预知返回 None
pub fn image_size(model: Model, ratio: &str, tier: &str) -> Option<[u32; 2]> {
    let r = parse_ratio(ratio)?;
    Some(match model {
        Model::GptImage => size_for_area(GPT_AREA, r),
        Model::NanoBanana => nano_size(tier, r),
    })
}

/// 等比缩放，使长边等于 `long`
pub fn scale_to_long(size: [u32; 2], long: u32) -> [u32; 2] {
    let cur = size[0].max(size[1]).max(1) as f64;
    let k = long as f64 / cur;
    [((size[0] as f64 * k).round() as u32).max(1), ((size[1] as f64 * k).round() as u32).max(1)]
}

/// 一次出图的宫格方案（N=1 时就是普通单张）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GridPlan {
    pub count: u8,
    pub cols: u8,
    pub rows: u8,
    /// 2 / 3 张时的排法："row" 横排、"column" 竖排；单张与 4 张为空
    pub orientation: String,
    /// 实际请求、实际出图的（整张）比例，就是用户选的比例
    pub request_ratio: String,
    /// 拆切后每张的比例（约分）；比例为自动时为空
    pub cell_ratio: String,
    /// Nano 请求的分辨率档位（可能升了一档）；GPT 为空
    pub request_tier: String,
    /// 用户选的分辨率档位（Nano）；GPT 为空
    pub tier: String,
    /// 整张图尺寸（估算）；比例为自动时为 [0, 0]
    pub request_size: [u32; 2],
    /// 拆切后每张的尺寸（估算）
    pub cell_size: [u32; 2],
    /// 最终每张的输出尺寸（开了放大时长边等于整张图的长边）
    pub output_size: [u32; 2],
    /// 是否放大到整张图的长边
    pub upscale: bool,
    /// 尺寸是否为估算
    pub estimated: bool,
}

/// 计算宫格方案。`upscale` 来自设置「拆切后放大到整张图的长边」。
pub fn plan(model: Model, params: &GenParams, count: u8, upscale: bool) -> Result<GridPlan, String> {
    let count = count.clamp(1, 4);
    if count > 1 && model == Model::GptImage && matches!(params.output_format.as_str(), "gif" | "svg") {
        return Err("GIF / SVG 输出无法按宫格拆切，多合一请用 PNG".into());
    }
    let tier = if params.image_size.is_empty() { "1K" } else { params.image_size.as_str() };
    let ratio_str = if params.aspect_ratio.is_empty() { "auto" } else { params.aspect_ratio.as_str() };
    let ratio = parse_ratio(ratio_str);
    let (cols, rows) = layout(ratio, count, &params.layout);
    let orientation = match (count, cols > rows) {
        (2 | 3, true) => "row",
        (2 | 3, false) => "column",
        _ => "",
    };
    let request_tier = match model {
        Model::NanoBanana if count > 1 && params.higher_res => higher_tier(tier),
        Model::NanoBanana => tier,
        Model::GptImage => "",
    };
    let request_size = image_size(model, ratio_str, request_tier).unwrap_or([0, 0]);
    let cell_size = [request_size[0] / cols as u32, request_size[1] / rows as u32];
    // 放大目标：用户所选档位下整张图的长边（升档时就是合成图长边的一半）
    let base_long = image_size(model, ratio_str, if model == Model::NanoBanana { tier } else { "" })
        .map(|[w, h]| w.max(h))
        .unwrap_or(0);
    let upscale = upscale && count > 1;
    let output_size = if upscale && base_long > cell_size[0].max(cell_size[1]) && cell_size[0] > 0 {
        scale_to_long(cell_size, base_long)
    } else {
        cell_size
    };
    Ok(GridPlan {
        count,
        cols,
        rows,
        orientation: orientation.into(),
        request_ratio: ratio_str.into(),
        cell_ratio: ratio.map(|r| cell_ratio(r, cols, rows)).map(|(a, b)| format!("{a}:{b}")).unwrap_or_default(),
        request_tier: request_tier.into(),
        tier: if model == Model::NanoBanana { tier.into() } else { String::new() },
        request_size,
        cell_size,
        output_size,
        upscale,
        estimated: true,
    })
}

/// 拆切后的放大目标长边：以下载到的合成图实际像素为准；合图升了档时除掉升档的倍数。
/// 返回 None 表示不放大。
pub fn target_long_side(composite: (u32, u32), plan: &GridPlan) -> Option<u32> {
    if !plan.upscale {
        return None;
    }
    let linear = (tier_scale(&plan.request_tier) / tier_scale(&plan.tier)).sqrt();
    Some((composite.0.max(composite.1) as f64 / linear).round() as u32)
}

/// 在用户提示词外包一层宫格指令；N=1 原样返回。
pub fn wrap_prompt(prompt: &str, plan: &GridPlan) -> String {
    if plan.count <= 1 {
        return prompt.to_string();
    }
    let cell = if plan.cell_ratio.is_empty() {
        "整张图按宫格等分，每格大小相同".to_string()
    } else {
        format!("整张图按宫格等分，每格画面比例约为 {}", plan.cell_ratio)
    };
    format!(
        "请在一张图中以 {cols} 列 × {rows} 行的宫格排列，画出 {n} 个不同的方案，每格一个方案。\
要求：{cell}；各格互相独立、各自完整构图，主体不跨格；\
格子之间不要任何边框、分隔线或留白，画面直接相接；不要出现任何文字、编号或标注；{n} 个方案在构图、角度或细节上要有明显区别。\n\n\
每个方案的内容：{prompt}",
        cols = plan.cols,
        rows = plan.rows,
        n = plan.count,
    )
}

/// 两个模型的比例列表（用于测试与布局表）
#[cfg(test)]
fn ratios(model: Model) -> Vec<&'static str> {
    match model {
        Model::GptImage => crate::api::gpt_image::SIZES.iter().map(|(r, _)| *r).collect(),
        Model::NanoBanana => vec!["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "5:4", "4:5", "21:9", "1:4", "4:1", "1:8", "8:1"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(ratio: &str, tier: &str) -> GenParams {
        GenParams { aspect_ratio: ratio.into(), image_size: tier.into(), ..GenParams::default() }
    }

    /// 两个模型 × 各比例 × N=2/3/4 × 横竖：请求比例不变、每格比例 = (a·rows):(b·cols)、尺寸合理
    #[test]
    fn every_model_ratio_and_count() {
        for model in [Model::GptImage, Model::NanoBanana] {
            for ratio in ratios(model) {
                let (a, b) = parse_ratio(ratio).unwrap();
                for n in 2..=4u8 {
                    for lay in ["", "row", "column"] {
                        let mut p = params(ratio, "1K");
                        p.layout = lay.into();
                        let plan = plan(model, &p, n, true).unwrap();
                        assert_eq!(plan.request_ratio, ratio, "整张比例就是所选比例");
                        assert_eq!(plan.cols as u32 * plan.rows as u32, n as u32);
                        if n == 4 {
                            assert_eq!((plan.cols, plan.rows), (2, 2));
                        } else if lay == "row" {
                            assert_eq!((plan.cols, plan.rows), (n, 1));
                        } else if lay == "column" {
                            assert_eq!((plan.cols, plan.rows), (1, n));
                        }
                        let (cw, ch) = parse_ratio(&plan.cell_ratio).unwrap();
                        assert_eq!(cw as u64 * (b as u64 * plan.cols as u64), ch as u64 * (a as u64 * plan.rows as u64));
                        assert_eq!(gcd(cw, ch), 1, "每格比例要约分");
                        let [w, h] = plan.cell_size;
                        assert!(w > 0 && h > 0);
                        assert!(((w as f64 / h as f64) / (cw as f64 / ch as f64) - 1.0).abs() < 0.05, "{model:?} {ratio} {n} {w}x{h} vs {cw}:{ch}");
                        assert!(plan.output_size[0].max(plan.output_size[1]) >= w.max(h));
                    }
                }
            }
        }
    }

    #[test]
    fn default_orientation_prefers_square_cells() {
        // 横图并排、竖图上下叠
        assert_eq!(layout(Some((16, 9)), 2, ""), (2, 1));
        assert_eq!(layout(Some((9, 16)), 2, ""), (1, 2));
        assert_eq!(layout(Some((1, 1)), 3, ""), (3, 1));
        assert_eq!(layout(Some((1, 4)), 3, ""), (1, 3));
        assert_eq!(layout(None, 2, ""), (2, 1));
        assert_eq!(layout(Some((16, 9)), 4, "row"), (2, 2), "4 张固定 2×2");
    }

    #[test]
    fn known_plans() {
        let p = plan(Model::NanoBanana, &params("16:9", "1K"), 4, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str(), p.cell_ratio.as_str()), (2, 2, "16:9", "16:9"));
        assert_eq!(p.request_size, [1376, 768]);
        assert_eq!(p.cell_size, [688, 384]);
        assert_eq!(p.output_size, [1376, 768]);

        let p = plan(Model::GptImage, &params("16:9", ""), 2, true).unwrap();
        assert_eq!((p.cols, p.rows, p.cell_ratio.as_str(), p.orientation.as_str()), (2, 1, "8:9", "row"));

        let mut col = params("16:9", "");
        col.layout = "column".into();
        let p = plan(Model::GptImage, &col, 2, false).unwrap();
        assert_eq!((p.cols, p.rows, p.cell_ratio.as_str()), (1, 2, "32:9"));
        assert_eq!(p.output_size, p.cell_size, "关闭放大时输出等于拆切尺寸");

        let p = plan(Model::GptImage, &params("1:1", ""), 3, true).unwrap();
        assert_eq!((p.cols, p.rows, p.cell_ratio.as_str()), (3, 1, "1:3"));
    }

    #[test]
    fn auto_ratio_allowed() {
        let p = plan(Model::NanoBanana, &params("auto", "1K"), 2, true).unwrap();
        assert_eq!((p.cols, p.rows, p.request_ratio.as_str(), p.cell_ratio.as_str()), (2, 1, "auto", ""));
        assert_eq!(p.request_size, [0, 0]);
        assert!(wrap_prompt("猫", &p).contains("每格大小相同"));
    }

    #[test]
    fn nano_higher_res_and_target() {
        let mut p = params("16:9", "1K");
        p.higher_res = true;
        let plan_hi = plan(Model::NanoBanana, &p, 4, true).unwrap();
        assert_eq!(plan_hi.request_tier, "2K");
        assert_eq!(plan_hi.request_size, [2752, 1536]);
        assert_eq!(plan_hi.cell_size, [1376, 768], "2K 合图切 4 格 = 原生 1K");
        assert_eq!(plan_hi.output_size, [1376, 768], "已是 1K 长边，不再放大");
        assert_eq!(target_long_side((2752, 1536), &plan_hi), Some(1376));
        // 同档：放大到整张图长边，以实际合成图为准
        let plan_1k = plan(Model::NanoBanana, &params("16:9", "1K"), 4, true).unwrap();
        assert_eq!(target_long_side((1376, 768), &plan_1k), Some(1376));
        assert_eq!(plan(Model::NanoBanana, &p, 1, true).unwrap().request_tier, "1K", "单张不升档");
        assert_eq!(higher_tier("4K"), "4K");
    }

    #[test]
    fn unsupported_formats() {
        let mut svg = params("1:1", "");
        svg.output_format = "svg".into();
        assert!(plan(Model::GptImage, &svg, 4, true).is_err());
        assert!(plan(Model::GptImage, &svg, 1, true).is_ok());
    }

    #[test]
    fn prompt_wrapping() {
        let p = plan(Model::GptImage, &params("1:1", ""), 4, true).unwrap();
        let text = wrap_prompt("一只猫", &p);
        assert!(text.contains("2 列 × 2 行") && text.contains("4 个不同的方案") && text.contains("每格画面比例约为 1:1"));
        assert!(text.contains("不要任何边框") && text.contains("不要出现任何文字"));
        assert!(text.ends_with("一只猫"));
        let one = plan(Model::GptImage, &params("1:1", ""), 1, true).unwrap();
        assert_eq!(wrap_prompt("一只猫", &one), "一只猫");
    }

    #[test]
    fn old_plan_json_still_loads() {
        let old = r#"{"count":4,"cols":2,"rows":2,"cellRatio":"16:9","requestRatio":"16:9","exact":true,"requestTier":"1K","requestSize":[1376,768],"cellSize":[688,384],"singleSize":[1376,768],"outputSize":[1376,768],"upscale":true,"estimated":true}"#;
        let p: GridPlan = serde_json::from_str(old).unwrap();
        assert_eq!((p.cols, p.rows), (2, 2));
        assert_eq!(p.tier, "");
    }

    /// 打印布局表：cargo test print_layout_table -- --ignored --nocapture
    #[test]
    #[ignore]
    fn print_layout_table() {
        for model in [Model::GptImage, Model::NanoBanana] {
            println!("\n## {}\n\n| 整张比例 | 2 张（默认 / 另一种） | 3 张（默认 / 另一种） | 4 张 |\n|---|---|---|---|", model.id());
            for ratio in ratios(model) {
                let cell = |n: u8, lay: &str| {
                    let mut p = params(ratio, "1K");
                    p.layout = lay.into();
                    let x = plan(model, &p, n, true).unwrap();
                    format!("{}×{} 每张 {}（约 {}×{}）", x.cols, x.rows, x.cell_ratio, x.cell_size[0], x.cell_size[1])
                };
                let pair = |n: u8| {
                    let d = plan(model, &params(ratio, "1K"), n, true).unwrap();
                    let other = if d.orientation == "row" { "column" } else { "row" };
                    format!("{} / {}", cell(n, ""), cell(n, other))
                };
                println!("| {ratio} | {} | {} | {} |", pair(2), pair(3), cell(4, ""));
            }
        }
    }
}
