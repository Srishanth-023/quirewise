//! Documents made of page pictures: the last step of "flatten everything to image".

use application::AppError;
use application::ports::{PageImage, PageImageWriter};
use domain::ErrorCode;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageReader};
use lopdf::{Document, Object, Stream, dictionary};
use std::io::Write;

use crate::RasterConverter;

/// JPEG quality of page pictures. High, because the picture is the whole page.
const PAGE_JPEG_QUALITY: u8 = 88;

fn unreadable() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "A page picture could not be read back while writing the document.",
    )
}

fn flate(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .and_then(|()| encoder.finish())
        .map_err(|e| AppError::from_io("compress a page picture", &e))
}

/// Pixels as an image stream: JPEG or lossless Flate, whichever is smaller. Flat pages (a scan
/// of plain text) often compress better without JPEG, and they stay sharp.
fn encode(rgb: &[u8], width: u32, height: u32) -> Result<(&'static str, Vec<u8>), AppError> {
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, PAGE_JPEG_QUALITY)
        .encode(rgb, width, height, ExtendedColorType::Rgb8)
        .map_err(|_| unreadable())?;
    let lossless = flate(rgb)?;
    Ok(if lossless.len() <= jpeg.len() {
        ("FlateDecode", lossless)
    } else {
        ("DCTDecode", jpeg)
    })
}

impl PageImageWriter for RasterConverter {
    fn pages_to_pdf(&self, pages: &[PageImage], dest: &std::path::Path) -> Result<(), AppError> {
        if pages.is_empty() {
            return Err(domain::DomainError::option("pages", "there are no pages to write").into());
        }
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::with_capacity(pages.len());
        for page in pages {
            let rgb = ImageReader::open(&page.png)
                .map_err(|e| AppError::from_io("read a page picture", &e))?
                .with_guessed_format()
                .map_err(|_| unreadable())?
                .decode()
                .map_err(|_| unreadable())?
                .to_rgb8();
            let (width, height) = rgb.dimensions();
            let (filter, data) = encode(rgb.as_raw(), width, height)?;
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => i64::from(width),
                    "Height" => i64::from(height),
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8,
                    "Filter" => filter,
                },
                data,
            );
            stream.allows_compression = false;
            let image_id = doc.add_object(stream);
            let content = doc.add_object(Stream::new(
                dictionary! {},
                format!(
                    "q {:.3} 0 0 {:.3} 0 0 cm /Im0 Do Q",
                    page.width_pt, page.height_pt
                )
                .into_bytes(),
            ));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), page.width_pt.into(), page.height_pt.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
                "Contents" => content,
            });
            kids.push(page_id.into());
        }
        let count = i64::try_from(kids.len()).unwrap_or(i64::MAX);
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the document", &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn png(
        dir: &tempfile::TempDir,
        name: &str,
        w: u32,
        h: u32,
        color: Rgb<u8>,
    ) -> std::path::PathBuf {
        let path = dir.path().join(name);
        RgbImage::from_pixel(w, h, color).save(&path).unwrap();
        path
    }

    #[test]
    fn writes_one_page_per_picture_at_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        let pages = [
            PageImage {
                png: png(&dir, "a.png", 40, 60, Rgb([255, 255, 255])),
                width_pt: 612.0,
                height_pt: 792.0,
            },
            PageImage {
                png: png(&dir, "b.png", 60, 40, Rgb([10, 20, 30])),
                width_pt: 300.0,
                height_pt: 200.0,
            },
        ];
        let dest = dir.path().join("out.pdf");
        RasterConverter.pages_to_pdf(&pages, &dest).unwrap();

        let doc = Document::load(&dest).unwrap();
        let ids: Vec<_> = doc.get_pages().into_values().collect();
        assert_eq!(ids.len(), 2);
        let media = |id| {
            doc.get_dictionary(id)
                .unwrap()
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_float().unwrap())
                .collect::<Vec<f32>>()
        };
        assert_eq!(media(ids[0]), [0.0, 0.0, 612.0, 792.0]);
        assert_eq!(media(ids[1]), [0.0, 0.0, 300.0, 200.0]);
    }

    #[test]
    fn flat_pictures_stay_lossless_and_photographs_use_jpeg() {
        let flat = vec![255u8; 64 * 64 * 3];
        assert_eq!(encode(&flat, 64, 64).unwrap().0, "FlateDecode");
        // A smooth gradient with a little texture, like a photograph: JPEG wins by a wide margin.
        let photo: Vec<u8> = (0..64u32 * 64)
            .flat_map(|i| {
                let (x, y) = (i % 64, i / 64);
                [
                    (x * 4 + (i % 7)) as u8,
                    (y * 4 + (i % 5)) as u8,
                    ((x + y) * 2 + (i % 3)) as u8,
                ]
            })
            .collect();
        assert_eq!(encode(&photo, 64, 64).unwrap().0, "DCTDecode");
    }

    #[test]
    fn nothing_to_write_is_an_option_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = RasterConverter
            .pages_to_pdf(&[], &dir.path().join("o.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn a_missing_picture_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let page = PageImage {
            png: dir.path().join("missing.png"),
            width_pt: 100.0,
            height_pt: 100.0,
        };
        assert!(
            RasterConverter
                .pages_to_pdf(&[page], &dir.path().join("o.pdf"))
                .is_err()
        );
    }
}
