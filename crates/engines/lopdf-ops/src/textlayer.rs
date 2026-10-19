//! Invisible text layers for scanned pages (OCR output).
//!
//! The recognizer writes a one-page PDF whose only content is invisible text, sized from the page
//! picture it read. That page becomes a Form XObject drawn over the real page, scaled to the page
//! as it is displayed, so the original picture, its quality and everything else on the page stay
//! as they were and the words can be searched and copied.

use application::AppError;
use domain::{ErrorCode, PageIndex};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::boxes::{display_matrix, displayed_size};
use crate::pageres::{
    add_named, cm, effective_resources, multiply, page_object, set_resources, wrap_content,
};

fn unreadable() -> AppError {
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        "The text recognized on a page could not be added to it.",
    )
}

/// The size of the first page of the overlay in points.
fn overlay_size(overlay: &Document, page: ObjectId) -> Option<(f32, f32)> {
    let mut node = overlay.get_dictionary(page).ok()?;
    for _ in 0..8 {
        if let Ok(Object::Array(b)) = node.get(b"MediaBox") {
            let n: Vec<f32> = b.iter().filter_map(|o| o.as_float().ok()).collect();
            if n.len() == 4 {
                return Some((n[2] - n[0], n[3] - n[1]));
            }
        }
        node = overlay
            .get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?)
            .ok()?;
    }
    None
}

/// Points every reference in `object` at its new number.
fn remap(object: &mut Object, mapping: &BTreeMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(id) => {
            if let Some(new) = mapping.get(id) {
                *id = *new;
            }
        }
        Object::Array(items) => items.iter_mut().for_each(|o| remap(o, mapping)),
        Object::Dictionary(d) => d.iter_mut().for_each(|(_, v)| remap(v, mapping)),
        Object::Stream(stream) => stream.dict.iter_mut().for_each(|(_, v)| remap(v, mapping)),
        _ => {}
    }
}

/// Draws the text-only page at `path` over each listed page.
pub fn add_text_layers(
    doc: &mut Document,
    layers: &[(PageIndex, PathBuf)],
) -> Result<(), AppError> {
    for (index, path) in layers {
        let target = page_object(doc, index.get())?;
        let mut overlay = Document::load(path).map_err(|_| unreadable())?;
        let page = overlay
            .get_pages()
            .values()
            .next()
            .copied()
            .ok_or_else(unreadable)?;
        let (ow, oh) = overlay_size(&overlay, page).ok_or_else(unreadable)?;
        if ow <= 0.0 || oh <= 0.0 {
            return Err(unreadable());
        }
        let content = overlay.get_page_content(page);
        let resources = effective_resources(&overlay, page);

        // Move what the layer needs (its fonts and the like) into the document under new
        // numbers. The overlay's own catalog, page tree and page are left behind.
        let mut skip: Vec<ObjectId> = vec![page];
        if let Ok(root) = overlay.trailer.get(b"Root").and_then(Object::as_reference) {
            skip.push(root);
            if let Ok(pages) = overlay
                .get_dictionary(root)
                .and_then(|c| c.get(b"Pages"))
                .and_then(Object::as_reference)
            {
                skip.push(pages);
            }
        }
        let mut next = doc.max_id;
        let mut mapping: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        for id in overlay.objects.keys().filter(|id| !skip.contains(id)) {
            next += 1;
            mapping.insert(*id, (next, 0));
        }
        let mut renumbered_resources = Object::Dictionary(resources);
        remap(&mut renumbered_resources, &mapping);
        for (old, mut object) in std::mem::take(&mut overlay.objects) {
            if let Some(new) = mapping.get(&old) {
                remap(&mut object, &mapping);
                doc.objects.insert(*new, object);
            }
        }
        doc.max_id = next;

        let form = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), ow.into(), oh.into()],
                "Resources" => renumbered_resources,
            },
            content,
        ));

        let (w, h) = displayed_size(doc, target);
        let fit = [w / ow, 0.0, 0.0, h / oh, 0.0, 0.0];
        let matrix = multiply(fit, display_matrix(doc, target));
        let mut page_resources = effective_resources(doc, target);
        let name = add_named(
            &mut page_resources,
            "XObject",
            "QwOcr",
            Object::Reference(form),
        );
        set_resources(doc, target, page_resources)?;
        wrap_content(
            doc,
            target,
            None,
            Some(format!("q\n{}/{name} Do\nQ\n", cm(matrix)).into_bytes()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::pdfs;

    /// A one-page PDF of `w` by `h` points with invisible text, like the recognizer writes.
    fn overlay(dir: &std::path::Path, w: i64, h: i64, text: &str) -> PathBuf {
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT 3 Tr /F1 12 Tf 20 30 Td ({text}) Tj ET").into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "Contents" => content,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let path = dir.join("layer.pdf");
        doc.save(&path).unwrap();
        path
    }

    #[test]
    fn the_overlay_page_becomes_a_form_scaled_to_the_page_and_its_font_comes_along() {
        let dir = tempfile::tempdir().unwrap();
        // The recognizer read a picture of 1000 by 1300 units; the page is 500 by 650 points.
        let layer = overlay(dir.path(), 1000, 1300, "hello");
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(500, 650, 0)])).unwrap();
        add_text_layers(&mut doc, &[(PageIndex::new(0), layer)]).unwrap();

        let id = doc.get_pages()[&1];
        let content = String::from_utf8_lossy(&doc.get_page_content(id)).into_owned();
        assert!(
            content.contains("0.5 0 0 0.5 0 0 cm") || content.contains("0.5000"),
            "{content}"
        );
        assert!(content.contains("/QwOcr0 Do"), "{content}");
        let resources = effective_resources(&doc, id);
        let Ok(Object::Dictionary(xobjects)) = resources.get(b"XObject") else {
            panic!("no XObject resources");
        };
        let form_id = xobjects.get(b"QwOcr0").unwrap().as_reference().unwrap();
        let form = doc.get_object(form_id).unwrap().as_stream().unwrap();
        let text = String::from_utf8_lossy(&form.content).into_owned();
        assert!(
            text.contains("(hello) Tj") && text.contains("3 Tr"),
            "{text}"
        );
        // The font the form uses exists in the document.
        let Ok(Object::Dictionary(inner)) = form.dict.get(b"Resources") else {
            panic!("form without resources");
        };
        let Ok(Object::Dictionary(fonts)) = inner.get(b"Font") else {
            panic!("form without fonts");
        };
        let font_id = fonts.get(b"F1").unwrap().as_reference().unwrap();
        assert!(doc.get_dictionary(font_id).is_ok());
    }

    #[test]
    fn a_rotated_page_gets_the_layer_in_its_displayed_orientation() {
        let dir = tempfile::tempdir().unwrap();
        let layer = overlay(dir.path(), 200, 300, "x");
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 200, 90)])).unwrap();
        add_text_layers(&mut doc, &[(PageIndex::new(0), layer)]).unwrap();
        let id = doc.get_pages()[&1];
        let content = String::from_utf8_lossy(&doc.get_page_content(id)).into_owned();
        // The displayed page is 200 by 300 like the overlay, so only the rotation matrix shows.
        assert!(content.contains("0 1 -1 0"), "{content}");
    }

    #[test]
    fn an_unreadable_overlay_is_reported_and_leaves_the_page_alone() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.pdf");
        std::fs::write(&junk, b"not a pdf").unwrap();
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 200, 0)])).unwrap();
        let before =
            String::from_utf8_lossy(&doc.get_page_content(doc.get_pages()[&1])).into_owned();
        let err = add_text_layers(&mut doc, &[(PageIndex::new(0), junk)]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
        let after =
            String::from_utf8_lossy(&doc.get_page_content(doc.get_pages()[&1])).into_owned();
        assert_eq!(before, after);
    }
}
