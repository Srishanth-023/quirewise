//! Excel workbooks: one worksheet per [`SheetData`], text as inline strings, numbers as numbers.

use application::AppError;
use application::ports::SheetData;
use std::path::Path;

use crate::{Package, core_properties, root_rels, xml_escape, xml_head};

/// Most rows and columns Excel holds.
const MAX_ROWS: usize = 1_048_576;
const MAX_COLUMNS: usize = 16_384;

/// The number a cell's text stands for, when it is a plain number: digits with an optional sign,
/// thousands commas and decimals. Text that only looks numeric, such as codes with leading zeros
/// or more than 15 digits, stays text.
pub fn cell_number(text: &str) -> Option<f64> {
    let t = text.trim();
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (body, None),
    };
    if whole.is_empty()
        || fraction.is_some_and(|f| f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let digits: String = if whole.contains(',') {
        let groups: Vec<&str> = whole.split(',').collect();
        let well_formed = (1..=3).contains(&groups[0].len())
            && groups[1..].iter().all(|g| g.len() == 3)
            && groups.iter().all(|g| g.chars().all(|c| c.is_ascii_digit()));
        if !well_formed {
            return None;
        }
        groups.concat()
    } else {
        if !whole.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        whole.to_owned()
    };
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    if digits.len() + fraction.map_or(0, str::len) > 15 {
        return None;
    }
    let value: f64 = match fraction {
        Some(f) => format!("{digits}.{f}").parse().ok()?,
        None => digits.parse().ok()?,
    };
    Some(if negative { -value } else { value })
}

/// Column letters of a zero-based column: A, B, ... Z, AA.
fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8_lossy(&name).into_owned()
}

/// A worksheet name Excel accepts: no `[]:*?/\`, at most 31 characters, not empty, and different
/// from the names before it (ignoring case).
fn sheet_name(wanted: &str, taken: &[String]) -> String {
    let cleaned: String = wanted
        .chars()
        .map(|c| {
            if "[]:*?/\\".contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let base: String = cleaned.trim().trim_matches('\'').chars().take(31).collect();
    let base = if base.is_empty() {
        "Sheet".to_owned()
    } else {
        base
    };
    let exists = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !exists(&base) {
        return base;
    }
    for n in 2.. {
        let suffix = format!(" ({n})");
        let keep = 31 - suffix.chars().count();
        let candidate = format!("{}{suffix}", base.chars().take(keep).collect::<String>());
        if !exists(&candidate) {
            return candidate;
        }
    }
    base
}

fn sheet_xml(sheet: &SheetData) -> String {
    let widest = sheet
        .rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .min(MAX_COLUMNS);
    let mut widths = vec![0usize; widest];
    for row in &sheet.rows {
        for (c, text) in row.iter().take(widest).enumerate() {
            let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            widths[c] = widths[c].max(longest);
        }
    }
    let mut xml = format!(
        "{}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
        xml_head()
    );
    if widest > 0 {
        xml.push_str("<cols>");
        for (c, w) in widths.iter().enumerate() {
            let width = (*w as f32 + 2.0).clamp(8.0, 60.0);
            xml.push_str(&format!(
                "<col min=\"{n}\" max=\"{n}\" width=\"{width:.1}\" customWidth=\"1\"/>",
                n = c + 1
            ));
        }
        xml.push_str("</cols>");
    }
    xml.push_str("<sheetData>");
    for (r, row) in sheet.rows.iter().take(MAX_ROWS).enumerate() {
        xml.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, text) in row.iter().take(MAX_COLUMNS).enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let at = format!("{}{}", column_name(c), r + 1);
            match cell_number(text) {
                Some(n) => xml.push_str(&format!("<c r=\"{at}\"><v>{n}</v></c>")),
                None => xml.push_str(&format!(
                    "<c r=\"{at}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    xml_escape(text)
                )),
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

const STYLES: &str = "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<fonts count=\"1\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>\
<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>\
<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>\
</styleSheet>";

pub(crate) fn write(sheets: &[SheetData], dest: &Path) -> Result<(), AppError> {
    let empty = [SheetData {
        name: "Sheet1".to_owned(),
        rows: Vec::new(),
    }];
    let sheets = if sheets.is_empty() {
        &empty[..]
    } else {
        sheets
    };
    let mut names: Vec<String> = Vec::new();
    for sheet in sheets {
        let name = sheet_name(&sheet.name, &names);
        names.push(name);
    }

    let mut package = Package::create(dest)?;
    let mut types = format!(
        "{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        xml_head()
    );
    let mut workbook = format!(
        "{}<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>",
        xml_head()
    );
    let mut rels = format!(
        "{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        xml_head()
    );
    for (i, sheet) in sheets.iter().enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ));
        workbook.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>",
            xml_escape(&names[i])
        ));
        rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        ));
        package.add_text(&format!("xl/worksheets/sheet{n}.xml"), &sheet_xml(sheet))?;
    }
    types.push_str("</Types>");
    workbook.push_str("</sheets></workbook>");
    rels.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>",
        sheets.len() + 1
    ));

    package.add_text("[Content_Types].xml", &types)?;
    package.add_text(
        "_rels/.rels",
        &root_rels("officeDocument", "xl/workbook.xml"),
    )?;
    package.add_text("docProps/core.xml", &core_properties("Tables from a PDF"))?;
    package.add_text("xl/workbook.xml", &workbook)?;
    package.add_text("xl/_rels/workbook.xml.rels", &rels)?;
    package.add_text("xl/styles.xml", &format!("{}{STYLES}", xml_head()))?;
    package.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{assert_well_formed, part, parts};

    #[test]
    fn plain_numbers_are_numbers_and_codes_stay_text() {
        assert_eq!(cell_number("12"), Some(12.0));
        assert_eq!(cell_number(" -3.50 "), Some(-3.5));
        assert_eq!(cell_number("1,234,567.89"), Some(1_234_567.89));
        assert_eq!(cell_number("0"), Some(0.0));
        assert_eq!(cell_number("0.25"), Some(0.25));
        for text in [
            "007",
            "1,23",
            "12,34,567",
            "1.",
            ".5",
            "--1",
            "12 kg",
            "",
            "1e5",
            "+4",
            "1234567890123456",
            "A1",
        ] {
            assert_eq!(cell_number(text), None, "{text:?}");
        }
    }

    #[test]
    fn column_letters_roll_over() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }

    #[test]
    fn sheet_names_are_made_valid_and_unique() {
        let mut taken = Vec::new();
        for wanted in [
            "Page 1: Table [1]",
            "page 1  table  1",
            "",
            "x".repeat(40).as_str(),
            "x".repeat(40).as_str(),
        ] {
            let n = sheet_name(wanted, &taken);
            assert!(n.chars().count() <= 31 && !n.is_empty(), "{n:?}");
            assert!(!n.contains([':', '[', ']']), "{n:?}");
            assert!(
                !taken.iter().any(|t: &String| t.eq_ignore_ascii_case(&n)),
                "{n:?} twice"
            );
            taken.push(n);
        }
        assert_eq!(taken[3].chars().count(), 31);
        assert!(taken[4].ends_with(" (2)"));
    }

    #[test]
    fn a_workbook_has_its_sheets_cells_and_widths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.xlsx");
        let rows = |data: &[&[&str]]| {
            data.iter()
                .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
                .collect()
        };
        write(
            &[
                SheetData {
                    name: "Prices".to_owned(),
                    rows: rows(&[&["Item", "Qty"], &["Pens & ink", "1,200"], &["", "3.5"]]),
                },
                SheetData {
                    name: "Empty".to_owned(),
                    rows: vec![],
                },
            ],
            &path,
        )
        .unwrap();
        assert_well_formed(&path);
        let names = parts(&path);
        assert!(names.contains(&"xl/worksheets/sheet1.xml".to_owned()));
        assert!(names.contains(&"xl/worksheets/sheet2.xml".to_owned()));
        let book = part(&path, "xl/workbook.xml");
        assert!(book.contains("name=\"Prices\"") && book.contains("name=\"Empty\""));
        let sheet = part(&path, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains(
            "<c r=\"A1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Item</t></is></c>"
        ));
        assert!(sheet.contains("Pens &amp; ink"));
        assert!(sheet.contains("<c r=\"B2\"><v>1200</v></c>"));
        assert!(sheet.contains("<c r=\"B3\"><v>3.5</v></c>"));
        // The empty cell is left out.
        assert!(!sheet.contains("r=\"A3\""));
        assert!(
            sheet.contains("<col min=\"1\" max=\"1\" width=\"12.0\""),
            "{sheet}"
        );
    }

    #[test]
    fn no_sheets_still_make_a_valid_workbook() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.xlsx");
        write(&[], &path).unwrap();
        assert_well_formed(&path);
        assert!(part(&path, "xl/workbook.xml").contains("Sheet1"));
    }
}
