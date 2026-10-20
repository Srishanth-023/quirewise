//! The XMP packet of a PDF/A file: it says which level the file claims and repeats the document
//! information, which PDF/A requires to agree with it to the letter.

use domain::tools::pdfa::PdfaLevel;
use lopdf::{Dictionary, Object};

use crate::metadata::{escape, read_text, text_string};

/// A date and time in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment(pub i64);

/// Days since 1970-01-01 of a calendar date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl Moment {
    /// The calendar parts: year, month, day, hour, minute, second.
    pub fn parts(self) -> (i64, i64, i64, i64, i64, i64) {
        let days = self.0.div_euclid(86_400);
        let secs = self.0.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        (year, month, day, secs / 3600, secs % 3600 / 60, secs % 60)
    }

    /// `D:YYYYMMDDHHmmSSZ`, the form of the information dictionary.
    pub fn pdf(self) -> String {
        let (y, mo, d, h, mi, s) = self.parts();
        format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
    }

    /// `YYYY-MM-DDTHH:MM:SSZ`, the form of XMP.
    pub fn xmp(self) -> String {
        let (y, mo, d, h, mi, s) = self.parts();
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    }

    /// Reads a PDF date such as `D:20261020120000+05'30'`, as the instant it names.
    pub fn from_pdf(text: &str) -> Option<Self> {
        let body = text.trim().strip_prefix("D:").unwrap_or(text.trim());
        let number =
            |from: usize, len: usize| -> Option<i64> { body.get(from..from + len)?.parse().ok() };
        let year = number(0, 4)?;
        let month = number(4, 2).unwrap_or(1);
        let day = number(6, 2).unwrap_or(1);
        let (h, mi, s) = (
            number(8, 2).unwrap_or(0),
            number(10, 2).unwrap_or(0),
            number(12, 2).unwrap_or(0),
        );
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || mi > 59 || s > 60 {
            return None;
        }
        let local = days_from_civil(year, month, day) * 86_400 + h * 3600 + mi * 60 + s.min(59);
        let rest = body.get(14..).unwrap_or("");
        let offset = match rest.chars().next() {
            Some(sign @ ('+' | '-')) => {
                let digits: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
                let hours: i64 = digits.get(0..2).and_then(|d| d.parse().ok()).unwrap_or(0);
                let minutes: i64 = digits.get(2..4).and_then(|d| d.parse().ok()).unwrap_or(0);
                let total = hours * 3600 + minutes * 60;
                if sign == '+' { total } else { -total }
            }
            _ => 0,
        };
        Some(Self(local - offset))
    }
}

/// The fields of the information dictionary that XMP repeats, as text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    /// Title.
    pub title: Option<String>,
    /// Author.
    pub author: Option<String>,
    /// Subject.
    pub subject: Option<String>,
    /// Keywords.
    pub keywords: Option<String>,
    /// The program that made the original.
    pub creator: Option<String>,
    /// The program that made the PDF.
    pub producer: Option<String>,
    /// When the document was made.
    pub created: Option<Moment>,
}

impl Info {
    /// Reads the fields of an information dictionary.
    pub fn read(dict: &Dictionary) -> Self {
        let text = |key: &[u8]| {
            dict.get(key)
                .ok()
                .and_then(read_text)
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
        };
        Self {
            title: text(b"Title"),
            author: text(b"Author"),
            subject: text(b"Subject"),
            keywords: text(b"Keywords"),
            creator: text(b"Creator"),
            producer: text(b"Producer"),
            created: text(b"CreationDate").and_then(|d| Moment::from_pdf(&d)),
        }
    }

    /// Writes the fields back as clean text, so that the dictionary and the packet agree exactly:
    /// dates in UTC, text in the one form the packet repeats.
    pub fn write(&self, dict: &mut Dictionary, modified: Moment) {
        for (key, value) in [
            ("Title", &self.title),
            ("Author", &self.author),
            ("Subject", &self.subject),
            ("Keywords", &self.keywords),
            ("Creator", &self.creator),
            ("Producer", &self.producer),
        ] {
            match value {
                Some(v) => dict.set(key, text_string(v)),
                None => {
                    dict.remove(key.as_bytes());
                }
            }
        }
        match self.created {
            Some(m) => dict.set("CreationDate", Object::string_literal(m.pdf())),
            None => {
                dict.remove(b"CreationDate");
            }
        }
        dict.set("ModDate", Object::string_literal(modified.pdf()));
    }
}

/// The packet for a document that claims `level`.
pub fn packet(info: &Info, level: PdfaLevel, modified: Moment) -> String {
    let lang = |tag: &str, value: &str| {
        format!(
            "<{tag}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{tag}>",
            escape(value)
        )
    };
    let mut body = String::new();
    body.push_str(&format!(
        "<pdfaid:part>{}</pdfaid:part><pdfaid:conformance>{}</pdfaid:conformance>",
        level.part(),
        level.conformance()
    ));
    if let Some(v) = &info.title {
        body.push_str(&lang("dc:title", v));
    }
    if let Some(v) = &info.author {
        body.push_str(&format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            escape(v)
        ));
    }
    if let Some(v) = &info.subject {
        body.push_str(&lang("dc:description", v));
    }
    if let Some(v) = &info.keywords {
        body.push_str(&format!("<pdf:Keywords>{}</pdf:Keywords>", escape(v)));
    }
    if let Some(v) = &info.producer {
        body.push_str(&format!("<pdf:Producer>{}</pdf:Producer>", escape(v)));
    }
    if let Some(v) = &info.creator {
        body.push_str(&format!("<xmp:CreatorTool>{}</xmp:CreatorTool>", escape(v)));
    }
    if let Some(m) = info.created {
        body.push_str(&format!("<xmp:CreateDate>{}</xmp:CreateDate>", m.xmp()));
    }
    body.push_str(&format!(
        "<xmp:ModifyDate>{0}</xmp:ModifyDate><xmp:MetadataDate>{0}</xmp:MetadataDate>",
        modified.xmp()
    ));
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">{body}</rdf:Description>\
         </rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_convert_both_ways_with_offsets() {
        assert_eq!(Moment(0).pdf(), "D:19700101000000Z");
        assert_eq!(Moment(1_800_000_000).xmp(), "2027-01-15T08:00:00Z");
        assert_eq!(
            Moment::from_pdf("D:20270115100000+02'00'"),
            Some(Moment(1_800_000_000))
        );
        assert_eq!(
            Moment::from_pdf("D:20270114230000-09'00'"),
            Some(Moment(1_800_000_000))
        );
        assert_eq!(
            Moment::from_pdf("D:20270115080000Z"),
            Some(Moment(1_800_000_000))
        );
        assert_eq!(Moment::from_pdf("garbage"), None);
        assert_eq!(Moment::from_pdf("D:20271315000000"), None);
        let m = Moment(1_234_567_890);
        assert_eq!(Moment::from_pdf(&m.pdf()), Some(m));
    }

    fn info() -> Info {
        Info {
            title: Some("Q3 <report> & plans".into()),
            author: Some("Ada".into()),
            subject: Some("Numbers".into()),
            keywords: Some("a, b".into()),
            creator: Some("Word".into()),
            producer: Some("Quirewise".into()),
            created: Some(Moment(1_700_000_000)),
        }
    }

    #[test]
    fn the_packet_claims_the_level_and_repeats_the_information() {
        let p = packet(&info(), PdfaLevel::A2u, Moment(1_800_000_000));
        assert!(
            p.contains("<pdfaid:part>2</pdfaid:part><pdfaid:conformance>U</pdfaid:conformance>")
        );
        assert!(p.contains("Q3 &lt;report&gt; &amp; plans"), "{p}");
        assert!(
            p.contains("<xmp:CreateDate>2023-11-14T22:13:20Z</xmp:CreateDate>"),
            "{p}"
        );
        assert!(p.contains("<xmp:ModifyDate>2027-01-15T08:00:00Z</xmp:ModifyDate>"));
        assert!(p.starts_with("<?xpacket begin=\"\u{feff}\""));
        assert!(p.ends_with("<?xpacket end=\"w\"?>"));
    }

    #[test]
    fn missing_fields_are_left_out() {
        let p = packet(&Info::default(), PdfaLevel::A1b, Moment(0));
        assert!(p.contains("<pdfaid:part>1</pdfaid:part>"));
        assert!(!p.contains("dc:title") && !p.contains("pdf:Producer"));
    }

    #[test]
    fn the_dictionary_is_rewritten_to_match() {
        let mut dict = Dictionary::new();
        dict.set("Title", Object::string_literal("old"));
        dict.set(
            "CreationDate",
            Object::string_literal("D:20231114231320+01'00'"),
        );
        dict.set("Custom", Object::string_literal("kept"));
        let read = Info::read(&dict);
        assert_eq!(read.title.as_deref(), Some("old"));
        assert_eq!(read.created, Some(Moment(1_700_000_000)));
        read.write(&mut dict, Moment(1_800_000_000));
        assert_eq!(
            read_text(dict.get(b"CreationDate").unwrap()).unwrap(),
            "D:20231114221320Z"
        );
        assert_eq!(
            read_text(dict.get(b"ModDate").unwrap()).unwrap(),
            "D:20270115080000Z"
        );
        assert!(dict.has(b"Custom"), "other keys stay");
        assert!(!dict.has(b"Author"));
    }
}
