//! Pictures taken out of a PDF (row 24).
//!
//! JPEG and JPEG 2000 pictures are written exactly as the file stores them. Raw and Flate pictures
//! in gray, RGB, CMYK, ICC based and indexed color spaces become PNG files, with their soft mask
//! as transparency. Scans stored as CCITT or JBIG2 are reported as unsupported.

use application::AppError;
use application::ports::{ExtractReport, ExtractedImage, ExtractedKind};
use domain::PageIndex;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashSet;

use crate::pageres::effective_resources;

/// Largest picture decoded, in pixels, so a hostile file cannot ask for gigabytes.
const MAX_PIXELS: u64 = 100_000_000;
/// Deepest nesting of forms inside forms searched for pictures.
const MAX_FORM_DEPTH: usize = 8;

/// A color space the extractor can turn into pixels.
#[derive(Debug, Clone)]
enum Space {
    Gray,
    Rgb,
    Cmyk,
    /// Index into `table`, which holds `base` samples per entry.
    Indexed {
        base: Box<Space>,
        table: Vec<u8>,
    },
}

impl Space {
    fn components(&self) -> usize {
        match self {
            Self::Gray | Self::Indexed { .. } => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        other => other,
    }
}

fn int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key)
        .ok()
        .map(|o| resolve(doc, o))
        .and_then(|o| o.as_i64().ok())
}

/// The names of a stream's filters, in the order they were applied.
fn filters(doc: &Document, dict: &Dictionary) -> Vec<Vec<u8>> {
    let Some(object) = dict
        .get(b"Filter")
        .ok()
        .or_else(|| dict.get(b"F").ok())
        .map(|o| resolve(doc, o))
    else {
        return Vec::new();
    };
    match object {
        Object::Name(n) => vec![n.clone()],
        Object::Array(items) => items
            .iter()
            .filter_map(|o| resolve(doc, o).as_name().ok().map(<[u8]>::to_vec))
            .collect(),
        _ => Vec::new(),
    }
}

fn space_of(doc: &Document, object: &Object, depth: usize) -> Option<Space> {
    if depth > 4 {
        return None;
    }
    match resolve(doc, object) {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"G" | b"CalGray" => Some(Space::Gray),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(Space::Rgb),
            b"DeviceCMYK" | b"CMYK" => Some(Space::Cmyk),
            _ => None,
        },
        Object::Array(items) => {
            let kind = resolve(doc, items.first()?).as_name().ok()?;
            match kind {
                b"CalGray" => Some(Space::Gray),
                b"CalRGB" => Some(Space::Rgb),
                b"ICCBased" => {
                    let stream = resolve(doc, items.get(1)?).as_stream().ok()?;
                    match int(doc, &stream.dict, b"N")? {
                        1 => Some(Space::Gray),
                        3 => Some(Space::Rgb),
                        4 => Some(Space::Cmyk),
                        _ => None,
                    }
                }
                b"Indexed" | b"I" => {
                    let base = space_of(doc, items.get(1)?, depth + 1)?;
                    if matches!(base, Space::Indexed { .. }) {
                        return None;
                    }
                    let table = match resolve(doc, items.get(3)?) {
                        Object::String(bytes, _) => bytes.clone(),
                        Object::Stream(s) => s.decompressed_content().ok()?,
                        _ => return None,
                    };
                    Some(Space::Indexed {
                        base: Box::new(base),
                        table,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Samples of one row unpacked to one byte each. Samples of 1, 2 and 4 bits are scaled to
/// 0 to 255 unless they are indexes, which keep their value.
fn unpack(data: &[u8], width: usize, comps: usize, bits: u8, scale: bool) -> Vec<u8> {
    let row_samples = width * comps;
    let row_bytes = (row_samples * usize::from(bits)).div_ceil(8);
    let rows = data.len() / row_bytes.max(1);
    let mut out = Vec::with_capacity(rows * row_samples);
    for row in data.chunks_exact(row_bytes.max(1)) {
        match bits {
            8 => out.extend_from_slice(&row[..row_samples]),
            16 => out.extend((0..row_samples).map(|i| row[i * 2])),
            1 | 2 | 4 => {
                let per_byte = 8 / usize::from(bits);
                let max = (1u16 << bits) - 1;
                for i in 0..row_samples {
                    let byte = row[i / per_byte];
                    let shift = 8 - usize::from(bits) * (i % per_byte + 1);
                    let value = u16::from((byte >> shift) & (max as u8));
                    out.push(if scale {
                        (value * 255 / max) as u8
                    } else {
                        value as u8
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> [u8; 3] {
    let k = u16::from(k);
    let channel = |v: u8| (u16::from(255 - v) * (255 - k) / 255) as u8;
    [channel(c), channel(m), channel(y)]
}

/// Pixels as gray (`false`) or RGB (`true`) bytes.
fn to_samples(space: &Space, samples: &[u8]) -> (bool, Vec<u8>) {
    match space {
        Space::Gray => (false, samples.to_vec()),
        Space::Rgb => (true, samples.to_vec()),
        Space::Cmyk => (
            true,
            samples
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| cmyk_to_rgb(p[0], p[1], p[2], p[3]))
                .collect(),
        ),
        Space::Indexed { base, table } => {
            let n = base.components();
            let entries = table.len() / n.max(1);
            let mut expanded = Vec::with_capacity(samples.len() * n);
            for index in samples {
                let i = usize::from(*index).min(entries.saturating_sub(1));
                expanded.extend_from_slice(&table[i * n..i * n + n]);
            }
            to_samples(base, &expanded)
        }
    }
}

fn encode_png(
    width: u32,
    height: u32,
    rgb: bool,
    alpha: Option<&[u8]>,
    samples: &[u8],
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new(&mut out);
    match (rgb, alpha) {
        (false, None) => encoder.write_image(samples, width, height, ExtendedColorType::L8),
        (true, None) => encoder.write_image(samples, width, height, ExtendedColorType::Rgb8),
        (false, Some(a)) => {
            let la: Vec<u8> = samples.iter().zip(a).flat_map(|(g, a)| [*g, *a]).collect();
            encoder.write_image(&la, width, height, ExtendedColorType::La8)
        }
        (true, Some(a)) => {
            let rgba: Vec<u8> = samples
                .as_chunks::<3>()
                .0
                .iter()
                .zip(a)
                .flat_map(|(p, a)| [p[0], p[1], p[2], *a])
                .collect();
            encoder.write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        }
    }
    .ok()?;
    Some(out)
}

/// What the extractor made of one picture.
enum Outcome {
    Picture(ExtractedKind, Vec<u8>),
    Unsupported,
}

/// The soft mask of a picture as 8-bit alpha, when it matches the picture's size.
fn alpha_of(doc: &Document, dict: &Dictionary, width: u32, height: u32) -> Option<Vec<u8>> {
    let mask = resolve(doc, dict.get(b"SMask").ok()?).as_stream().ok()?;
    if int(doc, &mask.dict, b"Width")? != i64::from(width)
        || int(doc, &mask.dict, b"Height")? != i64::from(height)
    {
        return None;
    }
    let bits = u8::try_from(int(doc, &mask.dict, b"BitsPerComponent").unwrap_or(8)).ok()?;
    let data = mask.decompressed_content().ok()?;
    let alpha = unpack(&data, width as usize, 1, bits, true);
    (alpha.len() == width as usize * height as usize).then_some(alpha)
}

fn invert_wanted(doc: &Document, dict: &Dictionary) -> bool {
    matches!(
        dict.get(b"Decode").ok().map(|o| resolve(doc, o)),
        Some(Object::Array(items))
            if items.len() == 2 && items[0].as_float().is_ok_and(|v| v > 0.5)
    )
}

fn convert(doc: &Document, stream: &Stream, keep_original: bool) -> Outcome {
    let dict = &stream.dict;
    let names = filters(doc, dict);
    let (Some(width), Some(height)) = (int(doc, dict, b"Width"), int(doc, dict, b"Height")) else {
        return Outcome::Unsupported;
    };
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Outcome::Unsupported;
    };
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Outcome::Unsupported;
    }
    match names.last().map(Vec::as_slice) {
        Some(b"DCTDecode" | b"DCT") if names.len() == 1 => {
            if keep_original {
                return Outcome::Picture(ExtractedKind::Jpeg, stream.content.clone());
            }
            let Ok(decoded) =
                image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg)
            else {
                return Outcome::Unsupported;
            };
            let (w, h) = (decoded.width(), decoded.height());
            let png = if decoded.color().has_color() {
                encode_png(w, h, true, None, decoded.to_rgb8().as_raw())
            } else {
                encode_png(w, h, false, None, decoded.to_luma8().as_raw())
            };
            return png.map_or(Outcome::Unsupported, |p| {
                Outcome::Picture(ExtractedKind::Png, p)
            });
        }
        Some(b"JPXDecode") if names.len() == 1 && keep_original => {
            return Outcome::Picture(ExtractedKind::Jpeg2000, stream.content.clone());
        }
        Some(
            b"JPXDecode" | b"DCTDecode" | b"DCT" | b"CCITTFaxDecode" | b"CCF" | b"JBIG2Decode",
        ) => {
            return Outcome::Unsupported;
        }
        _ => {}
    }
    let Ok(data) = (if names.is_empty() {
        Ok(stream.content.clone())
    } else {
        stream.decompressed_content()
    }) else {
        return Outcome::Unsupported;
    };

    let is_mask = dict
        .get(b"ImageMask")
        .ok()
        .map(|o| resolve(doc, o))
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    let space = if is_mask {
        Some(Space::Gray)
    } else {
        dict.get(b"ColorSpace")
            .ok()
            .and_then(|o| space_of(doc, o, 0))
    };
    let Some(space) = space else {
        return Outcome::Unsupported;
    };
    let bits = if is_mask {
        1
    } else {
        match int(doc, dict, b"BitsPerComponent").and_then(|b| u8::try_from(b).ok()) {
            Some(b @ (1 | 2 | 4 | 8 | 16)) => b,
            _ => return Outcome::Unsupported,
        }
    };
    let indexed = matches!(space, Space::Indexed { .. });
    let mut samples = unpack(&data, width as usize, space.components(), bits, !indexed);
    if samples.len() != width as usize * height as usize * space.components() {
        return Outcome::Unsupported;
    }
    if (is_mask || matches!(space, Space::Gray)) && invert_wanted(doc, dict) {
        for s in &mut samples {
            *s = 255 - *s;
        }
    }
    // A stencil paints where the sample is 0, so it comes out black on white.
    let (rgb, pixels) = to_samples(&space, &samples);
    let alpha = alpha_of(doc, dict, width, height);
    encode_png(width, height, rgb, alpha.as_deref(), &pixels).map_or(Outcome::Unsupported, |p| {
        Outcome::Picture(ExtractedKind::Png, p)
    })
}

/// The pixels of a picture as 8-bit gray or RGB samples, row by row from the top.
pub(crate) struct Pixels {
    pub width: u32,
    pub height: u32,
    /// Three samples to a pixel; otherwise one.
    pub rgb: bool,
    pub samples: Vec<u8>,
}

/// Decodes a picture for an edit that changes its pixels. Stencil masks, scans stored as CCITT or
/// JBIG2 and JPEG 2000 are not decoded.
pub(crate) fn decode_pixels(doc: &Document, stream: &Stream) -> Option<Pixels> {
    let dict = &stream.dict;
    let names = filters(doc, dict);
    let width = u32::try_from(int(doc, dict, b"Width")?).ok()?;
    let height = u32::try_from(int(doc, dict, b"Height")?).ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let is_mask = dict
        .get(b"ImageMask")
        .ok()
        .map(|o| resolve(doc, o))
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    if is_mask {
        return None;
    }
    match names.last().map(Vec::as_slice) {
        Some(b"DCTDecode" | b"DCT") if names.len() == 1 => {
            let decoded =
                image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg).ok()?;
            let (w, h) = (decoded.width(), decoded.height());
            return Some(if decoded.color().has_color() {
                Pixels {
                    width: w,
                    height: h,
                    rgb: true,
                    samples: decoded.to_rgb8().into_raw(),
                }
            } else {
                Pixels {
                    width: w,
                    height: h,
                    rgb: false,
                    samples: decoded.to_luma8().into_raw(),
                }
            });
        }
        Some(
            b"JPXDecode" | b"DCTDecode" | b"DCT" | b"CCITTFaxDecode" | b"CCF" | b"JBIG2Decode",
        ) => return None,
        _ => {}
    }
    let data = if names.is_empty() {
        stream.content.clone()
    } else {
        stream.decompressed_content().ok()?
    };
    let space = space_of(doc, dict.get(b"ColorSpace").ok()?, 0)?;
    let bits = match int(doc, dict, b"BitsPerComponent").and_then(|b| u8::try_from(b).ok())? {
        b @ (1 | 2 | 4 | 8 | 16) => b,
        _ => return None,
    };
    let indexed = matches!(space, Space::Indexed { .. });
    let mut samples = unpack(&data, width as usize, space.components(), bits, !indexed);
    if samples.len() != width as usize * height as usize * space.components() {
        return None;
    }
    if matches!(space, Space::Gray) && invert_wanted(doc, dict) {
        for s in &mut samples {
            *s = 255 - *s;
        }
    }
    let (rgb, samples) = to_samples(&space, &samples);
    Some(Pixels {
        width,
        height,
        rgb,
        samples,
    })
}

/// The picture XObjects of a resource dictionary, with the forms to search further.
fn collect(
    doc: &Document,
    resources: &Dictionary,
    depth: usize,
    seen_forms: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) {
    let Some(Object::Dictionary(xobjects)) = resources
        .get(b"XObject")
        .ok()
        .map(|o| resolve(doc, o).clone())
    else {
        return;
    };
    for (_, entry) in xobjects.iter() {
        let Object::Reference(id) = entry else {
            continue;
        };
        let Ok(stream) = doc.get_object(*id).and_then(Object::as_stream) else {
            continue;
        };
        match stream.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Image") => out.push(*id),
            Ok(b"Form") if depth < MAX_FORM_DEPTH && seen_forms.insert(*id) => {
                if let Some(Object::Dictionary(inner)) = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .map(|o| resolve(doc, o).clone())
                {
                    collect(doc, &inner, depth + 1, seen_forms, out);
                }
            }
            _ => {}
        }
    }
}

/// Calls `sink` for every picture of at least `min` pixels, in page order. A picture used on
/// several pages is written once, at the first page that shows it.
pub(crate) fn extract(
    doc: &Document,
    keep_original: bool,
    min: (u32, u32),
    sink: &mut dyn FnMut(ExtractedImage) -> Result<(), AppError>,
) -> Result<ExtractReport, AppError> {
    let mut report = ExtractReport::default();
    let mut done: HashSet<ObjectId> = HashSet::new();
    for (page_number, page_id) in doc.get_pages() {
        let resources = effective_resources(doc, page_id);
        let mut ids = Vec::new();
        collect(doc, &resources, 0, &mut HashSet::new(), &mut ids);
        let mut number = 0u32;
        for id in ids {
            if !done.insert(id) {
                continue;
            }
            number += 1;
            report.found += 1;
            let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
                report.unsupported += 1;
                continue;
            };
            let dim = |key: &[u8]| int(doc, &stream.dict, key).and_then(|v| u32::try_from(v).ok());
            let (width, height) = (dim(b"Width").unwrap_or(0), dim(b"Height").unwrap_or(0));
            if width < min.0 || height < min.1 {
                report.too_small += 1;
                continue;
            }
            match convert(doc, stream, keep_original) {
                Outcome::Picture(kind, bytes) => sink(ExtractedImage {
                    page: PageIndex::new(page_number.saturating_sub(1)),
                    number,
                    width,
                    height,
                    kind,
                    bytes,
                })?,
                Outcome::Unsupported => report.unsupported += 1,
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgb, RgbImage};
    use lopdf::dictionary;

    fn deflate(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn image_stream(
        width: i64,
        height: i64,
        space: Object,
        bits: i64,
        filter: Option<&str>,
        data: Vec<u8>,
    ) -> Stream {
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width,
            "Height" => height,
            "ColorSpace" => space,
            "BitsPerComponent" => bits,
        };
        if let Some(f) = filter {
            dict.set("Filter", f);
        }
        let mut stream = Stream::new(dict, data);
        stream.allows_compression = false;
        stream
    }

    /// A document with one page per entry; each page shows the pictures of its entry.
    fn document(pages: Vec<Vec<(&str, Stream)>>) -> Document {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for pictures in pages {
            let mut xobjects = Dictionary::new();
            for (name, stream) in pictures {
                let id = doc.add_object(stream);
                xobjects.set(name, id);
            }
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
                "Resources" => dictionary! { "XObject" => xobjects },
            });
            kids.push(Object::from(page));
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc
    }

    fn run(doc: &Document, keep: bool, min: (u32, u32)) -> (Vec<ExtractedImage>, ExtractReport) {
        let mut found = Vec::new();
        let report = extract(doc, keep, min, &mut |i| {
            found.push(i);
            Ok(())
        })
        .unwrap();
        (found, report)
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = RgbImage::from_fn(width, height, |x, y| {
            Rgb([(x * 3) as u8, (y * 4) as u8, 80])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode(img.as_raw(), width, height, ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    #[test]
    fn jpegs_come_out_exactly_as_stored_and_raw_pictures_become_png() {
        let bytes = jpeg(80, 60);
        let rgb: Vec<u8> = (0..60 * 60 * 3).map(|i| (i % 251) as u8).collect();
        let doc = document(vec![vec![
            (
                "A",
                image_stream(
                    80,
                    60,
                    "DeviceRGB".into(),
                    8,
                    Some("DCTDecode"),
                    bytes.clone(),
                ),
            ),
            (
                "B",
                image_stream(
                    60,
                    60,
                    "DeviceRGB".into(),
                    8,
                    Some("FlateDecode"),
                    deflate(&rgb),
                ),
            ),
        ]]);
        let (found, report) = run(&doc, true, (50, 50));
        assert_eq!(report.found, 2);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, ExtractedKind::Jpeg);
        assert_eq!(found[0].bytes, bytes);
        assert_eq!(found[1].kind, ExtractedKind::Png);
        let png = image::load_from_memory(&found[1].bytes).unwrap().to_rgb8();
        assert_eq!(png.as_raw(), &rgb);
        assert_eq!((found[0].page.get(), found[0].number), (0, 1));
        assert_eq!(found[1].number, 2);
    }

    #[test]
    fn keeping_the_format_off_writes_png_for_everything() {
        let doc = document(vec![vec![(
            "A",
            image_stream(
                80,
                60,
                "DeviceRGB".into(),
                8,
                Some("DCTDecode"),
                jpeg(80, 60),
            ),
        )]]);
        let (found, _) = run(&doc, false, (50, 50));
        assert_eq!(found[0].kind, ExtractedKind::Png);
        assert_eq!(
            image::load_from_memory(&found[0].bytes).unwrap().width(),
            80
        );
    }

    #[test]
    fn small_pictures_unsupported_ones_and_repeats_are_counted_not_written() {
        let gray = GrayImage::from_fn(60, 60, |x, _| Luma([x as u8]));
        let shared = image_stream(
            60,
            60,
            "DeviceGray".into(),
            8,
            Some("FlateDecode"),
            deflate(gray.as_raw()),
        );
        let mut doc = document(vec![
            vec![
                (
                    "Small",
                    image_stream(10, 10, "DeviceGray".into(), 8, None, vec![0; 100]),
                ),
                (
                    "Fax",
                    image_stream(
                        60,
                        60,
                        "DeviceGray".into(),
                        1,
                        Some("CCITTFaxDecode"),
                        vec![0; 10],
                    ),
                ),
                ("Same", shared),
            ],
            vec![],
        ]);
        // Page two shows the picture of page one again.
        let first_page = doc.get_pages()[&1];
        let same = {
            let resources = effective_resources(&doc, first_page);
            let Object::Dictionary(x) = resources.get(b"XObject").unwrap().clone() else {
                panic!("no xobjects")
            };
            x.get(b"Same").unwrap().clone()
        };
        let second_page = doc.get_pages()[&2];
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(second_page) {
            page.set(
                "Resources",
                dictionary! { "XObject" => dictionary! { "Again" => same } },
            );
        }
        let (found, report) = run(&doc, true, (50, 50));
        assert_eq!(found.len(), 1);
        assert_eq!(report.too_small, 1);
        assert_eq!(report.unsupported, 1);
        assert_eq!(report.found, 3, "the repeat is not counted twice");
    }

    #[test]
    fn indexed_cmyk_masks_and_soft_masks_are_decoded() {
        // Indexed, 2 bits: palette red, green, blue, white.
        let table = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let space = Object::Array(vec![
            "Indexed".into(),
            "DeviceRGB".into(),
            3.into(),
            Object::string_literal(table),
        ]);
        let indexed = image_stream(4, 1, space, 2, None, vec![0b0001_1011]);
        let cmyk = image_stream(1, 1, "DeviceCMYK".into(), 8, None, vec![0, 0, 0, 255]);
        let mask = image_stream(2, 1, "DeviceGray".into(), 1, None, vec![0b0100_0000]);
        let mut stencil = mask.clone();
        stencil.dict.remove(b"ColorSpace");
        stencil.dict.set("ImageMask", true);
        let mut doc = document(vec![vec![("I", indexed), ("C", cmyk), ("M", stencil)]]);
        let alpha_id = doc.add_object(image_stream(
            2,
            1,
            "DeviceGray".into(),
            8,
            None,
            vec![255, 64],
        ));
        let mut with_alpha = image_stream(
            2,
            1,
            "DeviceRGB".into(),
            8,
            None,
            vec![9, 9, 9, 200, 200, 200],
        );
        with_alpha.dict.set("SMask", alpha_id);
        let id = doc.add_object(with_alpha);
        let page = doc.get_pages()[&1];
        if let Ok(Object::Dictionary(p)) = doc.get_object_mut(page)
            && let Ok(Object::Dictionary(r)) = p.get_mut(b"Resources")
            && let Ok(Object::Dictionary(x)) = r.get_mut(b"XObject")
        {
            x.set("S", id);
        }
        let (found, report) = run(&doc, true, (1, 1));
        assert_eq!(report.unsupported, 0, "{report:?}");
        assert_eq!(found.len(), 4);
        let decode = |i: usize| image::load_from_memory(&found[i].bytes).unwrap();
        let idx = decode(0).to_rgb8();
        assert_eq!(idx.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(idx.get_pixel(1, 0).0, [0, 255, 0]);
        assert_eq!(idx.get_pixel(2, 0).0, [0, 0, 255]);
        assert_eq!(idx.get_pixel(3, 0).0, [255, 255, 255]);
        assert_eq!(
            decode(1).to_rgb8().get_pixel(0, 0).0,
            [0, 0, 0],
            "full black ink"
        );
        let stencil = decode(2).to_luma8();
        assert_eq!(
            (stencil.get_pixel(0, 0).0[0], stencil.get_pixel(1, 0).0[0]),
            (0, 255)
        );
        let soft = decode(3).to_rgba8();
        assert_eq!(soft.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(soft.get_pixel(1, 0).0, [200, 200, 200, 64]);
    }

    #[test]
    fn a_document_without_pictures_reports_none() {
        let doc = document(vec![vec![]]);
        let (found, report) = run(&doc, true, (50, 50));
        assert!(found.is_empty());
        assert_eq!(report, ExtractReport::default());
    }
}
