//! True redaction (row 54): what lies under the areas is removed from the page content, not only
//! covered.
//!
//! The content stream of each page is walked with the graphics and text state the PDF model
//! defines, so the place of every character, picture and shape is known:
//!
//! - Characters whose boxes meet an area are taken out of their strings. Each one is replaced by
//!   a gap of its advance, so the text that stays does not move.
//! - Pictures that meet an area are decoded, the pixels inside are cleared and the picture is
//!   written again, losslessly. Forms (reusable content) are cleaned the same way on a copy.
//! - Lines and shapes that meet an area are not painted.
//! - Annotations and form fields under an area are removed with their values.
//! - A box of the chosen color is drawn over every area.
//!
//! A page is left alone and reported when its content cannot be understood well enough to be
//! sure (inline pictures, fonts whose code maps are unknown, stencil masks, scans stored as CCITT
//! or JBIG2). The caller then replaces such pages with a picture.

use application::AppError;
use application::ports::{RedactOutcome, RedactPlan};
use domain::stamp::Rgb;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::annot::user_rect;
use crate::boxes::{display_matrix, displayed_size};
use crate::images::decode_pixels;
use crate::pageres::{add_named, effective_resources, multiply, num, page_object, set_resources};
use crate::paint::Boxed;
use crate::redactfont::FontInfo;

/// Largest content stream walked (after decoding).
const MAX_CONTENT_BYTES: usize = 256 * 1024 * 1024;
/// Deepest nesting of forms inside forms.
const MAX_FORM_DEPTH: usize = 8;
/// Share of a character's width, on each side, that may touch an area without the character
/// going. It keeps the neighbor of a word whose box was taken from the character boxes.
const GLYPH_SLACK: f32 = 0.06;

type Matrix = [f32; 6];
/// `[x0, y0, x1, y1]`.
type Rect4 = [f32; 4];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The content cannot be cleaned with certainty.
struct Unsupported;

fn apply_m(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn bounds(points: &[(f32, f32)]) -> Rect4 {
    let mut r = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (x, y) in points {
        r[0] = r[0].min(*x);
        r[1] = r[1].min(*y);
        r[2] = r[2].max(*x);
        r[3] = r[3].max(*y);
    }
    r
}

fn overlap(a: Rect4, b: Rect4) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

fn contains(outer: Rect4, inner: Rect4) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

fn grown(r: Rect4, by: f32) -> Rect4 {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

fn inverse(m: Matrix) -> Option<Matrix> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 || !det.is_finite() {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)])
}

fn floats(operands: &[Object]) -> Vec<f32> {
    operands.iter().filter_map(|o| o.as_float().ok()).collect()
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        other => other,
    }
}

/// A resource dictionary whose categories are direct dictionaries, so changing the copy never
/// touches an object other pages share.
fn direct_resources(doc: &Document, resources: &Dictionary) -> Dictionary {
    let mut out = resources.clone();
    let keys: Vec<Vec<u8>> = out.iter().map(|(k, _)| k.clone()).collect();
    for key in keys {
        let table = match out.get(&key) {
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
            Ok(Object::Dictionary(d)) => Some(d.clone()),
            _ => None,
        };
        if let Some(table) = table {
            out.set(key, Object::Dictionary(table));
        }
    }
    out
}

// ----- the walk ------------------------------------------------------------------------------

#[derive(Clone)]
struct GState {
    ctm: Matrix,
    tc: f32,
    tw: f32,
    th: f32,
    tl: f32,
    rise: f32,
    fs: f32,
    font: Vec<u8>,
    line_width: f32,
}

impl GState {
    fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            tl: 0.0,
            rise: 0.0,
            fs: 0.0,
            font: Vec::new(),
            line_width: 1.0,
        }
    }
}

/// The path being built.
#[derive(Default)]
struct PathInfo {
    bbox: Option<Rect4>,
    rects: Vec<Rect4>,
    /// Only rectangles were added.
    rects_only: bool,
    any: bool,
}

impl PathInfo {
    fn add(&mut self, p: (f32, f32)) {
        let r = self.bbox.get_or_insert([p.0, p.1, p.0, p.1]);
        r[0] = r[0].min(p.0);
        r[1] = r[1].min(p.1);
        r[2] = r[2].max(p.0);
        r[3] = r[3].max(p.1);
    }

    fn line_or_curve(&mut self) {
        self.rects_only = false;
        self.any = true;
    }
}

/// What one set of resources holds, with the fonts read so far.
struct Scope {
    resources: Dictionary,
    fonts: HashMap<Vec<u8>, Rc<FontInfo>>,
    changed: bool,
    /// Names of pictures and forms that were replaced by cleaned copies. The originals go from the
    /// resources unless the content still draws them somewhere else.
    replaced: Vec<Vec<u8>>,
}

impl Scope {
    fn new(resources: Dictionary) -> Self {
        Self {
            resources,
            fonts: HashMap::new(),
            changed: false,
            replaced: Vec::new(),
        }
    }

    /// Removes the replaced originals that `ops` no longer draw, so their data does not stay in
    /// the file.
    fn drop_replaced(&mut self, ops: &[Operation]) {
        if self.replaced.is_empty() {
            return;
        }
        let used: HashSet<&[u8]> = ops
            .iter()
            .filter(|op| op.operator == "Do")
            .filter_map(|op| match op.operands.first() {
                Some(Object::Name(n)) => Some(n.as_slice()),
                _ => None,
            })
            .collect();
        if let Ok(Object::Dictionary(table)) = self.resources.get_mut(b"XObject") {
            for name in &self.replaced {
                if !used.contains(name.as_slice()) {
                    table.remove(name);
                }
            }
        }
    }

    fn font(&mut self, doc: &Document, name: &[u8]) -> Rc<FontInfo> {
        if let Some(found) = self.fonts.get(name) {
            return Rc::clone(found);
        }
        let dict = self
            .resources
            .get(b"Font")
            .ok()
            .map(|o| resolve(doc, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|fonts| fonts.get(name).ok())
            .map(|o| resolve(doc, o))
            .and_then(|o| o.as_dict().ok())
            .cloned();
        let info = Rc::new(match dict {
            Some(d) => FontInfo::load(doc, &d),
            None => FontInfo::load(doc, &Dictionary::new()),
        });
        self.fonts.insert(name.to_vec(), Rc::clone(&info));
        info
    }
}

/// A piece of the array a modified text operator is rewritten to.
enum Piece {
    Bytes(Vec<u8>),
    Gap(f32),
}

struct Walker<'a> {
    doc: &'a mut Document,
    areas: &'a [Rect4],
    fill: Rgb,
    stats: RedactOutcome,
}

impl Walker<'_> {
    fn hits(&self, r: Rect4) -> bool {
        self.areas.iter().any(|a| overlap(r, *a))
    }

    /// Whether a painted path meets an area in a way that shows what the area holds.
    fn path_hits(
        &self,
        path: &PathInfo,
        fill: bool,
        stroke: bool,
        line_width: f32,
        scale: f32,
    ) -> bool {
        let Some(bbox) = path.bbox else {
            return false;
        };
        let half = if stroke {
            line_width * scale / 2.0
        } else {
            0.0
        };
        for area in self.areas {
            if !overlap(grown(bbox, half), *area) {
                continue;
            }
            if path.rects_only && !path.rects.is_empty() {
                for rect in &path.rects {
                    if !overlap(grown(*rect, half), *area) {
                        continue;
                    }
                    if fill && !stroke {
                        // A filled rectangle around an area has one color under it.
                        if contains(*rect, *area) {
                            continue;
                        }
                        return true;
                    }
                    // The sides of the rectangle are drawn; only an area that is clear of them is
                    // safe.
                    let inner = grown(*rect, -half);
                    if contains(inner, *area) {
                        continue;
                    }
                    return true;
                }
                continue;
            }
            if fill && !stroke && contains(bbox, *area) {
                continue;
            }
            return true;
        }
        false
    }

    /// Walks the operations of one content stream, returning the operations to keep.
    fn walk(
        &mut self,
        ops: Vec<Operation>,
        scope: &mut Scope,
        base: Matrix,
        depth: usize,
    ) -> Result<Vec<Operation>, Unsupported> {
        let mut out: Vec<Operation> = Vec::with_capacity(ops.len());
        let mut st = GState::new(base);
        let mut stack: Vec<GState> = Vec::new();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut stale = false;
        let mut path = PathInfo {
            rects_only: true,
            ..PathInfo::default()
        };

        for op in ops {
            // Operators that only set state go to the output as they are; the arms below keep the
            // walk's own copy of that state.
            if matches!(
                op.operator.as_str(),
                "q" | "Q"
                    | "cm"
                    | "w"
                    | "Tc"
                    | "Tw"
                    | "Tz"
                    | "TL"
                    | "Ts"
                    | "Tf"
                    | "BT"
                    | "Td"
                    | "TD"
                    | "Tm"
                    | "T*"
            ) {
                out.push(op.clone());
            }
            let operands = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push(st.clone()),
                "Q" => {
                    if let Some(saved) = stack.pop() {
                        st = saved;
                    }
                }
                "cm" => {
                    if let [a, b, c, d, e, f] = floats(operands)[..] {
                        st.ctm = multiply([a, b, c, d, e, f], st.ctm);
                    }
                }
                "w" => {
                    if let Some(v) = floats(operands).first() {
                        st.line_width = *v;
                    }
                }
                "Tc" => st.tc = floats(operands).first().copied().unwrap_or(0.0),
                "Tw" => st.tw = floats(operands).first().copied().unwrap_or(0.0),
                "Tz" => st.th = floats(operands).first().copied().unwrap_or(100.0) / 100.0,
                "TL" => st.tl = floats(operands).first().copied().unwrap_or(0.0),
                "Ts" => st.rise = floats(operands).first().copied().unwrap_or(0.0),
                "Tf" => {
                    if let (Some(Object::Name(name)), Some(size)) = (
                        operands.first(),
                        operands.get(1).and_then(|o| o.as_float().ok()),
                    ) {
                        st.font = name.clone();
                        st.fs = size;
                    }
                }
                "BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                    stale = false;
                }
                "Td" | "TD" => {
                    if let [tx, ty] = floats(operands)[..] {
                        if op.operator == "TD" {
                            st.tl = -ty;
                        }
                        tlm = multiply([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                        tm = tlm;
                        stale = false;
                    }
                }
                "Tm" => {
                    if let [a, b, c, d, e, f] = floats(operands)[..] {
                        tlm = [a, b, c, d, e, f];
                        tm = tlm;
                        stale = false;
                    }
                }
                "T*" => {
                    tlm = multiply([1.0, 0.0, 0.0, 1.0, 0.0, -st.tl], tlm);
                    tm = tlm;
                    stale = false;
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    let quote = op.operator == "'" || op.operator == "\"";
                    if quote {
                        if op.operator == "\"" {
                            let v = floats(&operands[..operands.len().min(2)]);
                            if let [aw, ac] = v[..] {
                                st.tw = aw;
                                st.tc = ac;
                            }
                        }
                        tlm = multiply([1.0, 0.0, 0.0, 1.0, 0.0, -st.tl], tlm);
                        tm = tlm;
                        stale = false;
                    }
                    let shown = if op.operator == "\"" {
                        operands.get(2..).unwrap_or(&[])
                    } else {
                        &operands[..]
                    };
                    let rewritten = self.show(scope, &st, &mut tm, &mut stale, shown)?;
                    match rewritten {
                        None => out.push(op),
                        Some(array) => {
                            scope.changed = true;
                            if op.operator == "\""
                                && let [aw, ac] = floats(&operands[..operands.len().min(2)])[..]
                            {
                                out.push(Operation::new("Tw", vec![Object::Real(aw)]));
                                out.push(Operation::new("Tc", vec![Object::Real(ac)]));
                            }
                            if quote {
                                out.push(Operation::new("T*", vec![]));
                            }
                            out.push(Operation::new("TJ", vec![Object::Array(array)]));
                        }
                    }
                }
                "m" | "l" => {
                    if let [x, y] = floats(operands)[..] {
                        path.add(apply_m(st.ctm, x, y));
                        path.line_or_curve();
                    }
                    out.push(op);
                }
                "c" => {
                    let v = floats(operands);
                    for p in v.as_chunks::<2>().0 {
                        path.add(apply_m(st.ctm, p[0], p[1]));
                    }
                    path.line_or_curve();
                    out.push(op);
                }
                "v" | "y" => {
                    let v = floats(operands);
                    for p in v.as_chunks::<2>().0 {
                        path.add(apply_m(st.ctm, p[0], p[1]));
                    }
                    path.line_or_curve();
                    out.push(op);
                }
                "re" => {
                    if let [x, y, w, h] = floats(operands)[..] {
                        let corners = [
                            apply_m(st.ctm, x, y),
                            apply_m(st.ctm, x + w, y),
                            apply_m(st.ctm, x, y + h),
                            apply_m(st.ctm, x + w, y + h),
                        ];
                        for c in corners {
                            path.add(c);
                        }
                        path.rects.push(bounds(&corners));
                        path.any = true;
                    }
                    out.push(op);
                }
                "h" | "W" | "W*" => out.push(op),
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    let (fill, stroke) = match op.operator.as_str() {
                        "S" | "s" => (false, true),
                        "f" | "F" | "f*" => (true, false),
                        "n" => (false, false),
                        _ => (true, true),
                    };
                    let scale = (st.ctm[0] * st.ctm[3] - st.ctm[1] * st.ctm[2]).abs().sqrt();
                    if (fill || stroke) && self.path_hits(&path, fill, stroke, st.line_width, scale)
                    {
                        self.stats.paths += 1;
                        scope.changed = true;
                        // `n` keeps a clip that `W` set before the paint operator.
                        out.push(Operation::new("n", vec![]));
                    } else {
                        out.push(op);
                    }
                    path = PathInfo {
                        rects_only: true,
                        ..PathInfo::default()
                    };
                }
                "Do" => {
                    let name = match operands.first() {
                        Some(Object::Name(n)) => Some(n.clone()),
                        _ => None,
                    };
                    let Some(name) = name else {
                        out.push(op);
                        continue;
                    };
                    match self.draw_object(scope, &st, &name, depth)? {
                        Some(new_name) => {
                            scope.changed = true;
                            scope.replaced.push(name.clone());
                            out.push(Operation::new(
                                "Do",
                                vec![Object::Name(new_name.into_bytes())],
                            ));
                        }
                        None => out.push(op),
                    }
                }
                "BDC" | "DP" => {
                    // Alternate text of marked content is a second copy of what the content says.
                    let mut op = op;
                    for operand in &mut op.operands {
                        if let Object::Dictionary(d) = operand {
                            for key in ["ActualText", "Alt", "E"] {
                                if d.remove(key.as_bytes()).is_some() {
                                    scope.changed = true;
                                }
                            }
                        }
                    }
                    out.push(op);
                }
                "BI" | "ID" | "EI" => {
                    // A picture inside the content stream cannot be located or cleaned here.
                    return Err(Unsupported);
                }
                _ => out.push(op),
            }
        }
        scope.drop_replaced(&out);
        Ok(out)
    }

    /// Works out the characters a text operator draws, takes out those under an area and moves
    /// the text matrix on. Returns the array the operator is rewritten to when something went.
    fn show(
        &mut self,
        scope: &mut Scope,
        st: &GState,
        tm: &mut Matrix,
        stale: &mut bool,
        operands: &[Object],
    ) -> Result<Option<Vec<Object>>, Unsupported> {
        let font = scope.font(self.doc, &st.font);
        let items: Vec<&Object> = match operands.first() {
            Some(Object::Array(list)) => list.iter().collect(),
            Some(first) => vec![first],
            None => return Ok(None),
        };

        if !font.usable || *stale || st.fs == 0.0 {
            // Without the advances the characters cannot be placed one by one. Text that could
            // lie on a line an area covers makes the page unsafe.
            let line = self.line_extent(&font, st, *tm);
            *stale = true;
            if self.areas.iter().any(|a| a[1] < line.1 && a[3] > line.0) && st.fs != 0.0 {
                return Err(Unsupported);
            }
            return Ok(None);
        }

        let fs = st.fs;
        let mut pieces: Vec<Piece> = Vec::new();
        let mut removed = false;
        let push_bytes = |pieces: &mut Vec<Piece>, byte: &[u8]| match pieces.last_mut() {
            Some(Piece::Bytes(b)) => b.extend_from_slice(byte),
            _ => pieces.push(Piece::Bytes(byte.to_vec())),
        };
        let push_gap = |pieces: &mut Vec<Piece>, gap: f32| match pieces.last_mut() {
            Some(Piece::Gap(g)) => *g += gap,
            _ => pieces.push(Piece::Gap(gap)),
        };

        for item in items {
            match item {
                Object::String(bytes, _) => {
                    let step = if font.two_byte { 2 } else { 1 };
                    for chunk in bytes.chunks(step) {
                        let code = chunk.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
                        let glyph = font.advance(code);
                        let space = if !font.two_byte && code == 32 {
                            st.tw
                        } else {
                            0.0
                        };
                        let advance = (glyph * fs + st.tc + space) * st.th;
                        let width = (glyph * fs * st.th).abs().max(0.05 * fs.abs());
                        let slack = GLYPH_SLACK * width;
                        let (x0, x1) = if glyph * fs * st.th >= 0.0 {
                            (slack, width - slack)
                        } else {
                            (-width + slack, -slack)
                        };
                        let y0 = st.rise + font.descent * fs;
                        let y1 = st.rise + font.ascent * fs;
                        let m = multiply(*tm, st.ctm);
                        let corners = [
                            apply_m(m, x0, y0),
                            apply_m(m, x1, y0),
                            apply_m(m, x0, y1),
                            apply_m(m, x1, y1),
                        ];
                        if self.hits(bounds(&corners)) {
                            removed = true;
                            self.stats.glyphs += 1;
                            push_gap(&mut pieces, -(glyph * fs + st.tc + space) * 1000.0 / fs);
                        } else {
                            push_bytes(&mut pieces, chunk);
                        }
                        *tm = multiply([1.0, 0.0, 0.0, 1.0, advance, 0.0], *tm);
                    }
                }
                other => {
                    if let Ok(n) = other.as_float() {
                        push_gap(&mut pieces, n);
                        let shift = -n / 1000.0 * fs * st.th;
                        *tm = multiply([1.0, 0.0, 0.0, 1.0, shift, 0.0], *tm);
                    }
                }
            }
        }
        if !removed {
            return Ok(None);
        }
        Ok(Some(
            pieces
                .into_iter()
                .map(|p| match p {
                    Piece::Bytes(b) => Object::String(b, StringFormat::Literal),
                    Piece::Gap(g) => Object::Real(g),
                })
                .collect(),
        ))
    }

    /// The vertical extent `(low, high)` in user space of a line of text at the text matrix.
    fn line_extent(&self, font: &FontInfo, st: &GState, tm: Matrix) -> (f32, f32) {
        let m = multiply(tm, st.ctm);
        let y0 = st.rise + font.descent * st.fs;
        let y1 = st.rise + font.ascent * st.fs;
        let a = apply_m(m, 0.0, y0).1;
        let b = apply_m(m, 0.0, y1).1;
        (a.min(b), a.max(b))
    }

    /// Cleans the picture or form a `Do` draws when it meets an area. Returns the name of the
    /// cleaned copy to draw instead.
    fn draw_object(
        &mut self,
        scope: &mut Scope,
        st: &GState,
        name: &[u8],
        depth: usize,
    ) -> Result<Option<String>, Unsupported> {
        let Some(id) = scope
            .resources
            .get(b"XObject")
            .ok()
            .map(|o| resolve(self.doc, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|table| match table.get(name) {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            })
        else {
            return Ok(None);
        };
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return Ok(None);
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .unwrap_or(b"")
            .to_vec();
        match subtype.as_slice() {
            b"Image" => {
                let unit = [
                    apply_m(st.ctm, 0.0, 0.0),
                    apply_m(st.ctm, 1.0, 0.0),
                    apply_m(st.ctm, 0.0, 1.0),
                    apply_m(st.ctm, 1.0, 1.0),
                ];
                if !self.hits(bounds(&unit)) {
                    return Ok(None);
                }
                let Some(new_id) = self.clear_picture(id, st.ctm)? else {
                    return Ok(None);
                };
                Ok(Some(add_named(
                    &mut scope.resources,
                    "XObject",
                    "QwP",
                    Object::Reference(new_id),
                )))
            }
            b"Form" => {
                let matrix = match stream
                    .dict
                    .get(b"Matrix")
                    .ok()
                    .map(|o| resolve(self.doc, o))
                {
                    Some(Object::Array(m)) => {
                        let v: Vec<f32> = m.iter().filter_map(|o| o.as_float().ok()).collect();
                        match v[..] {
                            [a, b, c, d, e, f] => [a, b, c, d, e, f],
                            _ => IDENTITY,
                        }
                    }
                    _ => IDENTITY,
                };
                let full = multiply(matrix, st.ctm);
                let bbox: Option<Vec<f32>> =
                    match stream.dict.get(b"BBox").ok().map(|o| resolve(self.doc, o)) {
                        Some(Object::Array(b)) => {
                            Some(b.iter().filter_map(|o| o.as_float().ok()).collect())
                        }
                        _ => None,
                    };
                if let Some(b) = bbox.as_deref()
                    && let [x0, y0, x1, y1] = b[..]
                {
                    let corners = [
                        apply_m(full, x0, y0),
                        apply_m(full, x1, y0),
                        apply_m(full, x0, y1),
                        apply_m(full, x1, y1),
                    ];
                    if !self.hits(bounds(&corners)) {
                        return Ok(None);
                    }
                }
                if depth >= MAX_FORM_DEPTH {
                    return Err(Unsupported);
                }
                let outer = scope.resources.clone();
                let Some(new_id) = self.clean_form(id, st.ctm, &outer, depth + 1)? else {
                    return Ok(None);
                };
                Ok(Some(add_named(
                    &mut scope.resources,
                    "XObject",
                    "QwF",
                    Object::Reference(new_id),
                )))
            }
            _ => Ok(None),
        }
    }

    /// A cleaned copy of a form, or `None` when nothing in it needed to change.
    fn clean_form(
        &mut self,
        id: ObjectId,
        ctm: Matrix,
        outer: &Dictionary,
        depth: usize,
    ) -> Result<Option<ObjectId>, Unsupported> {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return Ok(None);
        };
        let stream = stream.clone();
        let own = stream
            .dict
            .get(b"Resources")
            .ok()
            .map(|o| resolve(self.doc, o))
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_else(|| outer.clone());
        let matrix = match stream
            .dict
            .get(b"Matrix")
            .ok()
            .map(|o| resolve(self.doc, o))
        {
            Some(Object::Array(m)) => {
                let v: Vec<f32> = m.iter().filter_map(|o| o.as_float().ok()).collect();
                match v[..] {
                    [a, b, c, d, e, f] => [a, b, c, d, e, f],
                    _ => IDENTITY,
                }
            }
            _ => IDENTITY,
        };
        let bytes = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        if bytes.len() > MAX_CONTENT_BYTES {
            return Err(Unsupported);
        }
        let ops = Content::decode(&bytes).map_err(|_| Unsupported)?.operations;
        let mut scope = Scope::new(direct_resources(self.doc, &own));
        let ops = self.walk(ops, &mut scope, multiply(matrix, ctm), depth)?;
        if !scope.changed {
            return Ok(None);
        }
        let encoded = Content { operations: ops }
            .encode()
            .map_err(|_| Unsupported)?;
        let mut dict = stream.dict.clone();
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        dict.remove(b"Length");
        dict.set("Resources", Object::Dictionary(scope.resources));
        let mut new_stream = Stream::new(dict, encoded);
        let _ = new_stream.compress();
        Ok(Some(self.doc.add_object(new_stream)))
    }

    /// A copy of a picture with the pixels inside the areas cleared, or `None` when no pixel
    /// lies inside.
    fn clear_picture(
        &mut self,
        id: ObjectId,
        ctm: Matrix,
    ) -> Result<Option<ObjectId>, Unsupported> {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return Ok(None);
        };
        let stream = stream.clone();
        let mut pixels = decode_pixels(self.doc, &stream).ok_or(Unsupported)?;
        let back = inverse(ctm).ok_or(Unsupported)?;
        let (w, h) = (pixels.width as usize, pixels.height as usize);
        let channels = if pixels.rgb { 3 } else { 1 };
        let color: Vec<u8> = if pixels.rgb {
            vec![self.fill.r, self.fill.g, self.fill.b]
        } else {
            let gray = 0.299 * f32::from(self.fill.r)
                + 0.587 * f32::from(self.fill.g)
                + 0.114 * f32::from(self.fill.b);
            vec![gray.round() as u8]
        };
        let mut cleared = 0usize;
        for area in self.areas {
            // The pixels that could be inside: the area in the picture's own space.
            let corners = [
                apply_m(back, area[0], area[1]),
                apply_m(back, area[2], area[1]),
                apply_m(back, area[0], area[3]),
                apply_m(back, area[2], area[3]),
            ];
            let b = bounds(&corners);
            let c0 = (b[0] * w as f32).floor().max(0.0) as usize;
            let c1 = ((b[2] * w as f32).ceil().max(0.0) as usize).min(w);
            let r0 = (((1.0 - b[3]) * h as f32).floor().max(0.0)) as usize;
            let r1 = (((1.0 - b[1]) * h as f32).ceil().max(0.0) as usize).min(h);
            for row in r0..r1 {
                for col in c0..c1 {
                    let u = (col as f32 + 0.5) / w as f32;
                    let v = 1.0 - (row as f32 + 0.5) / h as f32;
                    let (x, y) = apply_m(ctm, u, v);
                    if x > area[0] && x < area[2] && y > area[1] && y < area[3] {
                        let at = (row * w + col) * channels;
                        pixels.samples[at..at + channels].copy_from_slice(&color);
                        cleared += 1;
                    }
                }
            }
        }
        if cleared == 0 {
            return Ok(None);
        }
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Image".to_vec()));
        dict.set("Width", Object::Integer(i64::from(pixels.width)));
        dict.set("Height", Object::Integer(i64::from(pixels.height)));
        dict.set(
            "ColorSpace",
            Object::Name(if pixels.rgb {
                b"DeviceRGB".to_vec()
            } else {
                b"DeviceGray".to_vec()
            }),
        );
        dict.set("BitsPerComponent", Object::Integer(8));
        // The soft mask stays when it still fits the picture.
        if let Ok(mask) = stream.dict.get(b"SMask")
            && let Object::Reference(mask_id) = mask
            && let Ok(Object::Stream(m)) = self.doc.get_object(*mask_id)
        {
            let same = |key: &[u8], v: u32| {
                m.dict
                    .get(key)
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .is_some_and(|n| n == i64::from(v))
            };
            if same(b"Width", pixels.width) && same(b"Height", pixels.height) {
                dict.set("SMask", Object::Reference(*mask_id));
            }
        }
        let mut new_stream = Stream::new(dict, pixels.samples);
        let _ = new_stream.compress();
        self.stats.pictures += 1;
        Ok(Some(self.doc.add_object(new_stream)))
    }
}

// ----- annotations and fields ----------------------------------------------------------------

fn rect_of(doc: &Document, dict: &Dictionary) -> Option<Rect4> {
    let Object::Array(items) = resolve(doc, dict.get(b"Rect").ok()?) else {
        return None;
    };
    let v: Vec<f32> = items
        .iter()
        .filter_map(|o| resolve(doc, o).as_float().ok())
        .collect();
    let [a, b, c, d] = v[..] else {
        return None;
    };
    Some([a.min(c), b.min(d), a.max(c), b.max(d)])
}

/// Takes a form field out of the form: from its parent's kids or from the form's field list. A
/// parent that keeps other kids loses its value, which may be the text that was removed.
fn detach_field(doc: &mut Document, id: ObjectId, seen: &mut HashSet<ObjectId>) {
    if !seen.insert(id) {
        return;
    }
    let parent = doc
        .get_dictionary(id)
        .ok()
        .and_then(|d| match d.get(b"Parent") {
            Ok(Object::Reference(p)) => Some(*p),
            _ => None,
        });
    if let Some(parent) = parent {
        let mut empty = false;
        if let Ok(p) = doc.get_dictionary_mut(parent) {
            if let Ok(Object::Array(kids)) = p.get_mut(b"Kids") {
                kids.retain(|k| !matches!(k, Object::Reference(r) if *r == id));
                empty = kids.is_empty();
            }
            p.remove(b"V");
            p.remove(b"DV");
        }
        if empty {
            detach_field(doc, parent, seen);
        }
        return;
    }
    let form = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|o| match o {
            Object::Reference(r) => Some(*r),
            _ => None,
        });
    let retain = |fields: &mut Vec<Object>| {
        fields.retain(|f| !matches!(f, Object::Reference(r) if *r == id));
    };
    if let Some(form) = form {
        if let Ok(d) = doc.get_dictionary_mut(form)
            && let Ok(Object::Array(fields)) = d.get_mut(b"Fields")
        {
            retain(fields);
        }
    } else if let Ok(catalog_id) = doc.trailer.get(b"Root").and_then(Object::as_reference)
        && let Ok(catalog) = doc.get_dictionary_mut(catalog_id)
        && let Ok(Object::Dictionary(d)) = catalog.get_mut(b"AcroForm")
        && let Ok(Object::Array(fields)) = d.get_mut(b"Fields")
    {
        retain(fields);
    }
}

/// Removes the annotations of a page that meet an area, with the form fields they belong to and
/// the pop-ups that hang on them.
fn remove_annotations(
    doc: &mut Document,
    page: ObjectId,
    areas: &[Rect4],
    stats: &mut RedactOutcome,
) {
    let list: Vec<Object> = match doc
        .get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
    {
        Some(Object::Array(items)) => items.clone(),
        Some(Object::Reference(r)) => match doc.get_object(*r) {
            Ok(Object::Array(items)) => items.clone(),
            _ => return,
        },
        _ => return,
    };
    let mut gone: HashSet<ObjectId> = HashSet::new();
    for item in &list {
        let Object::Reference(id) = item else {
            continue;
        };
        let Ok(dict) = doc.get_dictionary(*id) else {
            continue;
        };
        if rect_of(doc, dict).is_some_and(|r| areas.iter().any(|a| overlap(r, *a))) {
            gone.insert(*id);
        }
    }
    // Pop-ups of removed annotations, in either direction of the link.
    for item in &list {
        let Object::Reference(id) = item else {
            continue;
        };
        let Ok(dict) = doc.get_dictionary(*id) else {
            continue;
        };
        let parent_gone =
            matches!(dict.get(b"Parent"), Ok(Object::Reference(p)) if gone.contains(p));
        if parent_gone {
            gone.insert(*id);
        }
    }
    let popups: Vec<ObjectId> = gone
        .iter()
        .filter_map(|id| doc.get_dictionary(*id).ok())
        .filter_map(|d| match d.get(b"Popup") {
            Ok(Object::Reference(p)) => Some(*p),
            _ => None,
        })
        .collect();
    gone.extend(popups);
    if gone.is_empty() {
        return;
    }
    let mut seen = HashSet::new();
    for id in &gone {
        let is_widget = doc
            .get_dictionary(*id)
            .ok()
            .and_then(|d| d.get(b"Subtype").ok())
            .is_some_and(|s| matches!(s, Object::Name(n) if n == b"Widget"));
        if is_widget {
            detach_field(doc, *id, &mut seen);
        }
    }
    let kept: Vec<Object> = list
        .into_iter()
        .filter(|o| !matches!(o, Object::Reference(r) if gone.contains(r)))
        .collect();
    if let Ok(d) = doc.get_dictionary_mut(page) {
        d.set("Annots", Object::Array(kept));
    }
    stats.annotations += gone.len();
}

// ----- the pages -----------------------------------------------------------------------------

/// Cleans one page and covers its areas. Nothing of the page is changed when it cannot be
/// cleaned.
fn redact_page(
    doc: &mut Document,
    page: ObjectId,
    areas: &[Rect4],
    fill: Rgb,
) -> Result<RedactOutcome, Unsupported> {
    let bytes = doc
        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)
        .map_err(|_| Unsupported)?;
    let ops = Content::decode(&bytes).map_err(|_| Unsupported)?.operations;
    let mut scope = Scope::new(effective_resources(doc, page));
    let mut walker = Walker {
        doc: &mut *doc,
        areas,
        fill,
        stats: RedactOutcome::default(),
    };
    let ops = walker.walk(ops, &mut scope, IDENTITY, 0)?;
    let mut stats = walker.stats;
    let encoded = Content { operations: ops }
        .encode()
        .map_err(|_| Unsupported)?;

    let mut cover = String::from("q\n");
    cover.push_str(&format!(
        "{} {} {} rg\n",
        num(f32::from(fill.r) / 255.0),
        num(f32::from(fill.g) / 255.0),
        num(f32::from(fill.b) / 255.0)
    ));
    for a in areas {
        cover.push_str(&format!(
            "{} {} {} {} re f\n",
            num(a[0]),
            num(a[1]),
            num(a[2] - a[0]),
            num(a[3] - a[1])
        ));
    }
    cover.push_str("Q\n");

    let body = doc.add_object(Stream::new(Dictionary::new(), encoded));
    let top = doc.add_object(Stream::new(Dictionary::new(), cover.into_bytes()));
    let dict = doc.get_dictionary_mut(page).map_err(|_| Unsupported)?;
    dict.set(
        "Contents",
        Object::Array(vec![Object::Reference(body), Object::Reference(top)]),
    );
    // Thumbnails, private data and the page's own metadata keep copies of what was removed.
    dict.remove(b"Thumb");
    dict.remove(b"PieceInfo");
    dict.remove(b"Metadata");
    set_resources(doc, page, scope.resources).map_err(|_| Unsupported)?;
    remove_annotations(doc, page, areas, &mut stats);
    Ok(stats)
}

/// Removes what lies under the areas of `plan` from the content of the pages and draws boxes over
/// them. Pages that cannot be cleaned are left as they were and listed in the outcome.
pub fn redact(doc: &mut Document, plan: &RedactPlan) -> Result<RedactOutcome, AppError> {
    let mut outcome = RedactOutcome::default();
    for page_plan in &plan.pages {
        let id = page_object(doc, page_plan.page.get())?;
        let (dw, dh) = displayed_size(doc, id);
        let frame = display_matrix(doc, id);
        let areas: Vec<Rect4> = page_plan
            .regions
            .iter()
            .map(|r| {
                user_rect(
                    frame,
                    Boxed {
                        x: r.x * dw,
                        y: (1.0 - r.y - r.height) * dh,
                        w: r.width * dw,
                        h: r.height * dh,
                    },
                )
            })
            .collect();
        if areas.is_empty() {
            continue;
        }
        match redact_page(doc, id, &areas, plan.fill) {
            Ok(stats) => {
                outcome.glyphs += stats.glyphs;
                outcome.pictures += stats.pictures;
                outcome.paths += stats.paths;
                outcome.annotations += stats.annotations;
            }
            Err(Unsupported) => outcome.raster.push(page_plan.page),
        }
    }
    doc.prune_objects();
    Ok(outcome)
}

/// Replaces the whole content of pages with a picture of the page as displayed, and drops the
/// annotations and the data the pages carry. Each picture is a PNG or JPEG file.
pub fn replace_with_pictures(
    doc: &mut Document,
    pages: &[(domain::PageIndex, std::path::PathBuf)],
) -> Result<(), AppError> {
    for (index, path) in pages {
        let id = page_object(doc, index.get())?;
        let picture = crate::stamp::load_picture(doc, path)?;
        let (dw, dh) = displayed_size(doc, id);
        let frame = display_matrix(doc, id);
        let content = format!(
            "q\n{}{} 0 0 {} 0 0 cm\n/QwPage Do\nQ\n",
            crate::pageres::cm(frame),
            num(dw),
            num(dh)
        );
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let mut xobjects = Dictionary::new();
        xobjects.set("QwPage", Object::Reference(picture.id));
        let mut resources = Dictionary::new();
        resources.set("XObject", Object::Dictionary(xobjects));
        let dict = doc
            .get_dictionary_mut(id)
            .map_err(|_| AppError::internal("editing a page"))?;
        dict.set("Contents", Object::Reference(stream));
        dict.set("Resources", Object::Dictionary(resources));
        for key in ["Annots", "Thumb", "PieceInfo", "Metadata", "AA"] {
            dict.remove(key.as_bytes());
        }
    }
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::RedactPage;
    use domain::PageIndex;
    use domain::fonts::{FontFamily, StandardFont};
    use domain::tools::common::Region;
    use lopdf::dictionary;
    use test_support::pdfs::pdf_from_content;

    fn load(content: &str, rotate: u32) -> Document {
        Document::load_mem(&pdf_from_content(content, rotate)).unwrap()
    }

    /// The region of the displayed page that holds a user space rectangle of page 1.
    fn region_of(doc: &Document, rect: Rect4) -> Region {
        let id = doc.get_pages()[&1];
        let (dw, dh) = displayed_size(doc, id);
        let back = inverse(display_matrix(doc, id)).unwrap();
        let corners = [
            apply_m(back, rect[0], rect[1]),
            apply_m(back, rect[2], rect[1]),
            apply_m(back, rect[0], rect[3]),
            apply_m(back, rect[2], rect[3]),
        ];
        let b = bounds(&corners);
        Region {
            x: b[0] / dw,
            y: 1.0 - b[3] / dh,
            width: (b[2] - b[0]) / dw,
            height: (b[3] - b[1]) / dh,
        }
    }

    fn run(doc: &mut Document, rects: &[Rect4]) -> RedactOutcome {
        let plan = RedactPlan {
            pages: vec![RedactPage {
                page: PageIndex::new(0),
                regions: rects.iter().map(|r| region_of(doc, *r)).collect(),
            }],
            fill: Rgb::BLACK,
        };
        redact(doc, &plan).unwrap()
    }

    fn operations(doc: &Document) -> Vec<Operation> {
        let id = doc.get_pages()[&1];
        Content::decode(&doc.get_page_content(id))
            .unwrap()
            .operations
    }

    /// What the text operators of page 1 show, joined.
    fn shown(doc: &Document) -> String {
        let mut text = String::new();
        for op in operations(doc) {
            for operand in &op.operands {
                match operand {
                    Object::String(b, _) if matches!(op.operator.as_str(), "Tj" | "'" | "\"") => {
                        text.push_str(&String::from_utf8_lossy(b));
                    }
                    Object::Array(items) if op.operator == "TJ" => {
                        for item in items {
                            if let Object::String(b, _) = item {
                                text.push_str(&String::from_utf8_lossy(b));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        text
    }

    /// The numbers inside the TJ arrays of page 1, added up.
    fn gaps(doc: &Document) -> f32 {
        operations(doc)
            .iter()
            .filter(|o| o.operator == "TJ")
            .flat_map(|o| o.operands.clone())
            .filter_map(|o| match o {
                Object::Array(items) => Some(items),
                _ => None,
            })
            .flatten()
            .filter_map(|o| o.as_float().ok())
            .sum()
    }

    /// Whether the text appears in any stream or string anywhere in the document.
    fn anywhere(doc: &Document, needle: &str) -> bool {
        doc.objects.values().any(|object| match object {
            Object::Stream(s) => {
                let data = s
                    .decompressed_content()
                    .unwrap_or_else(|_| s.content.clone());
                String::from_utf8_lossy(&data).contains(needle)
            }
            Object::String(b, _) => String::from_utf8_lossy(b).contains(needle),
            _ => false,
        })
    }

    fn helvetica(text: &str, size: f32) -> f32 {
        StandardFont::new(FontFamily::Helvetica, false, false).text_width(text, size)
    }

    fn operators(doc: &Document) -> Vec<String> {
        operations(doc).into_iter().map(|o| o.operator).collect()
    }

    #[test]
    fn text_under_an_area_is_taken_out_and_the_rest_does_not_move() {
        let mut doc = load("BT /F1 12 Tf 100 700 Td (Hello SECRET world) Tj ET\n", 0);
        let x0 = 100.0 + helvetica("Hello ", 12.0);
        let x1 = x0 + helvetica("SECRET", 12.0);
        let outcome = run(&mut doc, &[[x0, 695.0, x1, 712.0]]);
        assert_eq!(outcome.glyphs, 6);
        assert!(outcome.raster.is_empty());
        assert_eq!(shown(&doc), "Hello  world");
        assert!(!anywhere(&doc, "SECRET"));
        // The six characters are replaced by their advance (S E C R E T = 4056 units), so the
        // text after them stays where it was.
        assert!((gaps(&doc) + 4056.0).abs() < 1.0, "gap {}", gaps(&doc));
        // Everything else is kept, and a box covers the area, drawn last.
        assert_eq!(
            operators(&doc),
            ["BT", "Tf", "Td", "TJ", "ET", "q", "rg", "re", "f", "Q"]
        );
    }

    #[test]
    fn a_neighbor_that_only_touches_the_area_stays() {
        let mut doc = load("BT /F1 12 Tf 100 700 Td (ab SECRET cd) Tj ET\n", 0);
        let x0 = 100.0 + helvetica("ab ", 12.0);
        let x1 = x0 + helvetica("SECRET", 12.0);
        run(&mut doc, &[[x0, 695.0, x1, 712.0]]);
        assert_eq!(shown(&doc), "ab  cd");
    }

    #[test]
    fn text_elsewhere_and_the_positions_of_later_lines_are_untouched() {
        let mut doc = load(
            "BT /F1 12 Tf 100 700 Td (keep) Tj 0 -20 Td (drop) Tj 0 -20 Td (also kept) Tj ET\n",
            0,
        );
        let outcome = run(&mut doc, &[[95.0, 676.0, 140.0, 692.0]]);
        assert_eq!(outcome.glyphs, 4);
        assert_eq!(shown(&doc), "keepalso kept");
        assert!(!anywhere(&doc, "drop"));
    }

    #[test]
    fn kerned_arrays_quote_operators_spacing_and_transforms_are_followed() {
        // TJ with kerning, a quote operator with leading, character and word spacing, under a
        // scaling transform.
        let content = "q 2 0 0 2 0 0 cm BT /F1 10 Tf 12 TL 1 Tc 3 Tw 20 300 Td [(A) -250 (SEC) 50 (RET)] TJ (next) ' ET Q\n";
        let mut doc = load(content, 0);
        // The first line starts at (40, 600) after the transform and is 20 pt tall. Cover
        // everything after the "A" and its kerning.
        let a = 2.0 * (helvetica("A", 10.0) + 1.0 + 2.5);
        let outcome = run(&mut doc, &[[40.0 + a + 2.0, 590.0, 600.0, 625.0]]);
        assert!(outcome.glyphs >= 6, "{outcome:?}");
        let text = shown(&doc);
        assert!(text.starts_with('A'), "{text}");
        assert!(!text.contains("SEC") && !text.contains("RET"), "{text}");
        assert!(!anywhere(&doc, "SEC"));
    }

    #[test]
    fn two_byte_fonts_are_followed_by_their_widths() {
        let mut doc = load("", 0);
        let id = doc.get_pages()[&1];
        let descendant = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "DW" => 500,
            "W" => vec![Object::Integer(1), Object::Array(vec![Object::Integer(1000)])],
        });
        let composite = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Custom",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)],
        });
        let resources = dictionary! {
            "Font" => dictionary! {"F9" => Object::Reference(composite)},
        };
        let stream = doc.add_object(Stream::new(
            Dictionary::new(),
            b"BT /F9 10 Tf 100 700 Td <0001000200030004> Tj ET\n".to_vec(),
        ));
        let page = doc.get_dictionary_mut(id).unwrap();
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(stream));
        // Glyph 1 is 10 pt wide, the others 5 pt: cover the second and third.
        let outcome = run(&mut doc, &[[110.5, 695.0, 119.5, 712.0]]);
        assert_eq!(outcome.glyphs, 2);
        let kept: Vec<u8> = operations(&doc)
            .iter()
            .filter(|o| o.operator == "TJ")
            .flat_map(|o| o.operands.clone())
            .filter_map(|o| match o {
                Object::Array(items) => Some(items),
                _ => None,
            })
            .flatten()
            .filter_map(|o| match o {
                Object::String(b, _) => Some(b),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(kept, vec![0, 1, 0, 4]);
    }

    fn odd_font(doc: &mut Document) {
        let id = doc.get_pages()[&1];
        let odd = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Odd",
        });
        let resources = dictionary! {"Font" => dictionary! {"F1" => Object::Reference(odd)}};
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Resources", Object::Dictionary(resources));
    }

    #[test]
    fn a_font_that_cannot_be_measured_makes_a_page_with_text_on_the_area_unsafe() {
        let mut doc = load("BT /F1 12 Tf 100 700 Td (Hello) Tj ET\n", 0);
        odd_font(&mut doc);
        let id = doc.get_pages()[&1];
        let before = doc.get_page_content(id);
        let outcome = run(&mut doc, &[[90.0, 690.0, 200.0, 715.0]]);
        assert_eq!(outcome.raster, vec![PageIndex::new(0)]);
        // The page was left as it was.
        assert_eq!(doc.get_page_content(id), before);
        // An area on another line is no problem.
        let mut far = load("BT /F1 12 Tf 100 700 Td (Hello) Tj ET\n", 0);
        odd_font(&mut far);
        let outcome = run(&mut far, &[[90.0, 100.0, 200.0, 120.0]]);
        assert!(outcome.raster.is_empty());
    }

    #[test]
    fn shapes_that_meet_an_area_are_not_painted_but_a_fill_around_it_stays() {
        let content = "1 0 0 rg 100 100 50 50 re f\n0 0 1 rg 0 0 600 790 re f\n0 0 0 RG 4 w 110 400 m 190 400 l S\n";
        let mut doc = load(content, 0);
        let outcome = run(
            &mut doc,
            &[[140.0, 110.0, 160.0, 120.0], [150.0, 395.0, 160.0, 405.0]],
        );
        // The red rectangle is cut by the first area, and the line crosses the second; the blue
        // page-sized fill surrounds both and stays.
        assert_eq!(outcome.paths, 2, "{outcome:?}");
        let ops = operators(&doc);
        // The blue fill, and the cover of each of the two areas.
        assert_eq!(
            ops.iter().filter(|o| o.as_str() == "f").count(),
            3,
            "{ops:?}"
        );
        assert!(ops.iter().filter(|o| o.as_str() == "n").count() >= 2);
        assert!(!ops.iter().any(|o| o == "S"));
    }

    #[test]
    fn the_sides_of_a_stroked_frame_stay_when_the_area_is_inside_it() {
        let mut doc = load("0 0 0 RG 2 w 100 100 200 100 re S\n", 0);
        let outcome = run(&mut doc, &[[150.0, 130.0, 250.0, 170.0]]);
        assert_eq!(outcome.paths, 0);
        assert!(operators(&doc).iter().any(|o| o == "S"));
    }

    fn picture(doc: &mut Document) -> ObjectId {
        // 2 by 2 pixels: red, green / blue, white, stored without a filter.
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Image".to_vec()));
        dict.set("Width", Object::Integer(2));
        dict.set("Height", Object::Integer(2));
        dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
        dict.set("BitsPerComponent", Object::Integer(8));
        doc.add_object(Stream::new(
            dict,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        ))
    }

    fn with_content(doc: &mut Document, content: &str, xobjects: &[(&str, ObjectId)]) {
        let id = doc.get_pages()[&1];
        let mut table = Dictionary::new();
        for (name, object) in xobjects {
            table.set(*name, Object::Reference(*object));
        }
        let resources = dictionary! {"XObject" => table};
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        let page = doc.get_dictionary_mut(id).unwrap();
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(stream));
    }

    fn xobject_of(doc: &Document, name: &str) -> Stream {
        let id = doc.get_pages()[&1];
        let resources = effective_resources(doc, id);
        let table = resources.get(b"XObject").unwrap().as_dict().unwrap();
        let reference = table.get(name.as_bytes()).unwrap().as_reference().unwrap();
        doc.get_object(reference)
            .unwrap()
            .as_stream()
            .unwrap()
            .clone()
    }

    #[test]
    fn pixels_under_an_area_are_cleared_and_the_original_picture_leaves_the_file() {
        let mut doc = load("", 0);
        let image = picture(&mut doc);
        // The picture covers (100, 400) to (300, 600); the area is its lower left pixel.
        with_content(
            &mut doc,
            "q 200 0 0 200 100 400 cm /Im1 Do Q\n",
            &[("Im1", image)],
        );
        let outcome = run(&mut doc, &[[100.5, 400.5, 190.0, 490.0]]);
        assert_eq!(outcome.pictures, 1);
        let id = doc.get_pages()[&1];
        let resources = effective_resources(&doc, id);
        let table = resources.get(b"XObject").unwrap().as_dict().unwrap();
        assert!(table.get(b"Im1").is_err(), "the original is still listed");
        let cleaned = xobject_of(&doc, "QwP0");
        let data = cleaned
            .decompressed_content()
            .unwrap_or(cleaned.content.clone());
        // Bottom left pixel (blue) is now black; the others are as they were.
        assert_eq!(data, vec![255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 255, 255]);
        assert!(
            doc.get_object(image).is_err(),
            "the original picture is still in the file"
        );
    }

    #[test]
    fn a_picture_drawn_twice_keeps_its_original_for_the_other_place() {
        let mut doc = load("", 0);
        let image = picture(&mut doc);
        with_content(
            &mut doc,
            "q 100 0 0 100 100 400 cm /Im1 Do Q q 100 0 0 100 400 400 cm /Im1 Do Q\n",
            &[("Im1", image)],
        );
        let outcome = run(&mut doc, &[[100.5, 400.5, 190.0, 490.0]]);
        assert_eq!(outcome.pictures, 1);
        let id = doc.get_pages()[&1];
        let resources = effective_resources(&doc, id);
        let table = resources.get(b"XObject").unwrap().as_dict().unwrap();
        assert!(table.get(b"Im1").is_ok());
        assert!(table.get(b"QwP0").is_ok());
    }

    #[test]
    fn a_picture_that_cannot_be_decoded_makes_the_page_unsafe() {
        let mut doc = load("", 0);
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Image".to_vec()));
        dict.set("Width", Object::Integer(8));
        dict.set("Height", Object::Integer(8));
        dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        dict.set("BitsPerComponent", Object::Integer(1));
        dict.set("Filter", Object::Name(b"CCITTFaxDecode".to_vec()));
        let scan = doc.add_object(Stream::new(dict, vec![0; 8]));
        with_content(
            &mut doc,
            "q 100 0 0 100 100 400 cm /Im1 Do Q\n",
            &[("Im1", scan)],
        );
        let outcome = run(&mut doc, &[[120.0, 420.0, 150.0, 450.0]]);
        assert_eq!(outcome.raster, vec![PageIndex::new(0)]);
        // A picture elsewhere on the page is not a problem.
        let outcome = run(&mut doc, &[[400.0, 100.0, 450.0, 150.0]]);
        assert!(outcome.raster.is_empty());
    }

    #[test]
    fn text_inside_a_form_is_cleaned_on_a_copy_and_the_original_leaves_the_file() {
        let mut doc = load("", 0);
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Form".to_vec()));
        dict.set(
            "BBox",
            Object::Array(vec![0.into(), 0.into(), 300.into(), 100.into()]),
        );
        dict.set(
            "Resources",
            Object::Dictionary(dictionary! {
                "Font" => dictionary! {"F1" => Object::Reference(font)},
            }),
        );
        let form = doc.add_object(Stream::new(
            dict,
            b"BT /F1 12 Tf 0 20 Td (SECRET) Tj ET\n".to_vec(),
        ));
        with_content(
            &mut doc,
            "q 1 0 0 1 100 600 cm /Fm1 Do Q\n",
            &[("Fm1", form)],
        );
        let outcome = run(&mut doc, &[[95.0, 610.0, 160.0, 640.0]]);
        assert_eq!(outcome.glyphs, 6);
        assert!(!anywhere(&doc, "SECRET"));
        assert!(doc.get_object(form).is_err());
        let copy = xobject_of(&doc, "QwF0");
        let data = copy.decompressed_content().unwrap_or(copy.content.clone());
        assert!(!String::from_utf8_lossy(&data).contains("SECRET"));
    }

    #[test]
    fn a_form_far_from_the_areas_is_left_alone() {
        let mut doc = load("", 0);
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Form".to_vec()));
        dict.set(
            "BBox",
            Object::Array(vec![0.into(), 0.into(), 100.into(), 100.into()]),
        );
        let form = doc.add_object(Stream::new(dict, b"0 0 50 50 re f\n".to_vec()));
        with_content(
            &mut doc,
            "q 1 0 0 1 100 600 cm /Fm1 Do Q\n",
            &[("Fm1", form)],
        );
        run(&mut doc, &[[400.0, 100.0, 450.0, 150.0]]);
        assert!(doc.get_object(form).is_ok());
        assert!(xobject_of(&doc, "Fm1").content.starts_with(b"0 0 50 50"));
    }

    #[test]
    fn a_picture_inside_the_content_makes_the_page_unsafe() {
        let mut doc = load(
            "q 10 0 0 10 100 100 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \0 EI Q\n",
            0,
        );
        let id = doc.get_pages()[&1];
        let before = doc.get_page_content(id);
        let outcome = run(&mut doc, &[[90.0, 90.0, 200.0, 200.0]]);
        assert_eq!(outcome.raster, vec![PageIndex::new(0)]);
        assert_eq!(doc.get_page_content(id), before);
    }

    #[test]
    fn marked_content_alternate_text_is_dropped() {
        let content =
            "/Span <</ActualText (SECRET words)>> BDC BT /F1 12 Tf 100 700 Td (SECRET) Tj ET EMC\n";
        let mut doc = load(content, 0);
        run(&mut doc, &[[95.0, 695.0, 160.0, 715.0]]);
        assert!(!anywhere(&doc, "SECRET"));
    }

    #[test]
    fn annotations_and_fields_under_an_area_go_with_their_values() {
        let mut doc =
            Document::load_mem(&test_support::pdfs::pdf_with_form_field("Name", "SECRET")).unwrap();
        let id = doc.get_pages()[&1];
        let annots: Vec<ObjectId> = match doc.get_dictionary(id).unwrap().get(b"Annots").unwrap() {
            Object::Array(items) => items.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => Vec::new(),
        };
        let widget = annots[0];
        let rect = rect_of(&doc, doc.get_dictionary(widget).unwrap()).unwrap();
        let outcome = run(&mut doc, &[rect]);
        assert_eq!(outcome.annotations, 1);
        assert!(!anywhere(&doc, "SECRET"));
        assert!(crate::forms::list_fields(&doc).is_empty());
    }

    #[test]
    fn rotated_pages_take_areas_as_they_are_displayed() {
        for rotate in [90, 180, 270] {
            let mut doc = load(
                "BT /F1 12 Tf 100 700 Td (Hello SECRET world) Tj ET\n",
                rotate,
            );
            let x0 = 100.0 + helvetica("Hello ", 12.0);
            let x1 = x0 + helvetica("SECRET", 12.0);
            let outcome = run(&mut doc, &[[x0, 695.0, x1, 712.0]]);
            assert_eq!(outcome.glyphs, 6, "rotate {rotate}");
            assert_eq!(shown(&doc), "Hello  world", "rotate {rotate}");
        }
    }

    #[test]
    fn thumbnails_and_page_metadata_are_dropped() {
        let mut doc = load("BT /F1 12 Tf 100 700 Td (SECRET) Tj ET\n", 0);
        let id = doc.get_pages()[&1];
        let thumb = doc.add_object(Stream::new(Dictionary::new(), b"thumbnail".to_vec()));
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Thumb", Object::Reference(thumb));
        run(&mut doc, &[[95.0, 695.0, 160.0, 715.0]]);
        assert!(doc.get_dictionary(id).unwrap().get(b"Thumb").is_err());
        assert!(!anywhere(&doc, "thumbnail"));
    }

    #[test]
    fn a_page_is_replaced_by_a_picture_of_itself() {
        let mut doc = load("BT /F1 12 Tf 100 700 Td (SECRET) Tj ET\n", 90);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.png");
        let mut png = Vec::new();
        {
            use image::ImageEncoder;
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&[255u8; 4 * 4 * 3], 4, 4, image::ExtendedColorType::Rgb8)
                .unwrap();
        }
        std::fs::write(&path, png).unwrap();
        replace_with_pictures(&mut doc, &[(PageIndex::new(0), path)]).unwrap();
        assert!(!anywhere(&doc, "SECRET"));
        let id = doc.get_pages()[&1];
        let text = String::from_utf8_lossy(&doc.get_page_content(id)).into_owned();
        assert!(text.contains("/QwPage Do"), "{text}");
    }
}
