//! Building a document from pages of several sources (and blank pages) with one qpdf run.

use application::AppError;
use application::ports::{AssemblyPlan, PlannedPage};
use domain::DomainError;
use domain::tools::common::RightAngle;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{QpdfCli, absolute, blank::blank_pdf};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Scratch files created for one assembly, removed when it ends.
struct Temps(Vec<PathBuf>);

impl Temps {
    fn path(&mut self, dir: &Path, name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = dir.join(format!("qw-asm-{}-{n}-{name}", std::process::id()));
        self.0.push(path.clone());
        path
    }
}

impl Drop for Temps {
    fn drop(&mut self) {
        for path in &self.0 {
            // Best effort; leftovers are wiped with the session scratch folder.
            let _ = fs::remove_file(path);
        }
    }
}

/// Compresses page numbers into a qpdf range list, merging ascending runs: `[1,2,3,7]` becomes
/// `1-3,7`. Order is preserved, so `[3,1]` stays `3,1`.
pub fn range_list(pages: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        let start = pages[i];
        let mut end = start;
        while i + 1 < pages.len() && pages[i + 1] == end + 1 {
            end += 1;
            i += 1;
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(",")
}

/// Rotation arguments for output pages: one `--rotate=+angle:range` per angle.
pub fn rotate_args(rotations: &[(u32, RightAngle)]) -> Vec<String> {
    [RightAngle::Deg90, RightAngle::Deg180, RightAngle::Deg270]
        .into_iter()
        .filter_map(|angle| {
            let pages: Vec<u32> = rotations
                .iter()
                .filter(|(_, a)| *a == angle)
                .map(|(p, _)| *p)
                .collect();
            let mut sorted = pages;
            sorted.sort_unstable();
            (!sorted.is_empty())
                .then(|| format!("--rotate=+{}:{}", angle.degrees(), range_list(&sorted)))
        })
        .collect()
}

/// A run of output pages taken from one file.
struct Run {
    file: usize,
    pages: Vec<u32>,
}

pub(crate) fn assemble(
    cli: &QpdfCli,
    plan: &AssemblyPlan,
    temp_dir: &Path,
    dest: &Path,
) -> Result<(), AppError> {
    if plan.pages.is_empty() {
        return Err(DomainError::option("pages", "the result would have no pages").into());
    }
    fs::create_dir_all(temp_dir).map_err(|e| AppError::from_io("create the scratch folder", &e))?;
    let temp_dir = absolute(temp_dir);
    let mut temps = Temps(Vec::new());

    // Encrypted sources are decrypted first, with the password on stdin, so that the page lists
    // below never need a password on a command line.
    let mut files: Vec<PathBuf> = Vec::with_capacity(plan.sources.len());
    for (i, source) in plan.sources.iter().enumerate() {
        match &source.password {
            Some(password) => {
                let plain = temps.path(&temp_dir, &format!("src{i}.pdf"));
                cli.decrypt(&source.path, password, &plain)?;
                files.push(plain);
            }
            None => files.push(absolute(&source.path)),
        }
    }

    // One blank-page file per distinct size.
    let mut blanks: Vec<((u32, u32), usize)> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut rotations: Vec<(u32, RightAngle)> = Vec::new();
    for (position, page) in plan.pages.iter().enumerate() {
        let output_page = u32::try_from(position + 1).unwrap_or(u32::MAX);
        let (file, number) = match *page {
            PlannedPage::Source {
                source,
                page,
                rotate,
            } => {
                if source >= files.len() {
                    return Err(AppError::internal("assembling the document"));
                }
                if rotate != RightAngle::Deg0 {
                    rotations.push((output_page, rotate));
                }
                (source, page.get() + 1)
            }
            PlannedPage::Blank {
                width_pt,
                height_pt,
            } => {
                // Sizes are keyed in hundredths of a point so equal sizes share one file.
                let key = (
                    (width_pt * 100.0).round() as u32,
                    (height_pt * 100.0).round() as u32,
                );
                let file = match blanks.iter().find(|(k, _)| *k == key) {
                    Some((_, index)) => *index,
                    None => {
                        let path = temps.path(&temp_dir, &format!("blank{}.pdf", blanks.len()));
                        fs::write(&path, blank_pdf(width_pt, height_pt))
                            .map_err(|e| AppError::from_io("prepare a blank page", &e))?;
                        files.push(path);
                        blanks.push((key, files.len() - 1));
                        files.len() - 1
                    }
                };
                (file, 1)
            }
        };
        match runs.last_mut() {
            Some(run) if run.file == file => run.pages.push(number),
            _ => runs.push(Run {
                file,
                pages: vec![number],
            }),
        }
    }

    // Arguments go in a file so a 10,000-page result cannot exceed the command line limit.
    let mut lines: Vec<String> = vec!["--empty".into(), "--pages".into()];
    for run in &runs {
        lines.push(files[run.file].to_string_lossy().into_owned());
        lines.push(range_list(&run.pages));
    }
    lines.push("--".into());
    lines.extend(rotate_args(&rotations));
    lines.push(absolute(dest).to_string_lossy().into_owned());

    let args_file = temps.path(&temp_dir, "args.txt");
    fs::write(&args_file, lines.join("\n"))
        .map_err(|e| AppError::from_io("prepare the merge", &e))?;
    let mut arg = OsString::from("@");
    arg.push(&args_file);
    cli.execute(vec![arg], None, false).map(|_| ())
}

impl QpdfCli {
    /// Decrypts `source` into `dest` using the password from standard input.
    pub(crate) fn decrypt(
        &self,
        source: &Path,
        password: &application::Password,
        dest: &Path,
    ) -> Result<(), AppError> {
        let args: Vec<OsString> = vec![
            "--password-file=-".into(),
            "--decrypt".into(),
            absolute(source).into_os_string(),
            absolute(dest).into_os_string(),
        ];
        self.execute(args, Some(password), true).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_merge_ascending_runs_and_keep_order() {
        assert_eq!(range_list(&[1, 2, 3, 7]), "1-3,7");
        assert_eq!(range_list(&[3, 1]), "3,1");
        assert_eq!(range_list(&[5]), "5");
        assert_eq!(range_list(&[1, 1, 2]), "1,1-2");
        assert_eq!(range_list(&[]), "");
    }

    #[test]
    fn rotations_group_by_angle() {
        let r = rotate_args(&[
            (3, RightAngle::Deg90),
            (1, RightAngle::Deg90),
            (2, RightAngle::Deg180),
            (4, RightAngle::Deg90),
        ]);
        assert_eq!(r, vec!["--rotate=+90:1,3-4", "--rotate=+180:2"]);
        assert!(rotate_args(&[]).is_empty());
    }
}
