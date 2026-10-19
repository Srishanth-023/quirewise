//! HEIC pictures, decoded by the Windows imaging component.
//!
//! Windows can read HEIC once the HEIF Image Extensions are installed from the Microsoft Store.
//! Quirewise cannot ship that codec, so the decoder is reached through Windows PowerShell, which
//! loads the same component the Photos app uses. Paths travel in environment variables, never in
//! the script text.

use application::AppError;
use domain::ErrorCode;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Longest a conversion may take.
const TIMEOUT: Duration = Duration::from_secs(60);

static COUNTER: AtomicU64 = AtomicU64::new(0);

const SCRIPT: &str = "$ErrorActionPreference='Stop'; Add-Type -AssemblyName PresentationCore; \
$d=[System.Windows.Media.Imaging.BitmapDecoder]::Create([Uri]$env:QW_IN,\
[System.Windows.Media.Imaging.BitmapCreateOptions]::PreservePixelFormat,\
[System.Windows.Media.Imaging.BitmapCacheOption]::OnLoad); \
$e=New-Object System.Windows.Media.Imaging.PngBitmapEncoder; $e.Frames.Add($d.Frames[0]); \
$f=[IO.File]::Create($env:QW_OUT); $e.Save($f); $f.Close()";

/// True when the bytes start like a HEIC or HEIF file.
pub(crate) fn is_heic(head: &[u8]) -> bool {
    head.len() >= 12
        && &head[4..8] == b"ftyp"
        && [&b"heic"[..], b"heix", b"hevc", b"mif1", b"msf1", b"heif"]
            .iter()
            .any(|brand| &head[8..12] == *brand)
}

fn missing_codec() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "This HEIC picture needs the HEIF Image Extensions from the Microsoft Store. Install them, then try again.",
    )
}

fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

/// Decodes a HEIC file and returns it as PNG bytes.
pub(crate) fn to_png(path: &Path, scratch: &Path) -> Result<Vec<u8>, AppError> {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let out = scratch.join(format!("qw-heic-{}-{n}.png", std::process::id()));
    let mut command = Command::new(powershell());
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("QW_IN", path)
        .env("QW_OUT", &out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up while the converter runs.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| missing_codec())?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&out);
                return Err(missing_codec());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(missing_codec()),
        }
    };
    let bytes = if status.success() {
        fs::read(&out).ok()
    } else {
        None
    };
    let _ = fs::remove_file(&out);
    bytes.ok_or_else(missing_codec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heic_brands_are_recognized_and_other_files_are_not() {
        let mut head = vec![0, 0, 0, 24];
        head.extend_from_slice(b"ftypheic");
        head.extend_from_slice(&[0; 4]);
        assert!(is_heic(&head));
        let mut mp4 = vec![0, 0, 0, 24];
        mp4.extend_from_slice(b"ftypisom");
        mp4.extend_from_slice(&[0; 4]);
        assert!(!is_heic(&mp4));
        assert!(!is_heic(b"\xFF\xD8\xFF\xE0 jpeg"));
        assert!(!is_heic(b""));
    }
}
