//! Tesseract adapter (Appendix A row 16): reads page pictures and writes their text, or a
//! one-page PDF that holds only the invisible text, which Quirewise lays over the real page.
//!
//! Tesseract is an on-demand component (ADR-0005) in two kinds: the program (`tesseract`) and one
//! language file per language (`tessdata-<code>`). A copy that sits next to the app in a
//! development tree is used too. Each run gets a private data folder that holds links to just the
//! language files it needs, so languages from different components can be combined.
#![forbid(unsafe_code)]

mod proc;

use application::AppError;
use application::CancellationToken;
use application::ports::{ComponentStore, OcrEngine, RenderedImage};
use domain::{Dpi, ErrorCode};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Id of the component that holds the program.
pub const PROGRAM_COMPONENT: &str = "tesseract";
/// Prefix of the components that hold one language each.
pub const LANGUAGE_PREFIX: &str = "tessdata-";
/// Longest one page may take to read.
const PAGE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`OcrEngine`] over the Tesseract command line.
#[derive(Clone, Default)]
pub struct Tesseract {
    components: Option<Arc<dyn ComponentStore>>,
    /// Folders searched for `tesseract.exe` and `tessdata`, after the components.
    extra: Vec<PathBuf>,
    /// A program to run instead of looking for one, for tests.
    fixed: Option<PathBuf>,
}

impl Tesseract {
    /// Creates the adapter. `components` finds installed components; `extra` lists folders that
    /// may hold `tesseract.exe` (and a `tessdata` folder), such as a development tree.
    pub fn new(components: Option<Arc<dyn ComponentStore>>, extra: Vec<PathBuf>) -> Self {
        Self {
            components,
            extra,
            fixed: None,
        }
    }

    /// An adapter that always runs `program`, with its data in a `tessdata` folder beside it.
    pub fn with_program(program: PathBuf) -> Self {
        Self {
            components: None,
            extra: program
                .parent()
                .map(Path::to_path_buf)
                .into_iter()
                .collect(),
            fixed: Some(program),
        }
    }

    /// Folders that may hold the program, best first.
    fn program_folders(&self) -> Vec<PathBuf> {
        let mut folders = Vec::new();
        if let Some(dir) = self
            .components
            .as_ref()
            .and_then(|c| c.locate(PROGRAM_COMPONENT))
        {
            folders.push(dir.clone());
            folders.push(dir.join("bin"));
        }
        folders.extend(self.extra.iter().cloned());
        folders
    }

    /// The `tesseract.exe` to run.
    pub fn program(&self) -> Option<PathBuf> {
        if let Some(fixed) = &self.fixed {
            return fixed.is_file().then(|| fixed.clone());
        }
        self.program_folders()
            .into_iter()
            .map(|d| d.join("tesseract.exe"))
            .find(|p| p.is_file())
    }

    /// Folders that may hold `<code>.traineddata` files.
    fn data_folders(&self) -> Vec<PathBuf> {
        let mut folders = Vec::new();
        for dir in self.program_folders() {
            folders.push(dir.join("tessdata"));
            folders.push(dir.join("share").join("tessdata"));
            folders.push(dir.join("..").join("share").join("tessdata"));
            folders.push(dir.join("..").join("tessdata"));
        }
        folders
    }

    /// The data file of a language, from its component or from a folder next to the program.
    fn language_file(&self, code: &str) -> Option<PathBuf> {
        let file = format!("{code}.traineddata");
        if let Some(dir) = self
            .components
            .as_ref()
            .and_then(|c| c.locate(&format!("{LANGUAGE_PREFIX}{}", code.replace('_', "-"))))
        {
            for candidate in [dir.join(&file), dir.join("tessdata").join(&file)] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        self.data_folders()
            .into_iter()
            .map(|d| d.join(&file))
            .find(|p| p.is_file())
    }
}

fn missing_program() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "Text recognition is not installed. Open Settings, then Components, to install Tesseract and a language.",
    )
}

fn missing_language(code: &str) -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        format!(
            "The language \"{code}\" is not installed. Open Settings, then Components, to install it."
        ),
    )
}

/// Writes the picture as an opaque RGB PNG, which every Tesseract build reads.
fn write_png(image: &RenderedImage, path: &Path) -> Result<(), AppError> {
    let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
    for px in image.rgba.as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        for c in &px[..3] {
            rgb.push(((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8);
        }
    }
    let file =
        File::create(path).map_err(|e| AppError::from_io("prepare the page for reading", &e))?;
    PngEncoder::new(BufWriter::new(file))
        .write_image(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
        .map_err(|_| AppError::internal("preparing the page for reading"))
}

/// What one run is given: private folders that are removed afterwards.
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn create(scratch: &Path) -> Result<Self, AppError> {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = scratch.join(format!("qw-ocr-{}-{n}", std::process::id()));
        fs::create_dir_all(root.join("tessdata"))
            .map_err(|e| AppError::from_io("prepare the text recognizer", &e))?;
        Ok(Self { root })
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Puts a language file where the run can see it: a hard link, or a copy when that is not possible.
fn provide(source: &Path, target: &Path) -> Result<(), AppError> {
    if fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| AppError::from_io("prepare the language data", &e))
}

impl Tesseract {
    /// Runs Tesseract on one picture. `config` is the output kind (`txt` or `pdf`) with its extra
    /// arguments; the result file is left in the workspace and returned.
    fn run(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        cancel: &CancellationToken,
        pdf: bool,
    ) -> Result<(Workspace, PathBuf), AppError> {
        let program = self.program().ok_or_else(missing_program)?;
        let work = Workspace::create(scratch)?;
        for code in languages {
            let source = self
                .language_file(code)
                .ok_or_else(|| missing_language(code))?;
            provide(
                &source,
                &work
                    .root
                    .join("tessdata")
                    .join(format!("{code}.traineddata")),
            )?;
        }
        let input = work.root.join("page.png");
        write_png(image, &input)?;
        let base = work.root.join("result");
        let mut command = Command::new(&program);
        command
            .arg(&input)
            .arg(&base)
            .arg("-l")
            .arg(domain::tools::recognize::language_argument(languages))
            .arg("--tessdata-dir")
            .arg(work.root.join("tessdata"))
            .arg("--dpi")
            .arg(dpi.get().to_string())
            // Pages run side by side, so every run keeps to one thread.
            .env("OMP_THREAD_LIMIT", "1");
        if pdf {
            // Settings instead of the `pdf` config name: the private data folder has no configs.
            command.args(["-c", "tessedit_create_pdf=1", "-c", "textonly_pdf=1"]);
        } else {
            command.arg("txt");
        }
        let outcome = proc::run(command, PAGE_TIMEOUT, cancel)?;
        let produced = base.with_extension(if pdf { "pdf" } else { "txt" });
        if !outcome.success || !produced.is_file() {
            let said = outcome
                .message
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(|l| l.chars().take(160).collect::<String>());
            return Err(AppError::new(
                ErrorCode::OutputVerifyFailed,
                match said {
                    Some(detail) => format!("The text on a page could not be read ({detail})."),
                    None => "The text on a page could not be read.".to_owned(),
                },
            ));
        }
        Ok((work, produced))
    }
}

impl OcrEngine for Tesseract {
    fn is_available(&self) -> bool {
        self.program().is_some()
    }

    fn languages(&self) -> Result<Vec<String>, AppError> {
        let mut found: BTreeSet<String> = BTreeSet::new();
        for dir in self.data_folders() {
            collect_languages(&dir, &mut found);
        }
        if let Some(components) = &self.components {
            for entry in components.installed()? {
                if let Some(code) = entry.id.strip_prefix(LANGUAGE_PREFIX) {
                    let code = code.replace('-', "_");
                    if self.language_file(&code).is_some() {
                        found.insert(code);
                    }
                }
            }
        }
        // Orientation data is not a language to read.
        found.remove("osd");
        Ok(found.into_iter().collect())
    }

    fn recognize(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        cancel: &CancellationToken,
    ) -> Result<String, AppError> {
        let (_work, produced) = self.run(image, languages, dpi, scratch, cancel, false)?;
        let bytes =
            fs::read(&produced).map_err(|e| AppError::from_io("read the recognized text", &e))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn text_layer(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<(), AppError> {
        let (_work, produced) = self.run(image, languages, dpi, scratch, cancel, true)?;
        fs::copy(&produced, dest)
            .map(|_| ())
            .map_err(|e| AppError::from_io("save the text layer", &e))
    }
}

fn collect_languages(dir: &Path, into: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(code) = name.strip_suffix(".traineddata")
            && domain::tools::recognize::check_language(code).is_ok()
        {
            into.insert(code.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> RenderedImage {
        RenderedImage {
            width: 20,
            height: 10,
            rgba: vec![255; 20 * 10 * 4],
        }
    }

    fn dpi() -> Dpi {
        Dpi::new(300).unwrap()
    }

    /// A tree like a development checkout: a program, a data folder, and a language file.
    fn tree(dir: &Path, script: &str) -> Tesseract {
        let root = dir.join("tess");
        fs::create_dir_all(root.join("tessdata")).unwrap();
        fs::write(root.join("tessdata").join("eng.traineddata"), b"data").unwrap();
        fs::write(root.join("tessdata").join("deu.traineddata"), b"data").unwrap();
        fs::write(root.join("tessdata").join("osd.traineddata"), b"data").unwrap();
        // The "program" is a batch file, so it can record how it was called.
        let batch = root.join("tesseract.cmd");
        fs::write(&batch, script).unwrap();
        Tesseract::with_program(batch)
    }

    #[test]
    fn languages_are_the_data_files_found_without_the_orientation_one() {
        let dir = tempfile::tempdir().unwrap();
        let t = tree(dir.path(), "@echo off\r\n");
        assert_eq!(t.languages().unwrap(), ["deu", "eng"]);
        assert!(t.is_available());
    }

    #[test]
    fn without_the_program_everything_reports_a_missing_component() {
        let t = Tesseract::new(None, vec![PathBuf::from("Z:\\nowhere")]);
        assert!(!t.is_available());
        assert!(t.languages().unwrap().is_empty());
        let dir = tempfile::tempdir().unwrap();
        let err = t
            .recognize(
                &picture(),
                &["eng".to_owned()],
                dpi(),
                dir.path(),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }

    #[test]
    fn a_language_that_is_not_installed_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let t = tree(dir.path(), "@echo off\r\n");
        let err = t
            .recognize(
                &picture(),
                &["fra".to_owned()],
                dpi(),
                dir.path(),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
        assert!(err.message.contains("\"fra\""), "{}", err.message);
    }

    #[test]
    fn the_program_gets_the_picture_languages_data_folder_and_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.txt");
        // %1 is the picture, %2 the output base; the program writes a text file there.
        let script = format!(
            "@echo off\r\n>>\"{log}\" echo %*\r\necho recognized text> \"%~2.txt\"\r\n",
            log = log.display()
        );
        let t = tree(dir.path(), &script);
        let text = t
            .recognize(
                &picture(),
                &["eng".to_owned(), "deu".to_owned()],
                dpi(),
                dir.path(),
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(text.trim(), "recognized text");
        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("page.png"), "{calls}");
        assert!(calls.contains("-l eng+deu"), "{calls}");
        assert!(calls.contains("--tessdata-dir"), "{calls}");
        assert!(calls.contains("--dpi 300"), "{calls}");
        assert!(calls.trim_end().ends_with("txt"), "{calls}");
        // The private folder and the picture are gone afterwards.
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("qw-ocr-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_text_layer_asks_for_text_only_pdf_output() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.txt");
        let script = format!(
            "@echo off\r\n>>\"{log}\" echo %*\r\necho %%PDF-1.4 fake> \"%~2.pdf\"\r\n",
            log = log.display()
        );
        let t = tree(dir.path(), &script);
        let dest = dir.path().join("layer.pdf");
        t.text_layer(
            &picture(),
            &["eng".to_owned()],
            dpi(),
            dir.path(),
            &dest,
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(fs::read(&dest).unwrap().starts_with(b"%PDF"));
        let calls = fs::read_to_string(&log).unwrap();
        assert!(
            calls.contains("textonly_pdf=1") && calls.contains("tessedit_create_pdf=1"),
            "{calls}"
        );
    }

    #[test]
    fn a_failing_program_says_what_it_reported() {
        let dir = tempfile::tempdir().unwrap();
        let t = tree(
            dir.path(),
            "@echo off\r\necho Error opening data file >&2\r\nexit /b 1\r\n",
        );
        let err = t
            .recognize(
                &picture(),
                &["eng".to_owned()],
                dpi(),
                dir.path(),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
        assert!(
            err.message.contains("Error opening data file"),
            "{}",
            err.message
        );
    }

    #[test]
    fn cancelling_stops_a_slow_page() {
        let dir = tempfile::tempdir().unwrap();
        let t = tree(dir.path(), "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n");
        let cancel = CancellationToken::new();
        let stopper = cancel.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            stopper.cancel();
        });
        let started = std::time::Instant::now();
        let err = t
            .recognize(&picture(), &["eng".to_owned()], dpi(), dir.path(), &cancel)
            .unwrap_err();
        handle.join().unwrap();
        assert_eq!(err.code, ErrorCode::JobCancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
