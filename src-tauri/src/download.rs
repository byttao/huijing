//! 结果图下载与文件命名：文件名含时间、模型、序号，不覆盖已有文件。

use chrono::{DateTime, Local, TimeZone};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 文件名：`20261113-173012_nano-banana-2.1_01.png`；一个任务返回多张时追加 `-2`、`-3`。
pub fn build_file_name(created_ms: i64, model_id: &str, seq: u32, sub: usize, ext: &str) -> String {
    let time: DateTime<Local> = Local.timestamp_millis_opt(created_ms).single().unwrap_or_else(Local::now);
    let suffix = if sub > 0 { format!("-{}", sub + 1) } else { String::new() };
    format!("{}_{}_{:02}{}.{}", time.format("%Y%m%d-%H%M%S"), model_id, seq, suffix, ext)
}

/// 占用一个不重名的文件路径：目标已存在时依次尝试 `名称 (2).ext`、`名称 (3).ext` …
/// 用 create_new 原子地先建空文件占位，避免两个并发任务算出同一个名字互相覆盖。
pub fn reserve_unique(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for i in 1u32.. {
        let path = if i == 1 { dir.join(name) } else { dir.join(format!("{stem} ({i}){ext}")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// 从 URL 路径取扩展名，取不到用 fallback。
pub fn ext_from_url(url: &str, fallback: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("");
    match last.rsplit_once('.') {
        Some((_, ext)) if (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => ext.to_ascii_lowercase(),
        _ => fallback.to_string(),
    }
}

/// 下载一张图到 `dest`，失败自动重试 3 次。先写 .part 再改名，避免留下半截文件。
pub async fn download(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
        }
        match try_download(client, url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn try_download(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let resp = client.get(url).send().await.map_err(|e| describe(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(if status.as_u16() == 404 {
            "图片链接已失效（HTTP 404，结果只保留约 2 小时）".into()
        } else {
            format!("HTTP {status}")
        });
    }
    let bytes = resp.bytes().await.map_err(|e| describe(&e))?;
    if bytes.is_empty() {
        return Err("下载到的文件为空".into());
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("创建输出目录失败：{e}"))?;
    }
    let part = dest.with_extension("part");
    tokio::fs::write(&part, &bytes).await.map_err(|e| format!("写入文件失败：{e}"))?;
    tokio::fs::rename(&part, dest).await.map_err(|e| format!("保存文件失败：{e}"))
}

fn describe(err: &reqwest::Error) -> String {
    let cause = crate::api::error::root_cause(err);
    if err.is_timeout() {
        "下载超时".into()
    } else if err.is_connect() {
        format!("无法连接图片服务器（{cause}）")
    } else {
        format!("下载中断（{cause}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_format() {
        let ms = Local.with_ymd_and_hms(2026, 11, 13, 17, 30, 12).unwrap().timestamp_millis();
        assert_eq!(build_file_name(ms, "nano-banana-2.1", 1, 0, "png"), "20261113-173012_nano-banana-2.1_01.png");
        assert_eq!(build_file_name(ms, "gpt-image-2.6", 3, 1, "jpg"), "20261113-173012_gpt-image-2.6_03-2.jpg");
    }

    #[test]
    fn reserve_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("huijing-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(reserve_unique(&dir, "x.png").unwrap(), dir.join("x.png"));
        assert_eq!(reserve_unique(&dir, "x.png").unwrap(), dir.join("x (2).png"));
        assert_eq!(reserve_unique(&dir, "x.png").unwrap(), dir.join("x (3).png"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ext_detection() {
        assert_eq!(ext_from_url("https://file1.aitohumanize.com/file/abc.PNG", "png"), "png");
        assert_eq!(ext_from_url("https://x/file/abc.jpeg?x=1", "png"), "jpeg");
        assert_eq!(ext_from_url("https://x/file/abc", "png"), "png");
    }
}
