//! The PowerShell that drives the Office programs. It reads everything from environment
//! variables, so no path or option is ever pasted into the script text. The script is passed as
//! an encoded command, which keeps quotes in it from being re-interpreted by the command line.

/// What the script does for each program. Messages that start with `QW:` describe a problem with
/// the options rather than with Office.
const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$in = $env:QW_IN
$out = $env:QW_OUT
function Stop-With([string]$message) { [Console]::Error.WriteLine($message); exit 3 }
try {
  switch ($env:QW_KIND) {
    'Word.Application' {
      $app = New-Object -ComObject Word.Application
      try {
        $app.Visible = $false
        $app.DisplayAlerts = 0
        $app.AutomationSecurity = 3
        $doc = $app.Documents.Open([ref]$in, [ref]$false, [ref]$true, [ref]$false)
        try { $doc.ExportAsFixedFormat($out, 17) } finally { $doc.Close([ref]0) }
      } finally { $app.Quit([ref]0) }
    }
    'PowerPoint.Application' {
      $app = New-Object -ComObject PowerPoint.Application
      try {
        $app.AutomationSecurity = 3
        $pres = $app.Presentations.Open($in, -1, 0, 0)
        try {
          if ($env:QW_HIDDEN -ne "0") { foreach ($slide in $pres.Slides) { $slide.SlideShowTransition.Hidden = 0 } }
          $pres.SaveAs($out, 32)
        } finally { $pres.Close() }
      } finally { $app.Quit() }
    }
    'Excel.Application' {
      $app = New-Object -ComObject Excel.Application
      try {
        $app.Visible = $false
        $app.DisplayAlerts = $false
        $app.AutomationSecurity = 3
        $wb = $app.Workbooks.Open($in, 0, $true)
        try {
          $count = $wb.Worksheets.Count
          $wanted = @()
          if ($env:QW_SHEETS) {
            $wanted = @($env:QW_SHEETS.Split(',') | ForEach-Object { [int]$_ } | Where-Object { $_ -ge 1 -and $_ -le $count })
            if ($wanted.Count -eq 0) { Stop-With "QW: The workbook has only $count sheets, so none of the sheets you asked for exist." }
          }
          if ($wanted.Count -gt 0) {
            $wb.Worksheets.Item($wanted[0]).Activate()
            for ($i = 1; $i -le $count; $i++) {
              if ($wanted -notcontains $i) { $wb.Worksheets.Item($i).Visible = 0 }
            }
          }
          $fit = $env:QW_FIT -eq '1'
          $orient = $env:QW_ORIENT
          if ($fit -or $orient -ne 'auto') {
            try { $app.PrintCommunication = $false } catch { }
            foreach ($ws in $wb.Worksheets) {
              if ($ws.Visible -ne -1) { continue }
              if ($fit) { $ws.PageSetup.Zoom = $false; $ws.PageSetup.FitToPagesWide = 1; $ws.PageSetup.FitToPagesTall = $false }
              if ($orient -eq 'portrait') { $ws.PageSetup.Orientation = 1 }
              if ($orient -eq 'landscape') { $ws.PageSetup.Orientation = 2 }
            }
            try { $app.PrintCommunication = $true } catch { }
          }
          $wb.ExportAsFixedFormat(0, $out)
        } finally { $wb.Close($false) }
      } finally { $app.Quit() }
    }
    default { Stop-With 'QW: This kind of document is not supported.' }
  }
} catch {
  Stop-With $_.Exception.Message
}
"#;

/// The script as PowerShell's `-EncodedCommand` wants it: UTF-16 little endian, in base64.
pub fn encoded() -> String {
    let bytes: Vec<u8> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_examples() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_encoded_script_stays_within_the_command_line_limit() {
        // A Windows command line holds 32,767 characters; stay well below.
        assert!(encoded().len() < 12_000, "{}", encoded().len());
    }

    #[test]
    fn the_script_never_receives_paths_in_its_text() {
        for name in [
            "QW_IN",
            "QW_OUT",
            "QW_SHEETS",
            "QW_FIT",
            "QW_ORIENT",
            "QW_HIDDEN",
        ] {
            assert!(SCRIPT.contains(name), "{name}");
        }
        assert!(!SCRIPT.contains("C:\\"));
    }
}
