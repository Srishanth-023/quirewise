//! Scanner and camera commands: pictures of pages come from a scanner (Windows Image Acquisition)
//! or from the camera preview in the UI, and are saved as files the Scan to PDF tool then reads.
//!
//! The pictures are kept in `Documents\Quirewise Scans`, because they are the person's originals
//! and the result PDF is written next to them.

use application::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::ErrorCode;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest camera picture accepted, in bytes.
const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
/// `exit` code the scanner script uses when the person closed the dialog.
const CANCELLED: i32 = 4;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// WIA from PowerShell. Messages go to the error stream, the saved file's path to the output.
const ACQUIRE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
  $dialog = New-Object -ComObject WIA.CommonDialog
  $image = $dialog.ShowAcquireImage(1, 0, 0, '{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}', $false, $true, $false)
  if ($null -eq $image) { exit 4 }
  $path = $env:QW_BASE + '.' + $image.FileExtension
  $image.SaveFile($path)
  Write-Output $path
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 3 }
"#;

/// Counts the scanners Windows knows about.
const COUNT_SCRIPT: &str = r#"
try { (New-Object -ComObject WIA.DeviceManager).DeviceInfos.Count } catch { 0 }
"#;

/// `Documents\Quirewise Scans`, created when missing.
fn scans_folder() -> Result<PathBuf, AppError> {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::internal("finding the Documents folder"))?;
    let dir = home.join("Documents").join("Quirewise Scans");
    fs::create_dir_all(&dir).map_err(|e| AppError::from_io("create the scans folder", &e))?;
    Ok(dir)
}

/// A name for a new picture that no file has: `scan-<seconds>-<n>`.
fn base_name(dir: &std::path::Path) -> PathBuf {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    loop {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = dir.join(format!("scan-{seconds}-{n}"));
        let taken = ["png", "bmp", "jpg", "tif", "gif"]
            .iter()
            .any(|e| base.with_extension(e).exists());
        if !taken {
            return base;
        }
    }
}

fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

/// The script as PowerShell's `-EncodedCommand` wants it: UTF-16 little endian, in base64.
fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    STANDARD.encode(bytes)
}

fn run_script(
    script: &str,
    base: Option<&std::path::Path>,
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(powershell());
    command
        .args([
            "-STA",
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat",
            "Text",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded(script),
        ])
        .stdin(Stdio::null());
    if let Some(base) = base {
        command.env("QW_BASE", base);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output()
}

/// True when Windows knows at least one scanner.
#[tauri::command(async)]
pub fn scan_available() -> bool {
    run_script(COUNT_SCRIPT, None)
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .is_some_and(|n| n > 0)
}

/// Shows the Windows scanning dialog and returns the picture it saved, or `None` when the person
/// closed the dialog.
#[tauri::command(async)]
pub fn scan_acquire() -> Result<Option<PathBuf>, AppError> {
    let dir = scans_folder()?;
    let base = base_name(&dir);
    let output = run_script(ACQUIRE_SCRIPT, Some(&base))
        .map_err(|e| AppError::from_io("start the scanner", &e))?;
    if output.status.code() == Some(CANCELLED) {
        return Ok(None);
    }
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.lines().map(str::trim).find(|l| !l.is_empty());
        return Err(AppError::new(
            ErrorCode::OutputVerifyFailed,
            match said {
                Some(detail) => format!(
                    "The scan did not work ({detail}). Check that the scanner is on and connected."
                ),
                None => {
                    "The scan did not work. Check that the scanner is on and connected.".to_owned()
                }
            },
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let path = PathBuf::from(path);
    if path.is_file() && path.starts_with(&dir) {
        Ok(Some(path))
    } else {
        Err(AppError::new(
            ErrorCode::OutputVerifyFailed,
            "The scanner gave no picture.",
        ))
    }
}

/// Saves a picture taken with the camera preview. `png_base64` is a PNG file as base64 text.
#[tauri::command(async)]
pub fn save_capture(png_base64: String) -> Result<PathBuf, AppError> {
    let bytes = STANDARD
        .decode(png_base64.trim().as_bytes())
        .map_err(|_| AppError::internal("reading the camera picture"))?;
    save_png(&scans_folder()?, &bytes)
}

fn save_png(dir: &std::path::Path, bytes: &[u8]) -> Result<PathBuf, AppError> {
    if bytes.len() > MAX_CAPTURE_BYTES || !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err(AppError::new(
            ErrorCode::InputNotPdf,
            "The camera picture is not usable. Try taking it again.",
        ));
    }
    let path = base_name(dir).with_extension("png");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| AppError::from_io("save the camera picture", &e))?;
    file.write_all(bytes)
        .map_err(|e| AppError::from_io("save the camera picture", &e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_repeat_and_pictures_are_checked_before_they_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3];
        let a = save_png(dir.path(), &png).unwrap();
        let b = save_png(dir.path(), &png).unwrap();
        assert_ne!(a, b);
        assert!(a.is_file() && b.is_file());
        assert_eq!(a.extension().unwrap(), "png");
        assert!(save_png(dir.path(), b"GIF89a").is_err());
        assert!(save_png(dir.path(), &[]).is_err());
        let huge = vec![0u8; MAX_CAPTURE_BYTES + 1];
        assert!(save_png(dir.path(), &huge).is_err());
    }

    #[test]
    fn scripts_travel_as_encoded_commands_without_quoting_trouble() {
        let text = encoded("Write-Output 'a \"b\"'");
        let bytes = STANDARD.decode(text).unwrap();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            "Write-Output 'a \"b\"'"
        );
    }

    #[test]
    fn asking_for_scanners_never_fails_even_without_one() {
        // True or false depending on the computer; the point is that it answers.
        let _ = scan_available();
    }
}
