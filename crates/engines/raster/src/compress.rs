//! Compress: downsamples and recompresses the pictures inside a document.
//!
//! Only images placed straight on a page are changed, because only there is the printed size
//! known. An image changes only when its new version is smaller, so Compress never makes a file
//! bigger. Images with masks, palettes, odd bit depths or CMYK are left alone.

use application::AppError;
use application::ports::{ImageOptimizer, ImageReport};
use domain::ErrorCode;
use domain::tools::optimize::CompressSettings;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{FilterType, resize};
use image::{DynamicImage, ExtendedColorType, GrayImage, ImageFormat, RgbImage};
use lopdf::content::Content;
use lopdf::{Document, LoadOptions, Object, ObjectId};
use std::collections::HashMap;
use std::path::Path;

use crate::RasterConverter;

/// Same cap as the structure reader: a stream may not expand beyond this while loading.
const MAX_STREAM_EXPANSION: usize = 256 * 1024 * 1024;

/// A 2D transform `[a b c d e f]` as in PDF content streams.
type Matrix = [f32; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m` applied first, then `ctm` (the order of the `cm` operator).
fn concat(m: Matrix, ctm: Matrix) -> Matrix {
    [
        m[0] * ctm[0] + m[1] * ctm[2],
        m[0] * ctm[1] + m[1] * ctm[3],
        m[2] * ctm[0] + m[3] * ctm[2],
        m[2] * ctm[1] + m[3] * ctm[3],
        m[4] * ctm[0] + m[5] * ctm[2] + ctm[4],
        m[4] * ctm[1] + m[5] * ctm[3] + ctm[5],
    ]
}

fn numbers(operands: &[Object]) -> Option<Matrix> {
    let mut m = [0.0f32; 6];
    if operands.len() != 6 {
        return None;
    }
    for (slot, o) in m.iter_mut().zip(operands) {
        *slot = o.as_float().ok()?;
    }
    Some(m)
}

/// How large an image is drawn: width and height in points.
#[derive(Debug, Clone, Copy, Default)]
struct Placement {
    width_pt: f32,
    height_pt: f32,
}

fn page_resources(doc: &Document, page: ObjectId) -> Vec<lopdf::Dictionary> {
    let Ok((direct, ids)) = doc.get_page_resources(page) else {
        return Vec::new();
    };
    let mut all: Vec<lopdf::Dictionary> = direct.into_iter().cloned().collect();
    all.extend(
        ids.into_iter()
            .filter_map(|id| doc.get_dictionary(id).ok().cloned()),
    );
    all
}

/// Image objects by resource name on one page.
fn page_images(doc: &Document, page: ObjectId) -> HashMap<Vec<u8>, ObjectId> {
    let mut out = HashMap::new();
    for resources in page_resources(doc, page) {
        let table = match resources.get(b"XObject") {
            Ok(Object::Dictionary(d)) => Some(d.clone()),
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
            _ => None,
        };
        let Some(table) = table else { continue };
        for (name, value) in table.iter() {
            if let Object::Reference(id) = value
                && let Ok(Object::Stream(s)) = doc.get_object(*id)
                && s.dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == b"Image")
            {
                out.insert(name.clone(), *id);
            }
        }
    }
    out
}

/// The largest size each image is drawn at, over every page that draws it.
fn placements(doc: &Document) -> HashMap<ObjectId, Placement> {
    let mut result: HashMap<ObjectId, Placement> = HashMap::new();
    for page in doc.get_pages().into_values() {
        let images = page_images(doc, page);
        if images.is_empty() {
            continue;
        }
        let Ok(bytes) = doc.get_page_content_with_limit(page, MAX_STREAM_EXPANSION) else {
            continue;
        };
        let Ok(content) = Content::decode(&bytes) else {
            continue;
        };
        let mut ctm = IDENTITY;
        let mut stack: Vec<Matrix> = Vec::new();
        for op in &content.operations {
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(IDENTITY),
                "cm" => {
                    if let Some(m) = numbers(&op.operands) {
                        ctm = concat(m, ctm);
                    }
                }
                "Do" => {
                    let Some(Object::Name(name)) = op.operands.first() else {
                        continue;
                    };
                    let Some(id) = images.get(name) else { continue };
                    let width_pt = ctm[0].hypot(ctm[1]);
                    let height_pt = ctm[2].hypot(ctm[3]);
                    let entry = result.entry(*id).or_default();
                    entry.width_pt = entry.width_pt.max(width_pt);
                    entry.height_pt = entry.height_pt.max(height_pt);
                }
                _ => {}
            }
        }
    }
    result
}

/// What kind of pixels an image holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pixels {
    Gray,
    Rgb,
}

/// Reads the colour space of a simple image (device spaces or an ICC profile of 1 or 3
/// components). Anything else is not touched.
fn pixel_kind(doc: &Document, dict: &lopdf::Dictionary) -> Option<Pixels> {
    let space = dict.get(b"ColorSpace").ok()?;
    let space = match space {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match space {
        Object::Name(n) if n == b"DeviceGray" => Some(Pixels::Gray),
        Object::Name(n) if n == b"DeviceRGB" => Some(Pixels::Rgb),
        Object::Array(items)
            if items.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") =>
        {
            let profile = items.get(1)?.as_reference().ok()?;
            let Object::Stream(s) = doc.get_object(profile).ok()? else {
                return None;
            };
            match s.dict.get(b"N").ok()?.as_i64().ok()? {
                1 => Some(Pixels::Gray),
                3 => Some(Pixels::Rgb),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The single filter of an image, when it has exactly one.
fn single_filter(dict: &lopdf::Dictionary) -> Option<Vec<u8>> {
    match dict.get(b"Filter").ok()? {
        Object::Name(n) => Some(n.clone()),
        Object::Array(items) if items.len() == 1 => items[0].as_name().ok().map(<[u8]>::to_vec),
        _ => None,
    }
}

/// Decodes an image that qualifies for recompression, or `None`.
fn decode(doc: &Document, id: ObjectId) -> Option<(DynamicImage, Pixels)> {
    let Object::Stream(stream) = doc.get_object(id).ok()? else {
        return None;
    };
    let dict = &stream.dict;
    let simple = dict
        .get(b"BitsPerComponent")
        .and_then(Object::as_i64)
        .is_ok_and(|b| b == 8)
        && !dict.has(b"Mask")
        && !dict.has(b"Decode")
        && !dict
            .get(b"ImageMask")
            .and_then(Object::as_bool)
            .unwrap_or(false);
    if !simple {
        return None;
    }
    let kind = pixel_kind(doc, dict)?;
    let (width, height) = (
        u32::try_from(dict.get(b"Width").ok()?.as_i64().ok()?).ok()?,
        u32::try_from(dict.get(b"Height").ok()?.as_i64().ok()?).ok()?,
    );
    if u64::from(width) * u64::from(height) > 100_000_000 {
        return None;
    }
    match single_filter(dict)?.as_slice() {
        b"DCTDecode" => {
            let image =
                image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg).ok()?;
            let comps = if image.color().has_color() {
                Pixels::Rgb
            } else {
                Pixels::Gray
            };
            (comps == kind).then_some((image, kind))
        }
        b"FlateDecode" => {
            if dict.has(b"DecodeParms") {
                return None;
            }
            let raw = stream
                .decompressed_content_with_limit(MAX_STREAM_EXPANSION)
                .ok()?;
            let comps = if kind == Pixels::Rgb { 3 } else { 1 };
            if raw.len() != width as usize * height as usize * comps {
                return None;
            }
            let image = match kind {
                Pixels::Rgb => DynamicImage::ImageRgb8(RgbImage::from_raw(width, height, raw)?),
                Pixels::Gray => DynamicImage::ImageLuma8(GrayImage::from_raw(width, height, raw)?),
            };
            Some((image, kind))
        }
        _ => None,
    }
}

/// Pixel size an image needs to print at `max_ppi`, or its own size when it is already small.
fn target_size(width: u32, height: u32, placed: Placement, max_ppi: u32) -> (u32, u32) {
    if placed.width_pt <= 0.0 || placed.height_pt <= 0.0 {
        return (width, height);
    }
    let need_w = (placed.width_pt / 72.0 * max_ppi as f32).ceil().max(1.0);
    let need_h = (placed.height_pt / 72.0 * max_ppi as f32).ceil().max(1.0);
    let scale = (need_w / width as f32).min(need_h / height as f32).min(1.0);
    if scale >= 1.0 {
        return (width, height);
    }
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

fn encode_jpeg(image: &DynamicImage, kind: Pixels, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    match kind {
        Pixels::Rgb => {
            let rgb = image.to_rgb8();
            encoder
                .encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .ok()?;
        }
        Pixels::Gray => {
            let gray = image.to_luma8();
            encoder
                .encode(
                    gray.as_raw(),
                    gray.width(),
                    gray.height(),
                    ExtendedColorType::L8,
                )
                .ok()?;
        }
    }
    Some(out)
}

impl ImageOptimizer for RasterConverter {
    fn optimize_images(
        &self,
        source: &Path,
        settings: &CompressSettings,
        dest: &Path,
    ) -> Result<ImageReport, AppError> {
        let options = LoadOptions {
            max_decompressed_size: Some(MAX_STREAM_EXPANSION),
            ..LoadOptions::default()
        };
        let mut doc = Document::load_with_options(source, options).map_err(|_| {
            AppError::new(
                ErrorCode::PdfDamaged,
                "This file is damaged or uses an unsupported format. Try the Repair tool.",
            )
        })?;
        let placed = placements(&doc);
        let mut report = ImageReport {
            found: doc
                .objects
                .values()
                .filter(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image")))
                .count(),
            ..ImageReport::default()
        };

        for (id, placement) in placed {
            let Some((image, kind)) = decode(&doc, id) else {
                continue;
            };
            let (width, height) = (image.width(), image.height());
            let (new_w, new_h) = target_size(width, height, placement, settings.max_ppi);
            let image = if (new_w, new_h) == (width, height) {
                image
            } else {
                resize_image(&image, kind, new_w, new_h)
            };
            let (image, kind) = if settings.grayscale && kind == Pixels::Rgb {
                (DynamicImage::ImageLuma8(image.to_luma8()), Pixels::Gray)
            } else {
                (image, kind)
            };
            let Some(jpeg) = encode_jpeg(&image, kind, settings.jpeg_quality) else {
                continue;
            };
            let Ok(Object::Stream(stream)) = doc.get_object_mut(id) else {
                continue;
            };
            // A bigger result is no gain unless the colour had to change.
            let forced = kind == Pixels::Gray && settings.grayscale && pixel_is_color(&stream.dict);
            if jpeg.len() >= stream.content.len() && !forced {
                continue;
            }
            stream.dict.set("Width", i64::from(image.width()));
            stream.dict.set("Height", i64::from(image.height()));
            stream.dict.set(
                "ColorSpace",
                Object::Name(if kind == Pixels::Gray {
                    b"DeviceGray".to_vec()
                } else {
                    b"DeviceRGB".to_vec()
                }),
            );
            stream.dict.set("BitsPerComponent", 8);
            stream
                .dict
                .set("Filter", Object::Name(b"DCTDecode".to_vec()));
            stream.dict.remove(b"DecodeParms");
            stream.dict.remove(b"Length");
            stream.content = jpeg;
            stream.allows_compression = false;
            report.replaced += 1;
        }
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the compressed document", &e))?;
        Ok(report)
    }
}

/// True when the image dictionary still names a colour (not gray) space.
fn pixel_is_color(dict: &lopdf::Dictionary) -> bool {
    !matches!(dict.get(b"ColorSpace"), Ok(Object::Name(n)) if n == b"DeviceGray")
}

fn resize_image(image: &DynamicImage, kind: Pixels, w: u32, h: u32) -> DynamicImage {
    match kind {
        Pixels::Rgb => {
            DynamicImage::ImageRgb8(resize(&image.to_rgb8(), w, h, FilterType::CatmullRom))
        }
        Pixels::Gray => {
            DynamicImage::ImageLuma8(resize(&image.to_luma8(), w, h, FilterType::CatmullRom))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::ImageConverter;
    use image::{Rgb, RgbImage};

    /// A noisy RGB picture, so JPEG and Flate both have real work to do.
    fn noisy(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let n = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663);
            Rgb([(n >> 3) as u8, (n >> 11) as u8, (n >> 19) as u8])
        })
    }

    /// A one-page PDF made from `image`, which `image_to_pdf` sizes at 150 dpi.
    fn pdf_of(dir: &tempfile::TempDir, image: &RgbImage, jpeg: bool) -> std::path::PathBuf {
        let file = dir.path().join(if jpeg { "in.jpg" } else { "in.png" });
        image.save(&file).unwrap();
        let out = dir.path().join("in.pdf");
        RasterConverter.image_to_pdf(&file, &out).unwrap();
        out
    }

    fn settings(quality: u8, max_ppi: u32, grayscale: bool) -> CompressSettings {
        CompressSettings {
            jpeg_quality: quality,
            max_ppi,
            grayscale,
        }
    }

    fn only_image(doc: &Document) -> lopdf::Stream {
        doc.objects
            .values()
            .find_map(|o| match o {
                Object::Stream(s)
                    if s.dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|n| n == b"Image") =>
                {
                    Some(s.clone())
                }
                _ => None,
            })
            .expect("one image")
    }

    #[test]
    fn a_150_dpi_picture_is_reduced_to_the_limit_and_the_file_shrinks() {
        let dir = tempfile::tempdir().unwrap();
        let src = pdf_of(&dir, &noisy(900, 600), false);
        let dest = dir.path().join("out.pdf");
        let report = RasterConverter
            .optimize_images(&src, &settings(60, 72, false), &dest)
            .unwrap();
        assert_eq!((report.found, report.replaced), (1, 1));

        let image = only_image(&Document::load(&dest).unwrap());
        assert_eq!(
            image.dict.get(b"Width").unwrap().as_i64().unwrap(),
            432,
            "150 dpi down to 72"
        );
        assert_eq!(image.dict.get(b"Height").unwrap().as_i64().unwrap(), 288);
        assert_eq!(single_filter(&image.dict).unwrap(), b"DCTDecode");
        assert!(std::fs::metadata(&dest).unwrap().len() < std::fs::metadata(&src).unwrap().len());
    }

    #[test]
    fn pictures_below_the_limit_keep_their_size() {
        let dir = tempfile::tempdir().unwrap();
        let src = pdf_of(&dir, &noisy(300, 200), false);
        let dest = dir.path().join("out.pdf");
        RasterConverter
            .optimize_images(&src, &settings(60, 288, false), &dest)
            .unwrap();
        let image = only_image(&Document::load(&dest).unwrap());
        assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 300);
    }

    #[test]
    fn jpeg_pictures_are_recompressed_at_the_chosen_quality() {
        let dir = tempfile::tempdir().unwrap();
        let src = pdf_of(&dir, &noisy(400, 400), true);
        let dest = dir.path().join("out.pdf");
        let high = RasterConverter
            .optimize_images(&src, &settings(95, 720, false), &dest)
            .unwrap();
        let low_dest = dir.path().join("low.pdf");
        let low = RasterConverter
            .optimize_images(&src, &settings(20, 720, false), &low_dest)
            .unwrap();
        assert_eq!(low.replaced, 1);
        assert!(
            std::fs::metadata(&low_dest).unwrap().len() < std::fs::metadata(&dest).unwrap().len()
                || high.replaced == 0
        );
    }

    #[test]
    fn grayscale_turns_color_pictures_gray() {
        let dir = tempfile::tempdir().unwrap();
        let src = pdf_of(&dir, &noisy(300, 300), false);
        let dest = dir.path().join("out.pdf");
        RasterConverter
            .optimize_images(&src, &settings(60, 720, true), &dest)
            .unwrap();
        let image = only_image(&Document::load(&dest).unwrap());
        assert_eq!(
            image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
            b"DeviceGray"
        );
    }

    #[test]
    fn a_picture_that_would_get_bigger_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        // A flat color compresses to almost nothing with Flate; JPEG cannot beat that.
        let flat = RgbImage::from_pixel(600, 600, Rgb([200, 30, 30]));
        let src = pdf_of(&dir, &flat, false);
        let dest = dir.path().join("out.pdf");
        let report = RasterConverter
            .optimize_images(&src, &settings(95, 720, false), &dest)
            .unwrap();
        assert_eq!(report.replaced, 0);
        let image = only_image(&Document::load(&dest).unwrap());
        assert_eq!(single_filter(&image.dict).unwrap(), b"FlateDecode");
    }

    #[test]
    fn placement_follows_the_transform_not_the_pixels() {
        // 300x300 pixels drawn 72 points wide is 300 ppi, so 150 ppi halves it.
        assert_eq!(
            target_size(
                300,
                300,
                Placement {
                    width_pt: 72.0,
                    height_pt: 72.0
                },
                150
            ),
            (150, 150)
        );
        assert_eq!(
            target_size(
                100,
                100,
                Placement {
                    width_pt: 72.0,
                    height_pt: 72.0
                },
                150
            ),
            (100, 100)
        );
        assert_eq!(target_size(300, 300, Placement::default(), 72), (300, 300));
        let m = concat(
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
            [72.0, 0.0, 0.0, 36.0, 5.0, 5.0],
        );
        assert_eq!(m[0], 144.0);
        assert_eq!(m[3], 72.0);
    }

    #[test]
    fn damaged_input_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("j.pdf");
        std::fs::write(&junk, b"nope").unwrap();
        let err = RasterConverter
            .optimize_images(&junk, &settings(60, 72, false), &dir.path().join("o.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PdfDamaged);
    }
}
