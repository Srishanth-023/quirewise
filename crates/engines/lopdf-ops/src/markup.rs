//! Annotate: highlights, underlines, strikeouts, sticky notes and freehand drawings, written as
//! real annotations with their own appearance, so every reader shows them the same way and the
//! person can still select, move or delete them.

use application::AppError;
use domain::tools::common::Region;
use domain::tools::editor::{Annotation, FreehandAnnotation, MarkupAnnotation, NoteAnnotation};
use lopdf::{Dictionary, Document, Object};

use crate::annot::{
    add_to_page, color_array, color_op, form_xobject, rect_array, text_string, to_user, user_rect,
};
use crate::pageres::num;
use crate::paint::{Boxed, Painter};

/// Side of a note's icon, in points.
const NOTE_SIZE: f32 = 20.0;

#[derive(Clone, Copy)]
enum Mark {
    Highlight,
    Underline,
    Strikeout,
}

impl Mark {
    fn subtype(self) -> &'static [u8] {
        match self {
            Self::Highlight => b"Highlight",
            Self::Underline => b"Underline",
            Self::Strikeout => b"StrikeOut",
        }
    }
}

/// The four corners of a box in user space: top left, top right, bottom left, bottom right.
fn quad(frame: [f32; 6], b: Boxed) -> [(f32, f32); 4] {
    [
        to_user(frame, b.x, b.y + b.h),
        to_user(frame, b.x + b.w, b.y + b.h),
        to_user(frame, b.x, b.y),
        to_user(frame, b.x + b.w, b.y),
    ]
}

fn union(rects: &[[f32; 4]]) -> [f32; 4] {
    rects.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |a, r| {
            [
                a[0].min(r[0]),
                a[1].min(r[1]),
                a[2].max(r[2]),
                a[3].max(r[3]),
            ]
        },
    )
}

fn markup(
    doc: &mut Document,
    painter: &Painter,
    mark: Mark,
    a: &MarkupAnnotation,
) -> Result<(), AppError> {
    let frame = painter.frame();
    let boxes: Vec<Boxed> = a
        .regions
        .iter()
        .map(|r: &Region| painter.boxed(r))
        .collect();
    let quads: Vec<[(f32, f32); 4]> = boxes.iter().map(|b| quad(frame, *b)).collect();
    let rect = union(
        &boxes
            .iter()
            .map(|b| user_rect(frame, *b))
            .collect::<Vec<_>>(),
    );

    let mut points = Vec::new();
    for q in &quads {
        for (x, y) in q {
            points.push(Object::Real(*x));
            points.push(Object::Real(*y));
        }
    }

    let mut content = String::new();
    let mut resources = Dictionary::new();
    match mark {
        Mark::Highlight => {
            let mut gs = Dictionary::new();
            gs.set("Type", Object::Name(b"ExtGState".to_vec()));
            gs.set("BM", Object::Name(b"Multiply".to_vec()));
            let mut states = Dictionary::new();
            states.set("GS", Object::Dictionary(gs));
            resources.set("ExtGState", Object::Dictionary(states));
            content.push_str("/GS gs\n");
            content.push_str(&color_op(&a.color, false)?);
            for q in &quads {
                content.push_str(&format!(
                    "{} {} m {} {} l {} {} l {} {} l h f\n",
                    num(q[0].0),
                    num(q[0].1),
                    num(q[1].0),
                    num(q[1].1),
                    num(q[3].0),
                    num(q[3].1),
                    num(q[2].0),
                    num(q[2].1)
                ));
            }
        }
        Mark::Underline | Mark::Strikeout => {
            content.push_str(&color_op(&a.color, true)?);
            content.push_str("1 w\n");
            for q in &quads {
                let (from, to) = if matches!(mark, Mark::Underline) {
                    (q[2], q[3])
                } else {
                    (
                        ((q[0].0 + q[2].0) / 2.0, (q[0].1 + q[2].1) / 2.0),
                        ((q[1].0 + q[3].0) / 2.0, (q[1].1 + q[3].1) / 2.0),
                    )
                };
                content.push_str(&format!(
                    "{} {} m {} {} l S\n",
                    num(from.0),
                    num(from.1),
                    num(to.0),
                    num(to.1)
                ));
            }
        }
    }
    let appearance = form_xobject(doc, rect, content, resources);
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(appearance));

    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"Annot".to_vec()));
    dict.set("Subtype", Object::Name(mark.subtype().to_vec()));
    dict.set("Rect", rect_array(rect));
    dict.set("QuadPoints", Object::Array(points));
    dict.set("C", color_array(&a.color)?);
    dict.set("F", Object::Integer(4));
    dict.set("AP", Object::Dictionary(ap));
    let id = doc.add_object(Object::Dictionary(dict));
    add_to_page(doc, painter.page(), id)
}

fn note(doc: &mut Document, painter: &Painter, a: &NoteAnnotation) -> Result<(), AppError> {
    let frame = painter.frame();
    let (x, y) = painter.at(&a.at);
    // The icon hangs from the point, like a pin.
    let icon = Boxed {
        x,
        y: y - NOTE_SIZE,
        w: NOTE_SIZE,
        h: NOTE_SIZE,
    };
    let rect = user_rect(frame, icon);
    let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
    let mut content = String::new();
    content.push_str(&color_op(&a.color, false)?);
    content.push_str("0.2 0.2 0.2 RG 0.75 w\n");
    content.push_str(&format!(
        "{} {} {} {} re B\n",
        num(rect[0] + 0.5),
        num(rect[1] + 0.5),
        num(w - 1.0),
        num(h - 1.0)
    ));
    for k in 1..=3 {
        let ly = rect[1] + h * k as f32 / 4.0;
        content.push_str(&format!(
            "{} {} m {} {} l S\n",
            num(rect[0] + w * 0.2),
            num(ly),
            num(rect[0] + w * 0.8),
            num(ly)
        ));
    }
    let appearance = form_xobject(doc, rect, content, Dictionary::new());
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(appearance));

    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"Annot".to_vec()));
    dict.set("Subtype", Object::Name(b"Text".to_vec()));
    dict.set("Rect", rect_array(rect));
    dict.set("Contents", text_string(&a.text));
    dict.set("Name", Object::Name(b"Note".to_vec()));
    dict.set("C", color_array(&a.color)?);
    // Print, no zoom, no rotate: the icon keeps its size and stays upright.
    dict.set("F", Object::Integer(4 | 8 | 16));
    dict.set("Open", Object::Boolean(false));
    dict.set("AP", Object::Dictionary(ap));
    let id = doc.add_object(Object::Dictionary(dict));
    add_to_page(doc, painter.page(), id)
}

fn freehand(doc: &mut Document, painter: &Painter, a: &FreehandAnnotation) -> Result<(), AppError> {
    let frame = painter.frame();
    let strokes: Vec<Vec<(f32, f32)>> = a
        .strokes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.iter()
                .map(|p| {
                    let (x, y) = painter.at(p);
                    to_user(frame, x, y)
                })
                .collect()
        })
        .collect();
    let all = strokes.iter().flatten();
    let pad = a.width_pt;
    let rect = all.fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |r, (x, y)| {
            [
                r[0].min(*x - pad),
                r[1].min(*y - pad),
                r[2].max(*x + pad),
                r[3].max(*y + pad),
            ]
        },
    );
    let mut content = String::new();
    content.push_str(&color_op(&a.color, true)?);
    content.push_str(&format!("{} w\n1 J 1 j\n", num(a.width_pt)));
    let mut ink = Vec::new();
    for stroke in &strokes {
        let mut list = Vec::new();
        for (i, (x, y)) in stroke.iter().enumerate() {
            content.push_str(&format!(
                "{} {} {}\n",
                num(*x),
                num(*y),
                if i == 0 { "m" } else { "l" }
            ));
            list.push(Object::Real(*x));
            list.push(Object::Real(*y));
        }
        if stroke.len() == 1 {
            content.push_str(&format!("{} {} l\n", num(stroke[0].0), num(stroke[0].1)));
        }
        content.push_str("S\n");
        ink.push(Object::Array(list));
    }
    let appearance = form_xobject(doc, rect, content, Dictionary::new());
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(appearance));
    let mut border = Dictionary::new();
    border.set("W", Object::Real(a.width_pt));

    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"Annot".to_vec()));
    dict.set("Subtype", Object::Name(b"Ink".to_vec()));
    dict.set("Rect", rect_array(rect));
    dict.set("InkList", Object::Array(ink));
    dict.set("C", color_array(&a.color)?);
    dict.set("BS", Object::Dictionary(border));
    dict.set("F", Object::Integer(4));
    dict.set("AP", Object::Dictionary(ap));
    let id = doc.add_object(Object::Dictionary(dict));
    add_to_page(doc, painter.page(), id)
}

/// Adds the annotations to their pages.
pub fn apply_annotations(doc: &mut Document, annotations: &[Annotation]) -> Result<(), AppError> {
    for annotation in annotations {
        let painter = Painter::new(doc, annotation.page().saturating_sub(1))?;
        match annotation {
            Annotation::Highlight(a) => markup(doc, &painter, Mark::Highlight, a)?,
            Annotation::Underline(a) => markup(doc, &painter, Mark::Underline, a)?,
            Annotation::Strikeout(a) => markup(doc, &painter, Mark::Strikeout, a)?,
            Annotation::Note(a) => note(doc, &painter, a)?,
            Annotation::Freehand(a) => freehand(doc, &painter, a)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::editor::Point;
    use test_support::{minimal_pdf, pdfs};

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn annots(doc: &Document, page: u32) -> Vec<Dictionary> {
        let id = doc.get_pages()[&page];
        let list = doc
            .get_dictionary(id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        list.iter()
            .map(|o| {
                doc.get_dictionary(o.as_reference().unwrap())
                    .unwrap()
                    .clone()
            })
            .collect()
    }

    #[test]
    fn a_highlight_has_quad_points_a_multiplying_appearance_and_a_color() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        apply_annotations(
            &mut doc,
            &[Annotation::Highlight(MarkupAnnotation {
                page: 1,
                regions: vec![region(0.1, 0.1, 0.5, 0.02), region(0.1, 0.13, 0.4, 0.02)],
                color: "#FFEB3B".to_owned(),
            })],
        )
        .unwrap();
        let a = &annots(&doc, 1)[0];
        assert_eq!(a.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
        assert_eq!(a.get(b"QuadPoints").unwrap().as_array().unwrap().len(), 16);
        assert_eq!(a.get(b"C").unwrap().as_array().unwrap().len(), 3);
        let ap = a
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object(ap).unwrap().as_stream().unwrap();
        let text = String::from_utf8_lossy(&stream.content);
        assert!(
            text.contains("/GS gs") && text.matches(" h f").count() == 2,
            "{text}"
        );
        let resources = stream.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let gs = resources
            .get(b"ExtGState")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"GS")
            .unwrap()
            .as_dict()
            .unwrap();
        assert_eq!(gs.get(b"BM").unwrap().as_name().unwrap(), b"Multiply");
        // The rectangle holds both lines: 0.1 from the left of a 612 point page.
        let rect = a.get(b"Rect").unwrap().as_array().unwrap();
        assert!((rect[0].as_float().unwrap() - 61.2).abs() < 0.1);
    }

    #[test]
    fn underline_and_strikeout_draw_a_line_in_their_color() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let mark = |kind: fn(MarkupAnnotation) -> Annotation| {
            kind(MarkupAnnotation {
                page: 1,
                regions: vec![region(0.1, 0.1, 0.5, 0.02)],
                color: "#FF0000".to_owned(),
            })
        };
        apply_annotations(
            &mut doc,
            &[mark(Annotation::Underline), mark(Annotation::Strikeout)],
        )
        .unwrap();
        let all = annots(&doc, 1);
        assert_eq!(
            all[0].get(b"Subtype").unwrap().as_name().unwrap(),
            b"Underline"
        );
        assert_eq!(
            all[1].get(b"Subtype").unwrap().as_name().unwrap(),
            b"StrikeOut"
        );
        let line = |a: &Dictionary| {
            let id = a
                .get(b"AP")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"N")
                .unwrap()
                .as_reference()
                .unwrap();
            String::from_utf8_lossy(&doc.get_object(id).unwrap().as_stream().unwrap().content)
                .into_owned()
        };
        assert!(line(&all[0]).contains("1 0 0 RG") && line(&all[0]).contains(" l S"));
        assert_ne!(
            line(&all[0]),
            line(&all[1]),
            "the strikeout runs through the middle"
        );
    }

    #[test]
    fn a_note_keeps_its_text_even_in_unicode_and_stays_upright() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        apply_annotations(
            &mut doc,
            &[Annotation::Note(NoteAnnotation {
                page: 1,
                at: Point { x: 0.5, y: 0.5 },
                text: "Check the total \u{20ac}".to_owned(),
                color: "#FFD54F".to_owned(),
            })],
        )
        .unwrap();
        let a = &annots(&doc, 1)[0];
        assert_eq!(a.get(b"Subtype").unwrap().as_name().unwrap(), b"Text");
        assert_eq!(
            crate::annot::read_text(a.get(b"Contents").unwrap()),
            "Check the total \u{20ac}"
        );
        assert_eq!(a.get(b"F").unwrap().as_i64().unwrap(), 28);
        let rect = a.get(b"Rect").unwrap().as_array().unwrap();
        assert!((rect[2].as_float().unwrap() - rect[0].as_float().unwrap() - 20.0).abs() < 0.01);
    }

    #[test]
    fn a_drawing_has_its_strokes_in_user_space_with_a_padded_rectangle() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        apply_annotations(
            &mut doc,
            &[Annotation::Freehand(FreehandAnnotation {
                page: 1,
                strokes: vec![
                    vec![Point { x: 0.2, y: 0.2 }, Point { x: 0.4, y: 0.3 }],
                    vec![Point { x: 0.6, y: 0.6 }],
                ],
                color: "#1565C0".to_owned(),
                width_pt: 3.0,
            })],
        )
        .unwrap();
        let a = &annots(&doc, 1)[0];
        assert_eq!(a.get(b"Subtype").unwrap().as_name().unwrap(), b"Ink");
        let ink = a.get(b"InkList").unwrap().as_array().unwrap();
        assert_eq!(ink.len(), 2);
        assert_eq!(ink[0].as_array().unwrap().len(), 4);
        let rect = a.get(b"Rect").unwrap().as_array().unwrap();
        // The leftmost point is 0.2 * 612 = 122.4; the rectangle starts one width before it.
        assert!((rect[0].as_float().unwrap() - 119.4).abs() < 0.1);
    }

    #[test]
    fn annotations_follow_a_rotated_page() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(612, 792, 90)])).unwrap();
        apply_annotations(
            &mut doc,
            &[Annotation::Highlight(MarkupAnnotation {
                page: 1,
                // The top left corner of the displayed page.
                regions: vec![region(0.0, 0.0, 0.1, 0.05)],
                color: "#FFEB3B".to_owned(),
            })],
        )
        .unwrap();
        let rect = annots(&doc, 1)[0]
            .get(b"Rect")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let v: Vec<f32> = rect.iter().map(|r| r.as_float().unwrap()).collect();
        // A quarter turn clockwise puts the displayed top left at the user bottom left.
        assert!(v[0] < 1.0 && v[1] < 1.0, "{v:?}");
        assert!(v[2] - v[0] < 50.0 && v[3] - v[1] > 50.0, "{v:?}");
    }
}
