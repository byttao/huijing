//! gpt-image-2.6 客户端：OpenAI 格式 `POST {base}/v1/images/generations`，同步返回。
//!
//! 请求体：`{model, prompt, size, quality, image?, background?, output_format?}`
//! 成功：`{"created":..,"data":[{"url":".."}],"usage":{..}}`
//! 失败：HTTP 400 `{"error":{"message":"apikey error"}}`
//!
//! 带参考图时改走异步任务协议（`/v1/api/generate`，见 async_task.rs）：实测 generations 的 `image`
//! 数组传 data URL 时请求挂起约 9 分钟后连接被断开、没有结果；异步接口的 `images` 明确支持 base64，
//! 实测约 25 秒出图，还能拿到进度、重启后继续轮询。

use super::{ApiContext, ApiError, GenRequest};
use serde_json::{Value, json};

/// 该模型只支持 auto 质量。
pub const QUALITY: &str = "auto";

/// 比例 → 1K 像素尺寸。auto 原样传。
pub const SIZES: &[(&str, &str)] = &[
    ("1:1", "1024x1024"),
    ("16:9", "1672x941"),
    ("9:16", "941x1672"),
    ("4:3", "1443x1090"),
    ("3:4", "1090x1443"),
    ("3:2", "1536x1024"),
    ("2:3", "1024x1536"),
    ("5:4", "1408x1120"),
    ("4:5", "1120x1408"),
    ("21:9", "1920x832"),
    ("9:21", "832x1920"),
    ("2:1", "1792x896"),
    ("1:2", "896x1792"),
];

/// 比例换算成接口的 size；不认识的比例回落到 auto。
pub fn size_for(aspect_ratio: &str) -> &'static str {
    SIZES
        .iter()
        .find(|(ratio, _)| *ratio == aspect_ratio)
        .map(|(_, size)| *size)
        .unwrap_or("auto")
}

/// 文生图（无参考图）的 generations 请求体。
pub fn build_body(req: &GenRequest) -> Value {
    let mut body = json!({
        "model": req.model.id(),
        "prompt": req.prompt,
        "size": size_for(&req.params.aspect_ratio),
        "quality": QUALITY,
    });
    let obj = body.as_object_mut().expect("json object");
    if req.params.background == "transparent" {
        obj.insert("background".into(), json!("transparent"));
    }
    let format = req.params.output_format.as_str();
    if matches!(format, "gif" | "svg") {
        obj.insert("output_format".into(), json!(format));
    }
    body
}

/// 带参考图时的异步请求体：`{model, prompt, images[], aspectRatio, quality, replyType, background?, outputFormat?}`
pub fn build_async_body(req: &GenRequest) -> Value {
    let ratio = if req.params.aspect_ratio.is_empty() { "auto" } else { req.params.aspect_ratio.as_str() };
    let mut body = json!({
        "model": req.model.id(),
        "prompt": req.prompt,
        "images": req.images,
        "aspectRatio": ratio,
        "quality": QUALITY,
        "replyType": "async",
    });
    let obj = body.as_object_mut().expect("json object");
    if req.params.background == "transparent" {
        obj.insert("background".into(), json!("transparent"));
    }
    let format = req.params.output_format.as_str();
    if matches!(format, "gif" | "svg") {
        obj.insert("outputFormat".into(), json!(format));
    }
    body
}

/// 从错误响应里取出 message：兼容 `{"error":{"message":..}}`、`{"error":".."}`、`{"message":..}`。
pub fn error_message(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let err = v.get("error");
    err.and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .or_else(|| err.and_then(Value::as_str))
        .or_else(|| v.get("message").and_then(Value::as_str))
        .map(str::to_string)
}

/// 解析响应：成功返回图片 URL 列表。
pub fn parse_response(status: u16, text: &str) -> Result<Vec<String>, ApiError> {
    if let Some(message) = error_message(text) {
        return Err(ApiError::classify(&message));
    }
    if status >= 500 {
        return Err(ApiError::Server { status, message: text.chars().take(200).collect() });
    }
    let v: Value = serde_json::from_str(text)
        .map_err(|_| ApiError::Api(format!("响应不是 JSON（HTTP {status}）：{}", text.chars().take(200).collect::<String>())))?;
    let urls: Vec<String> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("url").and_then(Value::as_str))
                .filter(|u| !u.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if urls.is_empty() {
        return Err(ApiError::Api(format!("响应里没有图片地址（HTTP {status}）")));
    }
    Ok(urls)
}

/// 发起生成并等待结果（实测约 40 秒，客户端超时 10 分钟）。
pub async fn generate(ctx: &ApiContext, req: &GenRequest) -> Result<Vec<String>, ApiError> {
    let resp = ctx
        .post("/v1/images/generations")
        .json(&build_body(req))
        .send()
        .await
        .map_err(ApiError::from_reqwest)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(ApiError::from_reqwest)?;
    parse_response(status, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{GenParams, Model};

    fn req(ratio: &str) -> GenRequest {
        GenRequest {
            model: Model::GptImage,
            prompt: "一只猫".into(),
            params: GenParams { aspect_ratio: ratio.into(), ..GenParams::default() },
            images: vec![],
        }
    }

    #[test]
    fn body_text_to_image() {
        let body = build_body(&req("16:9"));
        assert_eq!(body["model"], "gpt-image-2.6");
        assert_eq!(body["size"], "1672x941");
        assert_eq!(body["quality"], "auto");
        assert!(body.get("image").is_none());
        assert!(body.get("background").is_none());
        assert!(body.get("output_format").is_none());
    }

    #[test]
    fn body_with_options() {
        let mut r = req("auto");
        r.params.background = "transparent".into();
        r.params.output_format = "svg".into();
        let body = build_body(&r);
        assert_eq!(body["size"], "auto");
        assert_eq!(body["background"], "transparent");
        assert_eq!(body["output_format"], "svg");
    }

    #[test]
    fn async_body_with_reference() {
        let mut r = req("3:2");
        r.images = vec!["data:image/png;base64,AAAA".into()];
        r.params.background = "transparent".into();
        let body = build_async_body(&r);
        assert_eq!(body["images"][0], "data:image/png;base64,AAAA");
        assert_eq!(body["aspectRatio"], "3:2");
        assert_eq!(body["quality"], "auto");
        assert_eq!(body["replyType"], "async");
        assert_eq!(body["background"], "transparent");
        assert!(body.get("outputFormat").is_none());
    }

    #[test]
    fn all_sizes_mapped() {
        assert_eq!(size_for("9:21"), "832x1920");
        assert_eq!(size_for("1:2"), "896x1792");
        assert_eq!(size_for("weird"), "auto");
    }

    #[test]
    fn parse_success() {
        let text = r#"{"created":1,"data":[{"url":"https://file1.aitohumanize.com/file/a.png"}],"usage":{}}"#;
        assert_eq!(parse_response(200, text).unwrap(), vec!["https://file1.aitohumanize.com/file/a.png"]);
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse_response(400, r#"{"error":{"message":"apikey error"}}"#), Err(ApiError::InvalidKey));
        assert!(matches!(parse_response(502, "Bad Gateway"), Err(ApiError::Server { status: 502, .. })));
        assert!(matches!(parse_response(200, r#"{"data":[]}"#), Err(ApiError::Api(_))));
    }
}
