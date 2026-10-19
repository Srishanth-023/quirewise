//! Form fields: listing them, filling them and adding new ones.
//!
//! Fields are read from the AcroForm tree (names, inherited properties, widgets and the pages
//! they sit on). Filling sets the field value and writes a fresh appearance for every widget, so
//! the result looks right in readers that do not regenerate appearances; `NeedAppearances` is set
//! as well for the ones that do. Created fields come with appearances of their own.

use application::AppError;
use domain::ErrorCode;
use domain::forms::{FormField, FormFieldKind};
use domain::tools::common::Region;
use domain::tools::editor::{FieldValue, NewField, NewFieldKind};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, HashMap};

use crate::annot::{add_to_page, color_op, form_xobject, read_text, rect_array, text_string};
use crate::boxes::{display_matrix, displayed_size};
use crate::fonts::Face;
use crate::pageres::{num, page_object};
use crate::paint::wrap;
use crate::stamp::literal;
use domain::fonts::{FontFamily, StandardFont};

const FF_READ_ONLY: i64 = 1;
const FF_REQUIRED: i64 = 1 << 1;
const FF_MULTILINE: i64 = 1 << 12;
const FF_RADIO: i64 = 1 << 15;
const FF_PUSHBUTTON: i64 = 1 << 16;
const FF_NO_TOGGLE_OFF: i64 = 1 << 14;
const FF_COMBO: i64 = 1 << 17;
const FF_EDIT: i64 = 1 << 18;
/// Depth of the field tree that is followed, against cyclic files.
const MAX_DEPTH: usize = 32;
/// Size text is set at when the field leaves it to the reader and the box is tall enough.
const AUTO_SIZE: f32 = 12.0;
const FILL: &str = "#F4F8FF";
const BORDER: &str = "#6B7280";

/// What a field passes down to its children.
#[derive(Clone, Default)]
struct Inherited {
    ft: Option<Vec<u8>>,
    ff: i64,
    value: Option<Object>,
    options: Option<Object>,
    da: Option<String>,
    max_len: Option<i64>,
    quad: i64,
}

/// One widget of a terminal field.
#[derive(Clone)]
struct Raw {
    field: ObjectId,
    widget: ObjectId,
    name: String,
    inherited: Inherited,
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn acro_form_id(doc: &Document) -> Option<ObjectId> {
    let root = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    doc.get_dictionary(root)
        .ok()?
        .get(b"AcroForm")
        .ok()?
        .as_reference()
        .ok()
}

fn acro_form(doc: &Document) -> Option<&Dictionary> {
    if let Some(id) = acro_form_id(doc) {
        return doc.get_dictionary(id).ok();
    }
    let root = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    doc.get_dictionary(root)
        .ok()?
        .get(b"AcroForm")
        .ok()?
        .as_dict()
        .ok()
}

fn root_fields(doc: &Document) -> Vec<ObjectId> {
    let Some(form) = acro_form(doc) else {
        return Vec::new();
    };
    let Some(list) = form.get(b"Fields").ok().and_then(|o| resolve(doc, o)) else {
        return Vec::new();
    };
    list.as_array()
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default()
}

fn walk(
    doc: &Document,
    id: ObjectId,
    prefix: &str,
    inherited: &Inherited,
    out: &mut Vec<Raw>,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(dict) = doc.get_dictionary(id) else {
        return;
    };
    let mut inh = inherited.clone();
    if let Ok(ft) = dict.get(b"FT").and_then(Object::as_name) {
        inh.ft = Some(ft.to_vec());
    }
    if let Ok(ff) = dict.get(b"Ff").and_then(Object::as_i64) {
        inh.ff = ff;
    }
    if let Ok(v) = dict.get(b"V") {
        inh.value = Some(v.clone());
    }
    if let Ok(o) = dict.get(b"Opt") {
        inh.options = Some(o.clone());
    }
    if let Ok(Object::String(da, _)) = dict.get(b"DA") {
        inh.da = Some(String::from_utf8_lossy(da).into_owned());
    }
    if let Ok(m) = dict.get(b"MaxLen").and_then(Object::as_i64) {
        inh.max_len = Some(m);
    }
    if let Ok(q) = dict.get(b"Q").and_then(Object::as_i64) {
        inh.quad = q;
    }
    let part = dict.get(b"T").map(read_text).unwrap_or_default();
    let name = match (prefix.is_empty(), part.is_empty()) {
        (_, true) => prefix.to_owned(),
        (true, false) => part,
        (false, false) => format!("{prefix}.{part}"),
    };
    let kids: Vec<ObjectId> = dict
        .get(b"Kids")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let is_field = |kid: &ObjectId| doc.get_dictionary(*kid).is_ok_and(|d| d.get(b"T").is_ok());
    if kids.iter().any(is_field) {
        for kid in kids.iter().filter(|k| is_field(k)) {
            walk(doc, *kid, &name, &inh, out, depth + 1);
        }
        return;
    }
    let widgets = if kids.is_empty() { vec![id] } else { kids };
    for widget in widgets {
        out.push(Raw {
            field: id,
            widget,
            name: name.clone(),
            inherited: inh.clone(),
        });
    }
}

fn collect(doc: &Document) -> Vec<Raw> {
    let mut out = Vec::new();
    for id in root_fields(doc) {
        walk(doc, id, "", &Inherited::default(), &mut out, 0);
    }
    out
}

fn kind_of(inh: &Inherited) -> Option<FormFieldKind> {
    Some(match inh.ft.as_deref()? {
        b"Tx" => FormFieldKind::Text,
        b"Sig" => FormFieldKind::Signature,
        b"Ch" if inh.ff & FF_COMBO != 0 => FormFieldKind::Dropdown,
        b"Ch" => FormFieldKind::ListBox,
        b"Btn" if inh.ff & FF_PUSHBUTTON != 0 => FormFieldKind::Button,
        b"Btn" if inh.ff & FF_RADIO != 0 => FormFieldKind::Radio,
        b"Btn" => FormFieldKind::Checkbox,
        _ => return None,
    })
}

/// The choices of a list: the text shown for each, and the value it stands for.
fn choices(doc: &Document, inh: &Inherited) -> Vec<(String, String)> {
    let Some(list) = inh.options.as_ref().and_then(|o| resolve(doc, o)) else {
        return Vec::new();
    };
    let Ok(items) = list.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let item = resolve(doc, item)?;
            match item {
                Object::Array(pair) if pair.len() >= 2 => {
                    let export = resolve(doc, &pair[0]).map(read_text)?;
                    let shown = resolve(doc, &pair[1]).map(read_text)?;
                    Some((shown, export))
                }
                other => {
                    let text = read_text(other);
                    Some((text.clone(), text))
                }
            }
        })
        .collect()
}

/// The name of a widget's "on" appearance: the state that is not `Off`.
fn on_state(doc: &Document, widget: ObjectId) -> Option<String> {
    let dict = doc.get_dictionary(widget).ok()?;
    let ap = resolve(doc, dict.get(b"AP").ok()?)?.as_dict().ok()?;
    let normal = resolve(doc, ap.get(b"N").ok()?)?.as_dict().ok()?;
    normal
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .find(|k| k != "Off")
}

fn value_text(doc: &Document, inh: &Inherited) -> String {
    inh.value
        .as_ref()
        .and_then(|v| resolve(doc, v))
        .map(read_text)
        .unwrap_or_default()
}

fn number(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn widget_rect(doc: &Document, widget: ObjectId) -> Option<[f32; 4]> {
    let dict = doc.get_dictionary(widget).ok()?;
    let items = resolve(doc, dict.get(b"Rect").ok()?)?.as_array().ok()?;
    let values: Vec<f32> = items
        .iter()
        .filter_map(|o| resolve(doc, o).and_then(number))
        .collect();
    let [a, b, c, d] = <[f32; 4]>::try_from(values).ok()?;
    Some([a.min(c), b.min(d), a.max(c), b.max(d)])
}

/// Which page each widget is on: from its `/P`, or from the page lists.
fn widget_pages(doc: &Document) -> HashMap<ObjectId, u32> {
    let mut map = HashMap::new();
    for (number, page) in doc.get_pages() {
        let Some(list) = doc
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_array().ok())
        else {
            continue;
        };
        for annot in list {
            if let Ok(id) = annot.as_reference() {
                map.insert(id, number);
            }
        }
    }
    map
}

/// A user space rectangle as a region of the displayed page.
fn region_of(doc: &Document, page: ObjectId, rect: [f32; 4]) -> Option<Region> {
    let m = display_matrix(doc, page);
    let (w, h) = displayed_size(doc, page);
    let to_displayed = |ux: f32, uy: f32| {
        let (dx, dy) = (ux - m[4], uy - m[5]);
        (m[0] * dx + m[1] * dy, m[2] * dx + m[3] * dy)
    };
    let (ax, ay) = to_displayed(rect[0], rect[1]);
    let (bx, by) = to_displayed(rect[2], rect[3]);
    let (x0, x1) = (ax.min(bx), ax.max(bx));
    let (y0, y1) = (ay.min(by), ay.max(by));
    let region = Region {
        x: (x0 / w).clamp(0.0, 1.0),
        y: (1.0 - y1 / h).clamp(0.0, 1.0),
        width: ((x1 - x0) / w).clamp(0.001, 1.0),
        height: ((y1 - y0) / h).clamp(0.001, 1.0),
    };
    Some(region)
}

/// The fields of the form, one entry for every widget.
pub fn list_fields(doc: &Document) -> Vec<FormField> {
    let pages = widget_pages(doc);
    let page_ids = doc.get_pages();
    let mut out = Vec::new();
    for raw in collect(doc) {
        let Some(kind) = kind_of(&raw.inherited) else {
            continue;
        };
        let Some(&page) = pages.get(&raw.widget) else {
            continue;
        };
        let (Some(rect), Some(&page_id)) = (widget_rect(doc, raw.widget), page_ids.get(&page))
        else {
            continue;
        };
        let Some(region) = region_of(doc, page_id, rect) else {
            continue;
        };
        let inh = &raw.inherited;
        let (value, options) = match kind {
            FormFieldKind::Checkbox => {
                let state = doc
                    .get_dictionary(raw.widget)
                    .ok()
                    .and_then(|d| d.get(b"AS").ok())
                    .and_then(|o| o.as_name().ok().map(<[u8]>::to_vec))
                    .or_else(|| {
                        inh.value
                            .as_ref()
                            .and_then(|v| v.as_name().ok().map(<[u8]>::to_vec))
                    });
                let on = state.is_some_and(|s| s != b"Off");
                (on.to_string(), Vec::new())
            }
            FormFieldKind::Radio => {
                let mine = on_state(doc, raw.widget).unwrap_or_default();
                let chosen = inh
                    .value
                    .as_ref()
                    .and_then(|v| v.as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).into_owned())
                    .unwrap_or_default();
                (chosen, vec![mine])
            }
            FormFieldKind::Dropdown | FormFieldKind::ListBox => (
                value_text(doc, inh),
                choices(doc, inh)
                    .into_iter()
                    .map(|(shown, _)| shown)
                    .collect(),
            ),
            _ => (value_text(doc, inh), Vec::new()),
        };
        out.push(FormField {
            name: raw.name.clone(),
            kind,
            page,
            region,
            value,
            options,
            read_only: inh.ff & FF_READ_ONLY != 0,
            required: inh.ff & FF_REQUIRED != 0,
            multiline: inh.ff & FF_MULTILINE != 0,
        });
    }
    out
}

// ----- appearances -------------------------------------------------------------------------

/// The font every generated appearance uses: Helvetica, from the form's resources when it has it.
fn helvetica(doc: &mut Document) -> ObjectId {
    if let Some(form) = acro_form(doc)
        && let Some(font) = form
            .get(b"DR")
            .ok()
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Font").ok())
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Helv").ok())
            .and_then(|o| o.as_reference().ok())
    {
        return font;
    }
    Face::Standard(StandardFont::new(FontFamily::Helvetica, false, false)).write(doc)
}

fn da_parts(da: Option<&str>) -> (f32, String) {
    let mut size = 0.0;
    let mut color = "0 g".to_owned();
    if let Some(da) = da {
        let tokens: Vec<&str> = da.split_whitespace().collect();
        for (i, t) in tokens.iter().enumerate() {
            if *t == "Tf" && i >= 1 {
                size = tokens[i - 1].parse().unwrap_or(0.0);
            }
            if matches!(*t, "g" | "rg" | "k") {
                let n = match *t {
                    "g" => 1,
                    "rg" => 3,
                    _ => 4,
                };
                if i >= n {
                    color = format!("{} {t}", tokens[i - n..i].join(" "));
                }
            }
        }
    }
    (size, color)
}

fn border_and_fill(w: f32, h: f32, fill: bool, border: bool) -> String {
    let mut s = String::new();
    if fill {
        s.push_str(&color_op(FILL, false).unwrap_or_default());
        s.push_str(&format!("0 0 {} {} re f\n", num(w), num(h)));
    }
    if border {
        s.push_str(&color_op(BORDER, true).unwrap_or_default());
        s.push_str(&format!(
            "0.75 w 0.375 0.375 {} {} re S\n",
            num(w - 0.75),
            num(h - 0.75)
        ));
    }
    s
}

struct TextLook<'a> {
    value: &'a str,
    size: f32,
    color: &'a str,
    multiline: bool,
    quad: i64,
    fill: bool,
    border: bool,
}

/// The appearance of a text-like widget of the given size.
fn text_appearance(doc: &mut Document, w: f32, h: f32, look: &TextLook<'_>) -> ObjectId {
    let face = Face::Standard(StandardFont::new(FontFamily::Helvetica, false, false));
    let size = if look.size > 0.0 {
        look.size
    } else if look.multiline {
        AUTO_SIZE
    } else {
        (h * 0.65).clamp(4.0, AUTO_SIZE)
    };
    let mut content = border_and_fill(w, h, look.fill, look.border);
    if !look.value.is_empty() {
        // Characters WinAnsi lacks are drawn as a question mark rather than dropped.
        let safe: String = look
            .value
            .chars()
            .map(|c| {
                if StandardFont::encode_char(c).is_some() {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        content.push_str(&format!(
            "/Tx BMC\nq\n2 2 {} {} re W n\n",
            num(w - 4.0),
            num(h - 4.0)
        ));
        content.push_str(&format!("{}\nBT\n/Helv {} Tf\n", look.color, num(size)));
        let lines = if look.multiline {
            wrap(&face, &safe, size, w - 4.0)
        } else {
            vec![safe.replace(['\r', '\n'], " ")]
        };
        let mut y = if look.multiline {
            h - 2.0 - size * 0.8
        } else {
            (h - size * 0.72) / 2.0
        };
        let (mut last_x, mut last_y) = (0.0f32, 0.0f32);
        for line in &lines {
            let width = face.width(line, size);
            let x = match look.quad {
                1 => (w - width) / 2.0,
                2 => w - 2.0 - width,
                _ => 2.0,
            };
            let bytes = face.encode(line).unwrap_or_default();
            content.push_str(&format!(
                "{} {} Td\n{} Tj\n",
                num(x - last_x),
                num(y - last_y),
                literal(&bytes)
            ));
            last_x = x;
            last_y = y;
            y -= size * 1.2;
        }
        content.push_str("ET\nQ\nEMC\n");
    }
    let helv = helvetica(doc);
    let mut fonts = Dictionary::new();
    fonts.set("Helv", Object::Reference(helv));
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));
    form_xobject(doc, [0.0, 0.0, w, h], content, resources)
}

fn set_appearance(doc: &mut Document, widget: ObjectId, normal: Object) {
    let mut ap = Dictionary::new();
    ap.set("N", normal);
    if let Ok(dict) = doc.get_dictionary_mut(widget) {
        dict.set("AP", Object::Dictionary(ap));
    }
}

fn size_of(rect: [f32; 4]) -> (f32, f32) {
    ((rect[2] - rect[0]).abs(), (rect[3] - rect[1]).abs())
}

fn set_need_appearances(doc: &mut Document) {
    let Some(id) = acro_form_id(doc) else {
        if let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference)
            && let Ok(catalog) = doc.get_dictionary_mut(root)
            && let Ok(Object::Dictionary(form)) = catalog.get_mut(b"AcroForm")
        {
            form.set("NeedAppearances", Object::Boolean(true));
        }
        return;
    };
    if let Ok(form) = doc.get_dictionary_mut(id) {
        form.set("NeedAppearances", Object::Boolean(true));
    }
}

// ----- filling -----------------------------------------------------------------------------

fn field_error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::OptionInvalid, message)
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1" | "checked" | "x"
    )
}

/// Sets the values of fields and rewrites their appearances.
pub fn fill(doc: &mut Document, values: &[FieldValue]) -> Result<(), AppError> {
    let raws = collect(doc);
    if raws.is_empty() {
        return Err(field_error("This document has no form fields to fill."));
    }
    for value in values {
        let mine: Vec<Raw> = raws
            .iter()
            .filter(|r| r.name == value.name)
            .cloned()
            .collect();
        let Some(first) = mine.first() else {
            let mut names: Vec<&str> = raws.iter().map(|r| r.name.as_str()).collect();
            names.dedup();
            let list = names.iter().take(6).copied().collect::<Vec<_>>().join(", ");
            return Err(field_error(format!(
                "The form has no field named \"{}\". Fields it has: {list}.",
                value.name
            )));
        };
        let inh = first.inherited.clone();
        if inh.ff & FF_READ_ONLY != 0 {
            return Err(field_error(format!(
                "The field \"{}\" cannot be changed.",
                value.name
            )));
        }
        let Some(kind) = kind_of(&inh) else {
            return Err(field_error(format!(
                "The field \"{}\" cannot be filled.",
                value.name
            )));
        };
        match kind {
            FormFieldKind::Text => {
                if let Some(max) = inh.max_len
                    && max > 0
                    && value.value.chars().count() as i64 > max
                {
                    return Err(field_error(format!(
                        "The field \"{}\" takes at most {max} characters.",
                        value.name
                    )));
                }
                set_value(doc, first.field, text_string(&value.value));
                for raw in &mine {
                    write_text_widget(doc, raw, &value.value);
                }
            }
            FormFieldKind::Dropdown | FormFieldKind::ListBox => {
                let list = choices(doc, &inh);
                let stored = match list
                    .iter()
                    .find(|(shown, export)| *shown == value.value || *export == value.value)
                {
                    Some((_, export)) => export.clone(),
                    None if inh.ff & FF_EDIT != 0 => value.value.clone(),
                    None => {
                        return Err(field_error(format!(
                            "\"{}\" is not one of the choices of the field \"{}\".",
                            value.value, value.name
                        )));
                    }
                };
                let shown = list
                    .iter()
                    .find(|(_, export)| *export == stored)
                    .map_or(stored.clone(), |(s, _)| s.clone());
                set_value(doc, first.field, text_string(&stored));
                for raw in &mine {
                    write_text_widget(doc, raw, &shown);
                }
            }
            FormFieldKind::Checkbox => {
                let on = truthy(&value.value);
                for raw in &mine {
                    let state = if on {
                        on_state(doc, raw.widget).unwrap_or_else(|| "Yes".to_owned())
                    } else {
                        "Off".to_owned()
                    };
                    set_value(doc, raw.field, Object::Name(state.as_bytes().to_vec()));
                    if let Ok(dict) = doc.get_dictionary_mut(raw.widget) {
                        dict.set("AS", Object::Name(state.into_bytes()));
                    }
                }
            }
            FormFieldKind::Radio => {
                let mut chosen = false;
                for raw in &mine {
                    let state = on_state(doc, raw.widget).unwrap_or_default();
                    let selected = !state.is_empty() && state == value.value;
                    chosen |= selected;
                    if let Ok(dict) = doc.get_dictionary_mut(raw.widget) {
                        let name = if selected {
                            state.clone()
                        } else {
                            "Off".to_owned()
                        };
                        dict.set("AS", Object::Name(name.into_bytes()));
                    }
                }
                if !chosen {
                    return Err(field_error(format!(
                        "\"{}\" is not one of the choices of the group \"{}\".",
                        value.value, value.name
                    )));
                }
                set_value(
                    doc,
                    first.field,
                    Object::Name(value.value.as_bytes().to_vec()),
                );
            }
            FormFieldKind::Signature | FormFieldKind::Button => {
                return Err(field_error(format!(
                    "The field \"{}\" cannot be filled with a value; use a signature instead.",
                    value.name
                )));
            }
        }
    }
    set_need_appearances(doc);
    Ok(())
}

fn set_value(doc: &mut Document, field: ObjectId, value: Object) {
    if let Ok(dict) = doc.get_dictionary_mut(field) {
        dict.set("V", value);
    }
}

fn write_text_widget(doc: &mut Document, raw: &Raw, shown: &str) {
    let Some(rect) = widget_rect(doc, raw.widget) else {
        return;
    };
    let (w, h) = size_of(rect);
    let (size, color) = da_parts(raw.inherited.da.as_deref().or_else(|| {
        acro_form(doc)
            .and_then(|f| f.get(b"DA").ok())
            .and_then(|o| match o {
                Object::String(s, _) => std::str::from_utf8(s).ok(),
                _ => None,
            })
    }));
    // Keep the border and background the widget already has.
    let (border, fill) = doc
        .get_dictionary(raw.widget)
        .ok()
        .and_then(|d| d.get(b"MK").ok())
        .and_then(|o| o.as_dict().ok())
        .map_or((false, false), |mk| {
            (mk.get(b"BC").is_ok(), mk.get(b"BG").is_ok())
        });
    let multiline = raw.inherited.ff & FF_MULTILINE != 0;
    let look = TextLook {
        value: shown,
        size,
        color: &color,
        multiline,
        quad: raw.inherited.quad,
        fill,
        border,
    };
    let appearance = text_appearance(doc, w, h, &look);
    set_appearance(doc, raw.widget, Object::Reference(appearance));
}

// ----- creating ----------------------------------------------------------------------------

/// The form's dictionary, made when the document has none, with Helvetica among its resources.
fn ensure_form(doc: &mut Document) -> Result<ObjectId, AppError> {
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| AppError::internal("finding the document catalog"))?;
    if let Some(id) = acro_form_id(doc) {
        ensure_defaults(doc, id);
        return Ok(id);
    }
    let direct = doc
        .get_dictionary(root)
        .ok()
        .and_then(|d| d.get(b"AcroForm").ok())
        .and_then(|o| o.as_dict().ok())
        .cloned();
    let id = doc.add_object(Object::Dictionary(direct.unwrap_or_default()));
    doc.get_dictionary_mut(root)
        .map_err(|_| AppError::internal("finding the document catalog"))?
        .set("AcroForm", Object::Reference(id));
    ensure_defaults(doc, id);
    Ok(id)
}

fn ensure_defaults(doc: &mut Document, id: ObjectId) {
    let helv = helvetica(doc);
    let has_da = doc.get_dictionary(id).is_ok_and(|d| d.get(b"DA").is_ok());
    let Ok(form) = doc.get_dictionary_mut(id) else {
        return;
    };
    if !has_da {
        form.set(
            "DA",
            Object::String(b"/Helv 0 Tf 0 g".to_vec(), lopdf::StringFormat::Literal),
        );
    }
    let mut dr = match form.get(b"DR") {
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let mut fonts = match dr.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    if fonts.get(b"Helv").is_err() {
        fonts.set("Helv", Object::Reference(helv));
    }
    dr.set("Font", Object::Dictionary(fonts));
    form.set("DR", Object::Dictionary(dr));
    if form.get(b"Fields").is_err() {
        form.set("Fields", Object::Array(Vec::new()));
    }
}

fn add_to_fields(doc: &mut Document, form: ObjectId, field: ObjectId) -> Result<(), AppError> {
    let fields = doc
        .get_dictionary(form)
        .ok()
        .and_then(|d| d.get(b"Fields").ok())
        .cloned();
    match fields {
        Some(Object::Reference(list)) => {
            if let Ok(Object::Array(items)) = doc.get_object_mut(list) {
                items.push(Object::Reference(field));
                return Ok(());
            }
        }
        Some(Object::Array(mut items)) => {
            items.push(Object::Reference(field));
            doc.get_dictionary_mut(form)
                .map_err(|_| AppError::internal("editing the form"))?
                .set("Fields", Object::Array(items));
            return Ok(());
        }
        _ => {}
    }
    doc.get_dictionary_mut(form)
        .map_err(|_| AppError::internal("editing the form"))?
        .set("Fields", Object::Array(vec![Object::Reference(field)]));
    Ok(())
}

fn real_array(values: &[f32]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v)).collect())
}

fn mark_style() -> Dictionary {
    let mut mk = Dictionary::new();
    mk.set("BC", real_array(&[0.42, 0.45, 0.5]));
    mk.set("BG", real_array(&[0.957, 0.973, 1.0]));
    mk
}

fn circle(w: f32, h: f32, inset: f32) -> String {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = (w / 2.0 - inset, h / 2.0 - inset);
    let (kx, ky) = (rx * 0.552_284_7, ry * 0.552_284_7);
    let p = |x: f32, y: f32| format!("{} {}", num(x), num(y));
    format!(
        "{} m {} {} {} c {} {} {} c {} {} {} c {} {} {} c h\n",
        p(cx + rx, cy),
        p(cx + rx, cy + ky),
        p(cx + kx, cy + ry),
        p(cx, cy + ry),
        p(cx - kx, cy + ry),
        p(cx - rx, cy + ky),
        p(cx - rx, cy),
        p(cx - rx, cy - ky),
        p(cx - kx, cy - ry),
        p(cx, cy - ry),
        p(cx + kx, cy - ry),
        p(cx + rx, cy - ky),
        p(cx + rx, cy)
    )
}

/// The `Yes` and `Off` appearances of a check box or radio button.
fn button_states(doc: &mut Document, w: f32, h: f32, round: bool) -> (ObjectId, ObjectId) {
    let mut off = color_op(FILL, false).unwrap_or_default();
    off.push_str(&color_op(BORDER, true).unwrap_or_default());
    off.push_str("0.75 w\n");
    if round {
        off.push_str(&circle(w, h, 0.5));
        off.push_str("B\n");
    } else {
        off.push_str(&format!(
            "0.375 0.375 {} {} re B\n",
            num(w - 0.75),
            num(h - 0.75)
        ));
    }
    let mut on = off.clone();
    if round {
        on.push_str("0.1 0.1 0.1 rg\n");
        on.push_str(&circle(w, h, w.min(h) * 0.3));
        on.push_str("f\n");
    } else {
        on.push_str(&format!(
            "0.1 0.1 0.1 RG {} w 1 J 1 j {} {} m {} {} l {} {} l S\n",
            num((w.min(h) * 0.12).max(1.0)),
            num(w * 0.22),
            num(h * 0.52),
            num(w * 0.43),
            num(h * 0.27),
            num(w * 0.8),
            num(h * 0.78)
        ));
    }
    let on_id = form_xobject(doc, [0.0, 0.0, w, h], on, Dictionary::new());
    let off_id = form_xobject(doc, [0.0, 0.0, w, h], off, Dictionary::new());
    (on_id, off_id)
}

fn flags(required: bool, extra: i64) -> i64 {
    extra | if required { FF_REQUIRED } else { 0 }
}

/// Adds the fields to their pages and to the form.
pub fn create_fields(doc: &mut Document, fields: &[NewField]) -> Result<(), AppError> {
    let existing: Vec<String> = collect(doc).into_iter().map(|r| r.name).collect();
    for f in fields {
        if existing
            .iter()
            .any(|n| n == &f.name || n.starts_with(&format!("{}.", f.name)))
        {
            return Err(field_error(format!(
                "The document already has a field named \"{}\". Choose another name.",
                f.name
            )));
        }
    }
    let form = ensure_form(doc)?;
    let mut groups: BTreeMap<String, ObjectId> = BTreeMap::new();
    for f in fields {
        let page = page_object(doc, f.page.saturating_sub(1))?;
        let painter = crate::paint::Painter::new(doc, f.page.saturating_sub(1))?;
        let boxed = painter.boxed(&f.region);
        let rect = crate::annot::user_rect(painter.frame(), boxed);
        let (w, h) = size_of(rect);

        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"Annot".to_vec()));
        dict.set("Subtype", Object::Name(b"Widget".to_vec()));
        dict.set("Rect", rect_array(rect));
        dict.set("P", Object::Reference(page));
        dict.set("F", Object::Integer(4));
        if !f.tooltip.is_empty() {
            dict.set("TU", text_string(&f.tooltip));
        }
        let default_size = if h > 24.0 { AUTO_SIZE } else { 0.0 };
        let da = format!("/Helv {} Tf 0 g", num(default_size));
        let da_object = Object::String(da.clone().into_bytes(), lopdf::StringFormat::Literal);

        match &f.kind {
            NewFieldKind::Text {
                multiline,
                max_len,
                value,
            } => {
                dict.set("FT", Object::Name(b"Tx".to_vec()));
                dict.set("T", text_string(&f.name));
                dict.set(
                    "Ff",
                    Object::Integer(flags(f.required, if *multiline { FF_MULTILINE } else { 0 })),
                );
                dict.set("DA", da_object);
                dict.set("MK", Object::Dictionary(mark_style()));
                if *max_len > 0 {
                    dict.set("MaxLen", Object::Integer(i64::from(*max_len)));
                }
                if !value.is_empty() {
                    dict.set("V", text_string(value));
                }
                let (size, color) = da_parts(Some(&da));
                let look = TextLook {
                    value,
                    size,
                    color: &color,
                    multiline: *multiline,
                    quad: 0,
                    fill: true,
                    border: true,
                };
                let ap = text_appearance(doc, w, h, &look);
                set_appearance_in(&mut dict, Object::Reference(ap));
                let id = doc.add_object(Object::Dictionary(dict));
                add_to_page(doc, page, id)?;
                add_to_fields(doc, form, id)?;
            }
            NewFieldKind::Dropdown { options, value } => {
                dict.set("FT", Object::Name(b"Ch".to_vec()));
                dict.set("T", text_string(&f.name));
                dict.set("Ff", Object::Integer(flags(f.required, FF_COMBO)));
                dict.set("DA", da_object);
                dict.set("MK", Object::Dictionary(mark_style()));
                dict.set(
                    "Opt",
                    Object::Array(options.iter().map(|o| text_string(o)).collect()),
                );
                if !value.is_empty() {
                    dict.set("V", text_string(value));
                }
                let (size, color) = da_parts(Some(&da));
                let look = TextLook {
                    value,
                    size,
                    color: &color,
                    multiline: false,
                    quad: 0,
                    fill: true,
                    border: true,
                };
                let ap = text_appearance(doc, w, h, &look);
                set_appearance_in(&mut dict, Object::Reference(ap));
                let id = doc.add_object(Object::Dictionary(dict));
                add_to_page(doc, page, id)?;
                add_to_fields(doc, form, id)?;
            }
            NewFieldKind::Checkbox { checked } => {
                dict.set("FT", Object::Name(b"Btn".to_vec()));
                dict.set("T", text_string(&f.name));
                dict.set("Ff", Object::Integer(flags(f.required, 0)));
                dict.set("MK", Object::Dictionary(mark_style()));
                let (on, off) = button_states(doc, w, h, false);
                let mut states = Dictionary::new();
                states.set("Yes", Object::Reference(on));
                states.set("Off", Object::Reference(off));
                set_appearance_in(&mut dict, Object::Dictionary(states));
                let state: &[u8] = if *checked { b"Yes" } else { b"Off" };
                dict.set("V", Object::Name(state.to_vec()));
                dict.set("AS", Object::Name(state.to_vec()));
                let id = doc.add_object(Object::Dictionary(dict));
                add_to_page(doc, page, id)?;
                add_to_fields(doc, form, id)?;
            }
            NewFieldKind::Radio { option, checked } => {
                let group = match groups.get(&f.name) {
                    Some(id) => *id,
                    None => {
                        let mut parent = Dictionary::new();
                        parent.set("FT", Object::Name(b"Btn".to_vec()));
                        parent.set("T", text_string(&f.name));
                        parent.set(
                            "Ff",
                            Object::Integer(flags(f.required, FF_RADIO | FF_NO_TOGGLE_OFF)),
                        );
                        parent.set("Kids", Object::Array(Vec::new()));
                        parent.set("V", Object::Name(b"Off".to_vec()));
                        let id = doc.add_object(Object::Dictionary(parent));
                        add_to_fields(doc, form, id)?;
                        groups.insert(f.name.clone(), id);
                        id
                    }
                };
                let state = option.trim().replace(' ', "_");
                dict.set("Parent", Object::Reference(group));
                dict.set("MK", Object::Dictionary(mark_style()));
                let (on, off) = button_states(doc, w, h, true);
                let mut states = Dictionary::new();
                states.set(state.clone(), Object::Reference(on));
                states.set("Off", Object::Reference(off));
                set_appearance_in(&mut dict, Object::Dictionary(states));
                dict.set(
                    "AS",
                    Object::Name(if *checked {
                        state.clone().into_bytes()
                    } else {
                        b"Off".to_vec()
                    }),
                );
                let id = doc.add_object(Object::Dictionary(dict));
                add_to_page(doc, page, id)?;
                if let Ok(parent) = doc.get_dictionary_mut(group) {
                    if let Ok(Object::Array(kids)) = parent.get_mut(b"Kids") {
                        kids.push(Object::Reference(id));
                    }
                    if *checked {
                        parent.set("V", Object::Name(state.into_bytes()));
                    }
                }
            }
            NewFieldKind::Signature => {
                dict.set("FT", Object::Name(b"Sig".to_vec()));
                dict.set("T", text_string(&f.name));
                dict.set("Ff", Object::Integer(flags(f.required, 0)));
                let mut content = color_op(FILL, false).unwrap_or_default();
                content.push_str(&format!("0 0 {} {} re f\n", num(w), num(h)));
                content.push_str(&color_op(BORDER, true).unwrap_or_default());
                content.push_str(&format!(
                    "0.75 w [3 2] 0 d 0.375 0.375 {} {} re S\n",
                    num(w - 0.75),
                    num(h - 0.75)
                ));
                let ap = form_xobject(doc, [0.0, 0.0, w, h], content, Dictionary::new());
                set_appearance_in(&mut dict, Object::Reference(ap));
                let id = doc.add_object(Object::Dictionary(dict));
                add_to_page(doc, page, id)?;
                add_to_fields(doc, form, id)?;
            }
        }
    }
    Ok(())
}

fn set_appearance_in(dict: &mut Dictionary, normal: Object) {
    let mut ap = Dictionary::new();
    ap.set("N", normal);
    dict.set("AP", Object::Dictionary(ap));
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::{minimal_pdf, pdfs};

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn field(name: &str, kind: NewFieldKind, y: f32) -> NewField {
        NewField {
            page: 1,
            region: region(0.1, y, 0.4, 0.04),
            name: name.to_owned(),
            kind,
            required: false,
            tooltip: String::new(),
        }
    }

    fn text(value: &str) -> NewFieldKind {
        NewFieldKind::Text {
            multiline: false,
            max_len: 0,
            value: value.to_owned(),
        }
    }

    fn make() -> Document {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        create_fields(
            &mut doc,
            &[
                field("name", text(""), 0.1),
                field("agree", NewFieldKind::Checkbox { checked: false }, 0.2),
                field(
                    "size",
                    NewFieldKind::Radio {
                        option: "Small".to_owned(),
                        checked: false,
                    },
                    0.3,
                ),
                field(
                    "size",
                    NewFieldKind::Radio {
                        option: "Large".to_owned(),
                        checked: true,
                    },
                    0.35,
                ),
                field(
                    "color",
                    NewFieldKind::Dropdown {
                        options: vec!["red".to_owned(), "blue".to_owned()],
                        value: String::new(),
                    },
                    0.4,
                ),
                field("sign", NewFieldKind::Signature, 0.5),
            ],
        )
        .unwrap();
        doc
    }

    #[test]
    fn created_fields_are_listed_with_kinds_pages_places_and_values() {
        let doc = make();
        let fields = list_fields(&doc);
        let by = |n: &str| fields.iter().filter(|f| f.name == n).collect::<Vec<_>>();
        assert_eq!(fields.len(), 6);
        assert_eq!(by("name")[0].kind, FormFieldKind::Text);
        assert_eq!(by("agree")[0].kind, FormFieldKind::Checkbox);
        assert_eq!(by("agree")[0].value, "false");
        let sizes = by("size");
        assert_eq!(sizes.len(), 2);
        assert!(
            sizes
                .iter()
                .all(|f| f.kind == FormFieldKind::Radio && f.value == "Large")
        );
        assert_eq!(sizes[0].options, ["Small"]);
        assert_eq!(by("color")[0].options, ["red", "blue"]);
        assert_eq!(by("sign")[0].kind, FormFieldKind::Signature);
        // The text field sits 0.1 from the left and from the top, 0.4 wide.
        let r = by("name")[0].region;
        assert!(
            (r.x - 0.1).abs() < 0.001 && (r.y - 0.1).abs() < 0.001,
            "{r:?}"
        );
        assert!(
            (r.width - 0.4).abs() < 0.001 && (r.height - 0.04).abs() < 0.001,
            "{r:?}"
        );
        assert!(fields.iter().all(|f| f.page == 1));
    }

    #[test]
    fn filling_sets_values_and_appearances() {
        let mut doc = make();
        fill(
            &mut doc,
            &[
                FieldValue {
                    name: "name".into(),
                    value: "Ada Lovelace".into(),
                },
                FieldValue {
                    name: "agree".into(),
                    value: "true".into(),
                },
                FieldValue {
                    name: "size".into(),
                    value: "Small".into(),
                },
                FieldValue {
                    name: "color".into(),
                    value: "blue".into(),
                },
            ],
        )
        .unwrap();
        let fields = list_fields(&doc);
        let value = |n: &str| fields.iter().find(|f| f.name == n).unwrap().value.clone();
        assert_eq!(value("name"), "Ada Lovelace");
        assert_eq!(value("agree"), "true");
        assert_eq!(value("size"), "Small");
        assert_eq!(value("color"), "blue");
        // The text widget has an appearance that shows the value.
        let raw = collect(&doc)
            .into_iter()
            .find(|r| r.name == "name")
            .unwrap();
        let ap = doc
            .get_dictionary(raw.widget)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap();
        let stream = doc
            .get_object(ap.get(b"N").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert!(String::from_utf8_lossy(&stream.content).contains("(Ada Lovelace) Tj"));
        // Both radio buttons follow the group.
        let states: Vec<String> = collect(&doc)
            .into_iter()
            .filter(|r| r.name == "size")
            .map(|r| {
                String::from_utf8_lossy(
                    doc.get_dictionary(r.widget)
                        .unwrap()
                        .get(b"AS")
                        .unwrap()
                        .as_name()
                        .unwrap(),
                )
                .into_owned()
            })
            .collect();
        assert_eq!(states, ["Small", "Off"]);
        assert!(
            acro_form(&doc)
                .unwrap()
                .get(b"NeedAppearances")
                .unwrap()
                .as_bool()
                .unwrap()
        );
        // Unchecking works too.
        fill(
            &mut doc,
            &[FieldValue {
                name: "agree".into(),
                value: "no".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            list_fields(&doc)
                .iter()
                .find(|f| f.name == "agree")
                .unwrap()
                .value,
            "false"
        );
    }

    #[test]
    fn filling_refuses_unknown_names_bad_choices_and_locked_fields() {
        let mut doc = make();
        let err = fill(
            &mut doc,
            &[FieldValue {
                name: "nope".into(),
                value: "x".into(),
            }],
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(
            err.message.contains("nope") && err.message.contains("name"),
            "{}",
            err.message
        );
        for (name, value) in [("color", "green"), ("size", "Medium"), ("sign", "x")] {
            let err = fill(
                &mut doc,
                &[FieldValue {
                    name: name.into(),
                    value: value.into(),
                }],
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::OptionInvalid, "{name}");
        }
        // A document without a form says so.
        let mut plain = Document::load_mem(&minimal_pdf(1)).unwrap();
        let err = fill(
            &mut plain,
            &[FieldValue {
                name: "a".into(),
                value: "x".into(),
            }],
        )
        .unwrap_err();
        assert!(err.message.contains("no form fields"));
    }

    #[test]
    fn text_longer_than_the_limit_is_refused_and_multiline_text_wraps() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        create_fields(
            &mut doc,
            &[
                field(
                    "short",
                    NewFieldKind::Text {
                        multiline: false,
                        max_len: 3,
                        value: String::new(),
                    },
                    0.1,
                ),
                NewField {
                    region: region(0.1, 0.3, 0.2, 0.2),
                    ..field(
                        "long",
                        NewFieldKind::Text {
                            multiline: true,
                            max_len: 0,
                            value: String::new(),
                        },
                        0.3,
                    )
                },
            ],
        )
        .unwrap();
        assert!(
            fill(
                &mut doc,
                &[FieldValue {
                    name: "short".into(),
                    value: "abcd".into()
                }]
            )
            .is_err()
        );
        fill(
            &mut doc,
            &[FieldValue {
                name: "long".into(),
                value: "one two three four five six seven eight".into(),
            }],
        )
        .unwrap();
        let raw = collect(&doc)
            .into_iter()
            .find(|r| r.name == "long")
            .unwrap();
        let ap = doc
            .get_dictionary(raw.widget)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap();
        let stream = doc
            .get_object(ap.get(b"N").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&stream.content)
                .matches(" Tj")
                .count()
                >= 2
        );
    }

    #[test]
    fn existing_names_cannot_be_reused_and_a_second_run_adds_to_the_form() {
        let mut doc = make();
        let err = create_fields(&mut doc, &[field("name", text(""), 0.7)]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        create_fields(&mut doc, &[field("email", text(""), 0.7)]).unwrap();
        assert_eq!(list_fields(&doc).len(), 7);
    }

    #[test]
    fn fields_of_a_real_form_document_are_read_with_their_inherited_settings() {
        let doc = Document::load_mem(&pdfs::pdf_with_form_field("City", "Paris")).unwrap();
        let fields = list_fields(&doc);
        assert_eq!(fields.len(), 1, "{fields:?}");
        assert_eq!(fields[0].name, "City");
        assert_eq!(fields[0].value, "Paris");
        assert_eq!(fields[0].kind, FormFieldKind::Text);
    }

    #[test]
    fn rotated_pages_keep_fields_where_they_look() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(612, 792, 90)])).unwrap();
        create_fields(&mut doc, &[field("a", text(""), 0.1)]).unwrap();
        let f = &list_fields(&doc)[0];
        assert!(
            (f.region.x - 0.1).abs() < 0.001 && (f.region.y - 0.1).abs() < 0.001,
            "{:?}",
            f.region
        );
        assert!((f.region.width - 0.4).abs() < 0.001, "{:?}", f.region);
    }
}
