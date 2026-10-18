//! Syntactic path policy. Canonicalization and reparse-point checks need the file system and
//! live in infrastructure; this module rejects everything that can be refused from text alone.

use crate::error::DomainError;

const RESERVED_DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether UNC paths (`\\server\share`) are accepted. Only for paths the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UncPolicy {
    /// The user picked this path in a dialog or dropped it.
    UserPicked,
    /// The path was derived by the app (outputs, temp files, settings).
    Deny,
}

fn denied(reason: &str) -> DomainError {
    DomainError::PathDenied {
        reason: reason.to_owned(),
    }
}

/// Rejects device paths, alternate data streams, reserved device names, NUL bytes and
/// (unless user-picked) UNC paths. Accepts `C:\x\y.pdf` and verbatim disk paths `\\?\C:\x`.
pub fn check_path_text(path: &str, unc: UncPolicy) -> Result<(), DomainError> {
    if path.is_empty() {
        return Err(denied("the path is empty"));
    }
    if path.contains('\0') {
        return Err(denied("the path contains a NUL character"));
    }
    let normalized = path.replace('/', "\\");
    let mut rest = normalized.as_str();

    if let Some(after) = rest.strip_prefix("\\\\?\\") {
        let lower = after.to_ascii_lowercase();
        if let Some(unc_tail) = lower.strip_prefix("unc\\") {
            if unc == UncPolicy::Deny {
                return Err(denied("network paths are only allowed when you pick them"));
            }
            rest = after
                .get(after.len() - unc_tail.len()..)
                .unwrap_or_default();
        } else if is_drive_prefix(after) {
            rest = after;
        } else {
            return Err(denied("device and volume paths are not supported"));
        }
    } else if rest.starts_with("\\\\.\\") {
        return Err(denied("device paths are not supported"));
    } else if let Some(after) = rest.strip_prefix("\\\\") {
        if unc == UncPolicy::Deny {
            return Err(denied("network paths are only allowed when you pick them"));
        }
        rest = after;
    }

    // A colon is only valid in the drive prefix (`C:`).
    let colon_ok =
        |i: usize, s: &str| i == 1 && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    if let Some(i) = rest.find(':')
        && (!colon_ok(i, rest) || rest.get(i + 1..).is_some_and(|t| t.contains(':')))
    {
        return Err(denied(
            "alternate data streams and stray colons are not allowed",
        ));
    }

    for component in rest.split('\\').filter(|c| !c.is_empty()) {
        let stem = component.split('.').next().unwrap_or_default().trim_end();
        if RESERVED_DEVICES
            .iter()
            .any(|d| d.eq_ignore_ascii_case(stem))
        {
            return Err(denied(
                "reserved device names such as CON or NUL are not allowed",
            ));
        }
    }
    Ok(())
}

fn is_drive_prefix(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(d), Some(':')) if d.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn ok(p: &str) {
        assert!(
            check_path_text(p, UncPolicy::UserPicked).is_ok(),
            "should accept {p:?}"
        );
    }

    fn denied_path(p: &str) {
        let err = check_path_text(p, UncPolicy::UserPicked).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PathDenied, "should reject {p:?}");
    }

    #[test]
    fn accepts_normal_paths() {
        ok("C:\\Users\\me\\a.pdf");
        ok("D:/docs/report (2).pdf");
        ok("\\\\?\\C:\\Users\\me\\a.pdf");
        ok("relative\\dir\\file.pdf");
        ok("\\\\server\\share\\a.pdf");
        ok("\\\\?\\UNC\\server\\share\\a.pdf");
        ok("C:\\CON1\\COM10.pdf");
    }

    #[test]
    fn rejects_device_and_volume_paths() {
        denied_path("\\\\.\\PhysicalDrive0");
        denied_path("\\\\?\\GLOBALROOT\\Device\\x");
        denied_path("\\\\?\\Volume{1234}\\a.pdf");
    }

    #[test]
    fn rejects_streams_reserved_names_and_nul() {
        denied_path("C:\\a.pdf:secret");
        denied_path("a.pdf::$DATA");
        denied_path("C:\\x\\NUL");
        denied_path("C:\\x\\con.txt");
        denied_path("C:\\x\\Aux .pdf");
        denied_path("C:\\a\0b.pdf");
        denied_path("");
    }

    #[test]
    fn unc_needs_user_pick() {
        assert!(check_path_text("\\\\server\\share\\a.pdf", UncPolicy::Deny).is_err());
        assert!(check_path_text("\\\\?\\UNC\\server\\share\\a.pdf", UncPolicy::Deny).is_err());
        assert!(check_path_text("C:\\a.pdf", UncPolicy::Deny).is_ok());
    }
}
