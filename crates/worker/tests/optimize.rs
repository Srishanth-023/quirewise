//! End-to-end tests for the Optimize, Security and document tools of Phase 4: the real RunTool,
//! worker engines (PDFium, qpdf, lopdf, raster) and file store, run on generated PDFs.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use application::ports::{PdfReader, TextExtractor};
use application::{
    AppError, CancellationToken, InputDoc, NoProgress, OutputArtifact, Password, UnitRequest,
    UnitRunner,
};
use domain::naming::OutputNamingPattern;
use domain::secret::Secret;
use domain::tools::document::{
    AnnotationKind, FlattenDpi, FlattenMode, FlattenOptions, MetadataOptions,
    RemoveAnnotationsOptions,
};
use domain::tools::optimize::{CompressOptions, CompressPreset, LinearizeOptions, RepairOptions};
use domain::tools::secure::{Permissions, ProtectOptions, SanitizeOptions, UnlockOptions};
use domain::{ErrorCode, PageIndex, PageRange, ToolOptions};
use infrastructure::{AtomicFileStore, NativePaths, SystemClock, TracingLogger};
use quirewise_engine_pdfium::PdfiumEngine;
use quirewise_engine_raster::RasterConverter;
use quirewise_worker::engine::{Engines, build_services};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use test_support::pdfs;
use test_support::{encrypt_pdf, pdfium_dir, qpdf_exe};

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

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn run(&self, options: ToolOptions, input: InputDoc) -> Result<Vec<OutputArtifact>, AppError> {
        let request = UnitRequest {
            tool: options.tool_id(),
            options,
            inputs: vec![input],
            output_dir: Some(self.out()),
            naming_pattern: OutputNamingPattern::default(),
        };
        self.runner
            .run_unit(&request, &CancellationToken::new(), &NoProgress)
    }

    fn texts(&self, path: &Path, password: Option<&Password>) -> Vec<String> {
        let count = self.engine.open_info(path, password).unwrap().page_count;
        (0..count)
            .map(|i| {
                self.engine
                    .page_text(path, password, PageIndex::new(i))
                    .unwrap()
                    .trim()
                    .to_owned()
            })
            .collect()
    }

    /// The file with object streams expanded and streams readable, for looking inside.
    fn expanded(&self, path: &Path) -> String {
        let target = self.dir.path().join(format!(
            "expanded-{}.pdf",
            path.file_stem().unwrap().to_string_lossy()
        ));
        let status = Command::new(qpdf_exe().unwrap())
            .args(["--qdf", "--object-streams=disable"])
            .arg(path)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success() || status.code() == Some(3));
        String::from_utf8_lossy(&fs::read(&target).unwrap()).into_owned()
    }

    /// What no run may leave behind: scratch files or temporary outputs.
    fn tidy(&self) {
        let scratch = self.dir.path().join("scratch");
        assert_eq!(
            fs::read_dir(&scratch).unwrap().count(),
            0,
            "scratch files remain"
        );
        for entry in fs::read_dir(self.out()).unwrap().flatten() {
            assert!(!entry.file_name().to_string_lossy().ends_with(".qwtmp"));
        }
    }

    fn doc(&self, name: &str, bytes: &[u8]) -> InputDoc {
        InputDoc::new(self.put(name, bytes))
    }
}

fn name(output: &OutputArtifact) -> String {
    output
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// A one-page PDF made from a noisy picture, which the raster engine places at 150 dpi.
fn picture_pdf(lab: &Lab, width: u32, height: u32) -> PathBuf {
    use application::ports::ImageConverter;
    let png = lab.dir.path().join("picture.png");
    image::RgbImage::from_fn(width, height, |x, y| {
        let n = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663);
        image::Rgb([(n >> 3) as u8, (n >> 11) as u8, (n >> 19) as u8])
    })
    .save(&png)
    .unwrap();
    let pdf = lab.dir.path().join("picture.pdf");
    RasterConverter.image_to_pdf(&png, &pdf).unwrap();
    pdf
}

fn protect(user: &str, owner: &str, permissions: Permissions) -> ToolOptions {
    ToolOptions::Protect(ProtectOptions {
        user_password: (!user.is_empty()).then(|| Secret::new(user)),
        owner_password: (!owner.is_empty()).then(|| Secret::new(owner)),
        permissions,
        ..ProtectOptions::default()
    })
}

// ---- Compress -----------------------------------------------------------------------------

#[test]
fn compress_shrinks_a_picture_heavy_file_and_keeps_the_pages() {
    let lab = Lab::new();
    let source = picture_pdf(&lab, 1500, 1000);
    let before = fs::metadata(&source).unwrap().len();
    let out = lab
        .run(
            ToolOptions::Compress(CompressOptions {
                preset: CompressPreset::Extreme,
                ..CompressOptions::default()
            }),
            InputDoc::new(&source),
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(name(&out[0]), "picture (compressed).pdf");
    assert_eq!(out[0].pages, Some(1));
    assert!(out[0].bytes < before / 3, "{} vs {before}", out[0].bytes);
    assert_eq!(
        lab.engine.open_info(&out[0].path, None).unwrap().page_count,
        1
    );
    lab.tidy();
}

#[test]
fn compress_never_touches_the_input() {
    let lab = Lab::new();
    let source = picture_pdf(&lab, 600, 400);
    let before = fs::read(&source).unwrap();
    lab.run(
        ToolOptions::defaults_for(domain::ToolId::Compress),
        InputDoc::new(&source),
    )
    .unwrap();
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn compress_can_strip_metadata_and_keeps_text() {
    let lab = Lab::new();
    let input = lab.doc(
        "meta.pdf",
        &pdfs::pdf_with_metadata("Secret title", "Secret author"),
    );
    let out = lab
        .run(
            ToolOptions::Compress(CompressOptions {
                strip_metadata: true,
                ..CompressOptions::default()
            }),
            input,
        )
        .unwrap();
    let inside = lab.expanded(&out[0].path);
    assert!(!inside.contains("Secret title") && !inside.contains("Secret author"));
    assert_eq!(lab.texts(&out[0].path, None), ["Page 1"]);

    let kept = lab
        .run(
            ToolOptions::defaults_for(domain::ToolId::Compress),
            lab.doc(
                "meta2.pdf",
                &pdfs::pdf_with_metadata("Secret title", "Secret author"),
            ),
        )
        .unwrap();
    assert!(
        lab.expanded(&kept[0].path).contains("Secret title"),
        "kept by default"
    );
    lab.tidy();
}

#[test]
fn compress_reads_a_password_protected_file_and_writes_it_unlocked() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "open");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("open"));
    let out = lab
        .run(ToolOptions::defaults_for(domain::ToolId::Compress), input)
        .unwrap();
    assert_eq!(lab.texts(&out[0].path, None), ["one", "two"]);
    lab.tidy();
}

#[test]
fn compress_rejects_a_bad_quality_without_starting() {
    let lab = Lab::new();
    let err = lab
        .run(
            ToolOptions::Compress(CompressOptions {
                jpeg_quality: Some(200),
                ..CompressOptions::default()
            }),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

// ---- Repair -------------------------------------------------------------------------------

#[test]
fn repair_rebuilds_a_file_that_will_not_open() {
    let lab = Lab::new();
    let good = pdfs::pdf_with_texts(&["alpha", "beta", "gamma"]);
    let cut = good.windows(4).rposition(|w| w == b"xref").unwrap();
    let broken = lab.doc("broken.pdf", &good[..cut]);

    let out = lab
        .run(ToolOptions::Repair(RepairOptions::default()), broken)
        .unwrap();
    assert_eq!(name(&out[0]), "broken (repaired).pdf");
    assert_eq!(out[0].pages, Some(3));
    assert_eq!(lab.texts(&out[0].path, None), ["alpha", "beta", "gamma"]);
    lab.tidy();
}

#[test]
fn repair_gives_up_with_a_clear_error_and_leaves_nothing_behind() {
    let lab = Lab::new();
    let junk = lab.doc("junk.pdf", b"%PDF-1.4\nthere is nothing here\n");
    let err = lab
        .run(ToolOptions::Repair(RepairOptions::default()), junk)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfDamaged);
    assert!(
        err.message.contains("could not be repaired"),
        "{}",
        err.message
    );
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn repair_also_works_on_a_healthy_file() {
    let lab = Lab::new();
    let out = lab
        .run(
            ToolOptions::Repair(RepairOptions::default()),
            lab.doc("ok.pdf", &pdfs::pdf_with_texts(&["fine"])),
        )
        .unwrap();
    assert_eq!(lab.texts(&out[0].path, None), ["fine"]);
}

// ---- Linearize ----------------------------------------------------------------------------

fn linearization(path: &Path) -> String {
    let out = Command::new(qpdf_exe().unwrap())
        .arg("--check-linearization")
        .arg(path)
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn linearize_writes_a_fast_web_view_file_and_can_undo_it() {
    let lab = Lab::new();
    let texts: Vec<String> = (1..=8).map(|i| format!("Page {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let source = lab.doc("web.pdf", &pdfs::pdf_with_texts(&refs));

    let fast = lab
        .run(
            ToolOptions::Linearize(LinearizeOptions::default()),
            source.clone(),
        )
        .unwrap();
    assert_eq!(name(&fast[0]), "web (fast web view).pdf");
    assert!(linearization(&fast[0].path).contains("no linearization errors"));
    assert_eq!(lab.texts(&fast[0].path, None).len(), 8);

    let normal = lab
        .run(
            ToolOptions::Linearize(LinearizeOptions { enabled: false }),
            InputDoc::new(&fast[0].path),
        )
        .unwrap();
    assert!(linearization(&normal[0].path).contains("not linearized"));
    lab.tidy();
}

// ---- Protect and Unlock -------------------------------------------------------------------

#[test]
fn protect_makes_the_open_password_necessary_and_unlock_reverses_it() {
    let lab = Lab::new();
    let source = lab.doc("secret.pdf", &pdfs::pdf_with_texts(&["classified", "two"]));
    let out = lab
        .run(
            protect("open sesame", "boss", Permissions::default()),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "secret (protected).pdf");
    assert_eq!(out[0].pages, Some(2));
    assert_eq!(
        lab.engine.open_info(&out[0].path, None).unwrap_err().code,
        ErrorCode::PdfEncrypted
    );
    let right = Password::new("open sesame");
    assert_eq!(lab.texts(&out[0].path, Some(&right)), ["classified", "two"]);

    let mut locked = InputDoc::new(&out[0].path);
    locked.password = Some(right);
    let back = lab
        .run(ToolOptions::Unlock(UnlockOptions::default()), locked)
        .unwrap();
    assert_eq!(name(&back[0]), "secret (protected) (unlocked).pdf");
    assert_eq!(lab.texts(&back[0].path, None), ["classified", "two"]);
    lab.tidy();
}

#[test]
fn protect_applies_the_restrictions_to_the_output() {
    let lab = Lab::new();
    let source = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let out = lab
        .run(
            protect(
                "",
                "boss",
                Permissions {
                    print: false,
                    copy: false,
                    ..Permissions::default()
                },
            ),
            source,
        )
        .unwrap();
    let info = Command::new(qpdf_exe().unwrap())
        .arg("--show-encryption")
        .arg(&out[0].path)
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&info.stdout).into_owned();
    assert!(
        info.contains("print high resolution: not allowed"),
        "{info}"
    );
    assert!(
        info.contains("extract for any purpose: not allowed"),
        "{info}"
    );
    // No open password was set, so the file opens for anyone.
    assert_eq!(lab.texts(&out[0].path, None), ["x"]);
}

#[test]
fn protect_refuses_unsafe_password_combinations_before_writing() {
    let lab = Lab::new();
    let restricted = Permissions {
        modify: false,
        ..Permissions::default()
    };
    for options in [
        ToolOptions::defaults_for(domain::ToolId::Protect),
        protect("open", "", restricted),
        protect("same", "same", restricted),
    ] {
        let err = lab
            .run(options, lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

#[test]
fn protect_can_re_protect_an_already_locked_file_with_its_password() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "old");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("old"));
    let out = lab
        .run(protect("new", "boss", Permissions::default()), input)
        .unwrap();
    let new = Password::new("new");
    assert_eq!(lab.texts(&out[0].path, Some(&new)), ["x"]);
    assert_eq!(
        lab.engine
            .open_info(&out[0].path, Some(&Password::new("old")))
            .unwrap_err()
            .code,
        ErrorCode::PdfWrongPassword
    );
}

#[test]
fn unlock_reports_a_missing_or_wrong_password() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "right");
    let none = lab
        .run(
            ToolOptions::Unlock(UnlockOptions::default()),
            InputDoc::new(&locked),
        )
        .unwrap_err();
    assert_eq!(none.code, ErrorCode::PdfEncrypted);
    let mut wrong = InputDoc::new(&locked);
    wrong.password = Some(Password::new("wrong"));
    let err = lab
        .run(ToolOptions::Unlock(UnlockOptions::default()), wrong)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfWrongPassword);
    lab.tidy();
}

#[test]
fn unlock_removes_owner_restrictions_without_needing_a_password() {
    let lab = Lab::new();
    let restricted = lab
        .run(
            protect(
                "",
                "boss",
                Permissions {
                    modify: false,
                    ..Permissions::default()
                },
            ),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap();
    let free = lab
        .run(
            ToolOptions::Unlock(UnlockOptions::default()),
            InputDoc::new(&restricted[0].path),
        )
        .unwrap();
    let info = Command::new(qpdf_exe().unwrap())
        .arg("--show-encryption")
        .arg(&free[0].path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&info.stdout).contains("not encrypted"));
}

// ---- Sanitize -----------------------------------------------------------------------------

#[test]
fn sanitize_removes_hidden_and_risky_content_but_keeps_what_you_see() {
    let lab = Lab::new();
    let source = lab.doc("risky.pdf", &pdfs::pdf_with_risky_content());
    let out = lab
        .run(ToolOptions::Sanitize(SanitizeOptions::default()), source)
        .unwrap();
    assert_eq!(name(&out[0]), "risky (sanitized).pdf");
    let inside = lab.expanded(&out[0].path);
    for secret in [
        "Secret title",
        "Secret author",
        "app.alert",
        "attached secret",
        "Hidden text",
        "http://example.com",
        "page secret",
        "<secret/>",
    ] {
        assert!(!inside.contains(secret), "{secret} survived");
    }
    let text = lab.texts(&out[0].path, None).join(" ");
    assert!(
        text.contains("Visible text") && text.contains("Shown layer"),
        "{text}"
    );
    assert!(!text.contains("Hidden text"));
    lab.tidy();
}

#[test]
fn sanitize_with_one_option_leaves_the_rest_alone() {
    let lab = Lab::new();
    let only_scripts = SanitizeOptions {
        metadata: false,
        xmp: false,
        javascript: true,
        embedded_files: false,
        hidden_layers: false,
        form_actions: false,
        thumbnails: false,
        private_data: false,
    };
    let out = lab
        .run(
            ToolOptions::Sanitize(only_scripts),
            lab.doc("risky.pdf", &pdfs::pdf_with_risky_content()),
        )
        .unwrap();
    let inside = lab.expanded(&out[0].path);
    assert!(!inside.contains("app.alert"));
    assert!(inside.contains("Secret title"), "metadata stays");
    assert!(inside.contains("attached secret"), "attachment stays");
}

#[test]
fn sanitize_rejects_a_run_that_removes_nothing() {
    let lab = Lab::new();
    let nothing = SanitizeOptions {
        metadata: false,
        xmp: false,
        javascript: false,
        embedded_files: false,
        hidden_layers: false,
        form_actions: false,
        thumbnails: false,
        private_data: false,
    };
    let err = lab
        .run(
            ToolOptions::Sanitize(nothing),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

// ---- Edit metadata ------------------------------------------------------------------------

#[test]
fn edit_metadata_changes_fields_in_the_information_and_the_xmp_packet() {
    let lab = Lab::new();
    let source = lab.doc(
        "m.pdf",
        &pdfs::pdf_with_metadata_and_xmp("Old title", "Old author"),
    );
    let out = lab
        .run(
            ToolOptions::EditMetadata(MetadataOptions {
                title: Some("Brand new title".into()),
                subject: Some("Budget".into()),
                creation_date: Some("2026-10-19T14:30:00".into()),
                ..MetadataOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "m (metadata edited).pdf");
    let inside = lab.expanded(&out[0].path);
    assert!(inside.contains("Brand new title"));
    assert!(!inside.contains("Old title"), "XMP and information agree");
    assert!(inside.contains("Budget"));
    assert!(inside.contains("D:20261019143000Z"));
    assert!(inside.contains("Old author"), "untouched fields stay");
    assert_eq!(lab.texts(&out[0].path, None), ["Page 1"]);
    lab.tidy();
}

#[test]
fn edit_metadata_can_remove_everything_and_rejects_nothing_to_do() {
    let lab = Lab::new();
    let out = lab
        .run(
            ToolOptions::EditMetadata(MetadataOptions {
                remove_all: true,
                ..MetadataOptions::default()
            }),
            lab.doc("m.pdf", &pdfs::pdf_with_metadata_and_xmp("T", "A")),
        )
        .unwrap();
    let inside = lab.expanded(&out[0].path);
    assert!(
        !inside.contains("/Title") && !inside.contains("/Author") && !inside.contains("xmpmeta")
    );

    let err = lab
        .run(
            ToolOptions::defaults_for(domain::ToolId::EditMetadata),
            lab.doc("n.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

#[test]
fn edit_metadata_rejects_an_impossible_date() {
    let lab = Lab::new();
    let err = lab
        .run(
            ToolOptions::EditMetadata(MetadataOptions {
                creation_date: Some("2026-02-30".into()),
                ..MetadataOptions::default()
            }),
            lab.doc("n.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

// ---- Remove annotations -------------------------------------------------------------------

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn remove_annotations_removes_the_chosen_kinds_only() {
    let lab = Lab::new();
    let out = lab
        .run(
            ToolOptions::RemoveAnnotations(RemoveAnnotationsOptions {
                kinds: vec![AnnotationKind::Highlights, AnnotationKind::Links],
                pages: PageRange::all(),
            }),
            lab.doc("an.pdf", &pdfs::pdf_with_annotations()),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "an (annotations removed).pdf");
    let inside = lab.expanded(&out[0].path);
    assert_eq!(count(&inside, "/Subtype /Highlight"), 0);
    assert_eq!(count(&inside, "/Subtype /Link"), 0);
    assert_eq!(count(&inside, "/Subtype /StrikeOut"), 1);
    assert_eq!(count(&inside, "/Subtype /Text"), 2);
    assert_eq!(count(&inside, "/Subtype /Widget"), 1);
    assert_eq!(lab.texts(&out[0].path, None), ["Page 1", "Page 2"]);
    lab.tidy();
}

#[test]
fn remove_annotations_all_keeps_form_fields_and_respects_the_page_selection() {
    let lab = Lab::new();
    let out = lab
        .run(
            ToolOptions::RemoveAnnotations(RemoveAnnotationsOptions {
                kinds: vec![AnnotationKind::All],
                pages: PageRange::parse("2").unwrap(),
            }),
            lab.doc("an.pdf", &pdfs::pdf_with_annotations()),
        )
        .unwrap();
    let inside = lab.expanded(&out[0].path);
    assert_eq!(
        count(&inside, "/Subtype /Highlight"),
        1,
        "page 1 is untouched"
    );
    assert_eq!(
        count(&inside, "/Subtype /Text"),
        1,
        "only the note on page 1 is left"
    );
    assert_eq!(count(&inside, "/Subtype /Widget"), 1);
}

#[test]
fn remove_annotations_checks_the_page_selection() {
    let lab = Lab::new();
    let err = lab
        .run(
            ToolOptions::RemoveAnnotations(RemoveAnnotationsOptions {
                kinds: vec![AnnotationKind::All],
                pages: PageRange::parse("9").unwrap(),
            }),
            lab.doc("an.pdf", &pdfs::pdf_with_annotations()),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
}

// ---- Flatten ------------------------------------------------------------------------------

#[test]
fn flatten_turns_form_fields_into_page_content() {
    let lab = Lab::new();
    let source = lab.doc(
        "form.pdf",
        &pdfs::pdf_with_form_field("name", "Ada Lovelace"),
    );
    assert!(lab.expanded(&source.path).contains("/Subtype /Widget"));
    let out = lab
        .run(ToolOptions::defaults_for(domain::ToolId::Flatten), source)
        .unwrap();
    assert_eq!(name(&out[0]), "form (flattened).pdf");
    let inside = lab.expanded(&out[0].path);
    assert!(!inside.contains("/Subtype /Widget"), "the field is gone");
    assert_eq!(out[0].pages, Some(1));
    lab.tidy();
}

#[test]
fn flatten_everything_makes_pages_pictures_of_the_same_size() {
    let lab = Lab::new();
    let source = lab.doc(
        "pages.pdf",
        &pdfs::pdf_with_geometry(&[(612, 792, 0), (400, 300, 0), (300, 500, 90)]),
    );
    let sizes = |path: &Path| -> Vec<(f32, f32)> {
        lab.engine
            .page_sizes(path, None)
            .unwrap()
            .iter()
            .map(|s| (s.width_pt.round(), s.height_pt.round()))
            .collect()
    };
    let before = sizes(&source.path);
    let out = lab
        .run(
            ToolOptions::Flatten(FlattenOptions {
                mode: FlattenMode::Everything,
                dpi: FlattenDpi::Dpi75,
            }),
            source,
        )
        .unwrap();
    assert_eq!(out[0].pages, Some(3));
    // The picture shows the page as displayed, so the rotated page comes out turned already.
    assert_eq!(sizes(&out[0].path), [before[0], before[1], (500.0, 300.0)]);
    assert!(
        lab.texts(&out[0].path, None).iter().all(String::is_empty),
        "no text layer is left"
    );
    lab.tidy();
}

#[test]
fn flatten_everything_reads_a_protected_file_with_its_password() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["secret page"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(
            ToolOptions::Flatten(FlattenOptions {
                mode: FlattenMode::Everything,
                dpi: FlattenDpi::Dpi75,
            }),
            input,
        )
        .unwrap();
    assert_eq!(out[0].pages, Some(1));
    assert!(
        lab.engine.open_info(&out[0].path, None).is_ok(),
        "output is not protected"
    );
    lab.tidy();
}

// ---- Cancellation and safety --------------------------------------------------------------

#[test]
fn a_cancelled_run_leaves_no_files_behind() {
    let lab = Lab::new();
    let token = CancellationToken::new();
    token.cancel();
    let request = UnitRequest {
        tool: domain::ToolId::Compress,
        options: ToolOptions::defaults_for(domain::ToolId::Compress),
        inputs: vec![lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]))],
        output_dir: Some(lab.out()),
        naming_pattern: OutputNamingPattern::default(),
    };
    let err = lab
        .runner
        .run_unit(&request, &token, &NoProgress)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn every_new_tool_writes_a_new_file_and_never_replaces_an_earlier_result() {
    let lab = Lab::new();
    let input = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let first = lab
        .run(
            ToolOptions::Linearize(LinearizeOptions::default()),
            input.clone(),
        )
        .unwrap();
    let second = lab
        .run(ToolOptions::Linearize(LinearizeOptions::default()), input)
        .unwrap();
    assert_ne!(first[0].path, second[0].path);
    assert!(first[0].path.exists() && second[0].path.exists());
}
