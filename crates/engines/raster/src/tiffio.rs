//! TIFF files: reading every frame into a PDF (row 68) and writing pages as a multi-page TIFF
//! with LZW, Deflate or CCITT Group 4 compression.
//!
//! The writer is small and handwritten because the pages must be appended one at a time, so a long
//! document never sits in memory, and because Group 4 is not offered by the TIFF crate's encoder.

use application::AppError;
use application::ports::{RenderedImage, TiffPages, TiffSpec};
use domain::ErrorCode;
use domain::tools::convert::{PictureColor, Placement, TiffCompression};
use fax::{Color as FaxColor, VecWriter, encoder::Encoder as FaxEncoder};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use lopdf::Document;
use std::fs::File;
use std::io::{BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use tiff::ColorType;
use tiff::decoder::ifd::Value;
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

use crate::imagepdf::PageBuilder;
use crate::{MAX_PIXELS, Prepared, flate, too_big};

/// Most frames one TIFF may hold, to bound the work a damaged file can ask for.
const MAX_FRAMES: u32 = 5000;
/// Resolution used when the file does not say, or says something implausible.
const FALLBACK_DPI: f64 = 150.0;
/// PDF viewers handle pages up to 200 inches.
const MAX_PAGE_POINTS: f32 = 14_400.0;

fn damaged() -> AppError {
    AppError::new(
        ErrorCode::InputNotPdf,
        "This TIFF file could not be read. It may be damaged or use a compression Quirewise does not support.",
    )
}

fn io(what: &str, e: &std::io::Error) -> AppError {
    AppError::from_io(what, e)
}

// ----- writing ------------------------------------------------------------------------------

const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;

/// One IFD entry: tag, type, count and either the value or the offset of the values.
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: u32,
}

impl Entry {
    fn short(tag: u16, value: u16) -> Self {
        Self {
            tag,
            kind: TYPE_SHORT,
            count: 1,
            value: u32::from(value),
        }
    }
    fn long(tag: u16, value: u32) -> Self {
        Self {
            tag,
            kind: TYPE_LONG,
            count: 1,
            value,
        }
    }
}

/// Pixels reduced to what one TIFF page stores.
enum Samples {
    /// 8-bit RGB, three samples per pixel.
    Rgb(Vec<u8>),
    /// 8-bit gray.
    Gray(Vec<u8>),
    /// 1-bit rows packed from the left, 1 is black.
    Bilevel(Vec<u8>),
}

/// The brightness at which black and white part, found with Otsu's method.
fn otsu(gray: &[u8]) -> u8 {
    let mut histogram = [0u64; 256];
    for g in gray {
        histogram[usize::from(*g)] += 1;
    }
    let total = gray.len() as f64;
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, n)| i as f64 * *n as f64)
        .sum();
    let (mut below, mut sum_below, mut best, mut threshold) = (0.0f64, 0.0f64, 0.0f64, 127u8);
    for (level, count) in histogram.iter().enumerate() {
        below += *count as f64;
        if below == 0.0 {
            continue;
        }
        let above = total - below;
        if above == 0.0 {
            break;
        }
        sum_below += level as f64 * *count as f64;
        let mean_below = sum_below / below;
        let mean_above = (sum - sum_below) / above;
        let between = below * above * (mean_below - mean_above).powi(2);
        if between > best {
            best = between;
            threshold = u8::try_from(level).unwrap_or(255);
        }
    }
    threshold
}

/// The brightness of every pixel, paper white where the page is see-through.
pub(crate) fn luma(image: &RenderedImage) -> Vec<u8> {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            let blend =
                |c: u8| (u32::from(c) * u32::from(px[3]) + 255 * (255 - u32::from(px[3]))) / 255;
            ((blend(px[0]) * 299 + blend(px[1]) * 587 + blend(px[2]) * 114) / 1000) as u8
        })
        .collect()
}

/// Pixels as black and white after the Otsu threshold: `true` is black.
pub(crate) fn black_white(image: &RenderedImage) -> Vec<bool> {
    let gray = luma(image);
    let threshold = otsu(&gray);
    gray.iter().map(|g| *g <= threshold).collect()
}

fn samples_for(image: &RenderedImage, color: PictureColor) -> Samples {
    match color {
        PictureColor::Color => {
            let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
            for px in image.rgba.as_chunks::<4>().0 {
                let a = u16::from(px[3]);
                for c in &px[..3] {
                    rgb.push(((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8);
                }
            }
            Samples::Rgb(rgb)
        }
        PictureColor::Gray => Samples::Gray(luma(image)),
        PictureColor::BlackWhite => {
            let black = black_white(image);
            let width = image.width as usize;
            let row_bytes = width.div_ceil(8);
            let mut packed = vec![0u8; row_bytes * image.height as usize];
            for (i, is_black) in black.iter().enumerate() {
                if *is_black {
                    let (y, x) = (i / width, i % width);
                    packed[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
                }
            }
            Samples::Bilevel(packed)
        }
    }
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .and_then(|()| encoder.finish())
        .map_err(|e| io("compress a TIFF page", &e))
}

fn lzw(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
        .encode(bytes)
        .map_err(|_| AppError::internal("compressing a TIFF page"))
}

fn group4(black: &[bool], width: u32) -> Result<Vec<u8>, AppError> {
    let width16 = u16::try_from(width).map_err(|_| too_big())?;
    let mut encoder = FaxEncoder::new(VecWriter::new());
    for row in black.chunks(width as usize) {
        let pels = row
            .iter()
            .map(|b| if *b { FaxColor::Black } else { FaxColor::White });
        encoder
            .encode_line(pels, width16)
            .map_err(|_| AppError::internal("compressing a TIFF page"))?;
    }
    let writer = encoder
        .finish()
        .map_err(|_| AppError::internal("compressing a TIFF page"))?;
    Ok(writer.finish())
}

/// A TIFF file written one page at a time.
pub(crate) struct TiffWriter {
    out: BufWriter<File>,
    spec: TiffSpec,
    position: u64,
    /// Where each IFD starts, to link them in order when the file is finished.
    ifds: Vec<(u64, u64)>,
}

impl TiffWriter {
    pub(crate) fn create(dest: &Path, spec: &TiffSpec) -> Result<Self, AppError> {
        let file = File::create(dest).map_err(|e| io("write the TIFF file", &e))?;
        let mut writer = Self {
            out: BufWriter::new(file),
            spec: *spec,
            position: 0,
            ifds: Vec::new(),
        };
        // Byte order, the magic number, and a placeholder for the first IFD.
        writer.put(&[b'I', b'I', 42, 0, 0, 0, 0, 0])?;
        Ok(writer)
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), AppError> {
        self.out
            .write_all(bytes)
            .map_err(|e| io("write the TIFF file", &e))?;
        self.position += bytes.len() as u64;
        Ok(())
    }

    fn align(&mut self) -> Result<(), AppError> {
        if self.position % 2 == 1 {
            self.put(&[0])?;
        }
        Ok(())
    }

    fn page(&mut self, image: &RenderedImage) -> Result<(), AppError> {
        let (w, h) = (image.width, image.height);
        let color = if self.spec.compression == TiffCompression::CcittG4 {
            PictureColor::BlackWhite
        } else {
            self.spec.color
        };
        let samples = samples_for(image, color);
        let (data, compression, photometric, per_pixel, bits): (Vec<u8>, u16, u16, u16, u16) =
            match (&samples, self.spec.compression) {
                (Samples::Bilevel(_), TiffCompression::CcittG4) => {
                    let black = black_white(image);
                    (group4(&black, w)?, 4, 0, 1, 1)
                }
                (Samples::Bilevel(packed), c) => {
                    // 1 is black in `packed`; the file says "0 is white", so it stays as it is.
                    let (code, bytes) = match c {
                        TiffCompression::Deflate => (8, deflate(packed)?),
                        _ => (5, lzw(packed)?),
                    };
                    (bytes, code, 0, 1, 1)
                }
                (Samples::Gray(raw), c) => {
                    let (code, bytes) = match c {
                        TiffCompression::Deflate => (8, deflate(raw)?),
                        _ => (5, lzw(raw)?),
                    };
                    (bytes, code, 1, 1, 8)
                }
                (Samples::Rgb(raw), c) => {
                    let (code, bytes) = match c {
                        TiffCompression::Deflate => (8, deflate(raw)?),
                        _ => (5, lzw(raw)?),
                    };
                    (bytes, code, 2, 3, 8)
                }
            };

        // Strip data, then the values that do not fit an entry, then the IFD.
        self.align()?;
        let strip_offset = self.position;
        self.put(&data)?;
        self.align()?;
        let bits_offset = self.position;
        if per_pixel == 3 {
            for _ in 0..3 {
                self.put(&8u16.to_le_bytes())?;
            }
        }
        let dpi = self.spec.dpi.get();
        let resolution_offset = self.position;
        for _ in 0..2 {
            self.put(&dpi.to_le_bytes())?;
            self.put(&1u32.to_le_bytes())?;
        }

        let mut entries = vec![
            Entry::long(256, w),
            Entry::long(257, h),
            if per_pixel == 3 {
                Entry {
                    tag: 258,
                    kind: TYPE_SHORT,
                    count: 3,
                    value: u32::try_from(bits_offset).map_err(|_| too_big())?,
                }
            } else {
                Entry::short(258, bits)
            },
            Entry::short(259, compression),
            Entry::short(262, photometric),
            Entry::long(273, u32::try_from(strip_offset).map_err(|_| too_big())?),
            Entry::short(277, per_pixel),
            Entry::long(278, h),
            Entry::long(279, u32::try_from(data.len()).map_err(|_| too_big())?),
            Entry {
                tag: 282,
                kind: TYPE_RATIONAL,
                count: 1,
                value: u32::try_from(resolution_offset).map_err(|_| too_big())?,
            },
            Entry {
                tag: 283,
                kind: TYPE_RATIONAL,
                count: 1,
                value: u32::try_from(resolution_offset + 8).map_err(|_| too_big())?,
            },
            Entry::short(296, 2),
        ];
        entries.sort_by_key(|e| e.tag);
        let ifd_offset = self.position;
        self.put(&u16::try_from(entries.len()).unwrap_or(0).to_le_bytes())?;
        for e in &entries {
            self.put(&e.tag.to_le_bytes())?;
            self.put(&e.kind.to_le_bytes())?;
            self.put(&e.count.to_le_bytes())?;
            self.put(&e.value.to_le_bytes())?;
        }
        let next_field = self.position;
        self.put(&0u32.to_le_bytes())?;
        self.ifds.push((ifd_offset, next_field));
        Ok(())
    }

    fn close(mut self) -> Result<(), AppError> {
        self.out
            .flush()
            .map_err(|e| io("write the TIFF file", &e))?;
        let mut file = self
            .out
            .into_inner()
            .map_err(|e| io("write the TIFF file", e.error()))?;
        let offset = |value: u64| u32::try_from(value).map_err(|_| too_big());
        let mut patches: Vec<(u64, u32)> = Vec::new();
        if let Some((first, _)) = self.ifds.first() {
            patches.push((4, offset(*first)?));
        }
        for pair in self.ifds.windows(2) {
            patches.push((pair[0].1, offset(pair[1].0)?));
        }
        for (at, value) in patches {
            file.seek(SeekFrom::Start(at))
                .and_then(|_| file.write_all(&value.to_le_bytes()))
                .map_err(|e| io("write the TIFF file", &e))?;
        }
        file.sync_all().map_err(|e| io("write the TIFF file", &e))
    }
}

impl TiffPages for TiffWriter {
    fn add(&mut self, image: &RenderedImage) -> Result<(), AppError> {
        self.page(image)
    }

    fn finish(self: Box<Self>) -> Result<(), AppError> {
        self.close()
    }
}

/// Number of frames in a TIFF file, found by walking its IFD chain, and whether every frame
/// decodes its header.
pub(crate) fn count_frames(path: &Path) -> Result<u32, AppError> {
    let file = File::open(path).map_err(|e| io("read the TIFF file", &e))?;
    let mut decoder = Decoder::new(BufReader::new(file)).map_err(|_| damaged())?;
    let mut frames = 1;
    while decoder.more_images() {
        decoder.next_image().map_err(|_| damaged())?;
        frames += 1;
        if frames > MAX_FRAMES {
            return Err(damaged());
        }
    }
    Ok(frames)
}

// ----- reading ------------------------------------------------------------------------------

/// A tag stored as a rational or a real number.
fn rational(decoder: &mut Decoder<BufReader<File>>, tag: Tag) -> Option<f64> {
    match decoder.find_tag(tag).ok()?? {
        Value::Rational(n, d) if d != 0 => Some(f64::from(n) / f64::from(d)),
        Value::Double(v) => Some(v),
        Value::Float(v) => Some(f64::from(v)),
        _ => None,
    }
}

fn dpi_of(decoder: &mut Decoder<BufReader<File>>) -> f64 {
    let x = rational(decoder, Tag::XResolution);
    let unit = decoder
        .find_tag_unsigned::<u32>(Tag::ResolutionUnit)
        .ok()
        .flatten()
        .unwrap_or(2);
    let dpi = match (x, unit) {
        (Some(x), 2) => x,
        (Some(x), 3) => x * 2.54,
        _ => FALLBACK_DPI,
    };
    if (10.0..=2400.0).contains(&dpi) {
        dpi
    } else {
        FALLBACK_DPI
    }
}

fn high_bytes(values: &[u16]) -> Vec<u8> {
    values.iter().map(|v| (v >> 8) as u8).collect()
}

/// One frame as a PDF image.
fn prepare(
    decoder: &mut Decoder<BufReader<File>>,
    width: u32,
    height: u32,
) -> Result<Prepared, AppError> {
    let color = decoder.colortype().map_err(|_| damaged())?;
    let image = decoder.read_image().map_err(|_| damaged())?;
    let bytes = match &image {
        DecodingResult::U8(v) => v.clone(),
        DecodingResult::U16(v) => high_bytes(v),
        _ => return Err(damaged()),
    };
    let pixels = width as usize * height as usize;
    let base = |color_space: &'static str, data: Vec<u8>, alpha: Option<Vec<u8>>, bits: u8| {
        Ok::<_, AppError>(Prepared {
            width,
            height,
            color_space,
            filter: "FlateDecode",
            data: flate(&data)?,
            alpha: alpha.map(|a| flate(&a)).transpose()?,
            bits,
            turns: 0,
            decode_parms: None,
        })
    };
    let split = |samples: usize, bytes: &[u8]| -> (Vec<u8>, Vec<u8>) {
        let (mut color, mut alpha) = (Vec::new(), Vec::new());
        for px in bytes.chunks_exact(samples) {
            color.extend_from_slice(&px[..samples - 1]);
            alpha.push(px[samples - 1]);
        }
        (color, alpha)
    };
    match color {
        ColorType::Gray(1) if bytes.len() >= height as usize * (width as usize).div_ceil(8) => {
            base("DeviceGray", bytes, None, 1)
        }
        ColorType::Gray(8 | 16) if bytes.len() >= pixels => base("DeviceGray", bytes, None, 8),
        ColorType::GrayA(8 | 16) if bytes.len() >= pixels * 2 => {
            let (gray, alpha) = split(2, &bytes);
            base("DeviceGray", gray, Some(alpha), 8)
        }
        ColorType::RGB(8 | 16) | ColorType::YCbCr(8) if bytes.len() >= pixels * 3 => {
            base("DeviceRGB", bytes, None, 8)
        }
        ColorType::RGBA(8 | 16) if bytes.len() >= pixels * 4 => {
            let (rgb, alpha) = split(4, &bytes);
            base("DeviceRGB", rgb, Some(alpha), 8)
        }
        ColorType::CMYK(8 | 16) if bytes.len() >= pixels * 4 => base("DeviceCMYK", bytes, None, 8),
        ColorType::Palette(8) if bytes.len() >= pixels => {
            let map = decoder
                .get_tag_u16_vec(Tag::ColorMap)
                .map_err(|_| damaged())?;
            let entries = map.len() / 3;
            if entries == 0 {
                return Err(damaged());
            }
            let mut rgb = Vec::with_capacity(pixels * 3);
            for index in &bytes[..pixels] {
                let i = usize::from(*index).min(entries - 1);
                rgb.push((map[i] >> 8) as u8);
                rgb.push((map[entries + i] >> 8) as u8);
                rgb.push((map[2 * entries + i] >> 8) as u8);
            }
            base("DeviceRGB", rgb, None, 8)
        }
        _ => Err(AppError::new(
            ErrorCode::InputNotPdf,
            "This TIFF uses a pixel format Quirewise does not support yet.",
        )),
    }
}

/// Writes every frame of a TIFF file as a PDF page and returns how many it wrote.
pub(crate) fn tiff_to_pdf(tiff: &Path, dest: &Path) -> Result<u32, AppError> {
    let file = File::open(tiff).map_err(|e| io("read the TIFF file", &e))?;
    let mut decoder = Decoder::new(BufReader::new(file))
        .map_err(|_| damaged())?
        .with_limits(Limits::default());
    let mut builder = PageBuilder::new();
    let mut frames = 0u32;
    loop {
        let (width, height) = decoder.dimensions().map_err(|_| damaged())?;
        if width == 0 || height == 0 {
            return Err(damaged());
        }
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(too_big());
        }
        let dpi = dpi_of(&mut decoder);
        let prepared = prepare(&mut decoder, width, height)?;
        let k = (72.0 / dpi) as f32;
        let (mut w, mut h) = (width as f32 * k, height as f32 * k);
        let largest = w.max(h);
        if largest > MAX_PAGE_POINTS {
            let shrink = MAX_PAGE_POINTS / largest;
            (w, h) = (w * shrink, h * shrink);
        }
        builder.add_page(
            prepared,
            &Placement {
                page_w: w,
                page_h: h,
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        );
        frames += 1;
        if !decoder.more_images() {
            break;
        }
        if frames >= MAX_FRAMES {
            return Err(damaged());
        }
        decoder.next_image().map_err(|_| damaged())?;
    }
    let mut doc: Document = builder.finish();
    doc.save(dest).map_err(|e| io("write the PDF", &e))?;
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::PictureWriter;
    use domain::Dpi;

    /// A page with a dark square on white, big enough for every encoder.
    fn page(w: u32, h: u32, shade: u8) -> RenderedImage {
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        for y in h / 4..h / 2 {
            for x in w / 4..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[shade, shade / 2, 0, 255]);
            }
        }
        RenderedImage {
            width: w,
            height: h,
            rgba,
        }
    }

    fn write(
        dir: &Path,
        name: &str,
        spec: TiffSpec,
        pages: &[RenderedImage],
    ) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut tiff = crate::RasterConverter.start_tiff(&path, &spec).unwrap();
        for p in pages {
            tiff.add(p).unwrap();
        }
        tiff.finish().unwrap();
        path
    }

    fn spec(compression: TiffCompression, color: PictureColor) -> TiffSpec {
        TiffSpec {
            compression,
            color,
            dpi: Dpi::new(200).unwrap(),
        }
    }

    #[test]
    fn every_compression_and_color_round_trips_through_the_reader() {
        let dir = tempfile::tempdir().unwrap();
        let pages = [page(80, 60, 20), page(80, 60, 120), page(80, 60, 200)];
        for (compression, color, expected_bits) in [
            (TiffCompression::Lzw, PictureColor::Color, 8),
            (TiffCompression::Deflate, PictureColor::Color, 8),
            (TiffCompression::Lzw, PictureColor::Gray, 8),
            (TiffCompression::Deflate, PictureColor::Gray, 8),
            (TiffCompression::Lzw, PictureColor::BlackWhite, 1),
            (TiffCompression::Deflate, PictureColor::BlackWhite, 1),
            (TiffCompression::CcittG4, PictureColor::Color, 1),
        ] {
            let path = write(dir.path(), "a.tif", spec(compression, color), &pages);
            assert_eq!(count_frames(&path).unwrap(), 3, "{compression:?} {color:?}");
            let pdf = dir.path().join("a.pdf");
            assert_eq!(tiff_to_pdf(&path, &pdf).unwrap(), 3);
            let doc = Document::load(&pdf).unwrap();
            assert_eq!(doc.get_pages().len(), 3);
            let (_, first) = doc.get_pages().into_iter().next().unwrap();
            let media = doc.get_dictionary(first).unwrap().get(b"MediaBox").unwrap();
            let n: Vec<f32> = media
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_float().unwrap())
                .collect();
            // 80 x 60 pixels at 200 dpi.
            assert!(
                (n[2] - 28.8).abs() < 0.1 && (n[3] - 21.6).abs() < 0.1,
                "{n:?}"
            );
            let bits = doc
                .objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .find(|s| s.dict.has(b"BitsPerComponent") && !s.dict.has(b"SMask"))
                .and_then(|s| {
                    s.dict
                        .get(b"BitsPerComponent")
                        .ok()
                        .and_then(|b| b.as_i64().ok())
                })
                .unwrap();
            assert_eq!(bits, expected_bits, "{compression:?} {color:?}");
            std::fs::remove_file(&path).unwrap();
        }
    }

    #[test]
    fn group4_keeps_the_black_pixels_where_they_were() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "g4.tif",
            spec(TiffCompression::CcittG4, PictureColor::BlackWhite),
            &[page(64, 48, 0)],
        );
        let mut decoder = Decoder::new(BufReader::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(decoder.colortype().unwrap(), ColorType::Gray(1));
        let DecodingResult::U8(bits) = decoder.read_image().unwrap() else {
            panic!("expected bytes");
        };
        // After the reader's white-is-zero handling a set bit is white.
        let pixel = |x: usize, y: usize| bits[y * 8 + x / 8] & (0x80 >> (x % 8)) == 0;
        assert!(pixel(20, 18), "inside the square is black");
        assert!(!pixel(2, 2), "outside is white");
    }

    #[test]
    fn the_resolution_sets_the_page_size_and_a_missing_one_means_150_dpi() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "r.tif",
            TiffSpec {
                compression: TiffCompression::Lzw,
                color: PictureColor::Gray,
                dpi: Dpi::new(300).unwrap(),
            },
            &[page(300, 300, 50)],
        );
        let pdf = dir.path().join("r.pdf");
        tiff_to_pdf(&path, &pdf).unwrap();
        let doc = Document::load(&pdf).unwrap();
        let (_, id) = doc.get_pages().into_iter().next().unwrap();
        let media = doc.get_dictionary(id).unwrap().get(b"MediaBox").unwrap();
        let w = media.as_array().unwrap()[2].as_float().unwrap();
        assert!(
            (w - 72.0).abs() < 0.1,
            "300 px at 300 dpi is one inch, got {w}"
        );
    }

    #[test]
    fn files_that_are_not_tiffs_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no.tif");
        std::fs::write(&path, b"this is not a tiff file at all").unwrap();
        let err = tiff_to_pdf(&path, &dir.path().join("o.pdf")).unwrap_err();
        assert_eq!(err.code, ErrorCode::InputNotPdf);
        assert!(count_frames(&path).is_err());
    }

    #[test]
    fn otsu_splits_dark_ink_from_light_paper() {
        let gray: Vec<u8> = (0..100).map(|i| if i < 20 { 30 } else { 230 }).collect();
        let t = otsu(&gray);
        assert!((30..230).contains(&t));
    }
}
