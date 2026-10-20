//! The Ghostscript process: where it is, how it is called, and what it makes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use application::AppError;
use application::ports::{ComponentStore, PdfaNormalizer};
use domain::ErrorCode;
use domain::tools::pdfa::PdfaLevel;

/// Id of the component that may hold Ghostscript.
pub const COMPONENT: &str = "ghostscript";
/// Longest a document may take.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// [`PdfaNormalizer`] over the Ghostscript command line.
#[derive(Clone, Default)]
pub struct Ghostscript {
    components: Option<Arc<dyn ComponentStore>>,
    /// Folders that may hold `gswin64c.exe`, or hold folders that do.
    extra: Vec<PathBuf>,
}

/// The `gswin64c.exe` in `dir`, in its `bin` folder, or in the newest folder below that holds one.
fn find_in(dir: &Path) -> Option<PathBuf> {
    let direct = [
        dir.join("gswin64c.exe"),
        dir.join("bin").join("gswin64c.exe"),
    ];
    if let Some(found) = direct.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    let mut inner: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    inner.sort();
    inner
        .into_iter()
        .rev()
        .find_map(|d| Some(d.join("bin").join("gswin64c.exe")).filter(|p| p.is_file()))
}

/// The text of a PostScript string: parentheses and backslashes are escaped.
fn ps_string(text: &str) -> String {
    let mut out = String::from("(");
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii() && !c.is_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// The definition file that gives Ghostscript the output intent a PDF/A file needs.
pub fn definition(profile: &Path) -> String {
    let path = ps_string(&profile.to_string_lossy().replace('\\', "/"));
    format!(
        "%!\n\
         [/_objdef {{icc_PDFA}} /type /stream /OBJ pdfmark\n\
         [{{icc_PDFA}} <</N 3>> /PUT pdfmark\n\
         [{{icc_PDFA}} {path} (r) file /PUT pdfmark\n\
         [/_objdef {{OutputIntent_PDFA}} /type /dict /OBJ pdfmark\n\
         [{{OutputIntent_PDFA}} <<\n\
         /Type /OutputIntent\n\
         /S /GTS_PDFA1\n\
         /DestOutputProfile {{icc_PDFA}}\n\
         /OutputConditionIdentifier (sRGB)\n\
         >> /PUT pdfmark\n\
         [{{Catalog}} <</OutputIntents [ {{OutputIntent_PDFA}} ]>> /PUT pdfmark\n"
    )
}

/// The arguments of a run. Ghostscript makes level B files: it has no way to add tags, and the
/// Unicode level needs mappings it does not write.
pub fn arguments(level: PdfaLevel, definition: &Path, input: &Path, output: &Path) -> Vec<String> {
    vec![
        format!("-dPDFA={}", level.part()),
        "-dBATCH".to_owned(),
        "-dNOPAUSE".to_owned(),
        "-dQUIET".to_owned(),
        "-dNOOUTERSAVE".to_owned(),
        "-dSAFER".to_owned(),
        "-sDEVICE=pdfwrite".to_owned(),
        "-sColorConversionStrategy=RGB".to_owned(),
        "-dPDFACompatibilityPolicy=1".to_owned(),
        format!("-sOutputFile={}", output.display()),
        definition.display().to_string(),
        input.display().to_string(),
    ]
}

impl Ghostscript {
    /// Creates the adapter. `components` finds an installed component; `extra` lists folders to
    /// look in, such as `C:\Program Files\gs`.
    pub fn new(components: Option<Arc<dyn ComponentStore>>, extra: Vec<PathBuf>) -> Self {
        Self { components, extra }
    }

    /// The Ghostscript to run, when there is one.
    pub fn program(&self) -> Option<PathBuf> {
        self.components
            .as_ref()
            .and_then(|c| c.locate(COMPONENT))
            .into_iter()
            .chain(self.extra.iter().cloned())
            .find_map(|dir| find_in(&dir))
    }

    /// True when Ghostscript is installed.
    pub fn is_available(&self) -> bool {
        self.program().is_some()
    }
}

fn missing() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "Ghostscript is not installed. Install it yourself and tell Quirewise where it is in Settings; it is optional.",
    )
}

impl PdfaNormalizer for Ghostscript {
    fn normalize(
        &self,
        input: &Path,
        level: PdfaLevel,
        _now_unix: i64,
        dest: &Path,
    ) -> Result<Vec<String>, AppError> {
        let program = self.program().ok_or_else(missing)?;
        // The profile that Ghostscript ships with, next to its programs.
        let profile = program
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("iccprofiles").join("default_rgb.icc"))
            .filter(|p| p.is_file())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::ComponentMissing,
                    "This Ghostscript has no iccprofiles folder, which PDF/A needs.",
                )
            })?;
        let folder = dest.parent().unwrap_or_else(|| Path::new("."));
        let definition_file = folder.join(format!("qw-pdfa-def-{}.ps", std::process::id()));
        std::fs::write(&definition_file, definition(&profile))
            .map_err(|e| AppError::from_io("prepare the Ghostscript run", &e))?;
        let mut command = Command::new(&program);
        command
            .args(arguments(level, &definition_file, input, dest))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = run(command);
        let _ = std::fs::remove_file(&definition_file);
        result?;
        Ok(vec![
            "Rewrote the document with Ghostscript".to_owned(),
            "Ghostscript writes level B: tags and Unicode maps are not added".to_owned(),
        ])
    }
}

fn run(mut command: Command) -> Result<(), AppError> {
    let mut child = command
        .spawn()
        .map_err(|e| AppError::from_io("start Ghostscript", &e))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(AppError::new(
                    ErrorCode::OutputVerifyFailed,
                    "Ghostscript could not rewrite this document as PDF/A.",
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(AppError::from_io("wait for Ghostscript", &e)),
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(
                ErrorCode::EngineTimeout,
                "Ghostscript took too long. Try a smaller file.",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_definition_names_the_profile_and_escapes_it() {
        let text = definition(Path::new(
            "C:\\Program Files\\gs (x)\\iccprofiles\\default_rgb.icc",
        ));
        assert!(text.starts_with("%!\n"));
        assert!(
            text.contains("(C:/Program Files/gs \\(x\\)/iccprofiles/default_rgb.icc) (r) file"),
            "{text}"
        );
        assert!(text.contains("/S /GTS_PDFA1") && text.contains("/N 3"));
        assert!(!text.contains('\u{2014}'));
    }

    #[test]
    fn the_arguments_follow_the_part_and_stay_safe() {
        let args = arguments(
            PdfaLevel::A3u,
            Path::new("def.ps"),
            Path::new("in.pdf"),
            Path::new("out.pdf"),
        );
        assert!(args.contains(&"-dPDFA=3".to_owned()));
        assert!(
            args.contains(&"-dSAFER".to_owned()),
            "file access is restricted"
        );
        assert!(args.contains(&"-sOutputFile=out.pdf".to_owned()));
        assert_eq!(
            args[args.len() - 2],
            "def.ps",
            "the definition comes before the input"
        );
        assert_eq!(args.last().map(String::as_str), Some("in.pdf"));
    }

    #[test]
    fn the_program_is_found_in_the_newest_version_folder() {
        let dir = tempfile::tempdir().unwrap();
        for v in ["gs10.03.0", "gs10.04.0"] {
            let bin = dir.path().join(v).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("gswin64c.exe"), b"exe").unwrap();
        }
        let found = Ghostscript::new(None, vec![dir.path().to_path_buf()])
            .program()
            .expect("a program");
        assert!(found.to_string_lossy().contains("gs10.04.0"), "{found:?}");
    }

    #[test]
    fn a_missing_ghostscript_is_a_missing_component() {
        let none = Ghostscript::new(None, Vec::new());
        assert!(!none.is_available());
        let err = none
            .normalize(Path::new("a.pdf"), PdfaLevel::A2b, 0, Path::new("b.pdf"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }
}
