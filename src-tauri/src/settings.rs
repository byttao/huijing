//! 设置：保存为数据目录下的 settings.json，重启后保持。
//!
//! 数据目录：macOS 等用应用配置目录；Windows 便携版放在 exe 同目录的 `绘境数据/`，拷走即用。

use crate::api::{GenParams, Model};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const NODE_CN: &str = "https://grsai.dakka.com.cn";
pub const NODE_GLOBAL: &str = "https://grsaiapi.com";
pub const PORTABLE_DIR: &str = "绘境数据";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    None,
    System,
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxySettings {
    pub mode: ProxyMode,
    /// 自定义代理地址：http://host:port 或 socks5://host:port
    pub url: String,
    /// 是否用于 API 请求
    pub for_api: bool,
    /// 是否用于图片下载
    pub for_download: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self { mode: ProxyMode::None, url: String::new(), for_api: true, for_download: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub api_key: String,
    /// "cn" 国内节点（默认） / "global" 全球节点
    pub node: String,
    pub proxy: ProxySettings,
    /// 输出目录；空字符串表示默认「图片/绘境」
    pub output_dir: String,
    /// 并发数 1–5
    pub concurrency: u8,
    pub default_model: Model,
    pub gpt_params: GenParams,
    pub nano_params: GenParams,
    pub default_count: u8,
    /// 主题：ui-kit 的 accent 键，写在设置里，Windows 便携版拷走后也能保持
    pub theme_accent: String,
    /// light / dark / system
    pub theme_mode: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            node: "cn".into(),
            proxy: ProxySettings::default(),
            output_dir: String::new(),
            concurrency: 2,
            default_model: Model::NanoBanana,
            gpt_params: GenParams { aspect_ratio: "1:1".into(), ..GenParams::default() },
            nano_params: GenParams { aspect_ratio: "auto".into(), image_size: "1K".into(), ..GenParams::default() },
            default_count: 1,
            theme_accent: String::new(),
            theme_mode: String::new(),
        }
    }
}

impl Settings {
    pub fn base_url(&self) -> &'static str {
        if self.node == "global" { NODE_GLOBAL } else { NODE_CN }
    }

    /// 修正越界的值。
    pub fn normalized(mut self) -> Self {
        self.concurrency = self.concurrency.clamp(1, 5);
        self.default_count = self.default_count.clamp(1, 4);
        if self.node != "global" {
            self.node = "cn".into();
        }
        self.api_key = self.api_key.trim().to_string();
        self.proxy.url = self.proxy.url.trim().to_string();
        self
    }

    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
            .normalized()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(path, text.as_bytes())
    }
}

/// 先写临时文件再改名，避免写到一半崩溃留下坏文件。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建目录失败：{e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("保存 {} 失败：{e}", path.display()))
}

/// 按代理设置构造 HTTP 客户端。`use_proxy` 表示这类请求（API / 下载）是否勾选了走代理。
pub fn build_client(proxy: &ProxySettings, use_proxy: bool, timeout: Duration) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("huijing/", env!("CARGO_PKG_VERSION")));
    builder = match (&proxy.mode, use_proxy) {
        (ProxyMode::None, _) | (_, false) => builder.no_proxy(),
        // reqwest 默认读取系统代理（macOS 系统配置 / Windows 注册表 / 环境变量）
        (ProxyMode::System, true) => builder,
        (ProxyMode::Custom, true) => {
            let url = proxy.url.trim();
            if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("socks5://") || url.starts_with("socks5h://")) {
                return Err("自定义代理地址需以 http:// 或 socks5:// 开头".into());
            }
            builder.proxy(reqwest::Proxy::all(url).map_err(|e| format!("代理地址无效：{e}"))?)
        }
    };
    builder.build().map_err(|e| format!("创建网络客户端失败：{e}"))
}

/// 数据目录：Windows 便携版优先 exe 同目录的 `绘境数据/`（可写时），否则用应用配置目录。
pub fn resolve_data_dir(app_config_dir: Option<PathBuf>) -> PathBuf {
    if cfg!(windows)
        && let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.join(PORTABLE_DIR)))
        && fs::create_dir_all(&dir).is_ok()
        && fs::write(dir.join(".write-test"), b"ok").is_ok()
    {
        let _ = fs::remove_file(dir.join(".write-test"));
        return dir;
    }
    app_config_dir.unwrap_or_else(|| PathBuf::from(PORTABLE_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let s = Settings::default();
        assert_eq!(s.base_url(), NODE_CN);
        assert_eq!(s.concurrency, 2);
        assert_eq!(s.proxy.mode, ProxyMode::None);
    }

    #[test]
    fn partial_json_fills_defaults_and_clamps() {
        let s: Settings = serde_json::from_str(r#"{"apiKey":" sk-1 ","concurrency":9,"node":"global"}"#).unwrap();
        let s = s.normalized();
        assert_eq!(s.api_key, "sk-1");
        assert_eq!(s.concurrency, 5);
        assert_eq!(s.base_url(), NODE_GLOBAL);
        assert_eq!(s.default_count, 1);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("huijing-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings { api_key: "sk-test".into(), concurrency: 3, ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn custom_proxy_validation() {
        let p = ProxySettings { mode: ProxyMode::Custom, url: "127.0.0.1:7897".into(), for_api: true, for_download: true };
        assert!(build_client(&p, true, Duration::from_secs(5)).is_err());
        let p = ProxySettings { url: "socks5://127.0.0.1:7897".into(), ..p };
        assert!(build_client(&p, true, Duration::from_secs(5)).is_ok());
    }
}
