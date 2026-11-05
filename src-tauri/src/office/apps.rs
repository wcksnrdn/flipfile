// Word / Excel / PowerPoint -> PDF through an office app the user *already has*, for
// a result identical to what that app prints: Microsoft Office (Windows) first, then
// LibreOffice. When neither is installed, or either fails, flipfile's built-in engine
// is used instead, so the user is never asked to install anything.
//
// Safety: file paths are never spliced into script or command text (PowerShell gets
// them through environment variables, LibreOffice as plain arguments), every external
// process has a time limit, and on timeout only that child process is stopped,
// through its own handle.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Generous: a cold start of Word can take a while on slow machines.
const APP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Engine {
    Word,
    Excel,
    PowerPoint,
    LibreOffice,
    BuiltIn,
}

/// Runs `cmd`, waiting at most `limit`. On timeout the child (and only the child) is killed.
fn run_with_timeout(mut cmd: Command, limit: Duration) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let mut err = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut err);
                }
                return Err(format!("exit {status}: {}", err.trim()));
            }
            None if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn unique_temp_dir(tag: &str) -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("flipfile-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn valid_pdf(path: &Path) -> bool {
    fs::read(path).map(|b| b.starts_with(b"%PDF-") && b.len() > 100).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Microsoft Word

// macOS: not used. Word for Mac is sandboxed and cannot open or save files outside
// its own container, and flipfile is not allowed to place files there, so every
// attempt fails (and would show an Automation permission prompt first).
#[cfg(target_os = "macos")]
fn word(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not used on macOS (Word is sandboxed)".into())
}

#[cfg(windows)]
fn word(input: &Path, output: &Path) -> Result<(), String> {
    // Paths come in through environment variables, so no quoting can break the script.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$word = New-Object -ComObject Word.Application
try {
    $word.Visible = $false
    $word.DisplayAlerts = 0
    $doc = $word.Documents.Open($env:FLIPFILE_IN, $false, $true)
    $doc.SaveAs([ref]$env:FLIPFILE_OUT, [ref]17)
    $doc.Close([ref]0)
} finally {
    $word.Quit()
    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
"#;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("FLIPFILE_IN", input)
        .env("FLIPFILE_OUT", output)
        .creation_flags(CREATE_NO_WINDOW);
    run_with_timeout(cmd, APP_TIMEOUT)?;
    if valid_pdf(output) { Ok(()) } else { Err("Word produced no PDF".into()) }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn word(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not available on Linux".into())
}

// ---------------------------------------------------------------------------
// Microsoft Excel

// macOS: not used, for the same reason as Word (Excel for Mac is sandboxed).
#[cfg(target_os = "macos")]
fn excel(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not used on macOS (Excel is sandboxed)".into())
}

#[cfg(windows)]
fn excel(input: &Path, output: &Path) -> Result<(), String> {
    // Every visible sheet, with each sheet's own print setup (as File > Export does).
    // Paths come in through environment variables, so no quoting can break the script.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$excel = New-Object -ComObject Excel.Application
try {
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    $excel.AskToUpdateLinks = $false
    $book = $excel.Workbooks.Open($env:FLIPFILE_IN, 0, $true)
    $book.ExportAsFixedFormat(0, $env:FLIPFILE_OUT)
    $book.Close($false)
} finally {
    $excel.Quit()
    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
"#;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("FLIPFILE_IN", input)
        .env("FLIPFILE_OUT", output)
        .creation_flags(CREATE_NO_WINDOW);
    run_with_timeout(cmd, APP_TIMEOUT)?;
    if valid_pdf(output) { Ok(()) } else { Err("Excel produced no PDF".into()) }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn excel(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not available on Linux".into())
}

// ---------------------------------------------------------------------------
// Microsoft PowerPoint

// macOS: not used, for the same reason as Word (PowerPoint for Mac is sandboxed).
#[cfg(target_os = "macos")]
fn powerpoint(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not used on macOS (PowerPoint is sandboxed)".into())
}

#[cfg(windows)]
fn powerpoint(input: &Path, output: &Path) -> Result<(), String> {
    // PowerPoint cannot run hidden; opening without a window keeps it out of sight.
    // Paths come in through environment variables, so no quoting can break the script.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$app = New-Object -ComObject PowerPoint.Application
try {
    $app.DisplayAlerts = 1
    $deck = $app.Presentations.Open($env:FLIPFILE_IN, -1, 0, 0)
    $deck.SaveAs($env:FLIPFILE_OUT, 32)
    $deck.Close()
} finally {
    if ($app.Presentations.Count -eq 0) { $app.Quit() }
    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($app)
}
"#;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("FLIPFILE_IN", input)
        .env("FLIPFILE_OUT", output)
        .creation_flags(CREATE_NO_WINDOW);
    run_with_timeout(cmd, APP_TIMEOUT)?;
    if valid_pdf(output) { Ok(()) } else { Err("PowerPoint produced no PDF".into()) }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn powerpoint(_input: &Path, _output: &Path) -> Result<(), String> {
    Err("not available on Linux".into())
}

// ---------------------------------------------------------------------------
// LibreOffice

fn soffice() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    #[cfg(target_os = "macos")]
    candidates.push("/Applications/LibreOffice.app/Contents/MacOS/soffice".into());
    #[cfg(windows)]
    for base in [std::env::var_os("ProgramFiles"), std::env::var_os("ProgramFiles(x86)")].into_iter().flatten() {
        candidates.push(PathBuf::from(base).join("LibreOffice/program/soffice.exe"));
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        candidates.push("/usr/bin/soffice".into());
        candidates.push("/usr/bin/libreoffice".into());
        candidates.push("/usr/lib/libreoffice/program/soffice".into());
        candidates.push("/opt/libreoffice/program/soffice".into());
        candidates.push("/snap/bin/libreoffice".into());
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// `/tmp/a b` -> `file:///tmp/a%20b`, `C:\\x\\y` -> `file:///C:/x/y`
fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/").replace('%', "%25").replace(' ', "%20");
    if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }
}

fn libreoffice(input: &Path, output: &Path) -> Result<(), String> {
    libreoffice_to(input, output, "pdf")
}

/// Any document LibreOffice opens -> `ext` (pdf, pptx, …)
fn libreoffice_to(input: &Path, output: &Path, ext: &str) -> Result<(), String> {
    let soffice = soffice().ok_or("not installed")?;
    let work = unique_temp_dir("lo")?;
    // A private profile: works even while the user has LibreOffice open
    let profile = work.join("profile");
    let profile_url = file_url(&profile);
    let mut cmd = Command::new(&soffice);
    cmd.arg("--headless")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg("--nodefault")
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg(ext)
        .arg("--outdir")
        .arg(&work)
        .arg(input);
    let result = run_with_timeout(cmd, APP_TIMEOUT).and_then(|_| {
        let stem = input.file_stem().ok_or("bad file name")?;
        // Not `with_extension`: "report.v2.docx" must become "report.v2.pdf"
        let produced = work.join(format!("{}.{ext}", stem.to_string_lossy()));
        let ok = if ext == "pdf" { valid_pdf(&produced) } else { fs::metadata(&produced).map(|m| m.len() > 100).unwrap_or(false) };
        if ok {
            fs::copy(&produced, output).map(|_| ()).map_err(|e| e.to_string())
        } else {
            Err(format!("LibreOffice produced no {}", ext.to_uppercase()))
        }
    });
    let _ = fs::remove_dir_all(&work);
    result
}

/// An old presentation (.ppt, .odp) -> .pptx, through LibreOffice when it is installed
pub fn presentation_to_pptx(input: &Path, output: &Path) -> Result<(), String> {
    if cfg!(test) {
        return Err("not in tests".into());
    }
    libreoffice_to(input, output, "pptx").map_err(|_| "old .ppt and .odp files can only be converted on a computer with PowerPoint or LibreOffice; open it and save it as .pptx first".to_string())
}

// ---------------------------------------------------------------------------

/// Word document -> PDF file, with the best engine available on this computer.
/// Legacy `.doc` files need Word or LibreOffice; everything else always succeeds
/// through the built-in engine when the apps are missing.
pub fn word_to_pdf(input: &Path, output: &Path, legacy_doc: bool) -> Result<Engine, String> {
    // Unit tests must never launch Word/LibreOffice or trigger macOS permission prompts
    if !cfg!(test) {
        // Why an app was skipped goes to the log, so a silent fallback can be diagnosed
        match word(input, output) {
            Ok(()) => return Ok(Engine::Word),
            Err(e) => eprintln!("[flipfile] Word -> PDF with Microsoft Word skipped: {e}"),
        }
        match libreoffice(input, output) {
            Ok(()) => return Ok(Engine::LibreOffice),
            Err(e) => eprintln!("[flipfile] Word -> PDF with LibreOffice skipped: {e}"),
        }
    }
    if legacy_doc {
        return Err("old .doc files can only be converted on a computer with Word or LibreOffice; open it and save it as .docx first".into());
    }
    let bytes = fs::read(input).map_err(|e| format!("could not be read: {e}"))?;
    let pdf = super::docx_to_pdf_builtin(&bytes)?;
    fs::write(output, pdf).map_err(|e| format!("Could not save: {e}"))?;
    Ok(Engine::BuiltIn)
}

/// What kind of spreadsheet a file is
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SheetKind {
    /// .xlsx / .xlsm / .xltx: every engine, including the built-in one
    Xlsx,
    /// .csv: every engine, including the built-in one
    Csv,
    /// .xls / .ods: Excel or LibreOffice only
    Other,
}

/// Spreadsheet -> PDF file, with the best engine available on this computer
pub fn sheet_to_pdf(input: &Path, output: &Path, kind: SheetKind) -> Result<Engine, String> {
    if !cfg!(test) {
        // CSV opens in Excel with the region's own separators; the built-in reader
        // detects them itself, so Excel is only asked for real workbooks
        if kind != SheetKind::Csv {
            match excel(input, output) {
                Ok(()) => return Ok(Engine::Excel),
                Err(e) => eprintln!("[flipfile] Excel -> PDF with Microsoft Excel skipped: {e}"),
            }
        }
        match libreoffice(input, output) {
            Ok(()) => return Ok(Engine::LibreOffice),
            Err(e) => eprintln!("[flipfile] Excel -> PDF with LibreOffice skipped: {e}"),
        }
    }
    if kind == SheetKind::Other {
        return Err("old .xls and .ods files can only be converted on a computer with Excel or LibreOffice; open it and save it as .xlsx first".into());
    }
    let bytes = fs::read(input).map_err(|e| format!("could not be read: {e}"))?;
    let name = input.file_name().and_then(|n| n.to_str()).unwrap_or("workbook");
    let pdf = super::sheet_to_pdf_builtin(&bytes, name, kind == SheetKind::Csv)?;
    fs::write(output, pdf).map_err(|e| format!("Could not save: {e}"))?;
    Ok(Engine::BuiltIn)
}

/// Presentation -> PDF file, with the best engine available on this computer.
/// `legacy` (.ppt, .pps, .odp) needs PowerPoint or LibreOffice; .pptx always succeeds
/// through the built-in engine when the apps are missing.
pub fn presentation_to_pdf(input: &Path, output: &Path, legacy: bool) -> Result<Engine, String> {
    if !cfg!(test) {
        match powerpoint(input, output) {
            Ok(()) => return Ok(Engine::PowerPoint),
            Err(e) => eprintln!("[flipfile] PowerPoint -> PDF with Microsoft PowerPoint skipped: {e}"),
        }
        match libreoffice(input, output) {
            Ok(()) => return Ok(Engine::LibreOffice),
            Err(e) => eprintln!("[flipfile] PowerPoint -> PDF with LibreOffice skipped: {e}"),
        }
    }
    if legacy {
        return Err("old .ppt and .odp files can only be converted on a computer with PowerPoint or LibreOffice; open it and save it as .pptx first".into());
    }
    let bytes = fs::read(input).map_err(|e| format!("could not be read: {e}"))?;
    let pdf = super::pptx_to_pdf_builtin(&bytes)?;
    fs::write(output, pdf).map_err(|e| format!("Could not save: {e}"))?;
    Ok(Engine::BuiltIn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls() {
        assert_eq!(file_url(Path::new("/tmp/a b/p")), "file:///tmp/a%20b/p");
        assert_eq!(file_url(Path::new("/x/100%")), "file:///x/100%25");
    }

    #[test]
    fn timeouts_stop_only_the_child() {
        let mut cmd = Command::new(if cfg!(windows) { "ping" } else { "sleep" });
        if cfg!(windows) {
            cmd.args(["-n", "30", "127.0.0.1"]);
        } else {
            cmd.arg("30");
        }
        let start = Instant::now();
        assert_eq!(run_with_timeout(cmd, Duration::from_millis(300)), Err("timed out".into()));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
