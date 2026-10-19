//! Pictures on PDF pages: one page per picture, laid out by the Images to PDF options.

use application::AppError;
use domain::ErrorCode;
use domain::tools::convert::{ImagesToPdfOptions, Placement};
use image::metadata::Orientation;
use image::{ImageDecoder, ImageFormat, ImageReader};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use std::fs;
use std::io::Cursor;
use std::path::Path;

use crate::{MAX_PIXELS, Prepared, prepare_decoded, prepare_jpeg, too_big, unreadable};

/// Turns of the stored pixels needed to show a picture upright.
fn quarter_turns(orientation: Orientation) -> u16 {
    match orientation {
        Orientation::Rotate90 => 90,
        Orientation::Rotate180 => 180,
        Orientation::Rotate270 => 270,
        _ => 0,
    }
}

/// The orientation a JPEG asks for in its EXIF block.
fn jpeg_orientation(bytes: &[u8]) -> Orientation {
    image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
        .ok()
        .and_then(|mut decoder| decoder.orientation().ok())
        .unwrap_or(Orientation::NoTransforms)
}

/// Reads a picture file and prepares it for a PDF image object.
///
/// `scratch` holds the temporary PNG of a HEIC picture.
pub(crate) fn load_prepared(path: &Path, scratch: &Path) -> Result<Prepared, AppError> {
    let mut bytes = fs::read(path).map_err(|e| AppError::from_io("read the image", &e))?;
    let converted;
    if crate::heic::is_heic(&bytes) {
        converted = crate::heic::to_png(path, scratch)?;
        bytes = converted;
    }
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| unreadable())?;
    let format = reader.format().ok_or_else(unreadable)?;
    let (width, height) = reader.into_dimensions().map_err(|_| unreadable())?;
    if width == 0 || height == 0 {
        return Err(unreadable());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(too_big());
    }
    let decode = |bytes: &[u8]| image::load_from_memory(bytes).map_err(|_| unreadable());
    match format {
        ImageFormat::Jpeg => {
            if let Some(mut prepared) = prepare_jpeg(&bytes) {
                prepared.turns = quarter_turns(jpeg_orientation(&bytes));
                return Ok(prepared);
            }
            // CMYK and other JPEG kinds are decoded and stored losslessly instead.
            prepare_decoded(decode(&bytes)?)
        }
        _ => prepare_decoded(decode(&bytes)?),
    }
}

/// The matrix that puts a picture in its rectangle, turned the way its pixels need.
fn matrix(p: &Placement, turns: u16) -> [f32; 6] {
    let (w, h, x, y) = (p.w, p.h, p.x, p.y);
    match turns {
        90 => [0.0, -h, w, 0.0, x, y + h],
        180 => [-w, 0.0, 0.0, -h, x + w, y + h],
        270 => [0.0, h, -w, 0.0, x + w, y],
        _ => [w, 0.0, 0.0, h, x, y],
    }
}

/// A document that gains one page per picture.
pub(crate) struct PageBuilder {
    doc: Document,
    pages_id: ObjectId,
    kids: Vec<Object>,
}

impl PageBuilder {
    pub(crate) fn new() -> Self {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        Self {
            doc,
            pages_id,
            kids: Vec::new(),
        }
    }

    /// Adds a page of `placement.page_w` by `placement.page_h` with the picture on it.
    pub(crate) fn add_page(&mut self, prepared: Prepared, placement: &Placement) {
        let doc = &mut self.doc;
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(prepared.width),
            "Height" => i64::from(prepared.height),
            "ColorSpace" => prepared.color_space,
            "BitsPerComponent" => i64::from(prepared.bits),
            "Filter" => prepared.filter,
        };
        if let Some(alpha) = prepared.alpha {
            let mask = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => i64::from(prepared.width), "Height" => i64::from(prepared.height),
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8, "Filter" => "FlateDecode",
                },
                alpha,
            ));
            dict.set("SMask", mask);
        }
        let mut stream = Stream::new(dict, prepared.data);
        // The data is already compressed; stop lopdf from compressing it again on save.
        stream.allows_compression = false;
        let image_id = doc.add_object(stream);

        let m = matrix(placement, prepared.turns);
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!(
                "q {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} cm /Im0 Do Q",
                m[0], m[1], m[2], m[3], m[4], m[5]
            )
            .into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => self.pages_id,
            "MediaBox" => vec![0.into(), 0.into(), placement.page_w.into(), placement.page_h.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
            "Contents" => content,
        });
        self.kids.push(page.into());
    }

    /// Completes the page tree.
    pub(crate) fn finish(mut self) -> Document {
        let count = self.kids.len() as i64;
        self.doc.objects.insert(
            self.pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => self.kids, "Count" => count },
            ),
        );
        let catalog = self
            .doc
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => self.pages_id });
        self.doc.trailer.set("Root", catalog);
        self.doc
    }
}

/// Writes `images` as pages of one PDF at `dest`.
pub(crate) fn write_pdf(
    images: &[std::path::PathBuf],
    layout: &ImagesToPdfOptions,
    scratch: &Path,
    dest: &Path,
) -> Result<(), AppError> {
    if images.is_empty() {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            "Add at least one picture.",
        ));
    }
    let mut builder = PageBuilder::new();
    for path in images {
        let prepared = load_prepared(path, scratch)?;
        // The page shows the picture upright, so a picture turned a quarter swaps its sides.
        let (w, h) = if matches!(prepared.turns, 90 | 270) {
            (prepared.height, prepared.width)
        } else {
            (prepared.width, prepared.height)
        };
        let placement = layout.place(w, h)?;
        builder.add_page(prepared, &placement);
    }
    builder
        .finish()
        .save(dest)
        .map_err(|e| AppError::from_io("write the PDF", &e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use domain::tools::convert::{ImageMargin, ImageOrientation, ImagePageSize};
    use image::{ExtendedColorType, RgbImage};

    fn jpeg(width: u32, height: u32, orientation: Option<u16>) -> Vec<u8> {
        let img = RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 5) as u8, 90])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode(img.as_raw(), width, height, ExtendedColorType::Rgb8)
            .unwrap();
        let Some(value) = orientation else {
            return out;
        };
        // An EXIF block right after the start-of-image marker.
        let mut tiff = vec![b'M', b'M', 0, 42, 0, 0, 0, 8];
        tiff.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        tiff.extend_from_slice(&value.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut payload = vec![b'E', b'x', b'i', b'f', 0, 0];
        payload.extend_from_slice(&tiff);
        let mut segment = vec![0xFF, 0xE1];
        segment.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        segment.extend_from_slice(&payload);
        let mut bytes = out[..2].to_vec();
        bytes.extend_from_slice(&segment);
        bytes.extend_from_slice(&out[2..]);
        bytes
    }

    fn media_box(doc: &Document, page: ObjectId) -> Vec<f32> {
        doc.get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_float().unwrap())
            .collect()
    }

    fn content(doc: &Document, page: ObjectId) -> String {
        String::from_utf8(doc.get_page_content(page)).unwrap()
    }

    #[test]
    fn every_picture_gets_its_own_page_on_the_chosen_paper() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jpg");
        let b = dir.path().join("b.jpg");
        std::fs::write(&a, jpeg(200, 100, None)).unwrap();
        std::fs::write(&b, jpeg(100, 200, None)).unwrap();
        let layout = ImagesToPdfOptions {
            orientation: ImageOrientation::Auto,
            margin: ImageMargin::Small,
            page_size: ImagePageSize::A4,
            one_pdf: true,
        };
        let dest = dir.path().join("o.pdf");
        write_pdf(&[a, b], &layout, dir.path(), &dest).unwrap();
        let doc = Document::load(&dest).unwrap();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        assert_eq!(pages.len(), 2);
        let first = media_box(&doc, pages[0]);
        let second = media_box(&doc, pages[1]);
        assert!(first[2] > first[3], "wide picture, landscape page");
        assert!(second[2] < second[3], "tall picture, portrait page");
        assert!((first[2] - 841.89).abs() < 0.1, "{first:?}");
    }

    #[test]
    fn a_jpeg_that_asks_for_a_turn_is_shown_upright_without_re_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.jpg");
        let bytes = jpeg(200, 100, Some(6));
        std::fs::write(&path, &bytes).unwrap();
        let dest = dir.path().join("o.pdf");
        write_pdf(&[path], &ImagesToPdfOptions::default(), dir.path(), &dest).unwrap();
        let doc = Document::load(&dest).unwrap();
        let (_, page) = doc.get_pages().into_iter().next().unwrap();
        let m = media_box(&doc, page);
        // 200 x 100 pixels turned a quarter: a tall page of 100 x 200 pixels at 150 dpi.
        assert!(m[2] < m[3], "{m:?}");
        assert!(
            (m[2] - 48.0).abs() < 0.1 && (m[3] - 96.0).abs() < 0.1,
            "{m:?}"
        );
        let c = content(&doc, page);
        assert!(c.contains("0.0000 -96.0000 48.0000 0.0000"), "{c}");
        let embedded = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .any(|s| s.content == bytes);
        assert!(embedded, "the JPEG bytes are untouched");
    }

    #[test]
    fn no_pictures_is_an_option_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = write_pdf(
            &[],
            &ImagesToPdfOptions::default(),
            dir.path(),
            &dir.path().join("o.pdf"),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn a_picture_that_is_not_an_image_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.png");
        std::fs::write(&junk, b"nope").unwrap();
        let err = write_pdf(
            &[junk],
            &ImagesToPdfOptions::default(),
            dir.path(),
            &dir.path().join("o.pdf"),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InputNotPdf);
    }

    #[test]
    fn turned_pictures_get_a_matrix_that_keeps_them_in_their_rectangle() {
        let p = Placement {
            page_w: 100.0,
            page_h: 50.0,
            x: 10.0,
            y: 5.0,
            w: 80.0,
            h: 40.0,
        };
        assert_eq!(matrix(&p, 0), [80.0, 0.0, 0.0, 40.0, 10.0, 5.0]);
        // The unit square's corners must land inside the rectangle after each turn.
        for turns in [0, 90, 180, 270] {
            let m = matrix(&p, turns);
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let x = m[0] * u + m[2] * v + m[4];
                let y = m[1] * u + m[3] * v + m[5];
                assert!(
                    (10.0..=90.0).contains(&x) && (5.0..=45.0).contains(&y),
                    "{turns}"
                );
            }
        }
    }
}
