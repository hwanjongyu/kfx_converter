//! Cover Image Optimizer for All Kindle Devices (Paperwhite, Oasis, Scribe, Basic, Colorsoft, Voyage)
//!
//! Transcodes cover images to Kindle OS baseline JPEG format (RGB, no progressive scan,
//! standard thumbnail dimensions max 440x660) to guarantee lockscreen cover compatibility.

use std::io::Cursor;
use image::imageops::FilterType;
use image::DynamicImage;
use thiserror::Error;

pub const KINDLE_THUMB_MAX_WIDTH: u32 = 440;
pub const KINDLE_THUMB_MAX_HEIGHT: u64 = 660;
pub const KINDLE_MAX_CONTENT_IMAGE_DIM: u32 = 2560;
pub const MAX_SAFE_IMAGE_DIMENSION: u32 = 8192;
pub const MAX_SAFE_IMAGE_PIXELS: u64 = 30_000_000;

#[derive(Error, Debug)]
pub enum CoverError {
    #[error("Image decode error: {0}")]
    Decode(#[from] image::ImageError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported or empty image data")]
    EmptyData,
    #[error("Image dimensions ({0}x{1}) exceed safe limits")]
    ImageTooLarge(u32, u32),
}

pub struct CoverOptimizer;

impl CoverOptimizer {
    /// Decodes an image with strict safety limits on dimensions and allocations (CWE-400 guard).
    fn decode_with_safety_limits(data: &[u8]) -> Result<DynamicImage, CoverError> {
        if let Some((w, h)) = Self::get_image_dimensions(data) {
            if w > MAX_SAFE_IMAGE_DIMENSION
                || h > MAX_SAFE_IMAGE_DIMENSION
                || (w as u64) * (h as u64) > MAX_SAFE_IMAGE_PIXELS
            {
                return Err(CoverError::ImageTooLarge(w, h));
            }
        } else {
            return Err(CoverError::Decode(image::ImageError::Decoding(
                image::error::DecodingError::from_format_hint(image::error::ImageFormatHint::Unknown),
            )));
        }

        let mut reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SAFE_IMAGE_DIMENSION);
        limits.max_image_height = Some(MAX_SAFE_IMAGE_DIMENSION);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        Ok(reader.decode()?)
    }

    /// Resizes down high-resolution content images exceeding max_dim (default 2560x2560) to protect
    /// Kindle devices with 512MB RAM from pmond OOM crashes, and re-encodes Progressive JPEGs
    /// (SOF2 marker 0xFFC2) to Baseline JPEG (SOF0) so Kindle's hardware decoder can render them.
    pub fn optimize_content_image(data: &[u8], max_dim: u32) -> Result<Vec<u8>, CoverError> {
        if data.is_empty() {
            return Err(CoverError::EmptyData);
        }

        let is_jpeg = data.starts_with(b"\xff\xd8");
        let is_progressive_jpeg = is_jpeg && data.windows(2).any(|w| w == [0xff, 0xc2]);

        let img = Self::decode_with_safety_limits(data)?;
        let (w, h) = (img.width(), img.height());

        if w <= max_dim && h <= max_dim && !is_progressive_jpeg {
            // Already safe dimensions and standard baseline format, zero-cost bypass
            return Ok(data.to_vec());
        }

        let resized = if w > max_dim || h > max_dim {
            img.resize(max_dim, max_dim, FilterType::Lanczos3)
        } else {
            img
        };

        let mut out = Cursor::new(Vec::new());
        let is_png = data.starts_with(b"\x89PNG");
        if is_png && resized.color().has_alpha() {
            resized.write_to(&mut out, image::ImageFormat::Png)?;
        } else {
            let rgb = Self::to_rgb_with_white_bg(&resized);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
            encoder.encode_image(&rgb)?;
        }

        Ok(out.into_inner())
    }

    /// Transcodes a cover image to Kindle-compliant baseline RGB JPEG (JFIF) format (quality 90),
    /// alpha-compositing transparent pixels over white and downscaling if exceeding max_dim.
    /// This is strictly required for Kindle lockscreen ("Display Cover") compatibility.
    pub fn optimize_cover_image(data: &[u8], max_dim: u32) -> Result<Vec<u8>, CoverError> {
        if data.is_empty() {
            return Err(CoverError::EmptyData);
        }

        let img = Self::decode_with_safety_limits(data)?;
        let (w, h) = (img.width(), img.height());

        let resized = if w > max_dim || h > max_dim {
            img.resize(max_dim, max_dim, FilterType::Lanczos3)
        } else {
            img
        };

        let rgb = Self::to_rgb_with_white_bg(&resized);
        let mut out = Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        encoder.encode_image(&rgb)?;

        Ok(out.into_inner())
    }

    /// Extracts pixel dimensions (width, height) efficiently without decoding full raster pixels.
    pub fn get_image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        if data.is_empty() {
            return None;
        }
        image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    }

    /// Normalizes raw image bytes into a Kindle-compliant baseline JPEG thumbnail.
    pub fn optimize_thumbnail(data: &[u8]) -> Result<Vec<u8>, CoverError> {
        if data.is_empty() {
            return Err(CoverError::EmptyData);
        }

        let img = Self::decode_with_safety_limits(data)?;
        let rgb_img = Self::to_rgb_with_white_bg(&img);

        // Resize down if larger than 440x660
        let (w, h) = rgb_img.dimensions();
        let resized = if w > KINDLE_THUMB_MAX_WIDTH || (h as u64) > KINDLE_THUMB_MAX_HEIGHT {
            let dyn_img = DynamicImage::ImageRgb8(rgb_img);
            dyn_img
                .resize(
                    KINDLE_THUMB_MAX_WIDTH,
                    KINDLE_THUMB_MAX_HEIGHT as u32,
                    FilterType::Lanczos3,
                )
                .to_rgb8()
        } else {
            rgb_img
        };

        let mut out = Cursor::new(Vec::new());
        // Encode standard baseline JPEG (quality 85)
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        encoder.encode_image(&resized)?;

        Ok(out.into_inner())
    }

    /// Converts an image to RGB8, alpha-compositing transparent pixels over a white background.
    /// Fast path for opaque images (e.g. pure RGB JPEGs) directly converts to RGB8 without
    /// allocating an intermediate 4x byte RGBA buffer or iterating pixel coordinates.
    fn to_rgb_with_white_bg(img: &DynamicImage) -> image::RgbImage {
        if !img.color().has_alpha() {
            return img.to_rgb8();
        }

        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut rgb = image::RgbImage::new(width, height);

        for (x, y, pixel) in rgba.enumerate_pixels() {
            let [r, g, b, a] = pixel.0;
            if a == 255 {
                rgb.put_pixel(x, y, image::Rgb([r, g, b]));
            } else if a == 0 {
                // Fully transparent -> white
                rgb.put_pixel(x, y, image::Rgb([255, 255, 255]));
            } else {
                // Alpha blend over white (255, 255, 255)
                let alpha = (a as f32) / 255.0;
                let bg_alpha = 1.0 - alpha;
                let blended_r = ((r as f32) * alpha + 255.0 * bg_alpha).round() as u8;
                let blended_g = ((g as f32) * alpha + 255.0 * bg_alpha).round() as u8;
                let blended_b = ((b as f32) * alpha + 255.0 * bg_alpha).round() as u8;
                rgb.put_pixel(x, y, image::Rgb([blended_r, blended_g, blended_b]));
            }
        }

        rgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbaImage};

    #[test]
    fn test_optimize_rgba_png_to_jpeg() {
        // Create an 800x1200 RGBA test image in memory
        let mut rgba = RgbaImage::new(800, 1200);
        for pixel in rgba.pixels_mut() {
            *pixel = image::Rgba([100, 150, 200, 128]); // Semi-transparent blue
        }

        let mut png_bytes = Cursor::new(Vec::new());
        rgba.write_to(&mut png_bytes, ImageFormat::Png).unwrap();

        let optimized = CoverOptimizer::optimize_thumbnail(&png_bytes.into_inner()).unwrap();
        assert!(!optimized.is_empty());
        assert_eq!(&optimized[0..2], b"\xff\xd8"); // Valid JPEG SOI marker

        // Verify decoded dimensions are within 440x660
        let decoded = image::load_from_memory(&optimized).unwrap();
        assert!(decoded.width() <= 440);
        assert!(decoded.height() <= 660);
    }

    #[test]
    fn test_optimize_content_image_downscaling() {
        // 1. Image already within bounds (500x500) -> bytes returned untouched
        let small_img = RgbaImage::new(500, 500);
        let mut small_bytes = Cursor::new(Vec::new());
        small_img.write_to(&mut small_bytes, ImageFormat::Png).unwrap();
        let small_raw = small_bytes.into_inner();
        let result = CoverOptimizer::optimize_content_image(&small_raw, 2560).unwrap();
        assert_eq!(result, small_raw);

        // 2. Oversized image (3000x2000) -> resized down to within 2560
        let big_img = RgbaImage::new(3000, 2000);
        let mut big_bytes = Cursor::new(Vec::new());
        big_img.write_to(&mut big_bytes, ImageFormat::Png).unwrap();
        let result_big = CoverOptimizer::optimize_content_image(&big_bytes.into_inner(), 2560).unwrap();
        let decoded_big = image::load_from_memory(&result_big).unwrap();
        assert!(decoded_big.width() <= 2560);
        assert!(decoded_big.height() <= 2560);
    }

    #[test]
    fn test_optimize_content_image_progressive_jpeg_converted_to_baseline() {
        // A minimal valid Progressive JPEG (SOF2 marker [0xFF, 0xC2])
        let prog_jpeg: [u8; 521] = [
            255, 216, 255, 224, 0, 16, 74, 70, 73, 70, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 255, 219, 0, 67, 0, 8, 6, 6, 7, 6, 5, 8, 7, 7, 7, 9, 9, 8, 10, 12, 20, 13, 12, 11, 11, 12, 25, 18, 19, 15, 20, 29, 26, 31, 30, 29, 26, 28, 28, 32, 36, 46, 39, 32, 34, 44, 35, 28, 28, 40, 55, 41, 44, 48, 49, 52, 52, 52, 31, 39, 57, 61, 56, 50, 60, 46, 51, 52, 50, 255, 219, 0, 67, 1, 9, 9, 9, 12, 11, 12, 24, 13, 13, 24, 50, 33, 28, 33, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 255, 194, 0, 17, 8, 0, 10, 0, 10, 3, 1, 34, 0, 2, 17, 1, 3, 17, 1, 255, 196, 0, 21, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 255, 196, 0, 21, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 6, 255, 218, 0, 12, 3, 1, 0, 2, 16, 3, 16, 0, 0, 1, 138, 12, 184, 255, 196, 0, 20, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 255, 218, 0, 8, 1, 1, 0, 1, 5, 2, 31, 255, 196, 0, 20, 17, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 218, 0, 8, 1, 3, 1, 1, 63, 1, 127, 255, 196, 0, 20, 17, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 218, 0, 8, 1, 2, 1, 1, 63, 1, 127, 255, 196, 0, 20, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 255, 218, 0, 8, 1, 1, 0, 6, 63, 2, 31, 255, 196, 0, 20, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 255, 218, 0, 8, 1, 1, 0, 1, 63, 33, 31, 255, 218, 0, 12, 3, 1, 0, 2, 0, 3, 0, 0, 0, 16, 7, 255, 196, 0, 20, 17, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 218, 0, 8, 1, 3, 1, 1, 63, 16, 127, 255, 196, 0, 20, 17, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 218, 0, 8, 1, 2, 1, 1, 63, 16, 127, 255, 196, 0, 20, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 255, 218, 0, 8, 1, 1, 0, 1, 63, 16, 31, 255, 217
        ];
        assert!(prog_jpeg.windows(2).any(|w| w == [0xff, 0xc2]));

        let baseline_res = CoverOptimizer::optimize_content_image(&prog_jpeg, 2560).unwrap();

        // Verify converted to baseline JPEG (has SOF0 [0xFF, 0xC0] and NO SOF2 [0xFF, 0xC2])
        assert!(baseline_res.windows(2).any(|w| w == [0xff, 0xc0]));
        assert!(!baseline_res.windows(2).any(|w| w == [0xff, 0xc2]));
    }

    #[test]
    fn test_image_bomb_dimension_rejection() {
        // Minimal valid PNG (65 bytes) with 10,000 x 10,000 dimensions
        let png_bytes: [u8; 65] = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 39, 16, 0, 0,
            39, 16, 8, 0, 0, 0, 0, 159, 37, 61, 251, 0, 0, 0, 8, 73, 68, 65, 84, 120, 156, 3,
            0, 0, 0, 0, 1, 72, 6, 137, 210, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];

        let res = CoverOptimizer::optimize_content_image(&png_bytes, 2560);
        assert!(matches!(res, Err(CoverError::ImageTooLarge(10000, 10000))));
    }

    #[test]
    fn test_optimize_cover_image_transcodes_png_to_baseline_jpeg() {
        let mut rgba = RgbaImage::new(1200, 1600);
        for pixel in rgba.pixels_mut() {
            *pixel = image::Rgba([50, 100, 150, 255]);
        }
        let mut png_bytes = Cursor::new(Vec::new());
        rgba.write_to(&mut png_bytes, ImageFormat::Png).unwrap();

        let cover_jpeg = CoverOptimizer::optimize_cover_image(&png_bytes.into_inner(), 2560).unwrap();
        println!("Cover JPEG header: {:02x?}", &cover_jpeg[..20]);
        assert_eq!(&cover_jpeg[0..2], b"\xff\xd8"); // Baseline JPEG SOI
        let decoded = image::load_from_memory(&cover_jpeg).unwrap();
        assert_eq!(decoded.width(), 1200);
        assert_eq!(decoded.height(), 1600);
    }

    #[test]
    fn test_cover_optimizer_fast_path_opaque_rgb() {
        use image::RgbImage;
        let mut rgb = RgbImage::new(400, 600);
        for pixel in rgb.pixels_mut() {
            *pixel = image::Rgb([120, 80, 200]);
        }
        let dynamic = DynamicImage::ImageRgb8(rgb);
        assert!(!dynamic.color().has_alpha());

        let converted = CoverOptimizer::to_rgb_with_white_bg(&dynamic);
        assert_eq!(converted.width(), 400);
        assert_eq!(converted.height(), 600);
        assert_eq!(converted.get_pixel(10, 10).0, [120, 80, 200]);

        // Also test thumbnail optimization with opaque image
        let mut jpeg_bytes = Cursor::new(Vec::new());
        dynamic.write_to(&mut jpeg_bytes, ImageFormat::Jpeg).unwrap();
        let thumb_bytes = CoverOptimizer::optimize_thumbnail(&jpeg_bytes.into_inner()).unwrap();
        assert_eq!(&thumb_bytes[0..2], b"\xff\xd8");
        let decoded_thumb = image::load_from_memory(&thumb_bytes).unwrap();
        assert!(decoded_thumb.width() <= KINDLE_THUMB_MAX_WIDTH);
        assert!(decoded_thumb.height() <= KINDLE_THUMB_MAX_HEIGHT as u32);
    }
}
