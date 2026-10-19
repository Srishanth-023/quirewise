//! Grayscale for page content: colored text, lines and fills become shades of gray.
//!
//! Colors set with the device color operators are rewritten, and so are colors set in a color
//! space with 3 or 4 components (RGB, CMYK, ICC based). Pictures are converted by the raster
//! engine. Shadings, patterns, inline images, spot colors and annotation appearances keep their
//! color; the tool offers a raster fallback for those.

use application::AppError;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashSet;

use crate::pageres::effective_resources;

/// Largest content stream rewritten (after decoding).
const MAX_CONTENT_BYTES: usize = 256 * 1024 * 1024;
/// Deepest nesting of forms inside forms.
const MAX_FORM_DEPTH: usize = 12;

/// How many numbers the current color takes, when it is a color we can turn gray.
type Components = Option<usize>;

/// Brightness of an RGB color, 0 to 1.
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 1.0)
}

fn cmyk_luminance(c: f32, m: f32, y: f32, k: f32) -> f32 {
    let (c, m, y, k) = (
        c.clamp(0.0, 1.0),
        m.clamp(0.0, 1.0),
        y.clamp(0.0, 1.0),
        k.clamp(0.0, 1.0),
    );
    luminance(
        (1.0 - c) * (1.0 - k),
        (1.0 - m) * (1.0 - k),
        (1.0 - y) * (1.0 - k),
    )
}

fn floats(operands: &[Object]) -> Option<Vec<f32>> {
    operands.iter().map(|o| o.as_float().ok()).collect()
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        other => other,
    }
}

/// How many components a color space takes, for the spaces that can be turned gray.
fn space_components(
    doc: &Document,
    resources: &Dictionary,
    name: &[u8],
    depth: usize,
) -> Components {
    match name {
        b"DeviceGray" | b"G" | b"CalGray" => return Some(1),
        b"DeviceRGB" | b"RGB" | b"CalRGB" => return Some(3),
        b"DeviceCMYK" | b"CMYK" => return Some(4),
        b"Pattern" => return None,
        _ => {}
    }
    if depth > 4 {
        return None;
    }
    let spaces = match resources.get(b"ColorSpace").ok().map(|o| resolve(doc, o)) {
        Some(Object::Dictionary(d)) => d,
        _ => return None,
    };
    let entry = resolve(doc, spaces.get(name).ok()?);
    match entry {
        Object::Name(n) => space_components(doc, resources, n, depth + 1),
        Object::Array(items) => {
            let family = items.first().and_then(|o| o.as_name().ok())?;
            match family {
                b"ICCBased" => {
                    let stream = resolve(doc, items.get(1)?).as_stream().ok()?;
                    match stream.dict.get(b"N").ok()?.as_i64().ok()? {
                        1 => Some(1),
                        3 => Some(3),
                        4 => Some(4),
                        _ => None,
                    }
                }
                b"CalGray" => Some(1),
                b"CalRGB" => Some(3),
                _ => None,
            }
        }
        _ => None,
    }
}

fn real(v: f32) -> Object {
    Object::Real(v)
}

/// Rewrites the colors of a list of operations. Returns whether anything changed.
fn rewrite(doc: &Document, resources: &Dictionary, ops: &mut [Operation]) -> bool {
    let mut changed = false;
    let mut fill: Components = Some(1);
    let mut stroke: Components = Some(1);
    let mut stack: Vec<(Components, Components)> = Vec::new();
    for op in ops.iter_mut() {
        match op.operator.as_str() {
            "q" => stack.push((fill, stroke)),
            "Q" => {
                if let Some((f, s)) = stack.pop() {
                    fill = f;
                    stroke = s;
                }
            }
            "g" => fill = Some(1),
            "G" => stroke = Some(1),
            "rg" | "RG" => {
                if let Some([r, g, b]) = floats(&op.operands)
                    .as_deref()
                    .and_then(|v| <[f32; 3]>::try_from(v).ok())
                {
                    let l = luminance(r, g, b);
                    let fill_op = op.operator == "rg";
                    op.operator = if fill_op { "g" } else { "G" }.to_owned();
                    op.operands = vec![real(l)];
                    if fill_op {
                        fill = Some(1);
                    } else {
                        stroke = Some(1);
                    }
                    changed = true;
                }
            }
            "k" | "K" => {
                if let Some([c, m, y, k]) = floats(&op.operands)
                    .as_deref()
                    .and_then(|v| <[f32; 4]>::try_from(v).ok())
                {
                    let l = cmyk_luminance(c, m, y, k);
                    let fill_op = op.operator == "k";
                    op.operator = if fill_op { "g" } else { "G" }.to_owned();
                    op.operands = vec![real(l)];
                    if fill_op {
                        fill = Some(1);
                    } else {
                        stroke = Some(1);
                    }
                    changed = true;
                }
            }
            "cs" | "CS" => {
                let components = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .and_then(|n| space_components(doc, resources, n, 0));
                if op.operator == "cs" {
                    fill = components;
                } else {
                    stroke = components;
                }
            }
            "sc" | "scn" | "SC" | "SCN" => {
                let current = if op.operator.starts_with('s') {
                    fill
                } else {
                    stroke
                };
                let (Some(n), Some(values)) = (current, floats(&op.operands)) else {
                    continue;
                };
                if values.len() != n {
                    continue;
                }
                match n {
                    3 => {
                        let l = luminance(values[0], values[1], values[2]);
                        op.operands = vec![real(l), real(l), real(l)];
                        changed = true;
                    }
                    4 => {
                        let l = cmyk_luminance(values[0], values[1], values[2], values[3]);
                        op.operands = vec![real(0.0), real(0.0), real(0.0), real(1.0 - l)];
                        changed = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    changed
}

fn rewrite_bytes(doc: &Document, resources: &Dictionary, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > MAX_CONTENT_BYTES {
        return None;
    }
    let mut content = Content::decode(bytes).ok()?;
    if !rewrite(doc, resources, &mut content.operations) {
        return None;
    }
    content.encode().ok()
}

/// Forms drawn from `resources`, with the resources they use.
fn forms_of(doc: &Document, resources: &Dictionary) -> Vec<ObjectId> {
    let table = match resources.get(b"XObject").ok().map(|o| resolve(doc, o)) {
        Some(Object::Dictionary(d)) => d,
        _ => return Vec::new(),
    };
    table
        .iter()
        .filter_map(|(_, value)| match value {
            Object::Reference(id) => Some(*id),
            _ => None,
        })
        .filter(|id| {
            matches!(doc.get_object(*id), Ok(Object::Stream(s))
                if s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Form"))
        })
        .collect()
}

fn process_form(
    doc: &mut Document,
    id: ObjectId,
    outer: &Dictionary,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
) {
    if depth > MAX_FORM_DEPTH || !seen.insert(id) {
        return;
    }
    let Ok(Object::Stream(stream)) = doc.get_object(id) else {
        return;
    };
    let own = stream
        .dict
        .get(b"Resources")
        .ok()
        .map(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_else(|| outer.clone());
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let nested = forms_of(doc, &own);
    let replacement = rewrite_bytes(doc, &own, &bytes);
    if let Some(new_bytes) = replacement
        && let Ok(Object::Stream(s)) = doc.get_object_mut(id)
    {
        s.dict.remove(b"Filter");
        s.dict.remove(b"DecodeParms");
        s.set_plain_content(new_bytes);
    }
    for form in nested {
        process_form(doc, form, &own, seen, depth + 1);
    }
}

/// Turns the colors in the content of every page, and of the forms the pages draw, gray.
pub fn grayscale_content(doc: &mut Document) -> Result<(), AppError> {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    for page in pages {
        let resources = effective_resources(doc, page);
        if let Ok(bytes) = doc.get_page_content_with_limit(page, MAX_CONTENT_BYTES)
            && let Some(new_bytes) = rewrite_bytes(doc, &resources, &bytes)
        {
            let stream = doc.add_object(Stream::new(Dictionary::new(), new_bytes));
            doc.get_dictionary_mut(page)
                .map_err(|_| AppError::internal("editing a page"))?
                .set("Contents", Object::Reference(stream));
        }
        for form in forms_of(doc, &resources) {
            process_form(doc, form, &resources, &mut seen, 0);
        }
    }
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::minimal_pdf;

    fn doc_with(content: &str) -> Document {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let id = doc.get_pages()[&1];
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Contents", Object::Reference(stream));
        doc
    }

    fn content_of(doc: &Document) -> String {
        let id = doc.get_pages()[&1];
        String::from_utf8_lossy(&doc.get_page_content(id)).into_owned()
    }

    fn gray_values(text: &str, operator: &str) -> Vec<f32> {
        text.lines()
            .filter(|l| l.trim().ends_with(&format!(" {operator}")) || l.trim().ends_with(operator))
            .filter_map(|l| l.split_whitespace().next())
            .filter_map(|v| v.parse().ok())
            .collect()
    }

    #[test]
    fn device_colors_become_gray_with_the_standard_weights() {
        let mut doc = doc_with("1 0 0 rg\n0 0 1 RG\n0 1 0 0 k\n0.5 g\n");
        grayscale_content(&mut doc).unwrap();
        let text = content_of(&doc);
        assert!(
            !text.contains(" rg") && !text.contains(" RG") && !text.contains(" k\n"),
            "{text}"
        );
        let g = gray_values(&text, "g");
        assert!((g[0] - 0.299).abs() < 1e-3, "{g:?}");
        assert!((g[1] - 0.413).abs() < 1e-3, "magenta in CMYK: {g:?}");
        assert!((g[2] - 0.5).abs() < 1e-3, "an existing gray is kept");
        let stroke = gray_values(&text, "G");
        assert!((stroke[0] - 0.114).abs() < 1e-3, "{stroke:?}");
    }

    #[test]
    fn cmyk_black_stays_black_and_white_stays_white() {
        let mut doc = doc_with("0 0 0 1 k\n0 0 0 0 k\n");
        grayscale_content(&mut doc).unwrap();
        let g = gray_values(&content_of(&doc), "g");
        assert!(g[0].abs() < 1e-4 && (g[1] - 1.0).abs() < 1e-4, "{g:?}");
    }

    #[test]
    fn colors_in_an_rgb_space_keep_the_space_and_become_neutral() {
        let mut doc = doc_with("/DeviceRGB cs\n0.2 0.4 0.6 sc\n/DeviceRGB CS\n1 0 0 SCN\n");
        grayscale_content(&mut doc).unwrap();
        let text = content_of(&doc);
        let sc: Vec<f32> = text
            .lines()
            .find(|l| l.ends_with(" sc"))
            .unwrap()
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        assert_eq!(sc.len(), 3);
        assert!(
            (sc[0] - sc[1]).abs() < 1e-5 && (sc[1] - sc[2]).abs() < 1e-5,
            "{sc:?}"
        );
        assert!((sc[0] - 0.363).abs() < 1e-3);
        let scn: Vec<f32> = text
            .lines()
            .find(|l| l.ends_with(" SCN"))
            .unwrap()
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        assert!((scn[0] - 0.299).abs() < 1e-3 && (scn[1] - 0.299).abs() < 1e-3);
    }

    #[test]
    fn spot_and_pattern_colors_are_left_alone() {
        let mut doc = doc_with("/Pattern cs\n/P1 scn\n/Sep cs\n0.5 scn\n");
        grayscale_content(&mut doc).unwrap();
        let text = content_of(&doc);
        assert!(text.contains("/P1 scn"));
        assert!(text.contains("0.5 scn"));
    }

    #[test]
    fn graphics_state_restores_the_color_space() {
        // The RGB space is only active inside q ... Q; afterwards sc has one number (gray).
        let mut doc = doc_with("q\n/DeviceRGB cs\n1 1 1 sc\nQ\n0.7 sc\n");
        grayscale_content(&mut doc).unwrap();
        assert!(content_of(&doc).contains("0.7 sc"));
    }

    #[test]
    fn forms_are_rewritten_too_and_unchanged_pages_stay_as_they_were() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let id = doc.get_pages()[&1];
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Form".to_vec()));
        dict.set(
            "BBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(10),
                Object::Integer(10),
            ]),
        );
        let form = doc.add_object(Stream::new(dict, b"0 1 0 rg 0 0 10 10 re f".to_vec()));
        let mut table = Dictionary::new();
        table.set("Fm0", Object::Reference(form));
        let mut resources = Dictionary::new();
        resources.set("XObject", Object::Dictionary(table));
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Resources", Object::Dictionary(resources));
        let page_stream = doc.add_object(Stream::new(Dictionary::new(), b"/Fm0 Do".to_vec()));
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Contents", Object::Reference(page_stream));
        grayscale_content(&mut doc).unwrap();
        let Object::Stream(s) = doc.get_object(form).unwrap() else {
            panic!("form missing");
        };
        let text = String::from_utf8_lossy(&s.content).into_owned();
        assert!(text.contains(" g") && !text.contains(" rg"), "{text}");
        assert_eq!(
            content_of(&doc).trim(),
            "/Fm0 Do",
            "a page without colors is untouched"
        );
    }
}
