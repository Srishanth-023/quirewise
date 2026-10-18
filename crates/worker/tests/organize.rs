//! End-to-end tests for the Organize tools: the real RunTool, worker engines (PDFium, qpdf, lopdf)
//! and file store, run on generated PDFs. Outputs are read back with PDFium.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use application::ports::{OutlineReader, PdfReader, Renderer, TextExtractor};
use application::{
    AppError, CancellationToken, InputDoc, NoProgress, OutputArtifact, Password, ProgressEvent,
    ProgressSink, UnitRequest, UnitRunner,
};
use domain::naming::OutputNamingPattern;
use domain::tools::combine::{AlternateFileOptions, FormHandling, MergeFileOptions, OrganizedPage};
use domain::tools::common::{PageSizeChoice, Region, RightAngle};
use domain::tools::pages::{InsertMode, InsertWhere, SplitDirection};
use domain::tools::{
    AlternateMixOptions, ExtractPagesOptions, InsertPagesOptions, MergeOptions, OrganizeOptions,
    RemovePagesOptions, RotateOptions, SplitByBookmarksOptions, SplitBySizeOptions,
    SplitByTextOptions, SplitEveryNOptions, SplitInHalfOptions, SplitRangesOptions,
};
use domain::{Dpi, ErrorCode, PageIndex, PageRange, ToolOptions};
use infrastructure::{AtomicFileStore, NativePaths, SystemClock, TracingLogger};
use quirewise_engine_pdfium::PdfiumEngine;
use quirewise_worker::engine::{Engines, build_services};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use test_support::pdfs;
use test_support::{encrypt_pdf, minimal_pdf, pdfium_dir, qpdf_exe};

struct Lab {
    dir: tempfile::TempDir,
    runner: Arc<dyn UnitRunner>,
    engine: PdfiumEngine,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = NativePaths {
            pdfium_dir: pdfium_dir(),
            qpdf_exe: qpdf_exe().unwrap(),
        };
        let engines = Engines::load(&paths).unwrap();
        let services = build_services(
            &engines,
            Arc::new(AtomicFileStore),
            Arc::new(SystemClock),
            Arc::new(TracingLogger),
            Default::default(),
            dir.path().join("scratch"),
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("out")).unwrap();
        Self {
            dir,
            runner: services.runner,
            engine: engines.pdfium,
        }
    }

    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn pdf(&self, name: &str, texts: &[&str]) -> InputDoc {
        InputDoc::new(self.put(name, &pdfs::pdf_with_texts(texts)))
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn run_with(
        &self,
        options: ToolOptions,
        inputs: Vec<InputDoc>,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        let request = UnitRequest {
            tool: options.tool_id(),
            options,
            inputs,
            output_dir: Some(self.out()),
            naming_pattern: OutputNamingPattern::default(),
        };
        self.runner.run_unit(&request, cancel, progress)
    }

    fn run(
        &self,
        options: ToolOptions,
        inputs: Vec<InputDoc>,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        self.run_with(options, inputs, &CancellationToken::new(), &NoProgress)
    }

    /// Text of every page, trimmed.
    fn texts(&self, path: &Path) -> Vec<String> {
        let count = self.engine.open_info(path, None).unwrap().page_count;
        (0..count)
            .map(|i| {
                self.engine
                    .page_text(path, None, PageIndex::new(i))
                    .unwrap()
                    .trim()
                    .to_owned()
            })
            .collect()
    }

    fn sizes(&self, path: &Path) -> Vec<(f32, f32)> {
        self.engine
            .page_sizes(path, None)
            .unwrap()
            .iter()
            .map(|s| (s.width_pt.round(), s.height_pt.round()))
            .collect()
    }

    fn names(outputs: &[OutputArtifact]) -> Vec<String> {
        outputs
            .iter()
            .map(|o| o.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    /// Checks the properties every tool run must leave behind: nothing left in the scratch
    /// folder, no temp outputs next to the results, and the reported pages and sizes are real.
    fn finished(&self, outputs: &[OutputArtifact]) {
        let scratch = self.dir.path().join("scratch");
        assert_eq!(
            fs::read_dir(&scratch).unwrap().count(),
            0,
            "scratch files remain"
        );
        for entry in fs::read_dir(self.out()).unwrap().flatten() {
            assert!(
                !entry.file_name().to_string_lossy().ends_with(".qwtmp"),
                "temp output remains"
            );
        }
        for output in outputs {
            assert_eq!(output.bytes, fs::metadata(&output.path).unwrap().len());
            let pages = self
                .engine
                .open_info(&output.path, None)
                .unwrap()
                .page_count;
            assert_eq!(output.pages, Some(pages), "reported page count is real");
        }
    }

    /// Fraction of dark pixels in a rendered page, optionally limited to a horizontal band.
    fn darkness(&self, path: &Path, page: u32) -> (f32, f32) {
        let image = self
            .engine
            .render_page(path, None, PageIndex::new(page), Dpi::new(36).unwrap())
            .unwrap();
        let (w, h) = (image.width as usize, image.height as usize);
        let dark = |x0: usize, x1: usize| -> f32 {
            let mut count = 0usize;
            for y in 0..h {
                for x in x0..x1 {
                    if image.rgba[(y * w + x) * 4] < 128 {
                        count += 1;
                    }
                }
            }
            count as f32 / (h * (x1 - x0)).max(1) as f32
        };
        (dark(0, w / 2), dark(w / 2, w))
    }
}

fn range(s: &str) -> PageRange {
    PageRange::parse(s).unwrap()
}

fn hash(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

/// Compares the fingerprint (one `WxH | text` line per page) with the golden file, or rewrites the
/// file when `QW_UPDATE_GOLDEN` is set (see scripts/update-golden.ps1).
fn golden(name: &str, lab: &Lab, path: &Path) {
    let lines: Vec<String> = lab
        .texts(path)
        .iter()
        .zip(lab.sizes(path))
        .map(|(text, (w, h))| format!("{w}x{h} | {text}"))
        .collect();
    let file = test_support::repo_root()
        .join("fixtures")
        .join("golden")
        .join(format!("{name}.txt"));
    let actual = lines.join("\n") + "\n";
    if std::env::var_os("QW_UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, &actual).unwrap();
    }
    let expected = fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "missing golden file {}; run scripts/update-golden.ps1",
            file.display()
        )
    });
    assert_eq!(
        actual,
        expected.replace("\r\n", "\n"),
        "golden mismatch for {name}"
    );
}

// ---------------------------------------------------------------------------------------------
// Rotate
// ---------------------------------------------------------------------------------------------

#[test]
fn rotate_turns_selected_pages_and_leaves_the_input_untouched() {
    let lab = Lab::new();
    let input = lab.pdf("report.pdf", &["P1", "P2", "P3"]);
    let before = hash(&input.path);
    let outputs = lab
        .run(
            ToolOptions::Rotate(RotateOptions {
                angle: RightAngle::Deg90,
                pages: range("2"),
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(Lab::names(&outputs), vec!["report (rotated).pdf"]);
    let out = &outputs[0].path;
    assert_eq!(
        lab.sizes(out),
        vec![(612.0, 792.0), (792.0, 612.0), (612.0, 792.0)]
    );
    assert_eq!(lab.texts(out), vec!["P1", "P2", "P3"]);
    assert_eq!(hash(&input.path), before, "the input is never modified");
    golden("rotate-page-2", &lab, out);
    lab.finished(&outputs);
}

#[test]
fn rotate_even_and_odd_pages_and_a_second_run_gets_a_new_name() {
    let lab = Lab::new();
    let input = lab.pdf("a.pdf", &["P1", "P2", "P3", "P4"]);
    let options = ToolOptions::Rotate(RotateOptions {
        angle: RightAngle::Deg180,
        pages: range("even"),
    });
    let first = lab.run(options.clone(), vec![input.clone()]).unwrap();
    let second = lab.run(options, vec![input]).unwrap();
    assert_eq!(Lab::names(&first), vec!["a (rotated).pdf"]);
    assert_eq!(Lab::names(&second), vec!["a (rotated) (2).pdf"]);
    lab.finished(&second);
}

// ---------------------------------------------------------------------------------------------
// Extract, remove, split by ranges, every N
// ---------------------------------------------------------------------------------------------

#[test]
fn extract_pages_into_one_file_or_one_per_page() {
    let lab = Lab::new();
    let input = lab.pdf("doc.pdf", &["P1", "P2", "P3", "P4", "P5"]);
    let one = lab
        .run(
            ToolOptions::ExtractPages(ExtractPagesOptions {
                pages: range("4,2,last"),
                one_file: true,
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(
        lab.texts(&one[0].path),
        vec!["P4", "P2", "P5"],
        "pages keep the order written"
    );
    assert_eq!(Lab::names(&one), vec!["doc (extracted).pdf"]);
    golden("extract-4-2-last", &lab, &one[0].path);

    let each = lab
        .run(
            ToolOptions::ExtractPages(ExtractPagesOptions {
                pages: range("2-3"),
                one_file: false,
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        Lab::names(&each),
        vec!["doc (page 2).pdf", "doc (page 3).pdf"]
    );
    assert_eq!(lab.texts(&each[1].path), vec!["P3"]);
    lab.finished(&each);
}

#[test]
fn remove_pages_keeps_the_rest_and_refuses_to_remove_everything() {
    let lab = Lab::new();
    let input = lab.pdf("doc.pdf", &["P1", "P2", "P3", "P4"]);
    let out = lab
        .run(
            ToolOptions::RemovePages(RemovePagesOptions {
                pages: range("2-3"),
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(lab.texts(&out[0].path), vec!["P1", "P4"]);
    lab.finished(&out);

    let err = lab
        .run(
            ToolOptions::RemovePages(RemovePagesOptions {
                pages: range("all"),
            }),
            vec![input],
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

#[test]
fn split_by_ranges_makes_a_file_per_range_or_one_merged_file() {
    let lab = Lab::new();
    let input = lab.pdf("book.pdf", &["P1", "P2", "P3", "P4", "P5"]);
    let outputs = lab
        .run(
            ToolOptions::SplitRanges(SplitRangesOptions {
                ranges: range("1-2,4,5-"),
                merge_into_one: false,
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(
        Lab::names(&outputs),
        vec![
            "book (pages 1-2).pdf",
            "book (page 4).pdf",
            "book (page 5).pdf"
        ]
    );
    assert_eq!(lab.texts(&outputs[0].path), vec!["P1", "P2"]);
    lab.finished(&outputs);

    let merged = lab
        .run(
            ToolOptions::SplitRanges(SplitRangesOptions {
                ranges: range("1-2,4"),
                merge_into_one: true,
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(lab.texts(&merged[0].path), vec!["P1", "P2", "P4"]);

    let overlap = lab.run(
        ToolOptions::SplitRanges(SplitRangesOptions {
            ranges: range("1-3,3"),
            merge_into_one: false,
        }),
        vec![input],
    );
    assert_eq!(overlap.unwrap_err().code, ErrorCode::RangeSyntax);
}

#[test]
fn split_every_n_pages_covers_the_whole_document() {
    let lab = Lab::new();
    let input = lab.pdf("doc.pdf", &["P1", "P2", "P3", "P4", "P5"]);
    let outputs = lab
        .run(
            ToolOptions::SplitEveryN(SplitEveryNOptions { n: 2 }),
            vec![input],
        )
        .unwrap();
    let pages: Vec<usize> = outputs.iter().map(|o| lab.texts(&o.path).len()).collect();
    assert_eq!(pages, vec![2, 2, 1]);
    assert_eq!(lab.texts(&outputs[2].path), vec!["P5"]);
    lab.finished(&outputs);
}

// ---------------------------------------------------------------------------------------------
// Split in half, by size, by bookmarks, by text
// ---------------------------------------------------------------------------------------------

#[test]
fn split_in_half_vertically_gives_each_half_its_own_page() {
    let lab = Lab::new();
    // Text sits at the left edge, so only the left half of each page has ink.
    let input = lab.pdf("scan.pdf", &["AB", "CD"]);
    let outputs = lab
        .run(
            ToolOptions::SplitInHalf(SplitInHalfOptions::default()),
            vec![input.clone()],
        )
        .unwrap();
    let out = &outputs[0].path;
    assert_eq!(lab.sizes(out), vec![(306.0, 792.0); 4]);
    let (left_of_first, right_of_first) = lab.darkness(out, 0);
    let (left_of_second, right_of_second) = lab.darkness(out, 1);
    assert!(
        left_of_first > 0.0 && right_of_first == 0.0,
        "page 1 is the left half"
    );
    assert!(
        left_of_second == 0.0 && right_of_second == 0.0,
        "page 2 is the empty right half"
    );

    let rtl = lab
        .run(
            ToolOptions::SplitInHalf(SplitInHalfOptions {
                right_to_left: true,
                ..Default::default()
            }),
            vec![input.clone()],
        )
        .unwrap();
    let (a, _) = lab.darkness(&rtl[0].path, 0);
    assert_eq!(a, 0.0, "right-to-left puts the right half first");

    let skipped = lab
        .run(
            ToolOptions::SplitInHalf(SplitInHalfOptions {
                skip_first: true,
                skip_last: true,
                ..Default::default()
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        lab.sizes(&skipped[0].path),
        vec![(612.0, 792.0), (612.0, 792.0)],
        "skipped pages stay whole"
    );
    lab.finished(&skipped);
}

#[test]
fn split_in_half_horizontally_cuts_across_and_follows_rotation() {
    let lab = Lab::new();
    let input = InputDoc::new(lab.put(
        "land.pdf",
        &pdfs::pdf_with_geometry(&[(600, 400, 0), (600, 400, 90)]),
    ));
    let outputs = lab
        .run(
            ToolOptions::SplitInHalf(SplitInHalfOptions {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        lab.sizes(&outputs[0].path),
        vec![
            (600.0, 200.0),
            (600.0, 200.0),
            (400.0, 300.0),
            (400.0, 300.0)
        ],
        "unrotated pages halve in height; the turned page halves its displayed height"
    );
}

#[test]
fn split_by_size_packs_pages_and_isolates_a_page_over_the_limit() {
    let lab = Lab::new();
    // Six pages of about 400 KB: two fit in 1 MB, three do not.
    let input = InputDoc::new(lab.put(
        "big.pdf",
        &pdfs::pdf_with_heavy_pages(6, 400_000, &[0, 1, 2, 3, 4, 5]),
    ));
    let outputs = lab
        .run(
            ToolOptions::SplitBySize(SplitBySizeOptions { max_size_mb: 1 }),
            vec![input],
        )
        .unwrap();
    let counts: Vec<u32> = outputs.iter().map(|o| o.pages.unwrap()).collect();
    assert_eq!(
        counts.iter().sum::<u32>(),
        6,
        "every page lands in exactly one file"
    );
    assert!(
        outputs
            .iter()
            .all(|o| o.bytes <= 1024 * 1024 || o.pages == Some(1))
    );
    assert!(outputs.len() >= 3, "{counts:?}");
    lab.finished(&outputs);

    let lab2 = Lab::new();
    let heavy = InputDoc::new(lab2.put("one.pdf", &pdfs::pdf_with_heavy_pages(3, 1_500_000, &[1])));
    let outputs = lab2
        .run(
            ToolOptions::SplitBySize(SplitBySizeOptions { max_size_mb: 1 }),
            vec![heavy],
        )
        .unwrap();
    let oversized: Vec<&OutputArtifact> =
        outputs.iter().filter(|o| o.bytes > 1024 * 1024).collect();
    assert_eq!(
        oversized.len(),
        1,
        "the one page over the limit becomes its own file"
    );
    assert_eq!(oversized[0].pages, Some(1));
    assert_eq!(outputs.iter().map(|o| o.pages.unwrap()).sum::<u32>(), 3);
}

#[test]
fn split_by_bookmarks_names_files_after_the_bookmark_titles() {
    let lab = Lab::new();
    let pdf = pdfs::pdf_with_outline(
        6,
        &[
            ("Intro", 0, 1),
            ("Sub", 1, 2),
            ("Body", 2, 1),
            ("End", 5, 1),
        ],
    );
    let input = InputDoc::new(lab.put("manual.pdf", &pdf));
    let outputs = lab
        .run(
            ToolOptions::SplitByBookmarks(SplitByBookmarksOptions::default()),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(
        Lab::names(&outputs),
        vec![
            "manual (Intro).pdf",
            "manual (Body).pdf",
            "manual (End).pdf"
        ]
    );
    assert_eq!(
        outputs.iter().map(|o| o.pages.unwrap()).collect::<Vec<_>>(),
        vec![2, 3, 1]
    );
    assert_eq!(
        lab.texts(&outputs[1].path),
        vec!["Page 3", "Page 4", "Page 5"]
    );

    let plain = lab
        .run(
            ToolOptions::SplitByBookmarks(SplitByBookmarksOptions {
                level: 1,
                name_from_bookmark: false,
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert!(Lab::names(&plain)[0].contains("part 1"));

    let none = lab.run(
        ToolOptions::SplitByBookmarks(SplitByBookmarksOptions {
            level: 3,
            name_from_bookmark: true,
        }),
        vec![input],
    );
    assert_eq!(none.unwrap_err().code, ErrorCode::OptionInvalid);
    let no_outline = lab.pdf("flat.pdf", &["x", "y"]);
    let err = lab
        .run(
            ToolOptions::SplitByBookmarks(SplitByBookmarksOptions::default()),
            vec![no_outline],
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

#[test]
fn split_by_text_starts_a_new_file_when_the_region_text_changes() {
    let lab = Lab::new();
    let input = lab.pdf(
        "invoices.pdf",
        &["INV-1001", "INV-1001", "INV-1002", "INV-1003", "INV-1003"],
    );
    let region = Region {
        x: 0.0,
        y: 0.0,
        width: 0.7,
        height: 0.12,
    };
    let outputs = lab
        .run(
            ToolOptions::SplitByText(SplitByTextOptions {
                region: Some(region),
                starts_with: None,
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(
        Lab::names(&outputs),
        vec![
            "invoices (INV-1001).pdf",
            "invoices (INV-1002).pdf",
            "invoices (INV-1003).pdf"
        ]
    );
    assert_eq!(
        outputs.iter().map(|o| o.pages.unwrap()).collect::<Vec<_>>(),
        vec![2, 1, 2]
    );
    lab.finished(&outputs);

    let filtered = lab
        .run(
            ToolOptions::SplitByText(SplitByTextOptions {
                region: Some(region),
                starts_with: Some("inv-1003".into()),
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|o| o.pages.unwrap())
            .collect::<Vec<_>>(),
        vec![3, 2]
    );

    let missing = lab.run(
        ToolOptions::SplitByText(SplitByTextOptions::default()),
        vec![lab.pdf("x.pdf", &["a"])],
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::OptionInvalid);
}

// ---------------------------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------------------------

fn png_file(lab: &Lab, name: &str) -> InputDoc {
    let mut bytes = Vec::new();
    image::RgbImage::from_fn(60, 30, |x, _| image::Rgb([(x * 4) as u8, 40, 160]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    InputDoc::new(lab.put(name, &bytes))
}

fn outline_titles(lab: &Lab, path: &Path) -> Vec<(String, Option<u32>)> {
    lab.engine
        .outline(path, None)
        .unwrap()
        .into_iter()
        .map(|i| (i.title, i.page.map(|p| p.get())))
        .collect()
}

#[test]
fn merge_combines_pdfs_and_images_with_bookmarks_per_file() {
    let lab = Lab::new();
    let a = lab.pdf("alpha.pdf", &["A1", "A2"]);
    let b = lab.pdf("beta.pdf", &["B1", "B2", "B3"]);
    let img = png_file(&lab, "photo.png");
    let outputs = lab
        .run(ToolOptions::Merge(MergeOptions::default()), vec![a, b, img])
        .unwrap();
    assert_eq!(Lab::names(&outputs), vec!["alpha (merged).pdf"]);
    let out = &outputs[0].path;
    let texts = lab.texts(out);
    assert_eq!(&texts[..5], ["A1", "A2", "B1", "B2", "B3"]);
    assert_eq!(texts.len(), 6, "the image became the sixth page");
    assert_eq!(
        outline_titles(&lab, out),
        vec![
            ("alpha".into(), Some(0)),
            ("beta".into(), Some(2)),
            ("photo".into(), Some(5))
        ]
    );
    let (w, h) = lab.sizes(out)[5];
    assert!(w > h, "the landscape image keeps its proportions ({w}x{h})");
    golden("merge-two-pdfs-and-an-image", &lab, out);
    lab.finished(&outputs);
}

#[test]
fn merge_applies_per_file_ranges_and_rotation_and_can_skip_bookmarks() {
    let lab = Lab::new();
    let a = lab.pdf("a.pdf", &["A1", "A2", "A3"]);
    let b = lab.pdf("b.pdf", &["B1", "B2"]);
    let options = MergeOptions {
        per_file: vec![
            MergeFileOptions {
                pages: range("3,1"),
                rotation: RightAngle::Deg0,
            },
            MergeFileOptions {
                pages: range("all"),
                rotation: RightAngle::Deg90,
            },
        ],
        bookmark_per_file: false,
        ..MergeOptions::default()
    };
    let outputs = lab.run(ToolOptions::Merge(options), vec![a, b]).unwrap();
    let out = &outputs[0].path;
    assert_eq!(lab.texts(out), vec!["A3", "A1", "B1", "B2"]);
    assert_eq!(
        lab.sizes(out),
        vec![
            (612.0, 792.0),
            (612.0, 792.0),
            (792.0, 612.0),
            (792.0, 612.0)
        ]
    );
    assert!(outline_titles(&lab, out).is_empty());
    lab.finished(&outputs);
}

#[test]
fn merge_can_insert_a_table_of_contents_and_scale_pages_to_the_first_size() {
    let lab = Lab::new();
    let a = InputDoc::new(lab.put("big.pdf", &pdfs::pdf_with_geometry(&[(612, 792, 0)])));
    let b = InputDoc::new(lab.put(
        "small.pdf",
        &pdfs::pdf_with_geometry(&[(300, 400, 0), (300, 400, 0)]),
    ));
    let options = MergeOptions {
        table_of_contents: true,
        normalize_page_size: true,
        ..MergeOptions::default()
    };
    let outputs = lab.run(ToolOptions::Merge(options), vec![a, b]).unwrap();
    let out = &outputs[0].path;
    assert_eq!(
        outputs[0].pages,
        Some(4),
        "one contents page plus three pages"
    );
    assert_eq!(
        lab.sizes(out),
        vec![(612.0, 792.0); 4],
        "every page has the first page's size"
    );
    let first_page_text = lab.texts(out).remove(0);
    assert!(first_page_text.contains("Contents"), "{first_page_text:?}");
    assert!(first_page_text.contains("big") && first_page_text.contains("small"));
    // The listed start pages account for the contents page: big starts on 2, small on 3.
    assert!(first_page_text.contains('2') && first_page_text.contains('3'));
    let titles = outline_titles(&lab, out);
    assert_eq!(titles[0], ("Contents".to_owned(), Some(0)));
    assert_eq!(titles[1], ("big".to_owned(), Some(1)));
    assert_eq!(titles[2], ("small".to_owned(), Some(2)));
    lab.finished(&outputs);
}

#[test]
fn merge_flattens_form_fields_when_asked_and_keeps_them_otherwise() {
    let lab = Lab::new();
    let form = InputDoc::new(lab.put(
        "form.pdf",
        &pdfs::pdf_with_form_field("name", "Ada Lovelace"),
    ));
    let plain = lab.pdf("plain.pdf", &["Z1"]);
    let kept = lab
        .run(
            ToolOptions::Merge(MergeOptions::default()),
            vec![form.clone(), plain.clone()],
        )
        .unwrap();
    let flat = lab
        .run(
            ToolOptions::Merge(MergeOptions {
                forms: FormHandling::Flatten,
                ..Default::default()
            }),
            vec![form, plain],
        )
        .unwrap();
    let has_fields = |path: &Path| {
        use application::ports::PdfFacts;
        lab.engine.facts(path, None).unwrap().has_forms
    };
    assert!(has_fields(&kept[0].path), "fields stay fillable by default");
    assert!(!has_fields(&flat[0].path), "flattened output has no form");
    assert!(lab.texts(&flat[0].path)[0].contains("Ada Lovelace"));
    lab.finished(&flat);
}

#[test]
fn merge_reads_password_protected_inputs_with_their_password() {
    let lab = Lab::new();
    let plain = lab.put("secret-plain.pdf", &pdfs::pdf_with_texts(&["S1", "S2"]));
    let locked = lab.dir.path().join("secret.pdf");
    encrypt_pdf(&plain, &locked, "open");
    let mut protected = InputDoc::new(&locked);
    protected.password = Some(Password::new("open"));
    let other = lab.pdf("other.pdf", &["O1"]);
    let outputs = lab
        .run(
            ToolOptions::Merge(MergeOptions::default()),
            vec![protected, other.clone()],
        )
        .unwrap();
    assert_eq!(lab.texts(&outputs[0].path), vec!["S1", "S2", "O1"]);

    // Without the password the file is rejected before anything is written.
    let before = fs::read_dir(lab.out()).unwrap().count();
    let err = lab
        .run(
            ToolOptions::Merge(MergeOptions::default()),
            vec![InputDoc::new(&locked), other],
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfEncrypted);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), before);
}

// ---------------------------------------------------------------------------------------------
// Alternate and mix, organize, insert pages
// ---------------------------------------------------------------------------------------------

#[test]
fn alternate_and_mix_interleaves_and_can_reverse_a_duplex_scan() {
    let lab = Lab::new();
    let fronts = lab.pdf("fronts.pdf", &["F1", "F2", "F3"]);
    let backs = lab.pdf("backs.pdf", &["B3", "B2", "B1"]);
    let options = AlternateMixOptions {
        pages_per_turn: 1,
        per_file: vec![
            AlternateFileOptions { reverse: false },
            AlternateFileOptions { reverse: true },
        ],
    };
    let outputs = lab
        .run(
            ToolOptions::AlternateMix(options),
            vec![fronts.clone(), backs.clone()],
        )
        .unwrap();
    assert_eq!(
        lab.texts(&outputs[0].path),
        vec!["F1", "B1", "F2", "B2", "F3", "B3"]
    );
    golden("alternate-duplex", &lab, &outputs[0].path);

    let pairs = lab
        .run(
            ToolOptions::AlternateMix(AlternateMixOptions {
                pages_per_turn: 2,
                per_file: vec![],
            }),
            vec![fronts, backs],
        )
        .unwrap();
    assert_eq!(
        lab.texts(&pairs[0].path),
        vec!["F1", "F2", "B3", "B2", "F3", "B1"]
    );
    lab.finished(&pairs);

    let one = lab.run(
        ToolOptions::AlternateMix(AlternateMixOptions::default()),
        vec![lab.pdf("x.pdf", &["x"])],
    );
    assert_eq!(one.unwrap_err().code, ErrorCode::OptionInvalid);
}

#[test]
fn organize_applies_the_page_list_from_several_files() {
    let lab = Lab::new();
    let a = lab.pdf("a.pdf", &["A1", "A2", "A3"]);
    let b = lab.pdf("b.pdf", &["B1", "B2"]);
    let options = OrganizeOptions {
        pages: vec![
            OrganizedPage::Page {
                source: 0,
                page: 2,
                rotate: RightAngle::Deg0,
            },
            OrganizedPage::Blank {
                width_pt: 300.0,
                height_pt: 400.0,
            },
            OrganizedPage::Page {
                source: 1,
                page: 0,
                rotate: RightAngle::Deg90,
            },
            OrganizedPage::Page {
                source: 0,
                page: 0,
                rotate: RightAngle::Deg0,
            },
            OrganizedPage::Page {
                source: 0,
                page: 0,
                rotate: RightAngle::Deg0,
            },
        ],
    };
    let outputs = lab
        .run(ToolOptions::Organize(options), vec![a.clone(), b])
        .unwrap();
    let out = &outputs[0].path;
    assert_eq!(lab.texts(out), vec!["A3", "", "B1", "A1", "A1"]);
    assert_eq!(
        lab.sizes(out),
        vec![
            (612.0, 792.0),
            (300.0, 400.0),
            (792.0, 612.0),
            (612.0, 792.0),
            (612.0, 792.0)
        ]
    );
    assert_eq!(Lab::names(&outputs), vec!["a (organized).pdf"]);

    let bad = lab.run(
        ToolOptions::Organize(OrganizeOptions {
            pages: vec![OrganizedPage::Page {
                source: 0,
                page: 9,
                rotate: RightAngle::Deg0,
            }],
        }),
        vec![a.clone()],
    );
    assert_eq!(bad.unwrap_err().code, ErrorCode::RangeOutOfBounds);

    let same = lab
        .run(ToolOptions::Organize(OrganizeOptions::default()), vec![a])
        .unwrap();
    assert_eq!(
        lab.texts(&same[0].path),
        vec!["A1", "A2", "A3"],
        "no list means the file as it is"
    );
    lab.finished(&same);
}

#[test]
fn insert_blank_pages_take_the_neighbor_size_or_a_chosen_size() {
    let lab = Lab::new();
    let input = InputDoc::new(lab.put(
        "mixed.pdf",
        &pdfs::pdf_with_geometry(&[(612, 792, 0), (400, 300, 0)]),
    ));
    let outputs = lab
        .run(
            ToolOptions::InsertPages(InsertPagesOptions {
                pages: range("all"),
                position: InsertWhere::After,
                count: 1,
                ..InsertPagesOptions::default()
            }),
            vec![input.clone()],
        )
        .unwrap();
    assert_eq!(
        lab.sizes(&outputs[0].path),
        vec![
            (612.0, 792.0),
            (612.0, 792.0),
            (400.0, 300.0),
            (400.0, 300.0)
        ]
    );
    assert_eq!(
        lab.texts(&outputs[0].path),
        vec!["Page 1", "", "Page 2", ""]
    );
    assert_eq!(Lab::names(&outputs), vec!["mixed (pages inserted).pdf"]);

    let before = lab
        .run(
            ToolOptions::InsertPages(InsertPagesOptions {
                pages: range("1"),
                position: InsertWhere::Before,
                count: 2,
                page_size: PageSizeChoice::A4,
                ..InsertPagesOptions::default()
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        lab.sizes(&before[0].path),
        vec![
            (595.0, 842.0),
            (595.0, 842.0),
            (612.0, 792.0),
            (400.0, 300.0)
        ]
    );
    lab.finished(&before);
}

#[test]
fn duplicate_pages_repeat_the_selected_pages_in_place() {
    let lab = Lab::new();
    let input = lab.pdf("d.pdf", &["P1", "P2", "P3"]);
    let outputs = lab
        .run(
            ToolOptions::InsertPages(InsertPagesOptions {
                mode: InsertMode::Duplicate,
                pages: range("2"),
                count: 2,
                ..InsertPagesOptions::default()
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        lab.texts(&outputs[0].path),
        vec!["P1", "P2", "P2", "P2", "P3"]
    );
    assert_eq!(Lab::names(&outputs), vec!["d (pages duplicated).pdf"]);
    lab.finished(&outputs);
}

// ---------------------------------------------------------------------------------------------
// Negative cases, cancellation, progress
// ---------------------------------------------------------------------------------------------

#[test]
fn bad_inputs_fail_with_catalog_codes_and_write_nothing() {
    let lab = Lab::new();
    let rotate = || ToolOptions::Rotate(RotateOptions::default());

    let plain = lab.put("p.pdf", &minimal_pdf(2));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "open");
    assert_eq!(
        lab.run(rotate(), vec![InputDoc::new(&locked)])
            .unwrap_err()
            .code,
        ErrorCode::PdfEncrypted
    );
    let mut wrong = InputDoc::new(&locked);
    wrong.password = Some(Password::new("nope"));
    assert_eq!(
        lab.run(rotate(), vec![wrong]).unwrap_err().code,
        ErrorCode::PdfWrongPassword
    );

    let damaged = lab.put("bad.pdf", b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog");
    assert_eq!(
        lab.run(rotate(), vec![InputDoc::new(damaged)])
            .unwrap_err()
            .code,
        ErrorCode::PdfDamaged
    );

    let not_pdf = lab.put("x.pdf", b"PK\x03\x04 zip");
    assert_eq!(
        lab.run(rotate(), vec![InputDoc::new(not_pdf)])
            .unwrap_err()
            .code,
        ErrorCode::InputNotPdf
    );

    let empty = lab.put("none.pdf", &pdfs::pdf_without_pages());
    let err = lab.run(rotate(), vec![InputDoc::new(empty)]).unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::RangeOutOfBounds,
        "a document with no pages cannot be rotated"
    );

    let missing = lab
        .run(
            rotate(),
            vec![InputDoc::new(lab.dir.path().join("gone.pdf"))],
        )
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::PathDenied);

    assert_eq!(
        fs::read_dir(lab.out()).unwrap().count(),
        0,
        "nothing was written"
    );
}

#[test]
fn cancelling_before_the_run_writes_nothing() {
    let lab = Lab::new();
    let token = CancellationToken::new();
    token.cancel();
    let err = lab
        .run_with(
            ToolOptions::SplitEveryN(SplitEveryNOptions { n: 1 }),
            vec![lab.pdf("a.pdf", &["1", "2", "3"])],
            &token,
            &NoProgress,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

struct CancelAfterFirstFile {
    token: CancellationToken,
    seen_progress: AtomicBool,
}

impl ProgressSink for CancelAfterFirstFile {
    fn report(&self, event: ProgressEvent) {
        if event.done > 0 {
            self.seen_progress.store(true, Ordering::SeqCst);
            self.token.cancel();
        }
    }
}

#[test]
fn cancelling_midway_stops_before_the_next_file_and_keeps_finished_ones() {
    let lab = Lab::new();
    let token = CancellationToken::new();
    let sink = CancelAfterFirstFile {
        token: token.clone(),
        seen_progress: AtomicBool::new(false),
    };
    let texts: Vec<String> = (1..=8).map(|i| format!("P{i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let err = lab
        .run_with(
            ToolOptions::SplitEveryN(SplitEveryNOptions { n: 1 }),
            vec![lab.pdf("many.pdf", &refs)],
            &token,
            &sink,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert!(sink.seen_progress.load(Ordering::SeqCst));
    let written = fs::read_dir(lab.out()).unwrap().count();
    assert!(
        (1..8).contains(&written),
        "finished files stay, the rest are not written: {written}"
    );
    for entry in fs::read_dir(lab.out()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            name.ends_with(".pdf") && !name.ends_with(".qwtmp"),
            "{name}"
        );
    }
    assert_eq!(
        fs::read_dir(lab.dir.path().join("scratch"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn progress_is_determinate_and_ends_at_the_total() {
    struct Record(std::sync::Mutex<Vec<(u64, u64)>>);
    impl ProgressSink for Record {
        fn report(&self, event: ProgressEvent) {
            self.0.lock().unwrap().push((event.done, event.total));
        }
    }
    let lab = Lab::new();
    let record = Record(std::sync::Mutex::new(Vec::new()));
    lab.run_with(
        ToolOptions::SplitEveryN(SplitEveryNOptions { n: 2 }),
        vec![lab.pdf("p.pdf", &["1", "2", "3", "4", "5"])],
        &CancellationToken::new(),
        &record,
    )
    .unwrap();
    let events = record.0.lock().unwrap().clone();
    assert_eq!(events.first(), Some(&(0, 5)));
    assert_eq!(events.last(), Some(&(5, 5)));
    assert!(
        events.windows(2).all(|w| w[0].0 <= w[1].0),
        "progress never goes backwards"
    );
    assert!(
        events.iter().all(|(_, total)| *total == 5),
        "the total is known from the start"
    );
}

#[test]
fn encrypted_inputs_with_a_password_work_for_single_file_tools_too() {
    let lab = Lab::new();
    let plain = lab.put("e-plain.pdf", &pdfs::pdf_with_texts(&["E1", "E2", "E3"]));
    let locked = lab.dir.path().join("e.pdf");
    encrypt_pdf(&plain, &locked, "open");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("open"));
    let outputs = lab
        .run(
            ToolOptions::ExtractPages(ExtractPagesOptions {
                pages: range("2-3"),
                one_file: true,
            }),
            vec![input],
        )
        .unwrap();
    assert_eq!(
        lab.texts(&outputs[0].path),
        vec!["E2", "E3"],
        "the output opens without a password"
    );
    lab.finished(&outputs);
}
