//! nano-banana-2.1 客户端：只负责构造请求体，提交与轮询走异步任务协议（async_task.rs）。
//!
//! 请求体：`{model, prompt, images: string[](data URL), aspectRatio, imageSize: "1K"|"2K"|"4K", replyType: "async"}`

use super::GenRequest;
use serde_json::{Value, json};

pub fn build_body(req: &GenRequest) -> Value {
    json!({
        "model": req.model.id(),
        "prompt": req.prompt,
        "images": req.images,
        "aspectRatio": if req.params.aspect_ratio.is_empty() { "auto" } else { req.params.aspect_ratio.as_str() },
        "imageSize": if req.params.image_size.is_empty() { "1K" } else { req.params.image_size.as_str() },
        "replyType": "async",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{GenParams, Model};

    #[test]
    fn body_fields() {
        let req = GenRequest {
            model: Model::NanoBanana,
            prompt: "山水".into(),
            params: GenParams { aspect_ratio: "21:9".into(), image_size: "2K".into(), ..GenParams::default() },
            images: vec!["data:image/jpeg;base64,BBBB".into()],
        };
        let body = build_body(&req);
        assert_eq!(body["model"], "nano-banana-2.1");
        assert_eq!(body["aspectRatio"], "21:9");
        assert_eq!(body["imageSize"], "2K");
        assert_eq!(body["replyType"], "async");
        assert_eq!(body["images"][0], "data:image/jpeg;base64,BBBB");
    }
}
