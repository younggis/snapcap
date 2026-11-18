//! 剪贴板图片写入（§9.2）
//!
//! 用 `arboard::Clipboard::set_image` 把 BGRA 位图放到系统剪贴板。
//! 注意：不同 arboard 3.x 小版本对 `ImageData` 的像素顺序约定不同，
//! 这里统一转成 RGBA 写入；若锁定版本支持 `PixelData::Bgra8`，可改为直接传 BGRA。

use anyhow::{Context, Result};

/// 把 BGRA 图片写入系统剪贴板。
pub fn set_image(bgra: &[u8], width: u32, height: u32) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("打开剪贴板失败")?;

    // BGRA -> RGBA（arboard 的 ImageData 传统上按 RGBA8888 解释）。
    let mut rgba = Vec::with_capacity(bgra.len());
    for chunk in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
    }

    // NOTE: arboard 3.3+ 引入 `ImageData.bytes: PixelData`，旧版为 `Vec<u8>`。
    // 下面按旧/通用写法构造；锁定版本若要求 PixelData，请改为
    // `bytes: arboard::PixelData::Rgba8(rgba.into())`。
    let image = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: rgba.into(),
    };
    clipboard.set_image(image).context("写入剪贴板图片失败")?;
    Ok(())
}
