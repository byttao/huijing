//! 接口错误：统一转成用户看得懂的中文提示，显示在任务行和日志里。

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// API Key 无效、过期或未填写
    InvalidKey,
    /// 账户余额 / 额度不足
    InsufficientBalance,
    /// 提示词或参考图违规
    Violation(String),
    /// 超时
    Timeout,
    /// 网络连接失败
    Network(String),
    /// 服务器 5xx
    Server { status: u16, message: String },
    /// 结果已过期或任务不存在（结果只保留约 2 小时）
    Expired,
    /// 服务端生成失败（generate failed），多为上游临时故障
    GenerateFailed,
    /// 其他接口报错（原文）
    Api(String),
}

impl ApiError {
    /// 根据接口返回的错误文字归类。
    pub fn classify(message: &str) -> ApiError {
        let lower = message.to_lowercase();
        if lower.contains("apikey") || lower.contains("api key") || lower.contains("unauthorized") {
            ApiError::InvalidKey
        } else if ["balance", "credit", "insufficient", "quota", "余额", "积分", "点数不足"]
            .iter()
            .any(|k| lower.contains(k))
        {
            ApiError::InsufficientBalance
        } else if ["violation", "sensitive", "moderation", "safety", "违规", "敏感"].iter().any(|k| lower.contains(k)) {
            ApiError::Violation(message.to_string())
        } else if lower.contains("result not exist") || lower.contains("not found") {
            ApiError::Expired
        } else if lower.contains("generate failed") {
            ApiError::GenerateFailed
        } else if lower.contains("timeout") || lower.contains("timed out") {
            ApiError::Timeout
        } else {
            ApiError::Api(message.to_string())
        }
    }

    pub fn from_reqwest(err: reqwest::Error) -> ApiError {
        if err.is_timeout() {
            ApiError::Timeout
        } else if err.is_connect() {
            ApiError::Network(format!("无法连接服务器（{}）", root_cause(&err)))
        } else if err.is_decode() || err.is_body() {
            ApiError::Network(format!("读取响应失败（{}）", root_cause(&err)))
        } else {
            ApiError::Network(root_cause(&err))
        }
    }
}

/// 取最底层的错误原因，reqwest 外层文字通常只有一句 “error sending request”。
pub fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut cur: &(dyn std::error::Error + 'static) = err;
    while let Some(next) = cur.source() {
        cur = next;
    }
    cur.to_string()
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::InvalidKey => write!(f, "API Key 无效或已失效，请到「设置」检查后重试"),
            ApiError::InsufficientBalance => write!(f, "账户余额不足，请到 GRSAI 官网充值后重试"),
            ApiError::Violation(m) => {
                if m.is_empty() || m == "violation" {
                    write!(f, "内容违规：提示词或参考图未通过审核，请修改后重试")
                } else {
                    write!(f, "内容违规：提示词或参考图未通过审核（{m}），请修改后重试")
                }
            }
            ApiError::Timeout => write!(f, "请求超时：服务器长时间未返回结果，可稍后重试"),
            ApiError::Network(m) => write!(f, "网络连接失败：{m}。请检查网络或「设置」里的代理"),
            ApiError::Server { status, message } => {
                write!(f, "服务器出错（HTTP {status}）{}，请稍后重试", if message.is_empty() { String::new() } else { format!("：{message}") })
            }
            ApiError::Expired => write!(f, "任务结果已过期或不存在（结果只保留约 2 小时）"),
            ApiError::GenerateFailed => write!(f, "服务端生成失败（generate failed），多为上游临时故障，可点「重试」；反复失败可换个提示词或参考图"),
            ApiError::Api(m) => write!(f, "接口返回错误：{m}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_messages() {
        assert_eq!(ApiError::classify("apikey error"), ApiError::InvalidKey);
        assert_eq!(ApiError::classify("Insufficient balance"), ApiError::InsufficientBalance);
        assert!(matches!(ApiError::classify("violation"), ApiError::Violation(_)));
        assert_eq!(ApiError::classify("result not exist, valid for 2 hours"), ApiError::Expired);
        assert_eq!(ApiError::classify("generate failed"), ApiError::GenerateFailed);
        assert!(matches!(ApiError::classify("something odd"), ApiError::Api(_)));
    }

    #[test]
    fn chinese_text() {
        assert!(ApiError::InvalidKey.to_string().contains("API Key 无效"));
        assert!(ApiError::Violation("violation".into()).to_string().starts_with("内容违规"));
        assert!(ApiError::Timeout.to_string().contains("超时"));
    }
}
