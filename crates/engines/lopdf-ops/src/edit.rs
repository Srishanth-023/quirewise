//! Page edits: crop boxes and scaling pages to a size.

use application::AppError;
use application::ports::PdfEdit;
use domain::PageIndex;
use domain::tools::common::FractionBox;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::boxes::{displayed_to_user, rotation, visible_box};

fn real(v: f32) -> Object {
    Object::Real(v)
}

fn rect(values: [f32; 4]) -> Object {
    Object::Array(values.map(real).to_vec())
}

fn page_id(doc: &Document, index: PageIndex) -> Result<ObjectId, AppError> {
    doc.get_pages()
        .get(&(index.get() + 1))
        .copied()
        .ok_or_else(|| AppError::internal("finding a page to edit"))
}

fn page_dict(doc: &mut Document, id: ObjectId) -> Result<&mut Dictionary, AppError> {
    doc.get_dictionary_mut(id)
        .map_err(|_| AppError::internal("editing a page"))
}

/// Sets each page's crop box to a fraction of its visible area. The fractions describe the page
/// as displayed, so rotated pages are handled.
pub fn crop_fractions(
    doc: &mut Document,
    crops: &[(PageIndex, FractionBox)],
) -> Result<(), AppError> {
    for (index, fractions) in crops {
        let id = page_id(doc, *index)?;
        let [x0, y0, x1, y1] = visible_box(doc, id);
        let user = displayed_to_user(*fractions, rotation(doc, id));
        let (w, h) = (x1 - x0, y1 - y0);
        let crop = [
            x0 + user.left * w,
            y0 + user.bottom * h,
            x0 + user.right * w,
            y0 + user.top * h,
        ];
        page_dict(doc, id)?.set("CropBox", rect(crop));
    }
    Ok(())
}

/// The ids of a page's content streams, in order.
pub(crate) fn content_ids(doc: &Document, page: ObjectId) -> Vec<Object> {
    let Ok(dict) = doc.get_dictionary(page) else {
        return Vec::new();
    };
    match dict.get(b"Contents") {
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => items.clone(),
            _ => vec![Object::Reference(*id)],
        },
        Ok(Object::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Scales and centers every page's content onto pages of `width_pt` by `height_pt` as displayed.
/// Annotations are not moved, so pages with annotations should be flattened first.
pub fn scale_pages(doc: &mut Document, width_pt: f32, height_pt: f32) -> Result<(), AppError> {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for id in pages {
        let [x0, y0, x1, y1] = visible_box(doc, id);
        let (w, h) = (x1 - x0, y1 - y0);
        let turned = matches!(rotation(doc, id), 90 | 270);
        let (disp_w, disp_h) = if turned { (h, w) } else { (w, h) };
        let (unrot_w, unrot_h) = if turned {
            (height_pt, width_pt)
        } else {
            (width_pt, height_pt)
        };

        let already = (disp_w - width_pt).abs() < 0.5 && (disp_h - height_pt).abs() < 0.5;
        if already && x0 == 0.0 && y0 == 0.0 {
            continue;
        }
        if disp_w <= 0.0 || disp_h <= 0.0 {
            continue;
        }
        let scale = (width_pt / disp_w).min(height_pt / disp_h);
        let tx = (unrot_w - w * scale) / 2.0 - x0 * scale;
        let ty = (unrot_h - h * scale) / 2.0 - y0 * scale;

        let existing = content_ids(doc, id);
        if !existing.is_empty() {
            let before = doc.add_object(Stream::new(
                Dictionary::new(),
                format!("q {scale:.5} 0 0 {scale:.5} {tx:.3} {ty:.3} cm\n").into_bytes(),
            ));
            let after = doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
            let mut contents = vec![Object::Reference(before)];
            contents.extend(existing);
            contents.push(Object::Reference(after));
            page_dict(doc, id)?.set("Contents", Object::Array(contents));
        }
        let dict = page_dict(doc, id)?;
        dict.set("MediaBox", rect([0.0, 0.0, unrot_w, unrot_h]));
        dict.remove(b"CropBox");
    }
    Ok(())
}

/// Applies an edit to a loaded document.
pub fn apply(doc: &mut Document, edit: &PdfEdit) -> Result<(), AppError> {
    match edit {
        PdfEdit::SetOutline(entries) => crate::outline::set_outline(doc, entries),
        PdfEdit::CropFractions(crops) => crop_fractions(doc, crops),
        PdfEdit::ScalePagesTo {
            width_pt,
            height_pt,
        } => scale_pages(doc, *width_pt, *height_pt),
        PdfEdit::SetMetadata(change) => crate::metadata::set_metadata(doc, change),
        PdfEdit::RemoveAnnotations { kinds, pages } => {
            crate::annotations::remove_annotations(doc, kinds, pages)
        }
        PdfEdit::Sanitize(options) => crate::sanitize::sanitize(doc, options),
        PdfEdit::Stamp(pages) => crate::stamp::apply_stamps(doc, pages),
        PdfEdit::CropPages { crops, media_box } => {
            crop_fractions(doc, crops)?;
            if *media_box {
                let pages: Vec<PageIndex> = crops.iter().map(|(p, _)| *p).collect();
                crate::layout::crop_media_box(doc, &pages)?;
            }
            Ok(())
        }
        PdfEdit::ResizePages {
            width_pt,
            height_pt,
            margin_pt,
            scale,
            match_orientation,
        } => crate::layout::resize_pages(
            doc,
            *width_pt,
            *height_pt,
            *margin_pt,
            *scale,
            *match_orientation,
        ),
        PdfEdit::Flip { direction, pages } => crate::layout::flip_pages(doc, *direction, pages),
        PdfEdit::Sequence(steps) => steps.iter().try_for_each(|step| apply(doc, step)),
        PdfEdit::TextLayers(layers) => crate::textlayer::add_text_layers(doc, layers),
        PdfEdit::Deskew(turns) => crate::layout::deskew_pages(doc, turns),
        PdfEdit::NUp(options) => crate::layout::n_up(doc, options),
        PdfEdit::GrayscaleContent => crate::grayscale::grayscale_content(doc),
        PdfEdit::SetPageLabels(ranges) => crate::labels::set_page_labels(doc, ranges),
        PdfEdit::AddAttachments(paths) => crate::attachments::add(doc, paths),
        PdfEdit::RemoveAttachments(names) => crate::attachments::remove(doc, names),
    }
}
