//! 出图接口：两个模型走两套完全不同的接口，各自一个客户端实现，这里按模型枚举分发。
//!
//! - `gpt-image-2.6`：文生图走 OpenAI 格式 `POST /v1/images/generations`，同步返回；
//!   带参考图走异步任务协议（见 `gpt_image`）
//! - `nano-banana-2.1`：`POST /v1/api/generate` 异步提交 + `GET /v1/api/result` 轮询（见 `nano_banana`）

pub mod async_task;
pub mod error;
pub mod gpt_image;
pub mod nano_banana;

pub use error::ApiError;
use serde::{Deserialize, Serialize};

/// 支持的模型。序列化值就是接口里的 model 字段。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Model {
    #[serde(rename = "gpt-image-2.6")]
    GptImage,
    #[serde(rename = "nano-banana-2.1")]
    NanoBanana,
}

impl Model {
    /// 接口里的 model 字段值，也用于输出文件名。
    pub fn id(self) -> &'static str {
        match self {
            Model::GptImage => "gpt-image-2.6",
            Model::NanoBanana => "nano-banana-2.1",
        }
    }
}

/// 出图参数。两个模型共用一个结构，各取所需：
/// GPT 用 `aspect_ratio`（换算成 size）、`background`、`output_format`；Nano 用 `aspect_ratio`、`image_size`。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GenParams {
    /// 比例：auto、1:1、16:9 …
    pub aspect_ratio: String,
    /// Nano 分辨率：1K / 2K / 4K
    pub image_size: String,
    /// GPT 背景："" 默认，"transparent" 透明
    pub background: String,
    /// GPT 输出格式：png（默认）/ gif / svg
    pub output_format: String,
    /// Nano 多合一时用高一档分辨率请求（如 1K × 4 张用 2K 请求，切出来是原生 1K）
    pub higher_res: bool,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            aspect_ratio: "1:1".into(),
            image_size: "1K".into(),
            background: String::new(),
            output_format: "png".into(),
            higher_res: false,
        }
    }
}

/// 一次出图请求：参考图已经编码成 data URL。
#[derive(Clone, Debug)]
pub struct GenRequest {
    pub model: Model,
    pub prompt: String,
    pub params: GenParams,
    pub images: Vec<String>,
}

/// 调用接口所需的上下文：节点地址、Key 与已按代理设置构造好的 HTTP 客户端。
#[derive(Clone)]
pub struct ApiContext {
    pub base_url: String,
    pub api_key: String,
    pub client: reqwest::Client,
}

impl ApiContext {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(self.url(path)).bearer_auth(self.api_key.trim())
    }

    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.get(self.url(path)).bearer_auth(self.api_key.trim())
    }
}

/// 按模型与是否带参考图分发到具体接口。
#[derive(Debug, PartialEq)]
pub enum Route {
    /// gpt-image 文生图：同步 generations
    GptSync,
    /// 异步提交 + 轮询，附请求体
    Async(serde_json::Value),
}

pub fn route(req: &GenRequest) -> Route {
    match req.model {
        Model::GptImage if req.images.is_empty() => Route::GptSync,
        Model::GptImage => Route::Async(gpt_image::build_async_body(req)),
        Model::NanoBanana => Route::Async(nano_banana::build_body(req)),
    }
}

/// 测试连接（不扣费）：向 GPT 接口提交空提示词。
/// Key 有效时服务端先校验 Key 再校验参数，返回 `prompt is empty`；Key 无效返回 `apikey error`。
pub async fn test_connection(ctx: &ApiContext) -> Result<String, ApiError> {
    let resp = ctx
        .post("/v1/images/generations")
        .json(&serde_json::json!({ "model": Model::GptImage.id(), "prompt": "" }))
        .send()
        .await
        .map_err(ApiError::from_reqwest)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(ApiError::from_reqwest)?;
    interpret_test_response(status, &text)
}

/// 解析测试连接的响应，单独拆出便于单元测试。
pub fn interpret_test_response(status: u16, text: &str) -> Result<String, ApiError> {
    let message = gpt_image::error_message(text).unwrap_or_default();
    if message.contains("prompt is empty") {
        return Ok("连接成功，API Key 有效".into());
    }
    if !message.is_empty() {
        return Err(ApiError::classify(&message));
    }
    if status >= 500 {
        return Err(ApiError::Server { status, message: text.chars().take(200).collect() });
    }
    Err(ApiError::Api(format!("无法识别的响应（HTTP {status}）：{}", text.chars().take(200).collect::<String>())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_serializes_as_api_id() {
        assert_eq!(serde_json::to_string(&Model::GptImage).unwrap(), "\"gpt-image-2.6\"");
        assert_eq!(serde_json::from_str::<Model>("\"nano-banana-2.1\"").unwrap(), Model::NanoBanana);
    }

    #[test]
    fn routing() {
        let mut req = GenRequest { model: Model::GptImage, prompt: "p".into(), params: GenParams::default(), images: vec![] };
        assert_eq!(route(&req), Route::GptSync);
        req.images.push("data:image/png;base64,AA".into());
        assert!(matches!(route(&req), Route::Async(b) if b["model"] == "gpt-image-2.6" && b["images"][0] == "data:image/png;base64,AA"));
        req.model = Model::NanoBanana;
        assert!(matches!(route(&req), Route::Async(b) if b["imageSize"] == "1K"));
    }

    #[test]
    fn test_connection_valid_key() {
        let r = interpret_test_response(400, r#"{"error":{"message":"prompt is empty"}}"#);
        assert!(r.is_ok());
    }

    #[test]
    fn test_connection_invalid_key() {
        let r = interpret_test_response(400, r#"{"error":{"message":"apikey error"}}"#);
        assert!(matches!(r, Err(ApiError::InvalidKey)));
    }
}
