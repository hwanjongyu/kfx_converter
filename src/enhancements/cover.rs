//! Cover Image Optimizer for Kindle Scribe & Paperwhite
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

#[derive(Error, Debug)]
pub enum CoverError {
    #[error("Image decode error: {0}")]
    Decode(#[from] image::ImageError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported or empty image data")]
    EmptyData,
}

pub struct CoverOptimizer;

impl CoverOptimizer {
    /// Resizes down high-resolution content images exceeding max_dim (default 2560x2560) to protect
    /// Kindle devices with 512MB RAM from pmond OOM crashes.
    /// If image dimensions are already <= max_dim, returns the original bytes untouched.
    pub fn optimize_content_image(data: &[u8], max_dim: u32) -> Result<Vec<u8>, CoverError> {
        if data.is_empty() {
            return Err(CoverError::EmptyData);
        }

        let img = image::load_from_memory(data)?;
        let (w, h) = (img.width(), img.height());

        if w <= max_dim && h <= max_dim {
            // Already safe, zero-cost bypass
            return Ok(data.to_vec());
        }

        let resized = img.resize(max_dim, max_dim, FilterType::Lanczos3);

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

        let img = image::load_from_memory(data)?;
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
    fn to_rgb_with_white_bg(img: &DynamicImage) -> image::RgbImage {
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
}
