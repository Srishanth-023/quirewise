//! Reading page boxes and rotation, including values inherited from the page tree.

use domain::tools::common::FractionBox;
use lopdf::{Document, Object, ObjectId};

/// The visible area of a page in user space: `[x0, y0, x1, y1]`.
pub type PageBox = [f32; 4];

/// Letter, used when a page has no usable box at all.
const FALLBACK: PageBox = [0.0, 0.0, 612.0, 792.0];

fn number(doc: &Document, object: &Object) -> Option<f32> {
    match object {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| number(doc, o)),
        _ => None,
    }
}

/// Looks `key` up on the page and then on its ancestors in the page tree.
fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut id = page;
    // The depth cap stops a cyclic /Parent chain in a hostile file.
    for _ in 0..64 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(value);
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn read_box(doc: &Document, page: ObjectId, key: &[u8]) -> Option<PageBox> {
    let object = inherited(doc, page, key)?;
    let resolved = match object {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    let items = resolved.as_array().ok()?;
    let values: Vec<f32> = items.iter().filter_map(|o| number(doc, o)).collect();
    let [a, b, c, d] = <[f32; 4]>::try_from(values).ok()?;
    // PDF allows any two opposite corners, so normalize.
    Some([a.min(c), b.min(d), a.max(c), b.max(d)])
}

/// The visible box: the crop box when present, otherwise the media box.
pub fn visible_box(doc: &Document, page: ObjectId) -> PageBox {
    read_box(doc, page, b"CropBox")
        .or_else(|| read_box(doc, page, b"MediaBox"))
        .unwrap_or(FALLBACK)
}

/// The page rotation in degrees, normalized to 0, 90, 180 or 270.
pub fn rotation(doc: &Document, page: ObjectId) -> u16 {
    let raw = inherited(doc, page, b"Rotate")
        .and_then(|o| number(doc, o))
        .unwrap_or(0.0);
    let turns = (raw / 90.0).round() as i64;
    (turns.rem_euclid(4) * 90) as u16
}

/// Converts a box given as fractions of the displayed page into fractions of the unrotated user
/// space, for a page rotated clockwise by `rotation` degrees.
pub fn displayed_to_user(b: FractionBox, rotation: u16) -> FractionBox {
    match rotation {
        90 => FractionBox {
            left: 1.0 - b.top,
            bottom: b.left,
            right: 1.0 - b.bottom,
            top: b.right,
        },
        180 => FractionBox {
            left: 1.0 - b.right,
            bottom: 1.0 - b.top,
            right: 1.0 - b.left,
            top: 1.0 - b.bottom,
        },
        270 => FractionBox {
            left: b.bottom,
            bottom: 1.0 - b.right,
            right: b.top,
            top: 1.0 - b.left,
        },
        _ => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fb(left: f32, bottom: f32, right: f32, top: f32) -> FractionBox {
        FractionBox {
            left,
            bottom,
            right,
            top,
        }
    }

    #[test]
    fn displayed_left_half_maps_to_the_matching_user_space_half() {
        let left_half = fb(0.0, 0.0, 0.5, 1.0);
        assert_eq!(displayed_to_user(left_half, 0), left_half);
        // A page turned a quarter clockwise shows the user's bottom half on the left.
        assert_eq!(displayed_to_user(left_half, 90), fb(0.0, 0.0, 1.0, 0.5));
        // Upside down, the displayed left half is the user's right half.
        assert_eq!(displayed_to_user(left_half, 180), fb(0.5, 0.0, 1.0, 1.0));
        // Three quarters clockwise shows the user's top half on the left.
        assert_eq!(displayed_to_user(left_half, 270), fb(0.0, 0.5, 1.0, 1.0));
    }

    #[test]
    fn rotations_round_trip_for_whole_pages() {
        let whole = fb(0.0, 0.0, 1.0, 1.0);
        for r in [0, 90, 180, 270] {
            assert_eq!(displayed_to_user(whole, r), whole);
        }
    }
}
