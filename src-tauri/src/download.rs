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

/// 为一次下载占位：返回 (最终路径, 临时路径 `最终路径.part`)。
/// 最终文件名不与已有文件重名；临时文件用 create_new 原子占住，避免并发任务撞名。
/// 下载过程中文件夹里只有 `xxx.png.part`，完成后才原子改名成 `xxx.png`，不会出现打不开的半截 PNG。
pub fn reserve_download(dir: &Path, name: &str) -> std::io::Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for i in 1u32.. {
        let dest = if i == 1 { dir.join(name) } else { dir.join(format!("{stem} ({i}){ext}")) };
        if dest.exists() {
            continue;
        }
        let part = part_path(&dest);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&part) {
            Ok(_) => return Ok((dest, part)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// `a/b.png` → `a/b.png.part`
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// 流式下载到 `part`，完成后原子改名为 `dest`；失败自动重试 3 次。
/// `on_progress(已下载字节, 总字节)` 随下载推进调用（总字节取自 Content-Length，可能没有）。
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    dest: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
        }
        match try_download(client, url, part, &mut on_progress).await {
            Ok(()) => {
                return tokio::fs::rename(part, dest).await.map_err(|e| format!("保存文件失败：{e}"));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn try_download(client: &reqwest::Client, url: &str, part: &Path, on_progress: &mut impl FnMut(u64, Option<u64>)) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut resp = client.get(url).send().await.map_err(|e| describe(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(if status.as_u16() == 404 {
            "图片链接已失效（HTTP 404，结果只保留约 2 小时）".into()
        } else {
            format!("HTTP {status}")
        });
    }
    let total = resp.content_length().filter(|&n| n > 0);
    let mut file = tokio::fs::File::create(part).await.map_err(|e| format!("写入文件失败：{e}"))?;
    let mut done = 0u64;
    on_progress(0, total);
    while let Some(chunk) = resp.chunk().await.map_err(|e| describe(&e))? {
        file.write_all(&chunk).await.map_err(|e| format!("写入文件失败：{e}"))?;
        done += chunk.len() as u64;
        on_progress(done, total);
    }
    file.flush().await.map_err(|e| format!("写入文件失败：{e}"))?;
    if done == 0 {
        return Err("下载到的文件为空".into());
    }
    Ok(())
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
    fn reserve_download_uses_part_file() {
        let dir = std::env::temp_dir().join(format!("huijing-part-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (dest, part) = reserve_download(&dir, "a.png").unwrap();
        assert_eq!(dest, dir.join("a.png"));
        assert_eq!(part, dir.join("a.png.part"));
        assert!(part.exists() && !dest.exists(), "下载中只有 .part，没有最终文件");
        // 同名的 .part 被占住时换下一个名字
        let (dest2, _) = reserve_download(&dir, "a.png").unwrap();
        assert_eq!(dest2, dir.join("a (2).png"));
        // 已有最终文件也不覆盖
        std::fs::write(dir.join("b.png"), b"1").unwrap();
        assert_eq!(reserve_download(&dir, "b.png").unwrap().0, dir.join("b (2).png"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ext_detection() {
        assert_eq!(ext_from_url("https://file1.aitohumanize.com/file/abc.PNG", "png"), "png");
        assert_eq!(ext_from_url("https://x/file/abc.jpeg?x=1", "png"), "jpeg");
        assert_eq!(ext_from_url("https://x/file/abc", "png"), "png");
    }
}
