//! Raster adapter: turns an image file into a one-page PDF.
//!
//! JPEGs are embedded as they are (no re-encoding, no quality loss). Other formats are decoded and
//! stored losslessly with Flate compression, with transparency kept as a soft mask. The page is
//! sized from the pixels at 150 dpi. HEIC arrives with the Windows imaging component in Phase 6.
#![forbid(unsafe_code)]

mod compress;
mod heic;
mod imagepdf;
mod pages;
mod pictures;
mod scan;
mod scanpdf;
mod tiffio;

use application::AppError;
use application::ports::ImageConverter;
use domain::ErrorCode;
use domain::tools::convert::ImagesToPdfOptions;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::{ColorType, DynamicImage, ImageDecoder};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

/// Largest image accepted, in pixels, to bound memory (a 100 megapixel RGBA image is 400 MB).
const MAX_PIXELS: u64 = 100_000_000;

/// [`ImageConverter`] using the `image` crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct RasterConverter;

pub(crate) fn unreadable() -> AppError {
    AppError::new(
        ErrorCode::InputNotPdf,
        "This image could not be read. It may be damaged or in a format Quirewise does not support yet.",
    )
}

pub(crate) fn too_big() -> AppError {
    AppError::new(
        ErrorCode::PdfLimitExceeded,
        "This image is larger than the safety limit of 100 megapixels. Reduce its size first.",
    )
}

pub(crate) fn flate(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .and_then(|()| encoder.finish())
        .map_err(|e| AppError::from_io("compress the image", &e))
}

/// A decoded image prepared for a PDF image object.
struct Prepared {
    width: u32,
    height: u32,
    color_space: &'static str,
    filter: &'static str,
    data: Vec<u8>,
    /// Flate-compressed 8-bit alpha, when the image has transparency.
    alpha: Option<Vec<u8>>,
    /// Bits per color component: 8, or 1 for black and white scans.
    bits: u8,
    /// Quarter turns clockwise (0, 90, 180 or 270) that show the pixels upright.
    turns: u16,
    /// Extra settings of the filter, such as the width of a CCITT Group 4 image.
    decode_parms: Option<lopdf::Dictionary>,
}

/// JPEG bytes can be embedded directly when they are 8-bit gray or RGB.
fn prepare_jpeg(bytes: &[u8]) -> Option<Prepared> {
    let decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    let color_space = match decoder.color_type() {
        ColorType::L8 => "DeviceGray",
        ColorType::Rgb8 => "DeviceRGB",
        _ => return None,
    };
    Some(Prepared {
        width,
        height,
        color_space,
        filter: "DCTDecode",
        data: bytes.to_vec(),
        alpha: None,
        bits: 8,
        turns: 0,
        decode_parms: None,
    })
}

fn prepare_decoded(image: DynamicImage) -> Result<Prepared, AppError> {
    let (width, height) = (image.width(), image.height());
    if image.color().has_alpha() {
        let rgba = image.to_rgba8();
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for pixel in rgba.pixels() {
            rgb.extend_from_slice(&pixel.0[..3]);
            alpha.push(pixel.0[3]);
        }
        return Ok(Prepared {
            width,
            height,
            color_space: "DeviceRGB",
            filter: "FlateDecode",
            data: flate(&rgb)?,
            alpha: Some(flate(&alpha)?),
            bits: 8,
            turns: 0,
            decode_parms: None,
        });
    }
    let (color_space, raw) = if image.color().has_color() {
        ("DeviceRGB", image.to_rgb8().into_raw())
    } else {
        ("DeviceGray", image.to_luma8().into_raw())
    };
    Ok(Prepared {
        width,
        height,
        color_space,
        filter: "FlateDecode",
        data: flate(&raw)?,
        alpha: None,
        bits: 8,
        turns: 0,
        decode_parms: None,
    })
}

impl ImageConverter for RasterConverter {
    fn image_to_pdf(&self, image: &Path, dest: &Path) -> Result<(), AppError> {
        imagepdf::write_pdf(
            &[image.to_path_buf()],
            &ImagesToPdfOptions::default(),
            &std::env::temp_dir(),
            dest,
        )
    }

    fn images_to_pdf(
        &self,
        images: &[PathBuf],
        layout: &ImagesToPdfOptions,
        scratch: &Path,
        dest: &Path,
    ) -> Result<(), AppError> {
        imagepdf::write_pdf(images, layout, scratch, dest)
    }

    fn tiff_to_pdf(&self, tiff: &Path, dest: &Path) -> Result<u32, AppError> {
        tiffio::tiff_to_pdf(tiff, dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::convert::MAX_PAGE_POINTS;
    use image::ImageFormat;
    use image::{GrayImage, ImageEncoder, Luma, Rgb, RgbImage, Rgba, RgbaImage};
    use lopdf::{Document, Object};
    use std::fs;

    fn convert(bytes: &[u8], name: &str) -> (tempfile::TempDir, Document) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join(name);
        fs::write(&src, bytes).unwrap();
        let dest = dir.path().join("out.pdf");
        RasterConverter.image_to_pdf(&src, &dest).unwrap();
        let doc = Document::load(&dest).unwrap();
        (dir, doc)
    }

    fn png_of(image: DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn image_object(doc: &Document) -> &lopdf::Dictionary {
        doc.objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .map(|s| &s.dict)
            .find(|d| {
                d.get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == b"Image")
            })
            .expect("an image object")
    }

    #[test]
    fn rgb_png_becomes_a_flate_rgb_image_on_a_proportional_page() {
        let img = RgbImage::from_fn(300, 150, |x, _| Rgb([(x % 255) as u8, 10, 20]));
        let (_dir, doc) = convert(&png_of(DynamicImage::ImageRgb8(img)), "a.png");
        assert_eq!(doc.get_pages().len(), 1);
        let dict = image_object(&doc);
        assert_eq!(
            dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
            b"DeviceRGB"
        );
        assert_eq!(
            dict.get(b"Filter").unwrap().as_name().unwrap(),
            b"FlateDecode"
        );
        let (_, page) = doc.get_pages().into_iter().next().unwrap();
        let [x0, y0, x1, y1] = crate_box(&doc, page);
        // 300 x 150 px at 150 dpi is 144 x 72 pt.
        assert_eq!((x1 - x0, y1 - y0), (144.0, 72.0));
    }

    fn crate_box(doc: &Document, page: lopdf::ObjectId) -> [f32; 4] {
        let media = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap();
        let n: Vec<f32> = media.iter().map(|o| o.as_float().unwrap()).collect();
        [n[0], n[1], n[2], n[3]]
    }

    #[test]
    fn transparency_is_kept_as_a_soft_mask() {
        let img = RgbaImage::from_fn(20, 20, |x, _| Rgba([200, 0, 0, (x * 12) as u8]));
        let (_dir, doc) = convert(&png_of(DynamicImage::ImageRgba8(img)), "t.png");
        let has_smask = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .any(|s| s.dict.has(b"SMask"));
        assert!(has_smask);
    }

    #[test]
    fn gray_images_use_the_gray_color_space() {
        let img = GrayImage::from_fn(30, 30, |x, y| Luma([((x + y) * 4) as u8]));
        let (_dir, doc) = convert(&png_of(DynamicImage::ImageLuma8(img)), "g.png");
        assert!(
            doc.objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .any(|s| {
                    s.dict
                        .get(b"ColorSpace")
                        .and_then(Object::as_name)
                        .is_ok_and(|n| n == b"DeviceGray")
                })
        );
    }

    #[test]
    fn jpegs_are_embedded_without_re_encoding() {
        let img = RgbImage::from_fn(64, 48, |x, y| Rgb([(x * 4) as u8, (y * 5) as u8, 90]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
            .write_image(img.as_raw(), 64, 48, image::ExtendedColorType::Rgb8)
            .unwrap();
        let (_dir, doc) = convert(&jpeg, "p.jpg");
        let embedded = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find(|s| {
                s.dict
                    .get(b"Filter")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == b"DCTDecode")
            })
            .expect("a DCT image");
        assert_eq!(embedded.content, jpeg, "the JPEG bytes are untouched");
    }

    #[test]
    fn huge_pages_are_scaled_to_fit_the_pdf_limit() {
        // 40,000 x 10 px would be 19,200 pt wide at 150 dpi.
        let img = GrayImage::new(40_000, 10);
        let (_dir, doc) = convert(&png_of(DynamicImage::ImageLuma8(img)), "wide.png");
        let (_, page) = doc.get_pages().into_iter().next().unwrap();
        let [_, _, w, h] = crate_box(&doc, page);
        assert!((w - MAX_PAGE_POINTS).abs() < 1.0);
        assert!(h > 0.0 && h < 10.0);
    }

    #[test]
    fn unreadable_and_empty_files_are_rejected_with_a_catalog_code() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("junk.png", b"not an image".to_vec()),
            ("empty.jpg", Vec::new()),
        ] {
            let src = dir.path().join(name);
            fs::write(&src, bytes).unwrap();
            let err = RasterConverter
                .image_to_pdf(&src, &dir.path().join("o.pdf"))
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InputNotPdf, "{name}");
        }
        let missing = RasterConverter
            .image_to_pdf(&dir.path().join("none.png"), &dir.path().join("o.pdf"))
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::PathDenied);
    }
}
