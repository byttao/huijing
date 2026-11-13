//! 参考图编码：本地图片读成 data URL。长边超过 2048 时先缩小再编码，避免请求体过大。
//! 缩小统一用 fast_image_resize 的 Lanczos3（见 split.rs）。

use base64::Engine;
use image::{GenericImageView, ImageFormat};
use std::io::Cursor;
use std::path::Path;

pub const MAX_SIDE: u32 = 2048;
/// 不需要缩放、且不超过这个大小的 PNG / JPEG / WebP 原样发送
const RAW_LIMIT: usize = 6 * 1024 * 1024;

pub fn encode_reference(path: &Path) -> Result<String, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("读取参考图「{name}」失败：{e}"))?;
    encode_bytes(&bytes).map_err(|e| format!("参考图「{name}」{e}"))
}

pub fn encode_bytes(bytes: &[u8]) -> Result<String, String> {
    let format = image::guess_format(bytes).map_err(|_| "不是支持的图片格式".to_string())?;
    let img = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("无法解码：{e}"))?;
    let (w, h) = img.dimensions();
    let long = w.max(h);
    let raw_mime = match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    };
    if let Some(mime) = raw_mime
        && long <= MAX_SIDE
        && bytes.len() <= RAW_LIMIT
    {
        return Ok(data_url(mime, bytes));
    }
    let img = crate::split::shrink_to(&img, MAX_SIDE)?;
    let mut out = Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, ImageFormat::Png).map_err(|e| format!("编码失败：{e}"))?;
        Ok(data_url("image/png", out.get_ref()))
    } else {
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
            .encode_image(&rgb)
            .map_err(|e| format!("编码失败：{e}"))?;
        Ok(data_url("image/jpeg", out.get_ref()))
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([40, 120, 80])));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn decode(url: &str) -> DynamicImage {
        let b64 = url.split_once(',').unwrap().1;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        image::load_from_memory(&bytes).unwrap()
    }

    #[test]
    fn small_png_kept_as_is() {
        let url = encode_bytes(&png(64, 32)).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert_eq!(decode(&url).dimensions(), (64, 32));
    }

    #[test]
    fn large_image_downscaled_to_2048() {
        let url = encode_bytes(&png(3000, 1500)).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert_eq!(decode(&url).dimensions(), (2048, 1024));
    }

    #[test]
    fn rejects_non_image() {
        assert!(encode_bytes(b"hello world").is_err());
    }
}
