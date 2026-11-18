//! PNG / JPEG 编码（§9.2）
//!
//! 从 BGRA 缓冲编码为 PNG（无损）或 JPEG（可调质量），并生成 `data:` URL
//! 供 UI 预览。使用 `png` 与 `jpeg-encoder` crate，无系统依赖。

use crate::error::{AppError, E_INTERNAL};
use crate::models::ImageFormat;
use anyhow::{Context, Result};
use base64::Engine;

/// 编码结果：(文件字节, data URL)。
pub type EncodedImage = (Vec<u8>, String);

/// 把 BGRA 编码为指定格式，返回字节与 data URL。
pub fn encode_image(
    bgra: &[u8],
    width: u32,
    height: u32,
    format: ImageFormat,
    jpeg_quality: u8,
) -> Result<EncodedImage> {
    if width == 0 || height == 0 {
        return Err(AppError::internal("截图尺寸为零").into());
    }
    let expected = width as usize * height as usize * 4;
    if bgra.len() < expected {
        return Err(AppError::internal(format!(
            "BGRA 缓冲长度不足: {} < {}",
            bgra.len(),
            expected
        ))
        .into());
    }

    match format {
        ImageFormat::Png => {
            let bytes = encode_png(bgra, width, height)?;
            let url = data_url("image/png", &bytes);
            Ok((bytes, url))
        }
        ImageFormat::Jpeg => {
            let bytes = encode_jpeg(bgra, width, height, jpeg_quality)?;
            let url = data_url("image/jpeg", &bytes);
            Ok((bytes, url))
        }
    }
}

/// BGRA -> RGBA（PNG 使用 RGBA8）。
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for chunk in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
    }
    rgba
}

/// BGRA -> RGB（JPEG 无 alpha）。
fn bgra_to_rgb(bgra: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
    for chunk in bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
    }
    rgb
}

/// 使用 png crate 编码 RGBA8。
pub fn encode_png(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let rgba = bgra_to_rgba(bgra);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .context("PNG write_header 失败")?;
        writer
            .write_image_data(&rgba)
            .context("PNG write_image_data 失败")?;
    }
    Ok(out)
}

/// 使用 jpeg-encoder 编码 RGB。
pub fn encode_jpeg(bgra: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>> {
    let rgb = bgra_to_rgb(bgra);
    let mut out = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut out, quality);
    encoder
        .encode(
            &rgb,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .context("JPEG 编码失败")?;
    Ok(out)
}

/// 生成 base64 data URL。
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

/// 从一个已编码的 data URL 中还原字节（用于调试 / 测试）。
pub fn decode_data_url(url: &str) -> Result<Vec<u8>> {
    let b64 = url
        .split_once(',')
        .map(|(_, rest)| rest)
        .context("非法 data URL")?;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 解码失败")
}

/// 错误码占位：确保 `E_INTERNAL` 在模块中始终可达（文档用途）。
#[allow(dead_code)]
fn internal_code() -> &'static str {
    E_INTERNAL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        // 2x2 BGRA: red, green, blue, white
        vec![
            0, 0, 255, 255, 0, 255, 0, 255,
            255, 0, 0, 255, 255, 255, 255, 255,
        ]
    }

    #[test]
    fn png_roundtrip() {
        let (bytes, url) = encode_image(&sample(), 2, 2, ImageFormat::Png, 92).unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(url.starts_with("data:image/png;base64,"));
        assert_eq!(decode_data_url(&url).unwrap(), bytes);
    }

    #[test]
    fn jpeg_roundtrip() {
        let (bytes, url) = encode_image(&sample(), 2, 2, ImageFormat::Jpeg, 90).unwrap();
        assert!(bytes.starts_with(&[0xFF, 0xD8]));
        assert!(url.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn rejects_short_buffer() {
        assert!(encode_image(&[0u8; 4], 4, 4, ImageFormat::Png, 92).is_err());
    }

    #[test]
    fn bgra_to_rgba_swaps_channels() {
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 4]), vec![3, 2, 1, 4]);
    }
}
