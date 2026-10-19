//! Edit PDF and Sign PDF: text, pictures, shapes, patches, links and visual signatures added to
//! pages, drawn above the page's own content.

use application::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::ErrorCode;
use domain::tools::editor::{EditItem, LinkTarget, SignatureItem, SignatureSource};
use lopdf::{Dictionary, Document, Object};
use std::collections::BTreeMap;

use crate::annot::{add_to_page, rect_array, text_string, user_rect};
use crate::pageres::page_object;
use crate::paint::{Painter, Shared};

fn page_index(page: u32) -> u32 {
    page.saturating_sub(1)
}

/// Adds the items to their pages, in the order given.
pub fn apply_items(doc: &mut Document, items: &[EditItem]) -> Result<(), AppError> {
    let mut by_page: BTreeMap<u32, Vec<&EditItem>> = BTreeMap::new();
    for item in items {
        by_page.entry(item.page()).or_default().push(item);
    }
    let mut shared = Shared::default();
    for (page, items) in by_page {
        let mut painter = Painter::new(doc, page_index(page))?;
        let mut links: Vec<(Dictionary, crate::paint::Boxed)> = Vec::new();
        for item in items {
            match item {
                EditItem::Text(t) => {
                    painter.text(doc, &mut shared, &t.region, &t.text, &t.style, t.align)?;
                }
                EditItem::Image(i) => painter.image(doc, &mut shared, &i.region, &i.path)?,
                EditItem::Shape(s) => painter.shape(s)?,
                EditItem::Whiteout(w) => painter.patch(&w.region, &w.color)?,
                EditItem::Link(l) => {
                    let mut dict = Dictionary::new();
                    match &l.target {
                        LinkTarget::Uri { uri } => {
                            let mut action = Dictionary::new();
                            action.set("S", Object::Name(b"URI".to_vec()));
                            action.set("URI", text_string(uri.trim()));
                            dict.set("A", Object::Dictionary(action));
                        }
                        LinkTarget::Page { page: to } => {
                            let target = page_object(doc, page_index(*to))?;
                            dict.set(
                                "Dest",
                                Object::Array(vec![
                                    Object::Reference(target),
                                    Object::Name(b"Fit".to_vec()),
                                ]),
                            );
                        }
                    }
                    links.push((dict, painter.boxed(&l.region)));
                }
            }
        }
        let frame = painter.frame();
        let page_id = painter.page();
        painter.finish(doc)?;
        for (mut dict, boxed) in links {
            dict.set("Type", Object::Name(b"Annot".to_vec()));
            dict.set("Subtype", Object::Name(b"Link".to_vec()));
            dict.set("Rect", rect_array(user_rect(frame, boxed)));
            dict.set(
                "Border",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(0),
                ]),
            );
            dict.set("F", Object::Integer(4));
            let id = doc.add_object(Object::Dictionary(dict));
            add_to_page(doc, page_id, id)?;
        }
    }
    Ok(())
}

/// Places visual signatures. A date must already be written out as typed text.
pub fn apply_signatures(doc: &mut Document, items: &[SignatureItem]) -> Result<(), AppError> {
    let mut by_page: BTreeMap<u32, Vec<&SignatureItem>> = BTreeMap::new();
    for item in items {
        by_page.entry(item.page).or_default().push(item);
    }
    let mut shared = Shared::default();
    for (page, items) in by_page {
        let mut painter = Painter::new(doc, page_index(page))?;
        for item in items {
            match &item.source {
                SignatureSource::Drawn {
                    strokes,
                    color,
                    width_pt,
                } => painter.strokes(&item.region, strokes, color, *width_pt)?,
                SignatureSource::Typed { text, style } => {
                    painter.text_centered(doc, &mut shared, &item.region, text, style)?;
                }
                SignatureSource::Image { path } => {
                    painter.image(doc, &mut shared, &item.region, path)?;
                }
                SignatureSource::ImageData { data } => {
                    let bytes = STANDARD.decode(data.trim()).map_err(|_| {
                        AppError::new(ErrorCode::OptionInvalid, "A saved picture is damaged.")
                    })?;
                    // Equal pictures share one image object.
                    let key = format!("{}:{}", data.len(), &data[data.len().saturating_sub(32)..]);
                    painter.image_bytes(doc, &mut shared, &item.region, &key, &bytes)?;
                }
                SignatureSource::Date { .. } => {
                    return Err(AppError::new(
                        ErrorCode::Internal,
                        "A date was not written out before it was placed.",
                    ));
                }
            }
        }
        painter.finish(doc)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::common::Region;
    use domain::tools::editor::{
        ImageItem, LinkItem, Point, ShapeItem, ShapeKind, TextAlign, TextItem, TextStyle,
        WhiteoutItem,
    };
    use test_support::minimal_pdf;

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn content_of(doc: &Document, page: u32) -> String {
        let id = doc.get_pages()[&(page + 1)];
        String::from_utf8_lossy(&doc.get_page_content(id)).into_owned()
    }

    fn text(page: u32, t: &str) -> EditItem {
        EditItem::Text(TextItem {
            page,
            region: region(0.1, 0.1, 0.5, 0.1),
            text: t.to_owned(),
            style: TextStyle::default(),
            align: TextAlign::Left,
        })
    }

    #[test]
    fn items_land_on_their_own_pages_above_the_old_content_in_order() {
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        let before = content_of(&doc, 0);
        apply_items(
            &mut doc,
            &[
                text(1, "First"),
                EditItem::Whiteout(WhiteoutItem {
                    page: 1,
                    region: region(0.0, 0.0, 0.2, 0.2),
                    color: "#FFFFFF".to_owned(),
                }),
                text(1, "Second"),
            ],
        )
        .unwrap();
        let after = content_of(&doc, 0);
        assert!(after.find(&before).unwrap() < after.find("(First) Tj").unwrap());
        assert!(after.find("(First) Tj").unwrap() < after.find("1 1 1 rg").unwrap());
        assert!(after.find("1 1 1 rg").unwrap() < after.find("(Second) Tj").unwrap());
        // Page two has no changes.
        assert!(
            !content_of(&doc, 1).contains("Tj")
                || content_of(&doc, 1).matches("Tj").count()
                    == content_of(&Document::load_mem(&minimal_pdf(2)).unwrap(), 1)
                        .matches("Tj")
                        .count()
        );
    }

    #[test]
    fn text_is_placed_from_the_top_left_of_its_box_and_wraps() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        apply_items(
            &mut doc,
            &[EditItem::Text(TextItem {
                page: 1,
                region: region(0.1, 0.1, 0.15, 0.2),
                text: "alpha beta gamma delta".to_owned(),
                style: TextStyle::default(),
                align: TextAlign::Left,
            })],
        )
        .unwrap();
        let c = content_of(&doc, 0);
        assert!(c.matches(" Tj").count() >= 2, "wrapped: {c}");
        // 0.1 of the width from the left: 61.2 points on a Letter page.
        assert!(c.contains("61.2 "), "{c}");
    }

    #[test]
    fn shapes_draw_rectangles_ellipses_lines_and_arrows() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let shape = |kind, fill: Option<&str>| {
            EditItem::Shape(ShapeItem {
                page: 1,
                shape: kind,
                region: region(0.2, 0.2, 0.3, 0.2),
                flip: false,
                stroke_color: "#FF0000".to_owned(),
                stroke_width_pt: 2.0,
                fill_color: fill.map(str::to_owned),
            })
        };
        apply_items(
            &mut doc,
            &[
                shape(ShapeKind::Rectangle, None),
                shape(ShapeKind::Ellipse, Some("#00FF00")),
                shape(ShapeKind::Line, None),
                shape(ShapeKind::Arrow, None),
            ],
        )
        .unwrap();
        let c = content_of(&doc, 0);
        assert!(c.contains(" re S\n"), "{c}");
        assert!(c.contains("h B\n") && c.contains("0 1 0 rg"), "{c}");
        assert_eq!(c.matches(" l S\n").count(), 2, "line and arrow shaft: {c}");
        assert!(c.contains(" l h f\n"), "arrow head: {c}");
        assert!(c.contains("1 0 0 RG"));
    }

    #[test]
    fn links_become_annotations_with_uri_or_page_targets() {
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        apply_items(
            &mut doc,
            &[
                EditItem::Link(LinkItem {
                    page: 1,
                    region: region(0.1, 0.1, 0.3, 0.05),
                    target: LinkTarget::Uri {
                        uri: "https://example.com/a".to_owned(),
                    },
                }),
                EditItem::Link(LinkItem {
                    page: 1,
                    region: region(0.1, 0.3, 0.3, 0.05),
                    target: LinkTarget::Page { page: 2 },
                }),
            ],
        )
        .unwrap();
        let page = doc.get_pages()[&1];
        let annots = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(annots.len(), 2);
        let first = doc
            .get_dictionary(annots[0].as_reference().unwrap())
            .unwrap();
        assert_eq!(first.get(b"Subtype").unwrap().as_name().unwrap(), b"Link");
        let action = first.get(b"A").unwrap().as_dict().unwrap();
        assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"URI");
        let second = doc
            .get_dictionary(annots[1].as_reference().unwrap())
            .unwrap();
        let dest = second.get(b"Dest").unwrap().as_array().unwrap();
        assert_eq!(dest[0].as_reference().unwrap(), doc.get_pages()[&2]);
        // The box is 0.1 from the left and 0.1 from the top of the page.
        let rect = first.get(b"Rect").unwrap().as_array().unwrap();
        let x0 = rect[0].as_float().unwrap();
        assert!((x0 - 61.2).abs() < 0.1, "{x0}");
    }

    #[test]
    fn a_picture_is_embedded_once_for_many_uses_and_a_bad_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("p.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]))
            .save(&png)
            .unwrap();
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        let item = |page| {
            EditItem::Image(ImageItem {
                page,
                region: region(0.1, 0.1, 0.4, 0.4),
                path: png.clone(),
            })
        };
        apply_items(&mut doc, &[item(1), item(2)]).unwrap();
        let images = doc
            .objects
            .values()
            .filter(|o| {
                o.as_stream().is_ok_and(|s| {
                    s.dict
                        .get(b"Subtype")
                        .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
                })
            })
            .count();
        assert_eq!(images, 1);
        assert!(content_of(&doc, 0).contains(" Do"));

        let junk = dir.path().join("junk.png");
        std::fs::write(&junk, b"not a picture").unwrap();
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let err = apply_items(
            &mut doc,
            &[EditItem::Image(ImageItem {
                page: 1,
                region: region(0.1, 0.1, 0.4, 0.4),
                path: junk,
            })],
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn a_character_the_font_lacks_is_named() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let err = apply_items(&mut doc, &[text(1, "caf\u{e9} \u{4e2d}")]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains('\u{4e2d}'), "{}", err.message);
    }

    #[test]
    fn signatures_are_strokes_text_or_pictures_and_dates_must_be_written_out() {
        use domain::tools::editor::DateFormat;
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let item = |source| SignatureItem {
            page: 1,
            region: region(0.1, 0.8, 0.3, 0.1),
            source,
        };
        apply_signatures(
            &mut doc,
            &[
                item(SignatureSource::Drawn {
                    strokes: vec![
                        vec![
                            Point { x: 0.0, y: 0.5 },
                            Point { x: 0.5, y: 0.1 },
                            Point { x: 1.0, y: 0.5 },
                        ],
                        vec![Point { x: 0.3, y: 0.3 }],
                    ],
                    color: "#111827".to_owned(),
                    width_pt: 1.5,
                }),
                item(SignatureSource::Typed {
                    text: "Ada Lovelace".to_owned(),
                    style: TextStyle::default(),
                }),
            ],
        )
        .unwrap();
        let c = content_of(&doc, 0);
        assert!(
            c.contains("1 J 1 j") && c.contains(" l\nS") || c.contains(" l\nS\n"),
            "{c}"
        );
        assert!(c.contains("(Ada Lovelace) Tj"));
        let err = apply_signatures(
            &mut doc,
            &[item(SignatureSource::Date {
                format: DateFormat::Iso,
                style: TextStyle::default(),
            })],
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
    }
}
