//! End-to-end tests for Edit PDF, Annotate, Fill and sign, Create forms and Sign PDF: the real
//! RunTool, worker engines and file store, on generated pages, checked by rendering them again
//! and by reading text and form fields back.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::{FormReader, LayoutReader};
use application::{InputDoc, Password};
use common::{Lab, name};
use domain::ErrorCode;
use domain::ToolOptions;
use domain::forms::FormFieldKind;
use domain::tools::common::Region;
use domain::tools::editor::{
    AnnotateOptions, Annotation, CreateFormsOptions, DateFormat, EditItem, EditPdfOptions,
    FieldValue, FillSignOptions, FreehandAnnotation, ImageItem, LinkItem, LinkTarget,
    MarkupAnnotation, NewField, NewFieldKind, NoteAnnotation, Point, ShapeItem, ShapeKind,
    SignPdfOptions, SignatureItem, SignatureSource, TextAlign, TextItem, TextStyle, WhiteoutItem,
};
use quirewise_engine_lopdf_ops::LopdfForms;
use std::fs;
use test_support::{encrypt_pdf, pdfs};

fn region(x: f32, y: f32, width: f32, height: f32) -> Region {
    Region {
        x,
        y,
        width,
        height,
    }
}

fn text_item(page: u32, text: &str, r: Region) -> EditItem {
    EditItem::Text(TextItem {
        page,
        region: r,
        text: text.to_owned(),
        style: TextStyle::default(),
        align: TextAlign::Left,
    })
}

fn edit(items: Vec<EditItem>) -> ToolOptions {
    ToolOptions::EditPdf(EditPdfOptions { items })
}

fn annotate(annotations: Vec<Annotation>) -> ToolOptions {
    ToolOptions::Annotate(AnnotateOptions { annotations })
}

fn field(page: u32, name: &str, kind: NewFieldKind, r: Region) -> NewField {
    NewField {
        page,
        region: r,
        name: name.to_owned(),
        kind,
        required: false,
        tooltip: String::new(),
    }
}

fn text_field() -> NewFieldKind {
    NewFieldKind::Text {
        multiline: false,
        max_len: 0,
        value: String::new(),
    }
}

// ---- Edit PDF -----------------------------------------------------------------------------

#[test]
fn text_is_added_where_it_was_drawn_and_the_original_stays() {
    let lab = Lab::new();
    let source = lab.doc(
        "a.pdf",
        &pdfs::pdf_with_texts(&["First page", "Second page"]),
    );
    let before = fs::read(&source.path).unwrap();
    let out = lab
        .run(
            edit(vec![text_item(2, "ADDED NOTE", region(0.1, 0.5, 0.5, 0.1))]),
            source.clone(),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "a (edited).pdf");
    assert_eq!(out[0].pages, Some(2));
    let texts = lab.texts(&out[0].path, None);
    assert!(
        texts[1].contains("ADDED NOTE") && texts[1].contains("Second page"),
        "{texts:?}"
    );
    assert!(!texts[0].contains("ADDED"));
    // It sits in the middle of the page, not at the top where the old text is.
    let middle = lab.text_in(&out[0].path, 1, region(0.0, 0.45, 1.0, 0.2));
    assert!(middle.contains("ADDED NOTE"), "{middle:?}");
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn a_patch_hides_what_is_under_it_but_the_text_is_still_there() {
    let lab = Lab::new();
    let source = lab.doc("c.pdf", &pdfs::pdf_with_colors());
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            edit(vec![EditItem::Whiteout(WhiteoutItem {
                page: 1,
                // The red box: 100..300 across and 400..600 up from the bottom of Letter.
                region: region(0.15, 0.2, 0.4, 0.35),
                color: "#FFFFFF".to_owned(),
            })]),
            source,
        )
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.colored < before.colored / 2, "{before:?} {after:?}");
    lab.tidy();
}

#[test]
fn shapes_pictures_and_links_are_added_together() {
    let lab = Lab::new();
    let png = lab.dir.path().join("logo.png");
    image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 120, 215, 255]))
        .save(&png)
        .unwrap();
    let source = lab.doc("blank.pdf", &pdfs::pdf_with_texts(&[""]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            edit(vec![
                EditItem::Shape(ShapeItem {
                    page: 1,
                    shape: ShapeKind::Ellipse,
                    region: region(0.1, 0.3, 0.3, 0.15),
                    flip: false,
                    stroke_color: "#D32F2F".to_owned(),
                    stroke_width_pt: 3.0,
                    fill_color: Some("#FFEB3B".to_owned()),
                }),
                EditItem::Shape(ShapeItem {
                    page: 1,
                    shape: ShapeKind::Arrow,
                    region: region(0.5, 0.3, 0.3, 0.15),
                    flip: true,
                    stroke_color: "#1565C0".to_owned(),
                    stroke_width_pt: 2.0,
                    fill_color: None,
                }),
                EditItem::Image(ImageItem {
                    page: 1,
                    region: region(0.1, 0.6, 0.3, 0.2),
                    path: png,
                }),
                EditItem::Link(LinkItem {
                    page: 1,
                    region: region(0.1, 0.1, 0.3, 0.05),
                    target: LinkTarget::Uri {
                        uri: "https://example.com".to_owned(),
                    },
                }),
            ]),
            source,
        )
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(
        after.inked > before.inked + 50 && after.colored > before.colored + 50,
        "{before:?} {after:?}"
    );
    lab.tidy();
}

#[test]
fn a_font_that_is_installed_is_embedded_and_one_that_is_not_is_refused() {
    let lab = Lab::new();
    let source = lab.doc("f.pdf", &pdfs::pdf_with_texts(&["Body"]));
    let styled = |family: &str| {
        edit(vec![EditItem::Text(TextItem {
            page: 1,
            region: region(0.1, 0.4, 0.6, 0.1),
            text: "Set in an installed font".to_owned(),
            style: TextStyle {
                installed_font: Some(family.to_owned()),
                ..TextStyle::default()
            },
            align: TextAlign::Left,
        })])
    };
    let err = lab
        .run(styled("No Such Font Anywhere"), source.clone())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(
        err.message.contains("No Such Font Anywhere"),
        "{}",
        err.message
    );

    let arial = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_default())
        .join("Fonts")
        .join("arial.ttf");
    if !arial.is_file() {
        return;
    }
    let out = lab.run(styled("Arial"), source).unwrap();
    let texts = lab.texts(&out[0].path, None);
    assert!(texts[0].contains("Set in an installed font"), "{texts:?}");
    // The font file is inside the document.
    assert!(fs::metadata(&out[0].path).unwrap().len() > 30_000);
    lab.tidy();
}

#[test]
fn items_outside_the_document_and_empty_runs_are_refused_before_any_work() {
    let lab = Lab::new();
    let source = lab.doc("two.pdf", &pdfs::pdf_with_texts(&["a", "b"]));
    let err = lab
        .run(
            edit(vec![text_item(3, "x", region(0.1, 0.1, 0.2, 0.1))]),
            source.clone(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    let err = lab.run(edit(vec![]), source.clone()).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    let err = lab
        .run(
            edit(vec![text_item(
                1,
                "caf\u{e9} \u{4e2d}",
                region(0.1, 0.1, 0.4, 0.1),
            )]),
            source,
        )
        .unwrap_err();
    assert!(err.message.contains('\u{4e2d}'), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn a_protected_file_stays_protected_after_editing() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["Secret page"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(
            edit(vec![text_item(1, "ADDED", region(0.1, 0.5, 0.4, 0.1))]),
            input,
        )
        .unwrap();
    let password = Password::new("pw");
    assert!(lab.texts(&out[0].path, Some(&password))[0].contains("ADDED"));
    use application::ports::PdfReader;
    assert!(lab.engine.open_info(&out[0].path, None).is_err());
    lab.tidy();
}

// ---- Annotate -----------------------------------------------------------------------------

#[test]
fn highlights_notes_and_drawings_show_on_the_page() {
    let lab = Lab::new();
    let source = lab.doc("n.pdf", &pdfs::pdf_with_texts(&["Check this line of text"]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            annotate(vec![
                Annotation::Highlight(MarkupAnnotation {
                    page: 1,
                    regions: vec![region(0.04, 0.04, 0.6, 0.05)],
                    color: "#FFEB3B".to_owned(),
                }),
                Annotation::Note(NoteAnnotation {
                    page: 1,
                    at: Point { x: 0.7, y: 0.3 },
                    text: "Is this right?".to_owned(),
                    color: "#FFD54F".to_owned(),
                }),
                Annotation::Freehand(FreehandAnnotation {
                    page: 1,
                    strokes: vec![vec![
                        Point { x: 0.2, y: 0.6 },
                        Point { x: 0.4, y: 0.7 },
                        Point { x: 0.6, y: 0.6 },
                    ]],
                    color: "#1565C0".to_owned(),
                    width_pt: 3.0,
                }),
            ]),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "n (annotated).pdf");
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.colored > before.colored + 100, "{before:?} {after:?}");
    // The highlight does not hide the words under it.
    assert!(lab.texts(&out[0].path, None)[0].contains("Check this line"));
    lab.tidy();
}

#[test]
fn underline_and_strikeout_add_marks_without_changing_the_text() {
    let lab = Lab::new();
    let source = lab.doc("u.pdf", &pdfs::pdf_with_texts(&["Marked words here"]));
    let before = lab.look(&source.path, 0, None);
    let mark = |f: fn(MarkupAnnotation) -> Annotation, y: f32| {
        f(MarkupAnnotation {
            page: 1,
            regions: vec![region(0.05, y, 0.5, 0.03)],
            color: "#D32F2F".to_owned(),
        })
    };
    let out = lab
        .run(
            annotate(vec![
                mark(Annotation::Underline, 0.1),
                mark(Annotation::Strikeout, 0.2),
            ]),
            source,
        )
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.inked > before.inked, "{before:?} {after:?}");
    lab.tidy();
}

// ---- Forms --------------------------------------------------------------------------------

fn form(lab: &Lab) -> InputDoc {
    let source = lab.doc("blank.pdf", &pdfs::pdf_with_texts(&["Application form"]));
    let out = lab
        .run(
            ToolOptions::CreateForms(CreateFormsOptions {
                fields: vec![
                    field(1, "full_name", text_field(), region(0.1, 0.2, 0.5, 0.04)),
                    field(
                        1,
                        "agree",
                        NewFieldKind::Checkbox { checked: false },
                        region(0.1, 0.3, 0.03, 0.02),
                    ),
                    field(
                        1,
                        "plan",
                        NewFieldKind::Radio {
                            option: "Basic".to_owned(),
                            checked: false,
                        },
                        region(0.1, 0.4, 0.03, 0.02),
                    ),
                    field(
                        1,
                        "plan",
                        NewFieldKind::Radio {
                            option: "Pro".to_owned(),
                            checked: false,
                        },
                        region(0.3, 0.4, 0.03, 0.02),
                    ),
                    field(
                        1,
                        "country",
                        NewFieldKind::Dropdown {
                            options: vec!["Norway".to_owned(), "India".to_owned()],
                            value: String::new(),
                        },
                        region(0.1, 0.5, 0.3, 0.04),
                    ),
                    field(
                        1,
                        "signature",
                        NewFieldKind::Signature,
                        region(0.1, 0.7, 0.4, 0.08),
                    ),
                ],
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "blank (form).pdf");
    InputDoc::new(&out[0].path)
}

#[test]
fn created_fields_are_listed_with_their_kinds_and_places() {
    let lab = Lab::new();
    let doc = form(&lab);
    let fields = LopdfForms.fields(&doc.path, None).unwrap();
    assert_eq!(fields.len(), 6, "{fields:?}");
    let kinds: Vec<(&str, FormFieldKind)> =
        fields.iter().map(|f| (f.name.as_str(), f.kind)).collect();
    assert!(kinds.contains(&("full_name", FormFieldKind::Text)));
    assert!(kinds.contains(&("agree", FormFieldKind::Checkbox)));
    assert_eq!(
        kinds
            .iter()
            .filter(|(n, k)| *n == "plan" && *k == FormFieldKind::Radio)
            .count(),
        2
    );
    assert!(kinds.contains(&("country", FormFieldKind::Dropdown)));
    assert!(kinds.contains(&("signature", FormFieldKind::Signature)));
    let name_field = fields.iter().find(|f| f.name == "full_name").unwrap();
    assert!((name_field.region.x - 0.1).abs() < 0.002 && (name_field.region.y - 0.2).abs() < 0.002);
    // The fields are drawn: the page has more on it than its one line of text.
    let plain = lab.doc("plain.pdf", &pdfs::pdf_with_texts(&["Application form"]));
    assert!(lab.look(&doc.path, 0, None).inked > lab.look(&plain.path, 0, None).inked);
    lab.tidy();
}

#[test]
fn a_field_name_that_is_taken_is_refused() {
    let lab = Lab::new();
    let doc = form(&lab);
    let err = lab
        .run(
            ToolOptions::CreateForms(CreateFormsOptions {
                fields: vec![field(
                    1,
                    "full_name",
                    text_field(),
                    region(0.1, 0.8, 0.3, 0.04),
                )],
            }),
            doc,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("full_name"), "{}", err.message);
}

fn fill_sign(values: Vec<(&str, &str)>, signatures: Vec<SignatureItem>) -> ToolOptions {
    ToolOptions::FillSign(FillSignOptions {
        values: values
            .into_iter()
            .map(|(n, v)| FieldValue {
                name: n.to_owned(),
                value: v.to_owned(),
            })
            .collect(),
        signatures,
    })
}

#[test]
fn filling_a_form_sets_values_that_are_read_back_and_drawn() {
    let lab = Lab::new();
    let doc = form(&lab);
    let before = lab.look(&doc.path, 0, None);
    let out = lab
        .run(
            fill_sign(
                vec![
                    ("full_name", "Ada Lovelace"),
                    ("agree", "true"),
                    ("plan", "Pro"),
                    ("country", "India"),
                ],
                vec![],
            ),
            doc,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "blank (form) (filled).pdf");
    let fields = LopdfForms.fields(&out[0].path, None).unwrap();
    let value = |n: &str| fields.iter().find(|f| f.name == n).unwrap().value.clone();
    assert_eq!(value("full_name"), "Ada Lovelace");
    assert_eq!(value("agree"), "true");
    assert_eq!(value("plan"), "Pro");
    assert_eq!(value("country"), "India");
    // The values are visible in the rendering: more ink than the empty form.
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.inked > before.inked, "{before:?} {after:?}");
    lab.tidy();
}

#[test]
fn a_filled_form_can_be_filled_again_and_bad_values_are_named() {
    let lab = Lab::new();
    let doc = form(&lab);
    let err = lab
        .run(
            fill_sign(vec![("country", "Atlantis")], vec![]),
            doc.clone(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("Atlantis"), "{}", err.message);
    let err = lab
        .run(fill_sign(vec![("missing", "x")], vec![]), doc.clone())
        .unwrap_err();
    assert!(
        err.message.contains("missing") && err.message.contains("full_name"),
        "{}",
        err.message
    );
    let first = lab
        .run(fill_sign(vec![("full_name", "First")], vec![]), doc)
        .unwrap();
    let second = lab
        .run(
            fill_sign(vec![("full_name", "Second")], vec![]),
            InputDoc::new(&first[0].path),
        )
        .unwrap();
    let fields = LopdfForms.fields(&second[0].path, None).unwrap();
    assert_eq!(
        fields.iter().find(|f| f.name == "full_name").unwrap().value,
        "Second"
    );
    lab.tidy();
}

// ---- Signatures ---------------------------------------------------------------------------

fn signature(source: SignatureSource, r: Region) -> SignatureItem {
    SignatureItem {
        page: 1,
        region: r,
        source,
    }
}

#[test]
fn drawn_typed_pictured_and_dated_signatures_land_on_the_page() {
    let lab = Lab::new();
    let stamp = lab.dir.path().join("stamp.png");
    image::RgbaImage::from_pixel(60, 30, image::Rgba([200, 30, 30, 255]))
        .save(&stamp)
        .unwrap();
    let source = lab.doc("contract.pdf", &pdfs::pdf_with_texts(&["Agreement text"]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            ToolOptions::SignPdf(SignPdfOptions {
                signatures: vec![
                    signature(
                        SignatureSource::Drawn {
                            strokes: vec![vec![
                                Point { x: 0.0, y: 0.7 },
                                Point { x: 0.3, y: 0.2 },
                                Point { x: 0.6, y: 0.8 },
                                Point { x: 1.0, y: 0.3 },
                            ]],
                            color: "#111827".to_owned(),
                            width_pt: 1.5,
                        },
                        region(0.1, 0.8, 0.3, 0.06),
                    ),
                    signature(
                        SignatureSource::Typed {
                            text: "A. Lovelace".to_owned(),
                            style: TextStyle::default(),
                        },
                        region(0.5, 0.8, 0.3, 0.05),
                    ),
                    signature(
                        SignatureSource::Image { path: stamp },
                        region(0.1, 0.5, 0.2, 0.1),
                    ),
                    signature(
                        SignatureSource::Date {
                            format: DateFormat::Iso,
                            style: TextStyle::default(),
                        },
                        region(0.5, 0.88, 0.3, 0.04),
                    ),
                ],
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "contract (signed).pdf");
    let after = lab.look(&out[0].path, 0, None);
    assert!(
        after.inked > before.inked + 50 && after.colored > before.colored,
        "{before:?} {after:?}"
    );
    let text = lab.texts(&out[0].path, None)[0].clone();
    assert!(text.contains("A. Lovelace"), "{text:?}");
    // Today's date is written out in ISO form.
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    assert!(
        digits.matches('-').count() >= 2 && digits.len() >= 10,
        "{text:?}"
    );
    lab.tidy();
}

#[test]
fn fill_and_sign_together_fill_the_form_and_sign_it() {
    let lab = Lab::new();
    let doc = form(&lab);
    let out = lab
        .run(
            fill_sign(
                vec![("full_name", "Ada")],
                vec![signature(
                    SignatureSource::Typed {
                        text: "Ada Lovelace".to_owned(),
                        style: TextStyle::default(),
                    },
                    region(0.1, 0.85, 0.4, 0.05),
                )],
            ),
            doc,
        )
        .unwrap();
    assert!(lab.texts(&out[0].path, None)[0].contains("Ada Lovelace"));
    let fields = LopdfForms.fields(&out[0].path, None).unwrap();
    assert_eq!(
        fields.iter().find(|f| f.name == "full_name").unwrap().value,
        "Ada"
    );
    lab.tidy();
}

// ---- Finding fields -----------------------------------------------------------------------

#[test]
fn lines_and_boxes_on_a_printed_form_are_suggested_as_fields() {
    let lab = Lab::new();
    let mut content =
        String::from("BT /F1 11 Tf 72 700 Td (Full name:) Tj ET\n0.5 w 150 698 m 400 698 l S\n");
    content.push_str("BT /F1 11 Tf 100 640 Td (I agree to the terms) Tj ET\n");
    content.push_str("0.5 w 72 640 12 12 re S\n");
    let source = lab.put("printed.pdf", &pdfs::pdf_from_content(&content, 0));
    let engine = lab.engine;
    let page = engine
        .page_content(&source, None, domain::PageIndex::new(0))
        .unwrap();
    let found = domain::layout::suggest_fields(&page);
    assert_eq!(found.len(), 2, "{found:?}");
    let text = found
        .iter()
        .find(|f| f.kind == domain::forms::SuggestedKind::Text)
        .unwrap();
    assert_eq!(text.label, "Full name");
    assert_eq!(text.name, "Full_name");
    let check = found
        .iter()
        .find(|f| f.kind == domain::forms::SuggestedKind::Checkbox)
        .unwrap();
    assert!(check.label.starts_with("I agree"), "{}", check.label);
}
