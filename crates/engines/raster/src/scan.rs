//! Reading scans: finding how much a page is tilted, cutting the paper out of a photo, standing a
//! page photographed at an angle upright, and reducing the colors.
//!
//! Everything here works on small copies of the picture for the measuring and on the full picture
//! only for the final cut or warp, so a 600 dpi scan stays quick.

use application::AppError;
use application::ports::{RenderedImage, SkewDetector};
use domain::ErrorCode;
use domain::tools::convert::PictureColor;

use crate::RasterConverter;
use crate::tiffio::luma;

/// Longest edge of the copy used for measuring.
const MEASURE_EDGE: usize = 1000;
/// Most ink points used to measure the tilt.
const MAX_POINTS: usize = 250_000;
/// Fewest ink points that make a measurement worth anything.
const MIN_POINTS: usize = 200;
/// Step of the coarse and the fine search, in degrees.
const COARSE_STEP: f32 = 0.5;
const FINE_STEP: f32 = 0.05;
/// How much sharper the best angle must be than the average one to be believed.
const MIN_CONTRAST: f64 = 1.08;

/// An 8-bit gray picture.
#[derive(Debug, Clone)]
pub(crate) struct Gray {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Gray {
    pub(crate) fn from_rendered(image: &RenderedImage) -> Self {
        Self {
            width: image.width as usize,
            height: image.height as usize,
            data: luma(image),
        }
    }

    fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    /// A smaller copy no longer than `edge` on its longest side, by averaging blocks.
    fn shrunk(&self, edge: usize) -> (Self, usize) {
        let longest = self.width.max(self.height);
        let step = longest.div_ceil(edge).max(1);
        if step == 1 {
            return (self.clone(), 1);
        }
        let (w, h) = (self.width / step, self.height / step);
        let mut data = Vec::with_capacity(w * h);
        for by in 0..h {
            for bx in 0..w {
                let mut sum = 0u32;
                for y in 0..step {
                    let row = (by * step + y) * self.width + bx * step;
                    sum += self.data[row..row + step]
                        .iter()
                        .map(|v| u32::from(*v))
                        .sum::<u32>();
                }
                data.push((sum / (step * step) as u32) as u8);
            }
        }
        (
            Self {
                width: w.max(1),
                height: h.max(1),
                data,
            },
            step,
        )
    }
}

/// The brightness that parts ink from paper, found with Otsu's method.
pub(crate) fn otsu_level(gray: &[u8]) -> u8 {
    let mut histogram = [0u64; 256];
    for g in gray {
        histogram[usize::from(*g)] += 1;
    }
    let total = gray.len() as f64;
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, n)| i as f64 * *n as f64)
        .sum();
    let (mut below, mut sum_below, mut best, mut level) = (0.0f64, 0.0f64, 0.0f64, 127u8);
    for (i, count) in histogram.iter().enumerate() {
        below += *count as f64;
        if below == 0.0 {
            continue;
        }
        let above = total - below;
        if above == 0.0 {
            break;
        }
        sum_below += i as f64 * *count as f64;
        let between = below * above * (sum_below / below - (sum - sum_below) / above).powi(2);
        if between > best {
            best = between;
            level = u8::try_from(i).unwrap_or(255);
        }
    }
    level
}

/// The points of the ink: the darker side of the page when it is the smaller one.
fn ink_points(gray: &Gray) -> Vec<(f32, f32)> {
    let level = otsu_level(&gray.data);
    let dark = gray.data.iter().filter(|g| **g <= level).count();
    let ink_is_dark = dark * 2 <= gray.data.len();
    let stride = (gray.data.len() / MAX_POINTS).max(1);
    let mut points = Vec::new();
    let mut seen = 0usize;
    for y in 0..gray.height {
        for x in 0..gray.width {
            let is_ink = (gray.at(x, y) <= level) == ink_is_dark;
            if is_ink {
                if seen.is_multiple_of(stride) {
                    points.push((x as f32, y as f32));
                }
                seen += 1;
            }
        }
    }
    points
}

/// How sharply the points line up in rows along a tilt of `degrees`: the sum of the squares of
/// the row counts, which is largest when every text line falls into one row.
fn alignment(points: &[(f32, f32)], degrees: f32, rows: &mut Vec<u32>) -> f64 {
    let (s, c) = degrees.to_radians().sin_cos();
    // Projections run from about -width to +height; shift them so they index a table.
    let shift = points
        .iter()
        .map(|(x, y)| (x * s + y * c).floor())
        .fold(f32::INFINITY, f32::min);
    rows.clear();
    for (x, y) in points {
        let row = (x * s + y * c - shift) as usize;
        if row >= rows.len() {
            rows.resize(row + 1, 0);
        }
        rows[row] += 1;
    }
    rows.iter().map(|n| f64::from(*n) * f64::from(*n)).sum()
}

/// The angle that turns the content counter-clockwise until its text lines run level, in degrees,
/// or `None` when the page has too little to go by or no clear lines.
pub(crate) fn skew_correction(image: &RenderedImage, max_angle: f32) -> Option<f32> {
    let (small, _) = Gray::from_rendered(image).shrunk(MEASURE_EDGE);
    let points = ink_points(&small);
    if points.len() < MIN_POINTS {
        return None;
    }
    let mut rows = Vec::new();
    let mut best = (f64::MIN, 0.0f32);
    let mut total = 0.0f64;
    let mut count = 0u32;
    let mut angle = -max_angle;
    while angle <= max_angle + 1e-4 {
        let score = alignment(&points, angle, &mut rows);
        total += score;
        count += 1;
        if score > best.0 {
            best = (score, angle);
        }
        angle += COARSE_STEP;
    }
    let mean = total / f64::from(count.max(1));
    if mean <= 0.0 || best.0 / mean < MIN_CONTRAST {
        return None;
    }
    let start = best.1 - COARSE_STEP;
    let mut fine = (f64::MIN, best.1);
    let mut angle = start;
    while angle <= best.1 + COARSE_STEP + 1e-4 {
        let score = alignment(&points, angle, &mut rows);
        if score > fine.0 {
            fine = (score, angle);
        }
        angle += FINE_STEP;
    }
    // Rows tilted by `fine.1` clockwise on the screen need a counter-clockwise turn to level.
    Some(-fine.1)
}

impl SkewDetector for RasterConverter {
    fn skew(&self, image: &RenderedImage, max_angle_deg: f32) -> Result<Option<f32>, AppError> {
        if !(1.0..=90.0).contains(&max_angle_deg) {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "The largest tilt must be between 1 and 90 degrees.",
            ));
        }
        Ok(skew_correction(image, max_angle_deg))
    }

    fn rotate(&self, image: &RenderedImage, degrees: f32) -> Result<RenderedImage, AppError> {
        Ok(rotate_about_center(image, degrees))
    }
}

/// The picture turned counter-clockwise by `degrees` about its center, white where it no longer
/// reaches. The canvas keeps its size.
pub(crate) fn rotate_about_center(image: &RenderedImage, degrees: f32) -> RenderedImage {
    let (w, h) = (image.width as usize, image.height as usize);
    let (s, c) = f64::from(degrees).to_radians().sin_cos();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let mut rgba = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let (sx, sy) = (dx * c - dy * s + cx - 0.5, dx * s + dy * c + cy - 0.5);
            if sx < -0.5 || sy < -0.5 || sx > w as f64 - 0.5 || sy > h as f64 - 0.5 {
                continue;
            }
            let px = sample(image, sx, sy);
            let i = (y * w + x) * 4;
            // A see-through pixel becomes paper.
            let a = u16::from(px[3]);
            for k in 0..3 {
                rgba[i + k] = ((u16::from(px[k]) * a + 255 * (255 - a)) / 255) as u8;
            }
        }
    }
    RenderedImage {
        width: image.width,
        height: image.height,
        rgba,
    }
}

// ----- paper in a photo ---------------------------------------------------------------------

/// A corner of the paper, in pixels of the full picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Corner {
    pub x: f32,
    pub y: f32,
}

/// The four corners of the paper in order top left, top right, bottom right, bottom left.
pub(crate) type Quad = [Corner; 4];

/// The biggest group of connected bright pixels, as the points of its outline on a small copy.
fn paper_points(gray: &Gray) -> Option<(Vec<(usize, usize)>, usize)> {
    let level = otsu_level(&gray.data);
    let bright = gray.data.iter().filter(|g| **g > level).count();
    let fraction = bright as f64 / gray.data.len() as f64;
    // Almost everything bright means there is no background to cut away; almost nothing means the
    // paper is not the brighter part.
    if !(0.15..=0.985).contains(&fraction) {
        return None;
    }
    let (w, h) = (gray.width, gray.height);
    let mut label = vec![0u32; w * h];
    let mut sizes: Vec<usize> = vec![0];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if gray.data[start] <= level || label[start] != 0 {
            continue;
        }
        let id = u32::try_from(sizes.len()).ok()?;
        sizes.push(0);
        stack.push(start);
        label[start] = id;
        while let Some(i) = stack.pop() {
            sizes[id as usize] += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |nx: usize, ny: usize| {
                let n = ny * w + nx;
                if gray.data[n] > level && label[n] == 0 {
                    label[n] = id;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(x - 1, y);
            }
            if x + 1 < w {
                visit(x + 1, y);
            }
            if y > 0 {
                visit(x, y - 1);
            }
            if y + 1 < h {
                visit(x, y + 1);
            }
        }
    }
    let (best, &size) = sizes
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(_, size)| **size)?;
    // The paper must be a good part of the picture.
    if (size as f64) < 0.25 * (w * h) as f64 {
        return None;
    }
    let id = u32::try_from(best).ok()?;
    let points: Vec<(usize, usize)> = (0..w * h)
        .filter(|i| label[*i] == id)
        .map(|i| (i % w, i / w))
        .collect();
    Some((points, size))
}

/// The corners of the paper in the full picture, when there is paper to find.
pub(crate) fn find_paper(image: &RenderedImage) -> Option<Quad> {
    let (small, step) = Gray::from_rendered(image).shrunk(400);
    let (points, _) = paper_points(&small)?;
    // The corners are the points farthest along the two diagonals.
    let extreme = |score: &dyn Fn(usize, usize) -> i64, want_max: bool| {
        points.iter().copied().max_by_key(|(x, y)| {
            let s = score(*x, *y);
            if want_max { s } else { -s }
        })
    };
    let sum = |x: usize, y: usize| x as i64 + y as i64;
    let diff = |x: usize, y: usize| x as i64 - y as i64;
    let tl = extreme(&sum, false)?;
    let br = extreme(&sum, true)?;
    let tr = extreme(&diff, true)?;
    let bl = extreme(&diff, false)?;
    let scale = step as f32;
    // A pixel stands for a block of `step` pixels; use the outer edge of the block.
    let corner = |(x, y): (usize, usize), dx: f32, dy: f32| Corner {
        x: ((x as f32 + dx) * scale).clamp(0.0, image.width as f32 - 1.0),
        y: ((y as f32 + dy) * scale).clamp(0.0, image.height as f32 - 1.0),
    };
    let quad = [
        corner(tl, 0.0, 0.0),
        corner(tr, 1.0, 0.0),
        corner(br, 1.0, 1.0),
        corner(bl, 0.0, 1.0),
    ];
    (quad_area(&quad) > 0.25 * f64::from(image.width) * f64::from(image.height)).then_some(quad)
}

fn quad_area(q: &Quad) -> f64 {
    // Shoelace formula.
    let mut sum = 0.0f64;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        sum += f64::from(a.x) * f64::from(b.y) - f64::from(b.x) * f64::from(a.y);
    }
    sum.abs() / 2.0
}

fn distance(a: Corner, b: Corner) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// How far the corners are from a rectangle that sits straight in the picture, as a fraction of
/// the picture's size.
fn tilt_of(q: &Quad, width: u32, height: u32) -> f32 {
    let (w, h) = (width as f32, height as f32);
    let left = (q[0].x + q[3].x) / 2.0;
    let right = (q[1].x + q[2].x) / 2.0;
    let top = (q[0].y + q[1].y) / 2.0;
    let bottom = (q[2].y + q[3].y) / 2.0;
    let expected = [(left, top), (right, top), (right, bottom), (left, bottom)];
    q.iter()
        .zip(expected)
        .map(|(c, (x, y))| ((c.x - x).abs() / w).max((c.y - y).abs() / h))
        .fold(0.0, f32::max)
}

/// The picture cut to the straight box around the paper, with a little space left.
pub(crate) fn crop_to_paper(image: &RenderedImage, quad: &Quad) -> RenderedImage {
    let xs = quad.iter().map(|c| c.x);
    let ys = quad.iter().map(|c| c.y);
    let (min_x, max_x) = (
        xs.clone().fold(f32::MAX, f32::min),
        xs.fold(f32::MIN, f32::max),
    );
    let (min_y, max_y) = (
        ys.clone().fold(f32::MAX, f32::min),
        ys.fold(f32::MIN, f32::max),
    );
    let pad_x = (max_x - min_x) * 0.005;
    let pad_y = (max_y - min_y) * 0.005;
    let x0 = (min_x - pad_x).max(0.0) as u32;
    let y0 = (min_y - pad_y).max(0.0) as u32;
    let x1 = ((max_x + pad_x).ceil() as u32 + 1).min(image.width);
    let y1 = ((max_y + pad_y).ceil() as u32 + 1).min(image.height);
    let (w, h) = (x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y0 + h {
        let start = (y as usize * image.width as usize + x0 as usize) * 4;
        rgba.extend_from_slice(&image.rgba[start..start + w as usize * 4]);
    }
    RenderedImage {
        width: w,
        height: h,
        rgba,
    }
}

/// The projective map from the unit square to a quadrilateral (Heckbert's closed form).
struct Projection {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
}

impl Projection {
    fn from_unit_square(q: &Quad) -> Self {
        let p = |i: usize| (f64::from(q[i].x), f64::from(q[i].y));
        let ((x0, y0), (x1, y1), (x2, y2), (x3, y3)) = (p(0), p(1), p(2), p(3));
        let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
        let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);
        let (g, h);
        if dx3.abs() < 1e-9 && dy3.abs() < 1e-9 {
            (g, h) = (0.0, 0.0);
        } else {
            let det = dx1 * dy2 - dx2 * dy1;
            g = (dx3 * dy2 - dx2 * dy3) / det;
            h = (dx1 * dy3 - dx3 * dy1) / det;
        }
        Self {
            a: x1 - x0 + g * x1,
            b: x3 - x0 + h * x3,
            c: x0,
            d: y1 - y0 + g * y1,
            e: y3 - y0 + h * y3,
            f: y0,
            g,
            h,
        }
    }

    fn map(&self, u: f64, v: f64) -> (f64, f64) {
        let w = self.g * u + self.h * v + 1.0;
        (
            (self.a * u + self.b * v + self.c) / w,
            (self.d * u + self.e * v + self.f) / w,
        )
    }
}

fn sample(image: &RenderedImage, x: f64, y: f64) -> [u8; 4] {
    let (w, h) = (image.width as usize, image.height as usize);
    let x = x.clamp(0.0, (w - 1) as f64);
    let y = y.clamp(0.0, (h - 1) as f64);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let px = |xx: usize, yy: usize| {
        let i = (yy * w + xx) * 4;
        [
            f64::from(image.rgba[i]),
            f64::from(image.rgba[i + 1]),
            f64::from(image.rgba[i + 2]),
            f64::from(image.rgba[i + 3]),
        ]
    };
    let (p00, p10, p01, p11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bottom = p01[c] * (1.0 - fx) + p11[c] * fx;
        out[c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    out
}

/// The paper stood up straight: the quadrilateral becomes a rectangle as wide and as high as its
/// sides are long on average.
pub(crate) fn stand_upright(image: &RenderedImage, quad: &Quad) -> RenderedImage {
    let top = distance(quad[0], quad[1]);
    let bottom = distance(quad[3], quad[2]);
    let left = distance(quad[0], quad[3]);
    let right = distance(quad[1], quad[2]);
    let width = (((top + bottom) / 2.0).round() as u32).clamp(1, 20_000);
    let height = (((left + right) / 2.0).round() as u32).clamp(1, 20_000);
    let projection = Projection::from_unit_square(quad);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for j in 0..height {
        let v = (f64::from(j) + 0.5) / f64::from(height);
        for i in 0..width {
            let u = (f64::from(i) + 0.5) / f64::from(width);
            let (x, y) = projection.map(u, v);
            rgba.extend_from_slice(&sample(image, x, y));
        }
    }
    RenderedImage {
        width,
        height,
        rgba,
    }
}

/// What to do with a scanned picture before it becomes a page.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cleanup {
    pub auto_crop: bool,
    pub perspective_fix: bool,
}

/// Cuts away the background and straightens the page, as asked.
pub(crate) fn clean(image: RenderedImage, steps: Cleanup) -> RenderedImage {
    if !steps.auto_crop && !steps.perspective_fix {
        return image;
    }
    let Some(quad) = find_paper(&image) else {
        return image;
    };
    let tilt = tilt_of(&quad, image.width, image.height);
    if steps.perspective_fix && tilt > 0.01 {
        return stand_upright(&image, &quad);
    }
    crop_to_paper(&image, &quad)
}

// ----- colors -------------------------------------------------------------------------------

/// A page after the color choice: what the PDF will store.
pub(crate) enum Reduced {
    Rgb(Vec<u8>),
    Gray(Vec<u8>),
    /// One bit per pixel packed from the left, 1 is black.
    Bilevel(Vec<bool>),
}

/// Black and white by comparing every pixel with the average of the area around it, so a photo
/// with uneven light keeps its text (Bradley and Roth's method with an integral image).
fn adaptive_threshold(gray: &Gray) -> Vec<bool> {
    let (w, h) = (gray.width, gray.height);
    let mut integral = vec![0u64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u64;
        for x in 0..w {
            row += u64::from(gray.at(x, y));
            integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + row;
        }
    }
    let radius = (w.max(h) / 16).max(7);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(radius), (y + radius + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(radius), (x + radius + 1).min(w));
            let area = ((x1 - x0) * (y1 - y0)) as u64;
            let sum = integral[y1 * (w + 1) + x1] + integral[y0 * (w + 1) + x0]
                - integral[y0 * (w + 1) + x1]
                - integral[y1 * (w + 1) + x0];
            // Black when 15 percent darker than the neighborhood.
            out.push(u64::from(gray.at(x, y)) * area * 100 < sum * 85);
        }
    }
    out
}

pub(crate) fn reduce(image: &RenderedImage, color: PictureColor) -> Reduced {
    match color {
        PictureColor::Color => {
            let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
            for px in image.rgba.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
            }
            Reduced::Rgb(rgb)
        }
        PictureColor::Gray => Reduced::Gray(luma(image)),
        PictureColor::BlackWhite => {
            Reduced::Bilevel(adaptive_threshold(&Gray::from_rendered(image)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page with dark dashed text lines, turned by `degrees` (positive counter-clockwise on
    /// the screen) around its center.
    fn page_with_lines(width: u32, height: u32, degrees: f32) -> RenderedImage {
        let (w, h) = (width as f32, height as f32);
        let (s, c) = degrees.to_radians().sin_cos();
        let mut rgba = vec![255u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                // Where this pixel was before the page was turned.
                let (dx, dy) = (x as f32 - w / 2.0, y as f32 - h / 2.0);
                let (ox, oy) = (dx * c - dy * s + w / 2.0, dx * s + dy * c + h / 2.0);
                let on_line = (oy as i32).rem_euclid(24) < 5 && (ox as i32).rem_euclid(30) < 22;
                let inside = ox > 60.0 && ox < w - 60.0 && oy > 60.0 && oy < h - 60.0;
                if on_line && inside {
                    let i = ((y * width + x) * 4) as usize;
                    rgba[i..i + 3].copy_from_slice(&[20, 20, 20]);
                }
            }
        }
        RenderedImage {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn a_tilted_page_gives_the_angle_that_levels_it() {
        for degrees in [-6.0f32, -2.0, 0.0, 1.0, 3.5, 8.0] {
            let img = page_with_lines(900, 1200, degrees);
            let found = skew_correction(&img, 15.0).expect("lines are found");
            // Content turned counter-clockwise by `degrees` is levelled by turning it back.
            assert!(
                (found + degrees).abs() < 0.35,
                "page turned {degrees}, correction {found}"
            );
        }
    }

    #[test]
    fn turning_a_picture_by_the_correction_levels_its_lines() {
        let tilted = page_with_lines(900, 1200, 4.0);
        let correction = skew_correction(&tilted, 15.0).unwrap();
        let level = rotate_about_center(&tilted, correction);
        // Measured again, the lines run level.
        let again = skew_correction(&level, 15.0).unwrap_or(0.0);
        assert!(again.abs() < 0.3, "after levelling: {again}");
        assert_eq!((level.width, level.height), (900, 1200));
        // The corners the picture no longer reaches are paper.
        assert_eq!(&level.rgba[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn blank_and_featureless_pages_give_no_answer() {
        let blank = RenderedImage {
            width: 400,
            height: 500,
            rgba: vec![255; 400 * 500 * 4],
        };
        assert_eq!(skew_correction(&blank, 15.0), None);
        // Noise has no lines either.
        let mut noise = blank.clone();
        let mut state = 12345u32;
        for px in noise.rgba.as_chunks_mut::<4>().0.iter_mut() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if state >> 28 == 0 {
                px[..3].copy_from_slice(&[0, 0, 0]);
            }
        }
        assert_eq!(skew_correction(&noise, 15.0), None);
    }

    #[test]
    fn the_largest_tilt_is_checked() {
        let img = page_with_lines(300, 400, 0.0);
        assert!(RasterConverter.skew(&img, 0.5).is_err());
        assert!(RasterConverter.skew(&img, 20.0).is_ok());
    }

    /// Dark background with a bright quadrilateral that has a little ink on it.
    fn photo(quad: [(f32, f32); 4], size: (u32, u32)) -> RenderedImage {
        let (w, h) = size;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.as_chunks_mut::<4>().0.iter_mut() {
            px.copy_from_slice(&[40, 38, 36, 255]);
        }
        let inside = |x: f32, y: f32| {
            // Point in convex quad: all cross products have one sign.
            let mut sign = 0i8;
            for i in 0..4 {
                let (a, b) = (quad[i], quad[(i + 1) % 4]);
                let cross = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
                let s = if cross >= 0.0 { 1 } else { -1 };
                if sign == 0 {
                    sign = s;
                } else if sign != s {
                    return false;
                }
            }
            true
        };
        for y in 0..h {
            for x in 0..w {
                if inside(x as f32, y as f32) {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[240, 240, 235, 255]);
                }
            }
        }
        RenderedImage {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn the_paper_is_found_in_a_photo() {
        let img = photo(
            [(80.0, 60.0), (520.0, 90.0), (500.0, 700.0), (60.0, 660.0)],
            (600, 760),
        );
        let quad = find_paper(&img).expect("paper");
        let near = |c: Corner, x: f32, y: f32| (c.x - x).abs() < 14.0 && (c.y - y).abs() < 14.0;
        assert!(near(quad[0], 80.0, 60.0), "{:?}", quad[0]);
        assert!(near(quad[1], 520.0, 90.0), "{:?}", quad[1]);
        assert!(near(quad[2], 500.0, 700.0), "{:?}", quad[2]);
        assert!(near(quad[3], 60.0, 660.0), "{:?}", quad[3]);
    }

    #[test]
    fn a_page_without_background_has_no_paper_to_find() {
        let white = RenderedImage {
            width: 300,
            height: 400,
            rgba: vec![250; 300 * 400 * 4],
        };
        assert!(find_paper(&white).is_none());
    }

    #[test]
    fn a_page_photographed_at_an_angle_stands_upright() {
        let img = photo(
            [(80.0, 60.0), (520.0, 90.0), (500.0, 700.0), (60.0, 660.0)],
            (600, 760),
        );
        let fixed = clean(
            img,
            Cleanup {
                auto_crop: true,
                perspective_fix: true,
            },
        );
        // The sides are about 440 and 612 pixels long.
        assert!((fixed.width as i32 - 440).abs() < 25, "{}", fixed.width);
        assert!((fixed.height as i32 - 612).abs() < 25, "{}", fixed.height);
        // Almost every pixel is now paper, not background.
        let paper = fixed
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 200)
            .count();
        assert!(
            paper as f64 > 0.97 * f64::from(fixed.width * fixed.height),
            "{paper}"
        );
    }

    #[test]
    fn a_straight_sheet_is_only_cropped() {
        let img = photo(
            [(100.0, 80.0), (500.0, 80.0), (500.0, 680.0), (100.0, 680.0)],
            (600, 760),
        );
        let cropped = clean(
            img.clone(),
            Cleanup {
                auto_crop: true,
                perspective_fix: true,
            },
        );
        assert!(
            (cropped.width as i32 - 402).abs() < 20 && (cropped.height as i32 - 602).abs() < 20
        );
        let untouched = clean(
            img.clone(),
            Cleanup {
                auto_crop: false,
                perspective_fix: false,
            },
        );
        assert_eq!((untouched.width, untouched.height), (600, 760));
    }

    #[test]
    fn colors_reduce_to_gray_and_to_black_and_white_with_uneven_light() {
        // Text pixels on paper that is darker on the right than on the left.
        let (w, h) = (200u32, 100u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let paper = 240 - (x * 100 / w) as u8;
                let ink = (10..14).contains(&(y % 20)) && x % 10 < 6;
                let v = if ink { paper / 3 } else { paper };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let img = RenderedImage {
            width: w,
            height: h,
            rgba,
        };
        let Reduced::Bilevel(bits) = reduce(&img, PictureColor::BlackWhite) else {
            panic!("expected black and white");
        };
        let black_at = |x: u32, y: u32| bits[(y * w + x) as usize];
        // Ink is black on both sides, bare paper is white on both sides.
        assert!(black_at(2, 11) && black_at(192, 11));
        assert!(!black_at(2, 5) && !black_at(192, 5));
        assert!(
            matches!(reduce(&img, PictureColor::Gray), Reduced::Gray(g) if g.len() == (w * h) as usize)
        );
        assert!(
            matches!(reduce(&img, PictureColor::Color), Reduced::Rgb(c) if c.len() == (w * h * 3) as usize)
        );
    }

    #[test]
    fn otsu_finds_a_level_between_two_populations() {
        let mut values = vec![30u8; 100];
        values.extend(vec![220u8; 300]);
        let level = otsu_level(&values);
        assert!((30..220).contains(&level));
    }
}
