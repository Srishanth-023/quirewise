//! Page geometry edits: resizing pages, flipping them and putting several on one sheet.

use application::AppError;
use domain::PageIndex;
use domain::tools::layout::{FlipDirection, NUpOptions};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::boxes::{display_matrix, displayed_size, rotation, visible_box};
use crate::pageres::{add_named, cm, effective_resources, multiply, page_object, wrap_content};

/// Largest page content (after decoding) put into a sheet.
const MAX_CONTENT_BYTES: usize = 256 * 1024 * 1024;

fn real(v: f32) -> Object {
    Object::Real(v)
}

fn rect(values: [f32; 4]) -> Object {
    Object::Array(values.map(real).to_vec())
}

fn page_dict(doc: &mut Document, id: ObjectId) -> Result<&mut Dictionary, AppError> {
    doc.get_dictionary_mut(id)
        .map_err(|_| AppError::internal("editing a page"))
}

/// The inverse of an affine matrix, or the identity when it cannot be inverted.
fn invert(m: [f32; 6]) -> [f32; 6] {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    [a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)]
}

/// Gives every page in `pages` pages of `width_pt` by `height_pt` (as displayed), with the
/// content scaled to fit inside `margin_pt` of the edges and centered, or kept at its size.
/// Annotations are not moved, so pages with annotations should be flattened first.
pub fn resize_pages(
    doc: &mut Document,
    width_pt: f32,
    height_pt: f32,
    margin_pt: f32,
    scale: bool,
    match_orientation: bool,
) -> Result<(), AppError> {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for id in pages {
        let [x0, y0, x1, y1] = visible_box(doc, id);
        let (w, h) = (x1 - x0, y1 - y0);
        let turned = matches!(rotation(doc, id), 90 | 270);
        let (disp_w, disp_h) = if turned { (h, w) } else { (w, h) };
        if disp_w <= 0.0 || disp_h <= 0.0 {
            continue;
        }
        let (target_w, target_h) = if match_orientation && disp_w > disp_h {
            (height_pt, width_pt)
        } else {
            (width_pt, height_pt)
        };
        // The page is stored unrotated, so the target is turned the same way.
        let (unrot_w, unrot_h) = if turned {
            (target_h, target_w)
        } else {
            (target_w, target_h)
        };
        let factor = if scale {
            ((target_w - 2.0 * margin_pt) / disp_w).min((target_h - 2.0 * margin_pt) / disp_h)
        } else {
            1.0
        };
        let already = (factor - 1.0).abs() < 1e-4
            && (disp_w - target_w).abs() < 0.5
            && (disp_h - target_h).abs() < 0.5
            && x0 == 0.0
            && y0 == 0.0;
        if already {
            continue;
        }
        let tx = (unrot_w - w * factor) / 2.0 - x0 * factor;
        let ty = (unrot_h - h * factor) / 2.0 - y0 * factor;
        wrap_content(
            doc,
            id,
            Some(format!("q\n{}", cm([factor, 0.0, 0.0, factor, tx, ty])).into_bytes()),
            Some(b"Q\n".to_vec()),
        )?;
        let dict = page_dict(doc, id)?;
        dict.set("MediaBox", rect([0.0, 0.0, unrot_w, unrot_h]));
        for key in ["CropBox", "TrimBox", "BleedBox", "ArtBox"] {
            dict.remove(key.as_bytes());
        }
    }
    Ok(())
}

/// Mirrors the listed pages. A page's displayed horizontal axis is its user-space x axis unless
/// the page is turned a quarter, so the mirror is chosen by the rotation. Annotations are not
/// mirrored.
pub fn flip_pages(
    doc: &mut Document,
    direction: FlipDirection,
    pages: &[PageIndex],
) -> Result<(), AppError> {
    for index in pages {
        let id = page_object(doc, index.get())?;
        let [x0, y0, x1, y1] = visible_box(doc, id);
        let quarter = matches!(rotation(doc, id), 90 | 270);
        let mirror_user_x = (direction == FlipDirection::Horizontal) != quarter;
        let matrix = if mirror_user_x {
            [-1.0, 0.0, 0.0, 1.0, x0 + x1, 0.0]
        } else {
            [1.0, 0.0, 0.0, -1.0, 0.0, y0 + y1]
        };
        wrap_content(
            doc,
            id,
            Some(format!("q\n{}", cm(matrix)).into_bytes()),
            Some(b"Q\n".to_vec()),
        )?;
    }
    Ok(())
}

/// Turns the content of pages about the center of the page as the person sees it. The content
/// is wrapped in a matrix, so pictures and text keep their quality; the page keeps its size, and
/// annotations stay where they were.
pub fn deskew_pages(doc: &mut Document, turns: &[(PageIndex, f32)]) -> Result<(), AppError> {
    for (index, degrees) in turns {
        if degrees.abs() < f32::EPSILON {
            continue;
        }
        let id = page_object(doc, index.get())?;
        let (w, h) = displayed_size(doc, id);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let (s, c) = degrees.to_radians().sin_cos();
        // Counter-clockwise about the center, in the displayed frame (x right, y up).
        let turn = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
        let shown = display_matrix(doc, id);
        // User space to the displayed frame, turn, and back to user space.
        let matrix = multiply(multiply(invert(shown), turn), shown);
        wrap_content(
            doc,
            id,
            Some(format!("q\n{}", cm(matrix)).into_bytes()),
            Some(b"Q\n".to_vec()),
        )?;
    }
    Ok(())
}

/// Sets the media box of the listed pages to the crop box they were just given.
pub fn crop_media_box(doc: &mut Document, pages: &[PageIndex]) -> Result<(), AppError> {
    for index in pages {
        let id = page_object(doc, index.get())?;
        let visible = visible_box(doc, id);
        let dict = page_dict(doc, id)?;
        dict.set("MediaBox", rect(visible));
        dict.set("CropBox", rect(visible));
    }
    Ok(())
}

/// Catalog entries that point at pages by identity. They cannot survive a new page tree.
const PAGE_BOUND: [&str; 7] = [
    "Outlines",
    "OpenAction",
    "AcroForm",
    "PageLabels",
    "StructTreeRoot",
    "Dests",
    "Threads",
];

/// Puts several pages on each sheet. The pages become Form XObjects drawn into a grid; links,
/// form fields, bookmarks and page labels do not carry over.
pub fn n_up(doc: &mut Document, options: &NUpOptions) -> Result<(), AppError> {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let Some(first) = pages.first().copied() else {
        return Ok(());
    };
    let (first_w, first_h) = displayed_size(doc, first);
    let (sheet_w, sheet_h) = options.sheet_size(first_w, first_h);
    let cells = options.cells(sheet_w, sheet_h);

    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| AppError::internal("editing the document"))?;
    let pages_root = doc
        .get_dictionary(catalog_id)
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .and_then(|o| o.as_reference().ok())
        .ok_or_else(|| AppError::internal("editing the document"))?;

    // Every source page becomes a form that draws it as displayed.
    let mut forms: Vec<(ObjectId, f32, f32)> = Vec::with_capacity(pages.len());
    for id in &pages {
        let content = doc
            .get_page_content_with_limit(*id, MAX_CONTENT_BYTES)
            .map_err(|_| AppError::internal("reading a page"))?;
        let (dw, dh) = displayed_size(doc, *id);
        let visible = visible_box(doc, *id);
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Form".to_vec()));
        dict.set("BBox", rect(visible));
        dict.set(
            "Resources",
            Object::Dictionary(effective_resources(doc, *id)),
        );
        let form = doc.add_object(Stream::new(dict, content));
        forms.push((form, dw, dh));
    }

    let per_sheet = options.per_sheet.count() as usize;
    let mut kids: Vec<Object> = Vec::new();
    for (sheet_index, group) in forms.chunks(per_sheet).enumerate() {
        let mut resources = Dictionary::new();
        let mut content = String::new();
        for (slot, (form, dw, dh)) in group.iter().enumerate() {
            let Some(cell) = cells.get(slot) else { break };
            let page_id = pages[sheet_index * per_sheet + slot];
            let scale = (cell.width / dw).min(cell.height / dh);
            let place = [
                scale,
                0.0,
                0.0,
                scale,
                cell.x + (cell.width - dw * scale) / 2.0,
                cell.y + (cell.height - dh * scale) / 2.0,
            ];
            let matrix = multiply(invert(display_matrix(doc, page_id)), place);
            let name = add_named(&mut resources, "XObject", "QwP", Object::Reference(*form));
            content.push_str(&format!("q\n{}/{name} Do\nQ\n", cm(matrix)));
        }
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_root));
        page.set(
            "MediaBox",
            Object::Array(vec![real(0.0), real(0.0), real(sheet_w), real(sheet_h)]),
        );
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(content_id));
        kids.push(Object::Reference(doc.add_object(Object::Dictionary(page))));
    }

    let count = i64::try_from(kids.len()).unwrap_or(i64::MAX);
    let root = page_dict(doc, pages_root)?;
    root.set("Kids", Object::Array(kids));
    root.set("Count", Object::Integer(count));
    // The root may have carried inheritable attributes the old pages relied on; the new pages
    // carry their own, so drop them to avoid surprises.
    for key in ["MediaBox", "CropBox", "Rotate", "Resources"] {
        root.remove(key.as_bytes());
    }
    let catalog = page_dict(doc, catalog_id)?;
    for key in PAGE_BOUND {
        catalog.remove(key.as_bytes());
    }
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::layout::{FillOrder, SheetCount};
    use test_support::{minimal_pdf, pdfs};

    fn content_of(doc: &Document, page: u32) -> String {
        let id = doc.get_pages()[&(page + 1)];
        String::from_utf8_lossy(&doc.get_page_content(id)).into_owned()
    }

    fn media(doc: &Document, page: u32) -> [f32; 4] {
        let id = doc.get_pages()[&(page + 1)];
        let [a, b, c, d] = visible_box(doc, id);
        [a, b, c, d]
    }

    fn apply(m: [f32; 6], x: f32, y: f32) -> (f32, f32) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    /// The matrix of the last `cm` on a page.
    fn last_cm(doc: &Document, page: u32) -> [f32; 6] {
        let content = content_of(doc, page);
        let line = content
            .lines()
            .find(|l| l.ends_with(" cm"))
            .expect("a cm line");
        let n: Vec<f32> = line
            .trim_end_matches(" cm")
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        [n[0], n[1], n[2], n[3], n[4], n[5]]
    }

    #[test]
    fn deskew_turns_content_about_the_center_of_the_displayed_page() {
        for rotate in [0u32, 90, 180, 270] {
            let mut doc =
                Document::load_mem(&pdfs::pdf_with_geometry(&[(400, 600, rotate)])).unwrap();
            deskew_pages(&mut doc, &[(PageIndex::new(0), 90.0)]).unwrap();
            let m = last_cm(&doc, 0);
            let id = doc.get_pages()[&1];
            let shown = display_matrix(&doc, id);
            let (w, h) = displayed_size(&doc, id);
            // A point right of the center in the displayed frame ends up above it after a
            // counter-clockwise quarter turn.
            let user_of = |x: f32, y: f32| apply(shown, x, y);
            let before = user_of(w / 2.0 + 100.0, h / 2.0);
            let after = user_of(w / 2.0, h / 2.0 + 100.0);
            let got = apply(m, before.0, before.1);
            assert!(
                (got.0 - after.0).abs() < 1e-2 && (got.1 - after.1).abs() < 1e-2,
                "rotation {rotate}: {got:?} vs {after:?}"
            );
            // The center stays where it is.
            let center = user_of(w / 2.0, h / 2.0);
            let moved = apply(m, center.0, center.1);
            assert!((moved.0 - center.0).abs() < 1e-2 && (moved.1 - center.1).abs() < 1e-2);
        }
    }

    #[test]
    fn deskew_leaves_other_pages_and_zero_angles_alone() {
        let mut doc =
            Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0), (300, 400, 0)])).unwrap();
        let before = content_of(&doc, 1);
        deskew_pages(
            &mut doc,
            &[(PageIndex::new(0), 3.0), (PageIndex::new(1), 0.0)],
        )
        .unwrap();
        assert!(content_of(&doc, 0).contains(" cm"));
        assert_eq!(content_of(&doc, 1), before);
        assert!(deskew_pages(&mut doc, &[(PageIndex::new(9), 1.0)]).is_err());
    }

    #[test]
    fn matrices_invert() {
        let m = [0.0, 1.0, -1.0, 0.0, 300.0, 0.0];
        let product = multiply(m, invert(m));
        for (got, want) in product.iter().zip([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]) {
            assert!((got - want).abs() < 1e-5, "{product:?}");
        }
        assert_eq!(invert([0.0; 6]), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn resizing_scales_the_content_to_fit_and_centers_it() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0)])).unwrap();
        // 300x400 into 600x600 with a 50 pt margin: scale = min(500/300, 500/400) = 1.25.
        resize_pages(&mut doc, 600.0, 600.0, 50.0, true, false).unwrap();
        assert_eq!(media(&doc, 0), [0.0, 0.0, 600.0, 600.0]);
        let c = content_of(&doc, 0);
        // Content 375 wide and 500 high, centered: offsets 112.5 and 50.
        assert!(c.contains("1.25 0 0 1.25 112.5 50 cm"), "{c}");
    }

    #[test]
    fn resizing_without_scaling_only_centers() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0)])).unwrap();
        resize_pages(&mut doc, 600.0, 800.0, 0.0, false, false).unwrap();
        assert!(content_of(&doc, 0).contains("1 0 0 1 150 200 cm"));
    }

    #[test]
    fn landscape_pages_get_a_landscape_target_when_orientation_matches() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(800, 600, 0)])).unwrap();
        resize_pages(&mut doc, 600.0, 800.0, 0.0, true, true).unwrap();
        assert_eq!(media(&doc, 0), [0.0, 0.0, 800.0, 600.0]);
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(800, 600, 0)])).unwrap();
        resize_pages(&mut doc, 600.0, 800.0, 0.0, true, false).unwrap();
        assert_eq!(media(&doc, 0), [0.0, 0.0, 600.0, 800.0]);
    }

    #[test]
    fn a_page_that_already_fits_is_left_alone() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(600, 800, 0)])).unwrap();
        let before = content_of(&doc, 0);
        resize_pages(&mut doc, 600.0, 800.0, 0.0, true, true).unwrap();
        assert_eq!(content_of(&doc, 0), before);
    }

    #[test]
    fn rotated_pages_keep_their_rotation_and_swap_the_stored_target() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 90)])).unwrap();
        // Displayed 400x300 (landscape) into a 600x800 target that turns landscape: 800x600.
        resize_pages(&mut doc, 600.0, 800.0, 0.0, true, true).unwrap();
        // Stored unrotated: 600 wide and 800 high.
        assert_eq!(media(&doc, 0), [0.0, 0.0, 600.0, 800.0]);
        let id = doc.get_pages()[&1];
        assert_eq!(rotation(&doc, id), 90);
    }

    #[test]
    fn horizontal_flip_mirrors_user_x_and_vertical_mirrors_user_y() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0)])).unwrap();
        flip_pages(&mut doc, FlipDirection::Horizontal, &[PageIndex::new(0)]).unwrap();
        assert!(content_of(&doc, 0).contains("-1 0 0 1 300 0 cm"));
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0)])).unwrap();
        flip_pages(&mut doc, FlipDirection::Vertical, &[PageIndex::new(0)]).unwrap();
        assert!(content_of(&doc, 0).contains("1 0 0 -1 0 400 cm"));
    }

    #[test]
    fn flipping_a_turned_page_swaps_the_axes() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 90)])).unwrap();
        flip_pages(&mut doc, FlipDirection::Horizontal, &[PageIndex::new(0)]).unwrap();
        assert!(
            content_of(&doc, 0).contains("1 0 0 -1 0 400 cm"),
            "{}",
            content_of(&doc, 0)
        );
    }

    #[test]
    fn only_the_listed_pages_are_flipped() {
        let mut doc = Document::load_mem(&minimal_pdf(3)).unwrap();
        let before = content_of(&doc, 1);
        flip_pages(
            &mut doc,
            FlipDirection::Horizontal,
            &[PageIndex::new(0), PageIndex::new(2)],
        )
        .unwrap();
        assert_eq!(content_of(&doc, 1), before);
        assert!(content_of(&doc, 2).contains("-1 0 0 1 612 0 cm"));
    }

    #[test]
    fn cropping_the_media_box_follows_the_crop_box() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 0)])).unwrap();
        let id = doc.get_pages()[&1];
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("CropBox", rect([10.0, 20.0, 110.0, 220.0]));
        crop_media_box(&mut doc, &[PageIndex::new(0)]).unwrap();
        let dict = doc.get_dictionary(id).unwrap();
        let media = dict.get(b"MediaBox").unwrap().as_array().unwrap();
        let values: Vec<f32> = media.iter().filter_map(|o| o.as_float().ok()).collect();
        assert_eq!(values, vec![10.0, 20.0, 110.0, 220.0]);
    }

    #[test]
    fn two_up_makes_a_landscape_sheet_for_each_pair_and_drops_page_bound_entries() {
        let pdf = pdfs::pdf_with_outline(5, &[("A", 0, 1), ("B", 3, 1)]);
        let mut doc = Document::load_mem(&pdf).unwrap();
        n_up(&mut doc, &NUpOptions::default()).unwrap();
        assert_eq!(doc.get_pages().len(), 3, "5 pages on 2-up sheets");
        let [x0, y0, x1, y1] = media(&doc, 0);
        assert_eq!(
            (x1 - x0, y1 - y0),
            (792.0, 612.0),
            "a Letter page turned to landscape"
        );
        let c = content_of(&doc, 0);
        assert_eq!(c.matches(" Do").count(), 2, "{c}");
        assert_eq!(
            content_of(&doc, 2).matches(" Do").count(),
            1,
            "the last sheet holds one page"
        );
        let catalog = doc.catalog().unwrap();
        assert!(catalog.get(b"Outlines").is_err());
    }

    #[test]
    fn four_up_in_vertical_order_places_page_two_below_page_one() {
        let mut doc = Document::load_mem(&minimal_pdf(4)).unwrap();
        let options = NUpOptions {
            per_sheet: SheetCount::Four,
            order: FillOrder::Vertical,
            ..NUpOptions::default()
        };
        n_up(&mut doc, &options).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
        let c = content_of(&doc, 0);
        let places: Vec<Vec<f32>> = c
            .lines()
            .filter(|l| l.ends_with(" cm"))
            .map(|l| l.split(' ').filter_map(|v| v.parse().ok()).collect())
            .collect();
        assert_eq!(places.len(), 4);
        // Scale 0.5 on a Letter sheet; slot 0 is top left, slot 1 below it.
        assert!((places[0][0] - 0.5).abs() < 1e-3);
        assert!(
            places[0][4].abs() < 1e-3 && (places[0][5] - 396.0).abs() < 1e-2,
            "{:?}",
            places[0]
        );
        assert!(
            places[1][4].abs() < 1e-3 && places[1][5].abs() < 1e-2,
            "{:?}",
            places[1]
        );
        assert!((places[2][4] - 306.0).abs() < 1e-2);
    }

    #[test]
    fn n_up_places_turned_pages_upright() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 90)])).unwrap();
        n_up(
            &mut doc,
            &NUpOptions {
                per_sheet: SheetCount::Four,
                ..NUpOptions::default()
            },
        )
        .unwrap();
        let c = content_of(&doc, 0);
        // The inverse of the displayed frame of a page turned a quarter has a 90 degree twist.
        let line = c.lines().find(|l| l.ends_with(" cm")).unwrap();
        let m: Vec<f32> = line.split(' ').filter_map(|v| v.parse().ok()).collect();
        assert!(m[0].abs() < 1e-4 && m[3].abs() < 1e-4, "{line}");
    }
}
