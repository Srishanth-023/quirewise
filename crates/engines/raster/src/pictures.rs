//! Rendered pages written as JPEG, PNG or TIFF files.

use application::AppError;
use application::ports::{PictureSpec, PictureWriter, RenderedImage, TiffPages, TiffSpec};
use domain::ErrorCode;
use domain::tools::convert::{PictureColor, PictureFormat, TiffCompression};
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageReader};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use crate::RasterConverter;
use crate::tiffio::{TiffWriter, black_white, count_frames, luma};

fn encode_failed() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "A page picture could not be written. Try a lower resolution.",
    )
}

/// The pixels of a page as the samples one file format stores.
enum Pixels {
    Rgb(Vec<u8>),
    Gray(Vec<u8>),
}

fn pixels(image: &RenderedImage, color: PictureColor) -> Pixels {
    match color {
        PictureColor::Color => {
            let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
            for px in image.rgba.as_chunks::<4>().0 {
                let a = u16::from(px[3]);
                for c in &px[..3] {
                    rgb.push(((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8);
                }
            }
            Pixels::Rgb(rgb)
        }
        PictureColor::Gray => Pixels::Gray(luma(image)),
        PictureColor::BlackWhite => Pixels::Gray(
            black_white(image)
                .into_iter()
                .map(|black| if black { 0 } else { 255 })
                .collect(),
        ),
    }
}

impl PictureWriter for RasterConverter {
    fn write_picture(
        &self,
        image: &RenderedImage,
        spec: &PictureSpec,
        dest: &Path,
    ) -> Result<(), AppError> {
        if spec.format == PictureFormat::Tiff {
            let mut tiff = self.start_tiff(
                dest,
                &TiffSpec {
                    compression: TiffCompression::Lzw,
                    color: spec.color,
                    dpi: spec.dpi,
                },
            )?;
            tiff.add(image)?;
            return tiff.finish();
        }
        let samples = pixels(image, spec.color);
        let (data, color_type) = match &samples {
            Pixels::Rgb(v) => (v, ExtendedColorType::Rgb8),
            Pixels::Gray(v) => (v, ExtendedColorType::L8),
        };
        let file = File::create(dest).map_err(|e| AppError::from_io("write the picture", &e))?;
        let out = BufWriter::new(file);
        match spec.format {
            PictureFormat::Jpg => {
                let mut encoder = JpegEncoder::new_with_quality(out, spec.jpeg_quality);
                let dpi = u16::try_from(spec.dpi.get()).unwrap_or(u16::MAX);
                encoder.set_pixel_density(PixelDensity::dpi(dpi));
                encoder
                    .encode(data, image.width, image.height, color_type)
                    .map_err(|_| encode_failed())
            }
            _ => PngEncoder::new(out)
                .write_image(data, image.width, image.height, color_type)
                .map_err(|_| encode_failed()),
        }
    }

    fn start_tiff(&self, dest: &Path, spec: &TiffSpec) -> Result<Box<dyn TiffPages>, AppError> {
        Ok(Box::new(TiffWriter::create(dest, spec)?))
    }

    fn check_picture(
        &self,
        path: &Path,
        format: PictureFormat,
        pages: u32,
    ) -> Result<(), AppError> {
        let wrong = || {
            AppError::new(
                ErrorCode::OutputVerifyFailed,
                "The picture could not be read back after writing, so it was not saved.",
            )
        };
        if fs::metadata(path).map_err(|_| wrong())?.len() == 0 {
            return Err(wrong());
        }
        if format == PictureFormat::Tiff {
            return if count_frames(path)? == pages {
                Ok(())
            } else {
                Err(wrong())
            };
        }
        ImageReader::open(path)
            .map_err(|_| wrong())?
            .with_guessed_format()
            .map_err(|_| wrong())?
            .decode()
            .map(|_| ())
            .map_err(|_| wrong())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Dpi;

    fn page() -> RenderedImage {
        let (w, h) = (60u32, 40u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let ink = x > 10 && x < 30 && y > 8 && y < 24;
                rgba.extend_from_slice(if ink {
                    &[200, 20, 20, 255]
                } else {
                    &[255, 255, 255, 255]
                });
            }
        }
        RenderedImage {
            width: w,
            height: h,
            rgba,
        }
    }

    fn spec(format: PictureFormat, color: PictureColor) -> PictureSpec {
        PictureSpec {
            format,
            jpeg_quality: 85,
            color,
            dpi: Dpi::new(150).unwrap(),
        }
    }

    #[test]
    fn every_format_and_color_writes_a_file_that_decodes() {
        let dir = tempfile::tempdir().unwrap();
        for format in [PictureFormat::Jpg, PictureFormat::Png, PictureFormat::Tiff] {
            for color in [
                PictureColor::Color,
                PictureColor::Gray,
                PictureColor::BlackWhite,
            ] {
                let dest = dir.path().join(format!("p.{}", format.extension()));
                RasterConverter
                    .write_picture(&page(), &spec(format, color), &dest)
                    .unwrap();
                RasterConverter.check_picture(&dest, format, 1).unwrap();
                if format != PictureFormat::Tiff {
                    let decoded = image::open(&dest).unwrap();
                    assert_eq!((decoded.width(), decoded.height()), (60, 40));
                    if color != PictureColor::Color {
                        assert!(!decoded.color().has_color(), "{format:?} {color:?}");
                    }
                }
                std::fs::remove_file(&dest).unwrap();
            }
        }
    }

    #[test]
    fn black_and_white_has_only_two_values() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bw.png");
        RasterConverter
            .write_picture(
                &page(),
                &spec(PictureFormat::Png, PictureColor::BlackWhite),
                &dest,
            )
            .unwrap();
        let gray = image::open(&dest).unwrap().to_luma8();
        assert!(gray.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255));
        assert!(gray.pixels().any(|p| p.0[0] == 0));
    }

    #[test]
    fn the_check_catches_damage_and_wrong_page_counts() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.jpg");
        std::fs::write(&bad, b"not a jpeg").unwrap();
        assert!(
            RasterConverter
                .check_picture(&bad, PictureFormat::Jpg, 1)
                .is_err()
        );
        let empty = dir.path().join("empty.png");
        std::fs::write(&empty, b"").unwrap();
        assert!(
            RasterConverter
                .check_picture(&empty, PictureFormat::Png, 1)
                .is_err()
        );
        let tiff = dir.path().join("t.tif");
        RasterConverter
            .write_picture(
                &page(),
                &spec(PictureFormat::Tiff, PictureColor::Gray),
                &tiff,
            )
            .unwrap();
        assert!(
            RasterConverter
                .check_picture(&tiff, PictureFormat::Tiff, 2)
                .is_err()
        );
    }
}
