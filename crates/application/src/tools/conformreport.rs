//! The reports of the conformance checks, as a web page to read or as data for other programs.

use serde::Serialize;

use crate::conformance::ConformanceReport;
use domain::tools::pdfa::ReportFormat;

/// What a report is about.
pub struct Subject<'a> {
    /// Name of the file that was checked.
    pub file_name: &'a str,
    /// When the check ran, as text.
    pub checked: &'a str,
    /// What the standard is for, in a few words, such as `long-term archiving`.
    pub purpose: &'a str,
}

/// Escapes text for HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// One sentence that says how the file did.
pub fn verdict(report: &ConformanceReport) -> String {
    if report.compliant {
        format!("The file conforms to {}.", report.profile)
    } else {
        let rules = report.failed_rules;
        let places = report.failed_checks;
        format!(
            "The file does not conform to {}: {} {} broken in {} {}.",
            report.profile,
            rules,
            if rules == 1 { "rule is" } else { "rules are" },
            places,
            if places == 1 { "place" } else { "places" },
        )
    }
}

const STYLE: &str = "body{font-family:Segoe UI,system-ui,sans-serif;max-width:60rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#1f2328;background:#fdfdfc}\
h1{font-size:1.6rem}h2{font-size:1.2rem;margin-top:2rem}\
.verdict{font-weight:600;padding:.5rem .75rem;border:1px solid #5a5f66;border-radius:4px}\
.ok{border-color:#1b5e20}.bad{border-color:#9a1b1b}\
table{border-collapse:collapse;width:100%}th,td{border:1px solid #c9ccd1;padding:.35rem .5rem;text-align:left;vertical-align:top}\
code{font-family:Consolas,monospace;font-size:.9em}\
@media (prefers-color-scheme:dark){body{color:#e6e8ea;background:#16181b}th,td{border-color:#4a4f57}.verdict{border-color:#9aa0a6}.ok{border-color:#6fbf73}.bad{border-color:#f28b82}}";

/// The report as a web page.
pub fn html(subject: &Subject<'_>, report: &ConformanceReport) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!(
        "<title>{} report</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n",
        escape(&report.profile)
    ));
    out.push_str(&format!("<h1>{} report</h1>\n", escape(&report.profile)));
    out.push_str(&format!(
        "<p>File: <strong>{}</strong><br>Checked: {}<br>Standard: {} ({})<br>Checker: {}</p>\n",
        escape(subject.file_name),
        escape(subject.checked),
        escape(&report.profile),
        escape(subject.purpose),
        escape(&report.validator),
    ));
    out.push_str(&format!(
        "<p class=\"verdict {}\" role=\"status\">{}</p>\n",
        if report.compliant { "ok" } else { "bad" },
        escape(&verdict(report))
    ));
    out.push_str(&format!(
        "<p>{} rules held and {} were broken. {} places were checked and held; {} broke a rule.</p>\n",
        report.passed_rules, report.failed_rules, report.passed_checks, report.failed_checks
    ));
    if !report.violations.is_empty() {
        out.push_str("<h2>Broken rules</h2>\n<table>\n<thead><tr><th scope=\"col\">Clause</th><th scope=\"col\">Rule</th><th scope=\"col\">Places</th></tr></thead>\n<tbody>\n");
        for v in &report.violations {
            out.push_str(&format!(
                "<tr><td><code>{}</code><br><small>{} test {}</small></td><td>{}",
                escape(&v.clause),
                escape(&v.specification),
                v.test,
                escape(&v.description)
            ));
            if !v.examples.is_empty() {
                out.push_str("<ul>");
                for e in &v.examples {
                    out.push_str(&format!(
                        "<li>{} <code>{}</code></li>",
                        escape(&e.message),
                        escape(&e.place)
                    ));
                }
                out.push_str("</ul>");
            }
            out.push_str(&format!("</td><td>{}</td></tr>\n", v.count));
        }
        out.push_str("</tbody>\n</table>\n");
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

#[derive(Serialize)]
struct JsonReport<'a> {
    file: &'a str,
    checked: &'a str,
    #[serde(flatten)]
    report: &'a ConformanceReport,
}

/// The report as JSON.
pub fn json(subject: &Subject<'_>, report: &ConformanceReport) -> String {
    serde_json::to_string_pretty(&JsonReport {
        file: subject.file_name,
        checked: subject.checked,
        report,
    })
    .unwrap_or_default()
}

/// The report in the format asked for.
pub fn render(format: ReportFormat, subject: &Subject<'_>, report: &ConformanceReport) -> String {
    match format {
        ReportFormat::Html => html(subject, report),
        ReportFormat::Json => json(subject, report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::{Example, Violation};

    fn failing() -> ConformanceReport {
        ConformanceReport {
            profile: "PDF/A-2b".into(),
            compliant: false,
            passed_rules: 139,
            failed_rules: 1,
            passed_checks: 115,
            failed_checks: 3,
            violations: vec![Violation {
                specification: "ISO 19005-2:2011".into(),
                clause: "6.2.4.3".into(),
                test: 4,
                description: "DeviceGray shall only be used if <something>".into(),
                count: 3,
                examples: vec![Example {
                    place: "root/page[0]".into(),
                    message: "DeviceGray & no profile".into(),
                }],
            }],
            validator: "veraPDF 1.30.3".into(),
        }
    }

    fn subject() -> Subject<'static> {
        Subject {
            file_name: "<evil>.pdf",
            checked: "2026-10-20 12:00 (local time)",
            purpose: "long-term archiving",
        }
    }

    #[test]
    fn the_verdict_counts_in_words() {
        assert_eq!(
            verdict(&failing()),
            "The file does not conform to PDF/A-2b: 1 rule is broken in 3 places."
        );
        let mut ok = failing();
        ok.compliant = true;
        assert_eq!(verdict(&ok), "The file conforms to PDF/A-2b.");
    }

    #[test]
    fn the_page_escapes_everything_that_came_from_the_file() {
        let page = html(&subject(), &failing());
        assert!(page.contains("&lt;evil&gt;.pdf"), "{page}");
        assert!(page.contains("&lt;something&gt;"), "{page}");
        assert!(page.contains("DeviceGray &amp; no profile"), "{page}");
        assert!(!page.contains("<evil>"), "{page}");
        assert!(page.contains("6.2.4.3") && page.contains("root/page[0]"));
        assert!(page.contains("class=\"verdict bad\""));
    }

    #[test]
    fn a_conforming_file_has_no_table() {
        let mut ok = failing();
        ok.compliant = true;
        ok.violations.clear();
        let page = html(&subject(), &ok);
        assert!(page.contains("verdict ok") && !page.contains("<table>"));
    }

    #[test]
    fn the_data_form_holds_the_same_facts() {
        let text = json(&subject(), &failing());
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["file"], "<evil>.pdf");
        assert_eq!(value["compliant"], false);
        assert_eq!(value["violations"][0]["clause"], "6.2.4.3");
        assert_eq!(render(ReportFormat::Json, &subject(), &failing()), text);
    }

    #[test]
    fn the_page_has_no_dash_that_the_rules_forbid() {
        assert!(!html(&subject(), &failing()).contains('\u{2014}'));
    }
}
