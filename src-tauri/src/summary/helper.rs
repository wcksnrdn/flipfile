// Talks to flipfile-ai, the small Swift program that runs Apple's on-device model
// (src-tauri/intelligence/main.swift). On macOS it is bundled next to the app's own
// executable (`externalBin` in tauri.macos.conf.json); everywhere else, and on Macs
// without Apple Intelligence, summaries are unavailable.
//
// The helper reads one request on stdin and writes its progress as JSON lines on
// stdout. It runs as its own process, so a model that hangs or crashes never takes the
// app with it, and stopping a summary is simply ending that process.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Whether summaries can be written on this computer, and if not, why
#[derive(Clone, Serialize)]
pub struct Availability {
    /// The hardware and system can run Apple Intelligence: the ring offers Summary
    pub supported: bool,
    /// Apple Intelligence is ready right now
    pub available: bool,
    /// Why not: "appleIntelligenceNotEnabled", "modelNotReady", "deviceNotEligible", "unsupported"
    pub reason: Option<String>,
}

impl Availability {
    fn unsupported() -> Self {
        Availability { supported: false, available: false, reason: Some("unsupported".into()) }
    }
}

fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join("flipfile-ai");
    path.is_file().then_some(path)
}

/// Asks the helper whether Apple Intelligence is ready. On macOS before 26 the helper
/// cannot even start (its framework is missing): that, too, reads as unsupported.
pub fn availability() -> Availability {
    let Some(helper) = helper() else { return Availability::unsupported() };
    let Ok(output) = Command::new(helper).arg("status").stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return Availability::unsupported();
    };
    let line = String::from_utf8_lossy(&output.stdout);
    let Ok(status) = serde_json::from_str::<Value>(line.trim()) else { return Availability::unsupported() };
    if status["available"].as_bool() == Some(true) {
        return Availability { supported: true, available: true, reason: None };
    }
    let reason = status["reason"].as_str().unwrap_or("unsupported").to_string();
    // A Mac that can never run it (Intel, or no Apple Intelligence hardware) hides Summary
    let supported = !matches!(reason.as_str(), "deviceNotEligible" | "unsupported");
    Availability { supported, available: false, reason: Some(reason) }
}

/// What one summary needs
pub struct Request<'a> {
    pub text: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    pub pages: usize,
    /// "auto" (the document's own language) or a language code
    pub language: &'a str,
    /// Notes from an earlier run: only the summary itself is written again
    pub notes: &'a [String],
    /// A finished summary (JSON): translated into `language`, or, with `question`, the
    /// overview the answer starts from
    pub summary: Option<&'a str>,
    /// A question to answer from `text`, instead of summarising it
    pub question: Option<&'a str>,
}

/// The helper working right now, with the number of the run that started it
#[derive(Default)]
pub struct Running(Mutex<Option<(u64, Child)>>);

static RUNS: AtomicU64 = AtomicU64::new(1);

impl Running {
    /// Ends the helper's work at once (a newer job, or the panel was closed)
    pub fn stop(&self) {
        if let Some((_, mut child)) = self.0.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Runs one summary and hands every event the helper writes to `on_event`, until it
/// exits or is stopped. Returns whether it ended by itself (not stopped).
pub fn summarize(running: &Running, request: &Request, mut on_event: impl FnMut(Value)) -> Result<bool, String> {
    let helper = helper().ok_or("Apple Intelligence summaries are not available on this computer")?;
    let mut child = Command::new(helper)
        .arg("summarize")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Apple Intelligence could not be started ({e})"))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err("Apple Intelligence could not be started".into());
    };
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    *running.0.lock().unwrap() = Some((run, child));

    let body = json!({
        "text": request.text,
        "name": request.name,
        "kind": request.kind,
        "pages": request.pages,
        "language": request.language,
        "notes": request.notes,
        "summary": request.summary,
        "question": request.question,
    })
    .to_string();
    // A long text fills the pipe before the helper has read it all, so it is written
    // from a thread of its own; closing stdin afterwards tells the helper it is complete.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(body.as_bytes());
    });
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            on_event(event);
        }
    }
    let _ = writer.join();

    // Still ours: it finished by itself. Otherwise `stop` ended (and reaped) it.
    let mut slot = running.0.lock().unwrap();
    if slot.as_ref().is_some_and(|(id, _)| *id == run) {
        if let Some((_, mut child)) = slot.take() {
            let _ = child.wait();
        }
        return Ok(true);
    }
    Ok(false)
}
