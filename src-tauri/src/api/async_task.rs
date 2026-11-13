//! 异步任务协议：`POST {base}/v1/api/generate`（replyType=async）立即返回任务 id，
//! 再轮询 `GET {base}/v1/api/result?id=<id>` 直到 succeeded / failed / violation。
//! nano-banana-2.1 全部走这里；gpt-image-2.6 带参考图时也走这里（见 gpt_image.rs）。
//!
//! 结果：`{"id","status":"running|succeeded|failed|violation","progress":0-100,"results":[{"url"}],"error"}`

use super::{ApiContext, ApiError};
use serde_json::Value;
use std::time::{Duration, Instant};

/// 轮询间隔
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// 轮询总时长上限（4K 偶尔较慢，给足 15 分钟）
pub const POLL_LIMIT: Duration = Duration::from_secs(15 * 60);

/// 一次查询的结果。
#[derive(Debug, PartialEq)]
pub enum PollState {
    Running(u8),
    Succeeded(Vec<String>),
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// 解析提交响应，返回任务 id。
pub fn parse_submit(status: u16, text: &str) -> Result<String, ApiError> {
    let v: Value = serde_json::from_str(text).map_err(|_| {
        if status >= 500 {
            ApiError::Server { status, message: text.chars().take(200).collect() }
        } else {
            ApiError::Api(format!("响应不是 JSON（HTTP {status}）：{}", text.chars().take(200).collect::<String>()))
        }
    })?;
    check_failure(&v)?;
    let id = str_field(&v, "id");
    if id.is_empty() {
        return Err(ApiError::Api(format!("提交后没有拿到任务 id（HTTP {status}）")));
    }
    Ok(id.to_string())
}

/// status 为 failed / violation，或带 error 字段时转成错误。
fn check_failure(v: &Value) -> Result<(), ApiError> {
    let status = str_field(v, "status");
    let error = str_field(v, "error");
    match status {
        "violation" => Err(ApiError::Violation(error.to_string())),
        "failed" => Err(if error.is_empty() { ApiError::Api("生成失败".into()) } else { ApiError::classify(error) }),
        _ if !error.is_empty() && status != "succeeded" && status != "running" => Err(ApiError::classify(error)),
        _ => Ok(()),
    }
}

/// 解析查询响应。
pub fn parse_result(status: u16, text: &str) -> Result<PollState, ApiError> {
    let v: Value = serde_json::from_str(text).map_err(|_| {
        if status >= 500 {
            ApiError::Server { status, message: text.chars().take(200).collect() }
        } else {
            ApiError::Api(format!("响应不是 JSON（HTTP {status}）"))
        }
    })?;
    check_failure(&v)?;
    match str_field(&v, "status") {
        "succeeded" => {
            let urls: Vec<String> = v
                .get("results")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.get("url").and_then(Value::as_str))
                        .filter(|u| !u.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if urls.is_empty() {
                Err(ApiError::Api("任务成功但没有返回图片地址".into()))
            } else {
                Ok(PollState::Succeeded(urls))
            }
        }
        "running" | "" if status < 400 => {
            let progress = v.get("progress").and_then(Value::as_u64).unwrap_or(0).min(99) as u8;
            Ok(PollState::Running(progress))
        }
        other => Err(ApiError::Api(format!("未知状态 {other}（HTTP {status}）"))),
    }
}

/// 提交任务，返回任务 id。
pub async fn submit(ctx: &ApiContext, body: &Value) -> Result<String, ApiError> {
    let resp = ctx
        .post("/v1/api/generate")
        .json(body)
        .send()
        .await
        .map_err(ApiError::from_reqwest)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(ApiError::from_reqwest)?;
    parse_submit(status, &text)
}

/// 轮询直到出结果；每次拿到进度回调 `on_progress`。单次查询的网络错误会重试，连续 5 次失败才放弃。
pub async fn poll(ctx: &ApiContext, id: &str, mut on_progress: impl FnMut(u8)) -> Result<Vec<String>, ApiError> {
    let started = Instant::now();
    let mut net_failures = 0;
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        let attempt = async {
            let resp = ctx.get("/v1/api/result").query(&[("id", id)]).timeout(Duration::from_secs(30)).send().await.map_err(ApiError::from_reqwest)?;
            let status = resp.status().as_u16();
            let text = resp.text().await.map_err(ApiError::from_reqwest)?;
            parse_result(status, &text)
        }
        .await;
        match attempt {
            Ok(PollState::Succeeded(urls)) => return Ok(urls),
            Ok(PollState::Running(p)) => {
                net_failures = 0;
                on_progress(p);
            }
            Err(ApiError::Network(_) | ApiError::Timeout | ApiError::Server { .. }) if net_failures < 4 => {
                net_failures += 1;
            }
            Err(e) => return Err(e),
        }
        if started.elapsed() > POLL_LIMIT {
            return Err(ApiError::Timeout);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_ok_and_error() {
        assert_eq!(parse_submit(200, r#"{"id":"6-abc","status":"running"}"#).unwrap(), "6-abc");
        assert_eq!(parse_submit(400, r#"{"id":"","status":"failed","error":"apikey error"}"#), Err(ApiError::InvalidKey));
    }

    #[test]
    fn result_states() {
        assert_eq!(parse_result(200, r#"{"id":"1","status":"running","progress":42}"#), Ok(PollState::Running(42)));
        assert_eq!(
            parse_result(200, r#"{"id":"1","status":"succeeded","progress":100,"results":[{"url":"https://x/a.png"}]}"#),
            Ok(PollState::Succeeded(vec!["https://x/a.png".into()]))
        );
        assert!(matches!(parse_result(200, r#"{"id":"1","status":"violation","error":""}"#), Err(ApiError::Violation(_))));
        assert_eq!(parse_result(404, r#"{"error":"result not exist, valid for 2 hours"}"#), Err(ApiError::Expired));
        assert_eq!(parse_result(200, r#"{"id":"1","status":"failed","error":"generate failed"}"#), Err(ApiError::GenerateFailed));
    }
}
