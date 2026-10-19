//! Spreadsheet settings for LibreOffice, written into the workbook before it is converted.
//!
//! LibreOffice's command line cannot choose sheets, fit sheets to the page width or turn the page.
//! Those settings live in the workbook itself, so a copy of the `.xlsx` file gets them: sheets
//! that are not wanted are hidden (hidden sheets are not printed), and every sheet is set to fit
//! one page wide and to the page turn that was asked for.

use application::AppError;
use domain::ErrorCode;
use domain::tools::office::SheetOrientation;
use std::io::{Cursor, Read, Write};

/// Largest XML part rewritten, in bytes.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;

/// What to change in a workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The sheets to keep, by position from 1, or `None` for all of them.
    pub sheets: Option<Vec<u32>>,
    /// Fit every sheet to one page wide.
    pub fit_to_width: bool,
    /// Page turn of every sheet.
    pub orientation: SheetOrientation,
}

impl Settings {
    /// True when nothing would change.
    pub fn is_empty(&self) -> bool {
        self.sheets.is_none() && !self.fit_to_width && self.orientation == SheetOrientation::Auto
    }
}

fn damaged() -> AppError {
    AppError::new(
        ErrorCode::PdfDamaged,
        "This workbook could not be read. It may be damaged.",
    )
}

/// Adds or replaces an attribute inside the text of one start tag.
fn set_attribute(tag: &str, name: &str, value: &str) -> String {
    let marker = format!(" {name}=\"");
    let body_end = if tag.ends_with("/>") {
        tag.len() - 2
    } else {
        tag.len() - 1
    };
    let (open, close) = tag.split_at(body_end);
    let open = match open.find(&marker) {
        Some(start) => {
            let value_start = start + marker.len();
            let end = open[value_start..]
                .find('"')
                .map_or(open.len(), |e| value_start + e + 1);
            format!("{}{}", &open[..start], &open[end..])
        }
        None => open.to_owned(),
    };
    format!("{}{marker}{value}\"{close}", open.trim_end())
}

/// The first start tag called `name` in `xml`, as a byte range.
fn find_tag(xml: &str, name: &str) -> Option<(usize, usize)> {
    let needle = format!("<{name}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&needle) {
        let start = from + i;
        let after = xml[start + needle.len()..].chars().next()?;
        if after == ' ' || after == '/' || after == '>' {
            let end = xml[start..].find('>')? + start + 1;
            return Some((start, end));
        }
        from = start + needle.len();
    }
    None
}

/// Marks the sheets that are not wanted as hidden in `xl/workbook.xml`.
fn hide_other_sheets(workbook: &str, keep: &[u32]) -> Result<String, AppError> {
    let mut out = String::with_capacity(workbook.len() + 64);
    let mut rest = workbook;
    let mut index = 0u32;
    let mut kept = 0usize;
    while let Some((start, end)) = find_tag(rest, "sheet") {
        index += 1;
        out.push_str(&rest[..start]);
        let tag = &rest[start..end];
        if keep.contains(&index) {
            kept += 1;
            out.push_str(&set_attribute(tag, "state", "visible"));
        } else {
            out.push_str(&set_attribute(tag, "state", "hidden"));
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    if kept == 0 {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "The workbook has only {index} sheets, so none of the sheets you asked for exist."
            ),
        ));
    }
    Ok(out)
}

/// Gives a worksheet the fit and orientation settings.
fn patch_sheet(xml: &str, settings: &Settings) -> String {
    let mut xml = xml.to_owned();
    if settings.fit_to_width {
        xml = ensure_fit_to_page(&xml);
    }
    let orientation = match settings.orientation {
        SheetOrientation::Auto => None,
        SheetOrientation::Portrait => Some("portrait"),
        SheetOrientation::Landscape => Some("landscape"),
    };
    if !settings.fit_to_width && orientation.is_none() {
        return xml;
    }
    let apply = |tag: &str| {
        let mut tag = tag.to_owned();
        if settings.fit_to_width {
            tag = set_attribute(&tag, "fitToWidth", "1");
            tag = set_attribute(&tag, "fitToHeight", "0");
        }
        if let Some(o) = orientation {
            tag = set_attribute(&tag, "orientation", o);
        }
        tag
    };
    if let Some((start, end)) = find_tag(&xml, "pageSetup") {
        let patched = apply(&xml[start..end]);
        xml.replace_range(start..end, &patched);
        return xml;
    }
    // A new page setup goes after the margins, which a page setup needs in front of it.
    let setup = apply("<pageSetup/>");
    if let Some((_, end)) = find_tag(&xml, "pageMargins") {
        xml.insert_str(end, &setup);
        return xml;
    }
    let margins = "<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>";
    for after in [
        "headerFooter",
        "rowBreaks",
        "colBreaks",
        "customProperties",
        "cellWatches",
        "ignoredErrors",
        "smartTags",
        "drawing",
        "legacyDrawing",
        "legacyDrawingHF",
        "picture",
        "oleObjects",
        "controls",
        "webPublishItems",
        "tableParts",
        "extLst",
    ] {
        if let Some((start, _)) = find_tag(&xml, after) {
            xml.insert_str(start, &format!("{margins}{setup}"));
            return xml;
        }
    }
    if let Some(end) = xml.rfind("</worksheet>") {
        xml.insert_str(end, &format!("{margins}{setup}"));
    }
    xml
}

/// Makes the sheet print on pages as wide as the page: `sheetPr` carries `fitToPage`.
fn ensure_fit_to_page(xml: &str) -> String {
    let mut xml = xml.to_owned();
    if let Some((start, end)) = find_tag(&xml, "pageSetUpPr") {
        let patched = set_attribute(&xml[start..end], "fitToPage", "1");
        xml.replace_range(start..end, &patched);
        return xml;
    }
    if let Some((start, end)) = find_tag(&xml, "sheetPr") {
        let tag = xml[start..end].to_owned();
        if tag.ends_with("/>") {
            let open = format!("{}>", tag.trim_end_matches("/>").trim_end());
            xml.replace_range(
                start..end,
                &format!("{open}<pageSetUpPr fitToPage=\"1\"/></sheetPr>"),
            );
        } else if let Some(close) = xml.find("</sheetPr>") {
            xml.insert_str(close, "<pageSetUpPr fitToPage=\"1\"/>");
        }
        return xml;
    }
    if let Some((_, end)) = find_tag(&xml, "worksheet") {
        xml.insert_str(end, "<sheetPr><pageSetUpPr fitToPage=\"1\"/></sheetPr>");
    }
    xml
}

fn read_part(file: &mut zip::read::ZipFile<'_, Cursor<&[u8]>>) -> Result<String, AppError> {
    let mut text = String::new();
    file.by_ref()
        .take(MAX_PART_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| damaged())?;
    if text.len() as u64 > MAX_PART_BYTES {
        return Err(damaged());
    }
    Ok(text)
}

/// Returns a copy of the `.xlsx` bytes with `settings` applied.
pub fn apply(xlsx: &[u8], settings: &Settings) -> Result<Vec<u8>, AppError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(xlsx)).map_err(|_| damaged())?;
    let mut out = Vec::with_capacity(xlsx.len() + 1024);
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|_| damaged())?;
            let name = file.name().map_err(|_| damaged())?.into_owned();
            let patched = if name == "xl/workbook.xml" && settings.sheets.is_some() {
                let xml = read_part(&mut file)?;
                let keep = settings.sheets.clone().unwrap_or_default();
                Some(hide_other_sheets(&xml, &keep)?)
            } else if name.starts_with("xl/worksheets/")
                && name.ends_with(".xml")
                && (settings.fit_to_width || settings.orientation != SheetOrientation::Auto)
            {
                let xml = read_part(&mut file)?;
                Some(patch_sheet(&xml, settings))
            } else {
                None
            };
            match patched {
                Some(text) => {
                    let options = zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated);
                    writer.start_file(name, options).map_err(|_| damaged())?;
                    writer
                        .write_all(text.as_bytes())
                        .map_err(|e| AppError::from_io("prepare the workbook", &e))?;
                }
                None => writer.raw_copy_file(file).map_err(|_| damaged())?,
            }
        }
        writer.finish().map_err(|_| damaged())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8], part: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut file = archive.by_name(part).unwrap();
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        s
    }

    fn book() -> Vec<u8> {
        test_support::office::xlsx_with_sheets(&[
            ("One", vec![vec!["a"]]),
            ("Two", vec![vec!["b"]]),
            ("Three", vec![vec!["c"]]),
        ])
    }

    #[test]
    fn attributes_are_added_replaced_and_kept_tidy() {
        assert_eq!(
            set_attribute("<a b=\"1\"/>", "c", "2"),
            "<a b=\"1\" c=\"2\"/>"
        );
        assert_eq!(
            set_attribute("<a b=\"1\" c=\"9\"/>", "c", "2"),
            "<a b=\"1\" c=\"2\"/>"
        );
        assert_eq!(set_attribute("<a>", "c", "2"), "<a c=\"2\">");
        assert_eq!(
            set_attribute("<pageSetup/>", "x", "y"),
            "<pageSetup x=\"y\"/>"
        );
    }

    #[test]
    fn only_the_wanted_sheets_stay_visible() {
        let settings = Settings {
            sheets: Some(vec![1, 3]),
            fit_to_width: false,
            orientation: SheetOrientation::Auto,
        };
        let patched = apply(&book(), &settings).unwrap();
        let xml = text(&patched, "xl/workbook.xml");
        assert!(
            xml.contains("name=\"One\" sheetId=\"1\" r:id=\"rId1\" state=\"visible\""),
            "{xml}"
        );
        assert!(
            xml.contains("name=\"Two\" sheetId=\"2\" r:id=\"rId2\" state=\"hidden\""),
            "{xml}"
        );
        assert!(xml.contains("name=\"Three\"") && xml.contains("state=\"visible\"/></sheets>"));
    }

    #[test]
    fn asking_for_sheets_that_do_not_exist_is_an_option_error() {
        let settings = Settings {
            sheets: Some(vec![7]),
            fit_to_width: false,
            orientation: SheetOrientation::Auto,
        };
        let err = apply(&book(), &settings).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains("only 3 sheets"));
    }

    #[test]
    fn sheets_fit_one_page_wide_and_take_the_page_turn() {
        let settings = Settings {
            sheets: None,
            fit_to_width: true,
            orientation: SheetOrientation::Landscape,
        };
        let patched = apply(&book(), &settings).unwrap();
        for n in 1..=3 {
            let xml = text(&patched, &format!("xl/worksheets/sheet{n}.xml"));
            assert!(
                xml.contains("<sheetPr><pageSetUpPr fitToPage=\"1\"/></sheetPr>"),
                "{xml}"
            );
            assert!(
                xml.contains("fitToWidth=\"1\"") && xml.contains("fitToHeight=\"0\""),
                "{xml}"
            );
            assert!(xml.contains("orientation=\"landscape\""), "{xml}");
            // The page setup sits after the data, inside the worksheet, and is well formed.
            assert!(xml.find("</sheetData>").unwrap() < xml.find("<pageSetup").unwrap());
            assert!(xml.ends_with("</worksheet>"));
        }
    }

    #[test]
    fn existing_settings_are_updated_not_duplicated() {
        let xml = "<worksheet xmlns=\"x\"><sheetPr><tabColor rgb=\"FF0000\"/><pageSetUpPr/></sheetPr><sheetData/><pageMargins left=\"1\"/><pageSetup orientation=\"portrait\" scale=\"50\"/><headerFooter/></worksheet>";
        let settings = Settings {
            sheets: None,
            fit_to_width: true,
            orientation: SheetOrientation::Landscape,
        };
        let out = patch_sheet(xml, &settings);
        assert_eq!(out.matches("<pageSetup").count(), 1);
        assert_eq!(out.matches("<pageSetUpPr").count(), 1);
        assert!(out.contains("fitToPage=\"1\""));
        assert!(out.contains("orientation=\"landscape\"") && !out.contains("portrait"));
        assert!(out.contains("scale=\"50\""));
        assert!(out.contains("<tabColor"), "other sheet properties stay");
    }

    #[test]
    fn a_self_closing_sheet_properties_tag_is_opened_up() {
        let xml = "<worksheet><sheetPr codeName=\"S\"/><sheetData/></worksheet>";
        let out = ensure_fit_to_page(xml);
        assert!(
            out.contains("<sheetPr codeName=\"S\"><pageSetUpPr fitToPage=\"1\"/></sheetPr>"),
            "{out}"
        );
    }

    #[test]
    fn other_parts_are_copied_untouched_and_junk_is_rejected() {
        let settings = Settings {
            sheets: Some(vec![1]),
            fit_to_width: true,
            orientation: SheetOrientation::Auto,
        };
        let patched = apply(&book(), &settings).unwrap();
        assert_eq!(
            text(&patched, "[Content_Types].xml"),
            text(&book(), "[Content_Types].xml")
        );
        assert_eq!(
            apply(b"not a zip", &settings).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        assert!(
            Settings {
                sheets: None,
                fit_to_width: false,
                orientation: SheetOrientation::Auto
            }
            .is_empty()
        );
    }
}
