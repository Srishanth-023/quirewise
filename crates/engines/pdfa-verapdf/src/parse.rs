//! Reading the JSON report of veraPDF.

use application::AppError;
use application::conformance::{ConformanceReport, Example, Violation};
use domain::ErrorCode;
use serde_json::Value;

fn count(v: &Value, key: &str) -> u32 {
    v.get(key)
        .and_then(Value::as_u64)
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn violation(rule: &Value, max_examples: usize) -> Violation {
    let examples = rule
        .get("checks")
        .and_then(Value::as_array)
        .map(|checks| {
            checks
                .iter()
                .filter(|c| c.get("status").and_then(Value::as_str) == Some("failed"))
                .take(max_examples)
                .map(|c| Example {
                    place: text(c, "context"),
                    message: text(c, "errorMessage"),
                })
                .collect()
        })
        .unwrap_or_default();
    Violation {
        specification: text(rule, "specification"),
        clause: text(rule, "clause"),
        test: count(rule, "testNumber"),
        description: text(rule, "description"),
        count: count(rule, "failedChecks"),
        examples,
    }
}

/// Turns what veraPDF wrote into a report, or into the error that says why there is none.
pub fn report(
    stdout: &[u8],
    stderr: &str,
    max_examples: usize,
) -> Result<ConformanceReport, AppError> {
    let unreadable = || {
        AppError::new(
            ErrorCode::Internal,
            format!(
                "The PDF/A checker gave an answer that could not be read{}. Try again; if it keeps happening, check the log folder.",
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    " (it reported a problem)".to_owned()
                }
            ),
        )
    };
    let root: Value = serde_json::from_slice(stdout).map_err(|_| unreadable())?;
    let report = root.get("report").ok_or_else(unreadable)?;
    let job = report
        .get("jobs")
        .and_then(Value::as_array)
        .and_then(|j| j.first())
        .ok_or_else(unreadable)?;
    if let Some(exception) = job.get("taskException") {
        let kind = text(exception, "type");
        return Err(match kind.as_str() {
            "ENCRYPTED" => AppError::new(
                ErrorCode::PdfEncrypted,
                "This PDF is protected with a password. Unlock a copy first: PDF/A files cannot be encrypted.",
            ),
            _ => AppError::new(
                ErrorCode::PdfDamaged,
                "The PDF could not be read by the PDF/A checker. Run Repair first.",
            ),
        });
    }
    let result = job
        .get("validationResult")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
        .ok_or_else(unreadable)?;
    let details = result.get("details").ok_or_else(unreadable)?;
    let violations = details
        .get("ruleSummaries")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .filter(|r| r.get("ruleStatus").and_then(Value::as_str) == Some("FAILED"))
                .map(|r| violation(r, max_examples))
                .collect()
        })
        .unwrap_or_default();
    let profile = text(result, "profileName")
        .trim_end_matches(" validation profile")
        .to_owned();
    let version = report
        .get("buildInformation")
        .and_then(|b| b.get("releaseDetails"))
        .and_then(Value::as_array)
        .and_then(|d| d.iter().find(|r| text(r, "id") == "core"))
        .map(|r| text(r, "version"))
        .unwrap_or_default();
    Ok(ConformanceReport {
        profile,
        compliant: result.get("compliant").and_then(Value::as_bool) == Some(true),
        passed_rules: count(details, "passedRules"),
        failed_rules: count(details, "failedRules"),
        passed_checks: count(details, "passedChecks"),
        failed_checks: count(details, "failedChecks"),
        violations,
        validator: format!("veraPDF {version}").trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILED: &str = r#"{"report":{"buildInformation":{"releaseDetails":[{"id":"core","version":"1.30.3"}]},
      "jobs":[{"itemDetails":{"name":"a.pdf"},"validationResult":[{"details":{
        "passedRules":139,"failedRules":2,"passedChecks":115,"failedChecks":4,
        "ruleSummaries":[
          {"ruleStatus":"PASSED","specification":"ISO 19005-2:2011","clause":"6.1.2","testNumber":1,"description":"header"},
          {"ruleStatus":"FAILED","specification":"ISO 19005-2:2011","clause":"6.2.4.3","testNumber":4,"failedChecks":3,
           "description":"DeviceGray shall only be used if...","checks":[
             {"status":"failed","context":"root/page[0]","errorMessage":"DeviceGray without profile"},
             {"status":"passed","context":"root/page[1]","errorMessage":""},
             {"status":"failed","context":"root/page[2]","errorMessage":"again"}]},
          {"ruleStatus":"FAILED","specification":"ISO 19005-2:2011","clause":"6.6.2.1","testNumber":1,"failedChecks":1,
           "description":"metadata","checks":[{"status":"failed","context":"root","errorMessage":"no metadata"}]}]},
        "profileName":"PDF/A-2b validation profile","compliant":false}]}]}}"#;

    #[test]
    fn a_failed_report_lists_the_broken_rules_with_examples() {
        let r = report(FAILED.as_bytes(), "", 5).unwrap();
        assert_eq!(r.profile, "PDF/A-2b");
        assert!(!r.compliant);
        assert_eq!((r.passed_rules, r.failed_rules), (139, 2));
        assert_eq!((r.passed_checks, r.failed_checks), (115, 4));
        assert_eq!(r.validator, "veraPDF 1.30.3");
        assert_eq!(r.violations.len(), 2, "passed rules are not listed");
        let first = &r.violations[0];
        assert_eq!(
            (first.clause.as_str(), first.test, first.count),
            ("6.2.4.3", 4, 3)
        );
        assert_eq!(first.examples.len(), 2, "only failed places are examples");
        assert_eq!(first.examples[0].place, "root/page[0]");
        assert_eq!(first.examples[0].message, "DeviceGray without profile");
    }

    #[test]
    fn examples_are_capped() {
        let r = report(FAILED.as_bytes(), "", 1).unwrap();
        assert_eq!(r.violations[0].examples.len(), 1);
    }

    #[test]
    fn a_compliant_file_has_no_violations() {
        let ok = r#"{"report":{"jobs":[{"validationResult":[{"details":{"passedRules":144,"failedRules":0,
            "passedChecks":200,"failedChecks":0,"ruleSummaries":[]},"profileName":"PDF/UA-1 validation profile","compliant":true}]}]}}"#;
        let r = report(ok.as_bytes(), "", 5).unwrap();
        assert!(r.compliant && r.violations.is_empty());
        assert_eq!(r.profile, "PDF/UA-1");
    }

    #[test]
    fn unreadable_files_and_garbage_are_errors() {
        let broken = r#"{"report":{"jobs":[{"taskException":{"type":"PARSE"}}]}}"#;
        assert_eq!(
            report(broken.as_bytes(), "", 5).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        let locked = r#"{"report":{"jobs":[{"taskException":{"type":"ENCRYPTED"}}]}}"#;
        assert_eq!(
            report(locked.as_bytes(), "", 5).unwrap_err().code,
            ErrorCode::PdfEncrypted
        );
        assert_eq!(
            report(b"not json", "boom", 5).unwrap_err().code,
            ErrorCode::Internal
        );
        assert!(report(b"{}", "", 5).is_err());
    }
}
