//! 与存储无关的图像派生：解码（带 Limits）→ SIMD 缩放 → WebP 编码。
//!
//! 供 HTTP 同进程派生（`spawn_blocking`）等调用；不依赖 Blob / Redis / RPC。
//!
//! [`ImageError`] **全部变体均不可重试**（坏图 / 超限 / 编码失败）。
//! `Limits::max_alloc` 对部分 codec 非严格——主要靠 `max_image_width/height` +
//! [`MAX_SOURCE_BYTES`] 压缩体积上限兜底。

use std::cell::RefCell;
use std::io::Cursor;

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, IntoImageView, ResizeAlg, ResizeOptions, Resizer};
use image::ImageError as ImageImageError;
use image::{DynamicImage, ImageReader, Limits};
use webp::Encoder;

/// 缩略图最长边（像素）。
pub const THUMB_MAX_EDGE: u32 = 256;

/// 解码允许的最长边（防解压炸弹；严格 Limits）。
pub const MAX_DECODE_EDGE: u32 = 8192;

/// 解码器峰值分配上限（含输出缓冲；**非严格**，部分解码器可能忽略）。
pub const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;

/// WebP 质量（有损，0–100）。
const WEBP_QUALITY: f32 = 80.0;

/// 拒绝过大源图（按压缩字节）。
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

thread_local! {
    /// `Resizer` 构造有成本；同 blocking 线程复用。
    /// **禁止**在 `resize` 调用栈内再入本 TLS（`try_borrow_mut` 会失败 → `ProcessFailed`）。
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

/// 图像派生失败。所有变体对 job 队列均为永久失败（勿重试）。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImageError {
    /// 压缩体积或像素/内存 Limits 超限。
    #[error("source_too_large")]
    SourceTooLarge,
    /// 格式损坏 / 无法解码。
    #[error("decode_failed")]
    DecodeFailed,
    /// 缩放或编码失败。
    #[error("process_failed")]
    ProcessFailed,
}

/// 将任意图片字节渲染为最长边不超过 [`THUMB_MAX_EDGE`] 的 WebP。
#[inline]
pub fn render_thumb(source: &[u8]) -> Result<Vec<u8>, ImageError> {
    render_max_edge(source, THUMB_MAX_EDGE)
}

/// 将任意图片字节渲染为最长边不超过 `max_edge` 的 WebP（`max_edge == 0` → [`ProcessFailed`]）。
pub fn render_max_edge(source: &[u8], max_edge: u32) -> Result<Vec<u8>, ImageError> {
    if max_edge == 0 {
        return Err(ImageError::ProcessFailed);
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ImageError::SourceTooLarge);
    }
    let img = decode(source)?;
    let (w, h, rgba) = to_thumb_rgba(img, max_edge)?;
    encode_webp_rgba(&rgba, w, h)
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_EDGE);
    limits.max_image_height = Some(MAX_DECODE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    limits
}

fn decode(source: &[u8]) -> Result<DynamicImage, ImageError> {
    let mut reader = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|_| ImageError::DecodeFailed)?;
    reader.limits(decode_limits());
    match reader.decode() {
        Ok(img) => Ok(img),
        Err(ImageImageError::Limits(_)) => Err(ImageError::SourceTooLarge),
        Err(_) => Err(ImageError::DecodeFailed),
    }
}

/// 单次 `into_rgba8`：已够小则直接出缓冲；否则 fir 缩放后 `into_vec`。
fn to_thumb_rgba(img: DynamicImage, max_edge: u32) -> Result<(u32, u32, Vec<u8>), ImageError> {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return Err(ImageError::DecodeFailed);
    }
    if w <= max_edge && h <= max_edge {
        let rgba = img.into_rgba8();
        return Ok((rgba.width(), rgba.height(), rgba.into_raw()));
    }

    let (nw, nh) = scale_dims(w, h, max_edge);
    let src = DynamicImage::ImageRgba8(img.into_rgba8());
    let pixel_type = src.pixel_type().ok_or(ImageError::ProcessFailed)?;
    let mut dst = FirImage::new(nw, nh, pixel_type);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    RESIZER.with(|cell| {
        let mut resizer = cell
            .try_borrow_mut()
            .map_err(|_| ImageError::ProcessFailed)?;
        resizer
            .resize(&src, &mut dst, Some(&opts))
            .map_err(|_| ImageError::ProcessFailed)
    })?;
    Ok((nw, nh, dst.into_vec()))
}

#[inline]
fn scale_dims(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    if w >= h {
        let nh = ((u64::from(h) * u64::from(max_edge)) / u64::from(w)).max(1) as u32;
        (max_edge, nh)
    } else {
        let nw = ((u64::from(w) * u64::from(max_edge)) / u64::from(h)).max(1) as u32;
        (nw, max_edge)
    }
}

fn encode_webp_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ImageError> {
    let encoder = Encoder::from_rgba(rgba, width, height);
    Ok(encoder.encode(WEBP_QUALITY).to_vec())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crc32fast::Hasher;
    use image::{ImageBuffer, ImageFormat, Rgba};

    fn tiny_png() -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_fn(32, 16, |x, y| Rgba([x as u8, y as u8, 128, 255]));
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    }

    fn large_png() -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_fn(512, 256, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 64, 255])
        });
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    }

    /// 只写 IHDR 声明的大尺寸（不真分配像素），触发 decode Limits。
    fn png_claiming_dimensions(width: u32, height: u32) -> Vec<u8> {
        fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut c = Vec::with_capacity(12 + data.len());
            c.extend_from_slice(&(data.len() as u32).to_be_bytes());
            c.extend_from_slice(tag);
            c.extend_from_slice(data);
            let mut crc = Hasher::new();
            crc.update(tag);
            crc.update(data);
            c.extend_from_slice(&crc.finalize().to_be_bytes());
            c
        }
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
        let mut out = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn render_thumb_shrinks_and_is_webp() {
        let out = render_thumb(&tiny_png()).unwrap();
        assert!(!out.is_empty());
        let decoded = image::load_from_memory(&out).unwrap();
        assert!(decoded.width() <= THUMB_MAX_EDGE);
        assert!(decoded.height() <= THUMB_MAX_EDGE);
    }

    #[test]
    fn render_thumb_downscales_large() {
        let out = render_thumb(&large_png()).unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert!(decoded.width() <= THUMB_MAX_EDGE);
        assert!(decoded.height() <= THUMB_MAX_EDGE);
    }

    #[test]
    fn scale_dims_preserves_aspect() {
        assert_eq!(scale_dims(800, 400, 256), (256, 128));
        assert_eq!(scale_dims(400, 800, 256), (128, 256));
    }

    #[test]
    fn rejects_oversized_compressed_source() {
        let big = vec![0_u8; MAX_SOURCE_BYTES + 1];
        assert_eq!(render_thumb(&big), Err(ImageError::SourceTooLarge));
    }

    #[test]
    fn rejects_oversized_decode_dimensions() {
        let png = png_claiming_dimensions(MAX_DECODE_EDGE + 1, MAX_DECODE_EDGE + 1);
        assert!(png.len() < 256);
        assert_eq!(render_thumb(&png), Err(ImageError::SourceTooLarge));
    }
}
