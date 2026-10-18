//! Maps a region drawn on the displayed page to the unrotated user space that PDF text lives in.

use domain::tools::common::Region;

/// The unrotated page box in user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Left edge.
    pub left: f32,
    /// Bottom edge.
    pub bottom: f32,
    /// Width before rotation.
    pub width: f32,
    /// Height before rotation.
    pub height: f32,
}

/// A rectangle in user space (origin bottom-left, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserRect {
    /// Left edge.
    pub left: f32,
    /// Bottom edge.
    pub bottom: f32,
    /// Right edge.
    pub right: f32,
    /// Top edge.
    pub top: f32,
}

/// Converts a region, given as fractions of the displayed page from its top-left, into user
/// space for a page rotated clockwise by `rotation` degrees (0, 90, 180 or 270).
pub fn to_user_space(region: &Region, rotation: u16, page: Bounds) -> UserRect {
    // Displayed size: width and height swap for quarter turns.
    let (disp_w, disp_h) = if rotation == 90 || rotation == 270 {
        (page.height, page.width)
    } else {
        (page.width, page.height)
    };
    let x0 = region.x * disp_w;
    let x1 = (region.x + region.width) * disp_w;
    let y0 = region.y * disp_h; // distance from the top
    let y1 = (region.y + region.height) * disp_h;

    // Each displayed corner (x from the left, y from the top) maps to a user-space point.
    let map = |x: f32, y: f32| -> (f32, f32) {
        match rotation {
            90 => (y, x),
            180 => (page.width - x, y),
            270 => (page.width - y, page.height - x),
            _ => (x, page.height - y),
        }
    };
    let corners = [map(x0, y0), map(x1, y0), map(x0, y1), map(x1, y1)];
    let min = |f: fn(&(f32, f32)) -> f32| corners.iter().map(f).fold(f32::INFINITY, f32::min);
    let max = |f: fn(&(f32, f32)) -> f32| corners.iter().map(f).fold(f32::NEG_INFINITY, f32::max);
    UserRect {
        left: page.left + min(|c| c.0),
        right: page.left + max(|c| c.0),
        bottom: page.bottom + min(|c| c.1),
        top: page.bottom + max(|c| c.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: Bounds = Bounds {
        left: 0.0,
        bottom: 0.0,
        width: 600.0,
        height: 800.0,
    };
    const REGION: Region = Region {
        x: 0.1,
        y: 0.1,
        width: 0.5,
        height: 0.2,
    };

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn unrotated_regions_flip_the_y_axis() {
        let r = to_user_space(&REGION, 0, PAGE);
        assert!(near(r.left, 60.0) && near(r.right, 360.0));
        assert!(near(r.top, 720.0) && near(r.bottom, 560.0));
    }

    #[test]
    fn quarter_turns_keep_the_region_over_the_same_content() {
        // The region covers the top-left tenth of the displayed page in every rotation, so its
        // user-space corner must be the corner that ends up top-left after rotating.
        let tiny = Region {
            x: 0.0,
            y: 0.0,
            width: 0.1,
            height: 0.1,
        };
        let r90 = to_user_space(&tiny, 90, PAGE);
        // 90 degrees clockwise: the user bottom-left corner is displayed top-left.
        assert!(near(r90.left, 0.0) && near(r90.bottom, 0.0));
        let r180 = to_user_space(&tiny, 180, PAGE);
        // 180: the user bottom-right corner is displayed top-left.
        assert!(near(r180.right, 600.0) && near(r180.bottom, 0.0));
        let r270 = to_user_space(&tiny, 270, PAGE);
        // 270: the user top-right corner is displayed top-left.
        assert!(near(r270.right, 600.0) && near(r270.top, 800.0));
    }

    #[test]
    fn page_origin_offsets_are_applied() {
        let shifted = Bounds {
            left: 10.0,
            bottom: 20.0,
            ..PAGE
        };
        let r = to_user_space(&REGION, 0, shifted);
        assert!(near(r.left, 70.0) && near(r.bottom, 580.0));
    }
}
