//! Encryption, decryption, repacking and repair through qpdf.
//!
//! New passwords never appear on a command line: they go to qpdf on standard input as an
//! argument file (`@-`), one argument per line.

use application::AppError;
use application::Password;
use application::ports::{PackageOptions, PdfPackager, PdfRepairer, PdfSecurity, RepairOutcome};
use domain::tools::secure::{Cipher, ProtectPlan};
use std::ffi::OsString;
use std::path::Path;

use crate::{QpdfCli, absolute};

fn no_line_breaks(label: &str, text: &str) -> Result<(), AppError> {
    if text.contains(['\n', '\r']) {
        return Err(
            domain::DomainError::option(label, "a password cannot contain line breaks").into(),
        );
    }
    Ok(())
}

fn allow(flag: bool) -> &'static str {
    if flag { "y" } else { "n" }
}

/// The qpdf arguments for `plan`, as the lines of an argument file.
fn protect_lines(
    plan: &ProtectPlan,
    password: Option<&Password>,
    source: &Path,
    dest: &Path,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(p) = password {
        lines.push(format!("--password={}", p.expose()));
    }
    lines.push("--encrypt".to_owned());
    lines.push(plan.user.expose().to_owned());
    lines.push(plan.owner.expose().to_owned());
    lines.push(
        match plan.cipher {
            Cipher::Aes256 => "256",
            Cipher::Aes128 => "128",
        }
        .to_owned(),
    );
    if plan.cipher == Cipher::Aes128 {
        lines.push("--use-aes=y".to_owned());
    }
    let p = &plan.permissions;
    lines.push(format!("--print={}", if p.print { "full" } else { "none" }));
    lines.push(format!("--extract={}", allow(p.copy)));
    lines.push(format!("--modify-other={}", allow(p.modify)));
    lines.push(format!("--annotate={}", allow(p.annotate)));
    lines.push(format!("--form={}", allow(p.fill_forms)));
    lines.push(format!("--assemble={}", allow(p.assemble)));
    // Screen readers must keep working whatever else is restricted.
    lines.push("--accessibility=y".to_owned());
    lines.push("--".to_owned());
    lines.push(absolute(source).to_string_lossy().into_owned());
    lines.push(absolute(dest).to_string_lossy().into_owned());
    lines
}

impl PdfSecurity for QpdfCli {
    fn protect(
        &self,
        source: &Path,
        password: Option<&Password>,
        plan: &ProtectPlan,
        dest: &Path,
    ) -> Result<(), AppError> {
        no_line_breaks("user_password", plan.user.expose())?;
        no_line_breaks("owner_password", plan.owner.expose())?;
        if let Some(p) = password {
            no_line_breaks("password", p.expose())?;
        }
        let lines = protect_lines(plan, password, source, dest);
        self.execute_args_from_stdin(&lines, password.is_some())
            .map(|_| ())
    }

    fn unlock(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<(), AppError> {
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--decrypt".into());
        args.push(absolute(source).into_os_string());
        args.push(absolute(dest).into_os_string());
        self.execute(args, password, password.is_some()).map(|_| ())
    }
}

/// The flags that choose stream compression, object streams and linearization.
fn pack_flags(options: &PackageOptions) -> Vec<String> {
    let mut flags = vec![
        "--compress-streams=y".to_owned(),
        "--recompress-flate".to_owned(),
        "--compression-level=9".to_owned(),
        if options.object_streams {
            "--object-streams=generate"
        } else {
            "--object-streams=disable"
        }
        .to_owned(),
    ];
    if options.linearize {
        flags.push("--linearize".to_owned());
    }
    flags
}

impl PdfPackager for QpdfCli {
    fn package(
        &self,
        source: &Path,
        password: Option<&Password>,
        options: &PackageOptions,
        dest: &Path,
    ) -> Result<(), AppError> {
        if let Some(keep) = &options.keep_protection {
            // The passwords are in the arguments, so they go over standard input.
            no_line_breaks("password", keep.password.expose())?;
            if let Some(p) = password {
                no_line_breaks("password", p.expose())?;
            }
            let mut lines: Vec<String> = Vec::new();
            if let Some(p) = password {
                lines.push(format!("--password={}", p.expose()));
            }
            lines.push(format!(
                "--copy-encryption={}",
                absolute(&keep.path).to_string_lossy()
            ));
            lines.push(format!(
                "--encryption-file-password={}",
                keep.password.expose()
            ));
            lines.extend(pack_flags(options));
            lines.push("--".to_owned());
            lines.push(absolute(source).to_string_lossy().into_owned());
            lines.push(absolute(dest).to_string_lossy().into_owned());
            return self.execute_args_from_stdin(&lines, true).map(|_| ());
        }
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--decrypt".into());
        args.extend(pack_flags(options).into_iter().map(OsString::from));
        args.push(absolute(source).into_os_string());
        args.push(absolute(dest).into_os_string());
        self.execute(args, password, password.is_some()).map(|_| ())
    }
}

impl PdfRepairer for QpdfCli {
    fn name(&self) -> &'static str {
        "qpdf"
    }

    /// qpdf rebuilds a broken cross-reference table by scanning the file and writes a clean one.
    fn repair(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<RepairOutcome, AppError> {
        let mut args: Vec<OsString> = Vec::new();
        if password.is_some() {
            args.push("--password-file=-".into());
        }
        args.push("--decrypt".into());
        args.push(absolute(source).into_os_string());
        args.push(absolute(dest).into_os_string());
        let (output, _) = self.execute(args, password, password.is_some())?;
        let problems = output
            .stderr
            .lines()
            .filter(|l| l.trim_start().starts_with("WARNING:"))
            .count();
        Ok(RepairOutcome { problems })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckOutcome;
    use application::ports::ProtectionSource;
    use domain::ErrorCode;
    use domain::secret::Secret;
    use domain::tools::secure::{Permissions, ProtectOptions};
    use std::path::PathBuf;
    use std::process::Command;
    use test_support::{minimal_pdf, qpdf_exe};

    fn qpdf() -> QpdfCli {
        QpdfCli::new(qpdf_exe().expect("run scripts/fetch-qpdf.ps1 first"))
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn plan(user: &str, owner: &str, permissions: Permissions, cipher: Cipher) -> ProtectPlan {
        ProtectOptions {
            user_password: Some(Secret::new(user)),
            owner_password: Some(Secret::new(owner)),
            cipher,
            permissions,
        }
        .plan()
        .unwrap()
    }

    fn show_encryption(path: &Path, password: &str) -> String {
        let out = Command::new(qpdf_exe().unwrap())
            .arg(format!("--password={password}"))
            .arg("--show-encryption")
            .arg(path)
            .output()
            .unwrap();
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    #[test]
    fn protect_requires_the_open_password_and_keeps_the_pages() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(3));
        let dest = dir.path().join("p.pdf");
        let p = plan(
            "open sesame",
            "boss",
            Permissions::default(),
            Cipher::Aes256,
        );
        qpdf().protect(&src, None, &p, &dest).unwrap();

        assert_eq!(
            qpdf().check(&dest, None).unwrap_err().code,
            ErrorCode::PdfEncrypted
        );
        let wrong = Password::new("nope");
        assert_eq!(
            qpdf().check(&dest, Some(&wrong)).unwrap_err().code,
            ErrorCode::PdfWrongPassword
        );
        let right = Password::new("open sesame");
        assert_eq!(
            qpdf().check(&dest, Some(&right)).unwrap(),
            CheckOutcome::Clean
        );
        assert!(
            show_encryption(&dest, "open sesame").contains("R = 6"),
            "AES-256"
        );
    }

    #[test]
    fn protect_applies_the_restrictions_and_the_128_bit_cipher() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let dest = dir.path().join("p.pdf");
        let permissions = Permissions {
            print: false,
            copy: false,
            ..Permissions::default()
        };
        qpdf()
            .protect(
                &src,
                None,
                &plan("", "boss", permissions, Cipher::Aes128),
                &dest,
            )
            .unwrap();
        let info = show_encryption(&dest, "");
        assert!(info.contains("R = 4"), "{info}");
        assert!(info.contains("print low resolution: not allowed"), "{info}");
        assert!(
            info.contains("extract for any purpose: not allowed"),
            "{info}"
        );
        // Opens without a password because only the owner password was set.
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn passwords_with_spaces_and_quotes_survive_and_never_reach_the_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let dest = dir.path().join("p.pdf");
        let secret = " sp ace \"q'";
        qpdf()
            .protect(
                &src,
                None,
                &plan(secret, "boss", Permissions::default(), Cipher::Aes256),
                &dest,
            )
            .unwrap();
        let pw = Password::new(secret);
        assert_eq!(qpdf().check(&dest, Some(&pw)).unwrap(), CheckOutcome::Clean);
        let lines = protect_lines(
            &plan(secret, "boss", Permissions::default(), Cipher::Aes256),
            None,
            &src,
            &dest,
        );
        assert!(
            lines.iter().any(|l| l == secret),
            "passwords are lines of the argument file"
        );
    }

    #[test]
    fn protect_can_re_encrypt_an_already_protected_file_and_rejects_line_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let first = dir.path().join("one.pdf");
        let p1 = plan("one", "boss", Permissions::default(), Cipher::Aes256);
        qpdf().protect(&src, None, &p1, &first).unwrap();

        let second = dir.path().join("two.pdf");
        let p2 = plan("two", "boss", Permissions::default(), Cipher::Aes256);
        let old = Password::new("one");
        qpdf().protect(&first, Some(&old), &p2, &second).unwrap();
        let new = Password::new("two");
        assert_eq!(
            qpdf().check(&second, Some(&new)).unwrap(),
            CheckOutcome::Clean
        );

        let bad = plan("a\nb", "boss", Permissions::default(), Cipher::Aes256);
        let err = qpdf()
            .protect(&src, None, &bad, &dir.path().join("x.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
    }

    #[test]
    fn unlock_writes_an_unencrypted_copy_and_needs_the_right_password() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = dir.path().join("l.pdf");
        qpdf()
            .protect(
                &src,
                None,
                &plan("open", "boss", Permissions::default(), Cipher::Aes256),
                &locked,
            )
            .unwrap();
        let dest = dir.path().join("u.pdf");
        assert_eq!(
            qpdf().unlock(&locked, None, &dest).unwrap_err().code,
            ErrorCode::PdfEncrypted
        );
        let wrong = Password::new("no");
        assert_eq!(
            qpdf()
                .unlock(&locked, Some(&wrong), &dest)
                .unwrap_err()
                .code,
            ErrorCode::PdfWrongPassword
        );
        let right = Password::new("open");
        qpdf().unlock(&locked, Some(&right), &dest).unwrap();
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn unlock_removes_restrictions_when_only_an_owner_password_was_set() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let restricted = dir.path().join("r.pdf");
        let permissions = Permissions {
            modify: false,
            ..Permissions::default()
        };
        qpdf()
            .protect(
                &src,
                None,
                &plan("", "boss", permissions, Cipher::Aes256),
                &restricted,
            )
            .unwrap();
        let dest = dir.path().join("free.pdf");
        qpdf().unlock(&restricted, None, &dest).unwrap();
        assert!(show_encryption(&dest, "").contains("not encrypted"));
    }

    #[test]
    fn package_linearizes_and_toggles_object_streams() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(6));
        let dest = dir.path().join("fast.pdf");
        let options = PackageOptions::new(true, true);
        qpdf().package(&src, None, &options, &dest).unwrap();
        let out = Command::new(qpdf_exe().unwrap())
            .arg("--check-linearization")
            .arg(&dest)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );

        let plain = dir.path().join("plain.pdf");
        let options = PackageOptions::new(false, false);
        qpdf().package(&src, None, &options, &plain).unwrap();
        let out = Command::new(qpdf_exe().unwrap())
            .arg("--check-linearization")
            .arg(&plain)
            .output()
            .unwrap();
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(report.contains("not linearized"), "{report}");
        assert_eq!(qpdf().check(&plain, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn package_decrypts_with_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(1));
        let locked = dir.path().join("l.pdf");
        qpdf()
            .protect(
                &src,
                None,
                &plan("open", "boss", Permissions::default(), Cipher::Aes256),
                &locked,
            )
            .unwrap();
        let dest = dir.path().join("o.pdf");
        let pw = Password::new("open");
        let options = PackageOptions::new(false, true);
        qpdf().package(&locked, Some(&pw), &options, &dest).unwrap();
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn package_can_keep_the_protection_of_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = dir.path().join("l.pdf");
        let restricted = Permissions {
            copy: false,
            ..Permissions::default()
        };
        qpdf()
            .protect(
                &src,
                None,
                &plan("open", "boss", restricted, Cipher::Aes256),
                &locked,
            )
            .unwrap();
        // An edited copy, unprotected, as the edit pipeline hands it over.
        let edited = dir.path().join("edited.pdf");
        qpdf()
            .unlock(&locked, Some(&Password::new("open")), &edited)
            .unwrap();

        let dest = dir.path().join("again.pdf");
        let mut options = PackageOptions::new(false, true);
        options.keep_protection = Some(ProtectionSource {
            path: locked.clone(),
            password: Password::new("open"),
        });
        qpdf().package(&edited, None, &options, &dest).unwrap();

        // The copy asks for the same password and keeps the same restrictions.
        let pw = Password::new("open");
        assert_eq!(qpdf().check(&dest, Some(&pw)).unwrap(), CheckOutcome::Clean);
        assert!(
            qpdf().check(&dest, None).is_err(),
            "it still needs a password"
        );
        let report = show_encryption(&dest, "open");
        assert!(
            report.contains("extract for any purpose: not allowed"),
            "{report}"
        );
        assert!(
            report.contains("R = 6") || report.contains("256"),
            "{report}"
        );
        // The owner password still works too.
        assert!(qpdf().check(&dest, Some(&Password::new("boss"))).is_ok());
    }

    #[test]
    fn package_can_repack_an_encrypted_file_and_keep_its_protection_in_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "a.pdf", &minimal_pdf(2));
        let locked = dir.path().join("l.pdf");
        qpdf()
            .protect(
                &src,
                None,
                &plan("open", "boss", Permissions::default(), Cipher::Aes256),
                &locked,
            )
            .unwrap();
        let dest = dir.path().join("fast.pdf");
        let pw = Password::new("open");
        let mut options = PackageOptions::new(true, true);
        options.keep_protection = Some(ProtectionSource {
            path: locked.clone(),
            password: pw.clone(),
        });
        qpdf().package(&locked, Some(&pw), &options, &dest).unwrap();
        assert_eq!(qpdf().check(&dest, Some(&pw)).unwrap(), CheckOutcome::Clean);
        assert!(qpdf().check(&dest, None).is_err());
    }

    #[test]
    fn repair_rebuilds_a_broken_cross_reference_table() {
        let dir = tempfile::tempdir().unwrap();
        let good = minimal_pdf(3);
        // Cut off the cross-reference table and trailer: the file keeps its objects.
        let cut = good
            .windows(4)
            .rposition(|w| w == b"xref")
            .expect("the minimal PDF has an xref table");
        let broken = write(&dir, "broken.pdf", &good[..cut]);
        assert!(
            qpdf().check(&broken, None).is_err()
                || qpdf().check(&broken, None) == Ok(CheckOutcome::Warnings)
        );

        let dest = dir.path().join("fixed.pdf");
        let outcome = qpdf().repair(&broken, None, &dest).unwrap();
        assert!(outcome.problems > 0);
        assert_eq!(qpdf().check(&dest, None).unwrap(), CheckOutcome::Clean);
    }

    #[test]
    fn repair_gives_up_on_files_without_any_pdf_content() {
        let dir = tempfile::tempdir().unwrap();
        let junk = write(&dir, "j.pdf", b"%PDF-1.4\nthis is nothing\n");
        let err = qpdf()
            .repair(&junk, None, &dir.path().join("o.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PdfDamaged);
    }
}
