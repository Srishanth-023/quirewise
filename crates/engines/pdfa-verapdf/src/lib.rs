//! veraPDF adapter (Appendix A rows 31 and 70): checks files against PDF/A and PDF/UA.
//!
//! veraPDF is a Java program, an on-demand component (ADR-0005): the component `verapdf` is a folder
//! with `bin\cli-<version>.jar` and, optionally, a `jre` folder with the Java it runs on. Without
//! a bundled `jre`, the Java of `JAVA_HOME` or the path is used. A copy in a development tree
//! (`vendor\verapdf\<version>`) is found too. It runs as a separate process.
#![forbid(unsafe_code)]

mod parse;
mod proc;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use application::AppError;
use application::CancellationToken;
use application::conformance::{ConformanceReport, PdfaValidator, Profile};
use application::ports::ComponentStore;
use domain::ErrorCode;

/// Id of the component that holds veraPDF.
pub const COMPONENT: &str = "verapdf";
/// Longest one file may take.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most failed places listed for each broken rule.
const MAX_EXAMPLES: u32 = 5;

/// A veraPDF and the Java to run it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// `cli-<version>.jar`.
    pub jar: PathBuf,
    /// `java.exe`.
    pub java: PathBuf,
}

/// [`PdfaValidator`] over the veraPDF command line.
#[derive(Clone, Default)]
pub struct VeraPdf {
    components: Option<Arc<dyn ComponentStore>>,
    /// Folders searched after the components: each may hold `bin\cli-*.jar`, or hold folders
    /// that do (such as `vendor\verapdf`, which holds one folder per version).
    extra: Vec<PathBuf>,
}

fn find_jar(dir: &Path) -> Option<PathBuf> {
    let bin = dir.join("bin");
    let entries = std::fs::read_dir(&bin).ok()?;
    let mut jars: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("cli-") && n.ends_with(".jar"))
        })
        .collect();
    jars.sort();
    jars.pop()
}

/// The `java.exe` that sits in a `jre` folder inside `dir`, when there is one.
fn bundled_java(dir: &Path) -> Option<PathBuf> {
    let java = dir.join("jre").join("bin").join("java.exe");
    java.is_file().then_some(java)
}

/// The Java of `JAVA_HOME`, or the first `java.exe` on the path.
fn system_java() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let java = PathBuf::from(home).join("bin").join("java.exe");
        if java.is_file() {
            return Some(java);
        }
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("java.exe"))
        .find(|p| p.is_file())
}

impl VeraPdf {
    /// Creates the adapter. `components` finds installed components; `extra` lists folders that
    /// may hold a copy, such as a development tree.
    pub fn new(components: Option<Arc<dyn ComponentStore>>, extra: Vec<PathBuf>) -> Self {
        Self { components, extra }
    }

    /// An adapter that runs the veraPDF in `dir` (the folder that holds `bin`).
    pub fn in_folder(dir: PathBuf) -> Self {
        Self {
            components: None,
            extra: vec![dir],
        }
    }

    /// The folders that may hold veraPDF, best first.
    fn folders(&self) -> Vec<PathBuf> {
        let mut folders = Vec::new();
        if let Some(dir) = self.components.as_ref().and_then(|c| c.locate(COMPONENT)) {
            folders.push(dir);
        }
        for dir in &self.extra {
            folders.push(dir.clone());
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut inner: Vec<PathBuf> = entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                inner.sort();
                // The newest version folder first.
                folders.extend(inner.into_iter().rev());
            }
        }
        folders
    }

    /// The veraPDF and Java to run, when both are there.
    pub fn install(&self) -> Option<Install> {
        for dir in self.folders() {
            let Some(jar) = find_jar(&dir) else { continue };
            let java = bundled_java(&dir).or_else(system_java)?;
            return Some(Install { jar, java });
        }
        None
    }
}

fn missing() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "The PDF/A checker is not installed. Open Settings, then Components, to install veraPDF.",
    )
}

fn flavour(profile: Profile) -> &'static str {
    match profile {
        Profile::Auto => "0",
        Profile::PdfA(level) => level.code(),
        Profile::PdfUa1 => "ua1",
    }
}

impl PdfaValidator for VeraPdf {
    fn is_available(&self) -> bool {
        self.install().is_some()
    }

    fn validate(
        &self,
        path: &Path,
        profile: Profile,
        cancel: &CancellationToken,
    ) -> Result<ConformanceReport, AppError> {
        let install = self.install().ok_or_else(missing)?;
        cancel.check()?;
        let mut command = Command::new(&install.java);
        command
            .args(["-Xmx1536m", "-Dfile.encoding=UTF-8", "-jar"])
            .arg(&install.jar)
            .args(["--format", "json", "--flavour", flavour(profile)])
            .args(["--maxfailuresdisplayed", &MAX_EXAMPLES.to_string()])
            .arg(path);
        let output = proc::run(command, TIMEOUT, cancel)?;
        parse::report(&output.stdout, &output.stderr, MAX_EXAMPLES as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::pdfa::PdfaLevel;

    #[test]
    fn the_flavour_names_follow_the_profile() {
        assert_eq!(flavour(Profile::Auto), "0");
        assert_eq!(flavour(Profile::PdfA(PdfaLevel::A3u)), "3u");
        assert_eq!(flavour(Profile::PdfUa1), "ua1");
    }

    #[test]
    fn a_missing_install_is_reported_as_a_missing_component() {
        let dir = tempfile::tempdir().unwrap();
        let checker = VeraPdf::in_folder(dir.path().to_path_buf());
        assert!(!checker.is_available() || system_java().is_some());
        let none = VeraPdf::new(None, Vec::new());
        assert!(!none.is_available());
        let err = none
            .validate(Path::new("a.pdf"), Profile::Auto, &CancellationToken::new())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
        assert!(err.message.contains("Components"), "{}", err.message);
    }

    #[test]
    fn the_newest_version_folder_and_a_bundled_java_are_preferred() {
        let dir = tempfile::tempdir().unwrap();
        for version in ["1.28.2", "1.30.3"] {
            let bin = dir.path().join(version).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join(format!("cli-{version}.jar")), b"jar").unwrap();
        }
        let java = dir.path().join("1.30.3").join("jre").join("bin");
        std::fs::create_dir_all(&java).unwrap();
        std::fs::write(java.join("java.exe"), b"exe").unwrap();
        let install = VeraPdf::in_folder(dir.path().to_path_buf())
            .install()
            .expect("an install");
        assert!(install.jar.ends_with("cli-1.30.3.jar"), "{:?}", install.jar);
        assert_eq!(install.java, java.join("java.exe"));
    }
}
