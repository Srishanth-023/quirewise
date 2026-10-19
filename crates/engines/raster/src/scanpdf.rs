//! Scan to PDF: pictures of pages become the pages of a PDF, cleaned and in the colors asked for.

use application::AppError;
use application::CancellationToken;
use application::ports::{RenderedImage, ScanPageWriter};
use domain::ErrorCode;
use domain::tools::convert::{MAX_PAGE_POINTS, Placement};
use domain::tools::recognize::ScanToPdfOptions;
use fax::{Color as FaxColor, VecWriter, encoder::Encoder as FaxEncoder};
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageReader};
use lopdf::dictionary;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::imagepdf::PageBuilder;
use crate::scan::{Cleanup, Reduced, clean, reduce};
use crate::{MAX_PIXELS, Prepared, RasterConverter, too_big, unreadable};

/// JPEG quality of scanned pages.
const JPEG_QUALITY: u8 = 85;

/// Reads a picture file into pixels, turned upright by its EXIF orientation.
pub(crate) fn load_pixels(path: &Path, scratch: &Path) -> Result<RenderedImage, AppError> {
    let mut bytes = fs::read(path).map_err(|e| AppError::from_io("read the picture", &e))?;
    if crate::heic::is_heic(&bytes) {
        bytes = crate::heic::to_png(path, scratch)?;
    }
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| unreadable())?;
    let mut decoder = reader.into_decoder().map_err(|_| unreadable())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(unreadable());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(too_big());
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|_| unreadable())?;
    picture.apply_orientation(orientation);
    let rgba = picture.to_rgba8();
    Ok(RenderedImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn encode_failed() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "A scanned page could not be stored. Try a lower resolution.",
    )
}

fn jpeg(
    samples: &[u8],
    width: u32,
    height: u32,
    color: ExtendedColorType,
    dpi: u32,
) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    encoder.set_pixel_density(PixelDensity::dpi(u16::try_from(dpi).unwrap_or(u16::MAX)));
    encoder
        .encode(samples, width, height, color)
        .map_err(|_| encode_failed())?;
    Ok(out)
}

/// A page ready for the PDF: JPEG for color and gray, CCITT Group 4 for black and white.
fn prepare(image: &RenderedImage, reduced: &Reduced, dpi: u32) -> Result<Prepared, AppError> {
    let (width, height) = (image.width, image.height);
    let base = |color_space, filter, data, bits, decode_parms| Prepared {
        width,
        height,
        color_space,
        filter,
        data,
        alpha: None,
        bits,
        turns: 0,
        decode_parms,
    };
    Ok(match reduced {
        Reduced::Rgb(rgb) => base(
            "DeviceRGB",
            "DCTDecode",
            jpeg(rgb, width, height, ExtendedColorType::Rgb8, dpi)?,
            8,
            None,
        ),
        Reduced::Gray(gray) => base(
            "DeviceGray",
            "DCTDecode",
            jpeg(gray, width, height, ExtendedColorType::L8, dpi)?,
            8,
            None,
        ),
        Reduced::Bilevel(black) => {
            let columns = u16::try_from(width).map_err(|_| too_big())?;
            let mut encoder = FaxEncoder::new(VecWriter::new());
            for row in black.chunks(width as usize) {
                let pels = row
                    .iter()
                    .map(|b| if *b { FaxColor::Black } else { FaxColor::White });
                encoder
                    .encode_line(pels, columns)
                    .map_err(|_| encode_failed())?;
            }
            let data = encoder.finish().map_err(|_| encode_failed())?.finish();
            base(
                "DeviceGray",
                "CCITTFaxDecode",
                data,
                1,
                Some(dictionary! {
                    "K" => -1,
                    "Columns" => i64::from(width),
                    "Rows" => i64::from(height),
                }),
            )
        }
    })
}

/// The page size in points for a picture of `width` by `height` pixels at `dpi`.
fn placement(width: u32, height: u32, dpi: u32) -> Placement {
    let k = 72.0 / dpi as f32;
    let (mut w, mut h) = (width as f32 * k, height as f32 * k);
    let largest = w.max(h);
    if largest > MAX_PAGE_POINTS {
        let shrink = MAX_PAGE_POINTS / largest;
        (w, h) = (w * shrink, h * shrink);
    }
    Placement {
        page_w: w,
        page_h: h,
        x: 0.0,
        y: 0.0,
        w,
        h,
    }
}

impl ScanPageWriter for RasterConverter {
    fn scans_to_pdf(
        &self,
        images: &[PathBuf],
        options: &ScanToPdfOptions,
        scratch: &Path,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<u32, AppError> {
        if images.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "Add at least one picture.",
            ));
        }
        let steps = Cleanup {
            auto_crop: options.auto_crop,
            perspective_fix: options.perspective_fix,
        };
        let mut builder = PageBuilder::new();
        for path in images {
            cancel.check()?;
            let cleaned = clean(load_pixels(path, scratch)?, steps);
            let reduced = reduce(&cleaned, options.color);
            let page = prepare(&cleaned, &reduced, options.dpi.get())?;
            builder.add_page(
                page,
                &placement(cleaned.width, cleaned.height, options.dpi.get()),
            );
        }
        builder
            .finish()
            .save(dest)
            .map_err(|e| AppError::from_io("write the PDF", &e))?;
        Ok(images.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Dpi;
    use domain::tools::convert::PictureColor;
    use lopdf::{Document, Object};

    /// A photo of a page: white paper with dark text rows on a dark table, saved as a PNG file.
    fn photo_file(dir: &Path, name: &str) -> PathBuf {
        let (w, h) = (500u32, 650u32);
        let mut rgb = image::RgbImage::from_pixel(w, h, image::Rgb([50, 48, 46]));
        for y in 60..590 {
            for x in 50..450 {
                let ink = (y / 12) % 3 == 0 && (x / 7) % 4 != 0;
                let v = if ink { 30 } else { 235 };
                rgb.put_pixel(x, y, image::Rgb([v, v, v]));
            }
        }
        let path = dir.join(name);
        rgb.save(&path).unwrap();
        path
    }

    fn options(color: PictureColor) -> ScanToPdfOptions {
        ScanToPdfOptions {
            color,
            dpi: Dpi::new(200).unwrap(),
            ..ScanToPdfOptions::default()
        }
    }

    fn filters(doc: &Document) -> Vec<String> {
        doc.objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == b"Image")
            })
            .filter_map(|s| {
                s.dict
                    .get(b"Filter")
                    .ok()?
                    .as_name()
                    .ok()
                    .map(|n| String::from_utf8_lossy(n).into_owned())
            })
            .collect()
    }

    #[test]
    fn every_picture_becomes_a_cropped_page_in_the_color_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (
            photo_file(dir.path(), "a.png"),
            photo_file(dir.path(), "b.png"),
        );
        for (color, filter, bits) in [
            (PictureColor::Color, "DCTDecode", 8),
            (PictureColor::Gray, "DCTDecode", 8),
            (PictureColor::BlackWhite, "CCITTFaxDecode", 1),
        ] {
            let dest = dir.path().join("o.pdf");
            let n = RasterConverter
                .scans_to_pdf(
                    &[a.clone(), b.clone()],
                    &options(color),
                    dir.path(),
                    &dest,
                    &CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(n, 2);
            let doc = Document::load(&dest).unwrap();
            assert_eq!(doc.get_pages().len(), 2);
            assert_eq!(filters(&doc), [filter, filter], "{color:?}");
            let image = doc
                .objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .find(|s| s.dict.has(b"BitsPerComponent"))
                .unwrap();
            assert_eq!(
                image
                    .dict
                    .get(b"BitsPerComponent")
                    .unwrap()
                    .as_i64()
                    .unwrap(),
                bits
            );
            // The table around the paper was cut away: about 400 x 530 pixels at 200 dpi.
            let (_, page) = doc.get_pages().into_iter().next().unwrap();
            let media = doc
                .get_dictionary(page)
                .unwrap()
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap();
            let w = media[2].as_float().unwrap();
            assert!((w - 400.0 * 72.0 / 200.0).abs() < 12.0, "{w}");
            fs::remove_file(&dest).unwrap();
        }
    }

    #[test]
    fn without_cropping_the_page_keeps_the_whole_picture() {
        let dir = tempfile::tempdir().unwrap();
        let a = photo_file(dir.path(), "a.png");
        let mut o = options(PictureColor::Color);
        o.auto_crop = false;
        o.perspective_fix = false;
        let dest = dir.path().join("o.pdf");
        RasterConverter
            .scans_to_pdf(&[a], &o, dir.path(), &dest, &CancellationToken::new())
            .unwrap();
        let doc = Document::load(&dest).unwrap();
        let (_, page) = doc.get_pages().into_iter().next().unwrap();
        let media = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap();
        assert!((media[2].as_float().unwrap() - 500.0 * 72.0 / 200.0).abs() < 0.1);
    }

    #[test]
    fn nothing_to_scan_and_broken_pictures_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("o.pdf");
        let none = RasterConverter
            .scans_to_pdf(
                &[],
                &options(PictureColor::Color),
                dir.path(),
                &dest,
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(none.code, ErrorCode::OptionInvalid);
        let junk = dir.path().join("junk.png");
        fs::write(&junk, b"not a picture").unwrap();
        let broken = RasterConverter
            .scans_to_pdf(
                &[junk],
                &options(PictureColor::Color),
                dir.path(),
                &dest,
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(broken.code, ErrorCode::InputNotPdf);
        assert!(!dest.exists());
    }

    #[test]
    fn a_cancelled_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let a = photo_file(dir.path(), "a.png");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let dest = dir.path().join("o.pdf");
        let err = RasterConverter
            .scans_to_pdf(
                &[a],
                &options(PictureColor::Color),
                dir.path(),
                &dest,
                &cancel,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::JobCancelled);
        assert!(!dest.exists());
    }
}
