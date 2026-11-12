// AI summaries: drop a document on "Summary" and a glass panel beside the circle tells
// what it says, written live by Apple's on-device model. Nothing leaves the computer:
// the text is read here (convert::text) and summarised by the flipfile-ai helper.
//
// - helper: starting flipfile-ai and reading what it writes
// - report: a finished summary as Markdown, for Copy and Save
// - window: the panel window and its place beside the circle
//
// The circle starts a job with `summarize` (only files the user handed over, as for
// convert_batch). The panel runs it with `summary_run` and receives every step on its
// channel as it happens, while the circle shows the progress (`summary-progress`).
//
// Waiting is kept short three ways: the model is woken up while a file is still being
// dragged or read (`summary_warm`), the panel shows a few telling sentences of the
// document the moment it is read (`Peek`), and a file summarised before and unchanged
// since comes back at once from memory (`Summaries::cache`).

mod helper;
mod report;
mod window;

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};

use crate::convert::text::{self, Text};
use crate::state::grants::Grants;
use crate::state::history::History;

/// One step of a summary job, sent to the panel
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// A new job: the files' names, in order
    Start { files: Vec<String> },
    /// One file is summarised again: in another language (translated), or after a failure
    Restart { file: usize, language: Option<String> },
    /// Reading the file's text; `pages` is known for PDFs and scans. `listening`: a
    /// recording is being transcribed, `page` of `pages` is then percent of 100
    Reading { file: usize, page: usize, pages: usize, listening: bool },
    /// The text is read
    Read { file: usize, kind: &'static str, pages: usize, words: usize, ocr: bool },
    /// A few telling sentences of the text, picked without the model: something to read
    /// while it works
    Peek { file: usize, sentences: Vec<String> },
    /// A long document is read in parts; `kept` is below 1 when it was condensed first
    Plan { file: usize, parts: usize, kept: f64 },
    /// Taking notes on one part, and the notes so far
    Part { file: usize, part: usize, parts: usize },
    Notes { file: usize, part: usize, text: String },
    /// Writing the summary itself
    Writing { file: usize },
    /// The summary so far: its fields appear one after another as the model writes them
    Summary { file: usize, data: Value },
    /// `cached`: the file was summarised before and has not changed since
    Done { file: usize, data: Value, language: String, document_language: Option<String>, seconds: f64, cached: bool },
    Failed { file: usize, code: String, message: String },
    /// The answer to a question about a summarised file, as it is written
    Answer { file: usize, text: String, done: bool, error: Option<String> },
    /// Nothing more comes for this job
    Finished,
}

/// The circle's status line while the panel works
#[derive(Clone, Serialize)]
struct Progress {
    title: String,
    hint: String,
    tone: &'static str, // "busy", "ok", "err", or "idle" to clear it
    progress: Option<f64>,
}

/// A file as it was when dropped: summarised again only once it changes
#[derive(Clone, PartialEq)]
struct Key {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

impl Key {
    fn of(path: &Path) -> Option<Key> {
        let meta = std::fs::metadata(path).ok()?;
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        Some(Key { path, size: meta.len(), modified: meta.modified().ok() })
    }
}

/// Summaries kept in memory (never on disk) until the app quits
const CACHED: usize = 16;

/// One file of the job, kept for Copy, Save and another language
#[derive(Clone)]
struct Doc {
    path: PathBuf,
    key: Option<Key>,
    text: Option<Text>,
    notes: Vec<String>,
    /// The summary in every language it was written in, the document's own first
    versions: Vec<(String, Value)>,
    /// The version shown now, which Copy and Save use
    current: Option<Value>,
    document_language: Option<String>,
    /// Questions asked about it, with their answers (Copy and Save include them)
    answers: Vec<(String, String)>,
}

#[derive(Default)]
struct Job {
    /// Bumped by every new job, retry and close: a run that sees another number stops
    generation: u64,
    /// Files dropped on Summary, waiting for the panel to start them
    pending: Option<Vec<PathBuf>>,
    /// Pasted clipboard images (temporary files) save their summary in Downloads
    to_downloads: bool,
    docs: Vec<Doc>,
}

#[derive(Default)]
pub struct Summaries {
    job: Mutex<Job>,
    running: helper::Running,
    /// Files summarised earlier, least recently used first
    cache: Mutex<Vec<Doc>>,
}

impl Summaries {
    fn current(&self, generation: u64) -> bool {
        self.job.lock().unwrap().generation == generation
    }

    /// Starts over: whatever runs now stops, and its events are no longer sent
    fn next(&self) -> u64 {
        let mut job = self.job.lock().unwrap();
        job.generation += 1;
        self.running.stop();
        job.generation
    }

    /// Keeps a finished file (its text, notes and every language it was written in) for
    /// when it is dropped again. Questions start over: they belong to that one look.
    fn remember(&self, file: usize) {
        let Some(mut doc) = self.job.lock().unwrap().docs.get(file).cloned() else { return };
        let Some(key) = doc.key.clone() else { return };
        if doc.current.is_none() || doc.text.is_none() {
            return;
        }
        doc.answers.clear();
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|earlier| earlier.key.as_ref() != Some(&key));
        cache.push(doc);
        if cache.len() > CACHED {
            cache.remove(0);
        }
    }

    /// The earlier summary of an unchanged file, now the most recently used
    fn recall(&self, key: &Key) -> Option<Doc> {
        let mut cache = self.cache.lock().unwrap();
        let index = cache.iter().position(|earlier| earlier.key.as_ref() == Some(key))?;
        let doc = cache.remove(index);
        cache.push(doc.clone());
        Some(doc)
    }
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("File")
}

/// Sends a run's events to its panel channel (until a newer run takes over) and keeps
/// the circle's status in step.
struct Sender<'a> {
    app: &'a AppHandle,
    generation: u64,
    channel: &'a Channel<Event>,
    files: usize,
    done: AtomicUsize,
    failed: Mutex<Option<String>>,
}

impl<'a> Sender<'a> {
    fn new(app: &'a AppHandle, generation: u64, channel: &'a Channel<Event>, files: usize) -> Self {
        Sender { app, generation, channel, files, done: AtomicUsize::new(0), failed: Mutex::new(None) }
    }

    fn live(&self) -> bool {
        self.app.state::<Summaries>().current(self.generation)
    }

    fn send(&self, event: Event) {
        if !self.live() {
            return;
        }
        if let Some(progress) = self.progress(&event) {
            let _ = self.app.emit_to("main", "summary-progress", progress);
        }
        let _ = self.channel.send(event);
    }

    fn progress(&self, event: &Event) -> Option<Progress> {
        let busy = |title: &str, hint: String, progress: Option<f64>| {
            Some(Progress { title: title.into(), hint, tone: "busy", progress })
        };
        // "2/3 · Part 1 of 4" when several files were dropped
        let of = |file: usize, hint: String| if self.files > 1 { format!("{}/{} · {hint}", file + 1, self.files) } else { hint };
        match event {
            Event::Restart { language: Some(_), .. } => busy("Translating", "Apple Intelligence".into(), None),
            Event::Start { .. } | Event::Restart { .. } => busy("Reading", "Apple Intelligence".into(), None),
            Event::Reading { file, page, listening: true, .. } => busy("Listening", of(*file, format!("{page}%")), Some(*page as f64 / 100.0)),
            Event::Reading { file, page, pages, .. } if *pages > 1 => {
                busy("Reading", of(*file, format!("Page {}/{pages}", (page + 1).min(*pages))), Some(*page as f64 / *pages as f64))
            }
            Event::Part { file, part, parts } => {
                busy("Reading", of(*file, format!("Part {part} of {parts}")), Some((*part - 1) as f64 / *parts as f64))
            }
            Event::Writing { file } => busy("Writing", of(*file, "Apple Intelligence".into()), None),
            Event::Done { .. } => {
                self.done.fetch_add(1, Ordering::Relaxed);
                None
            }
            Event::Failed { message, .. } => {
                *self.failed.lock().unwrap() = Some(message.clone());
                None
            }
            Event::Finished => {
                let done = self.done.load(Ordering::Relaxed);
                Some(if done > 0 {
                    Progress { title: "✓ Summary".into(), hint: "Click to show".into(), tone: "ok", progress: None }
                } else {
                    let hint = self.failed.lock().unwrap().clone().unwrap_or_else(|| "Nothing to summarize".into());
                    Progress { title: "Failed".into(), hint, tone: "err", progress: None }
                })
            }
            _ => None,
        }
    }
}

/// Whether summaries can be written here (the ring only offers Summary where they can)
#[tauri::command]
pub async fn summary_status() -> Result<helper::Availability, String> {
    tauri::async_runtime::spawn_blocking(helper::availability).await.map_err(|e| e.to_string())
}

/// Files dropped (or pasted and clicked) on Summary: opens the panel, which runs them
#[tauri::command]
pub async fn summarize(app: AppHandle, paths: Vec<String>, to_downloads: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    // Only files the user dropped or pasted (see grants.rs)
    if let Err(denied) = app.state::<Grants>().check(&paths) {
        return Err(format!("\"{}\" was not given to flipfile", file_name(Path::new(denied))));
    }
    let state = app.state::<Summaries>();
    state.next();
    {
        let mut job = state.job.lock().unwrap();
        job.pending = Some(paths.into_iter().map(PathBuf::from).collect());
        job.to_downloads = to_downloads;
    }
    if !window::show(&app).map_err(|e| e.to_string())? {
        let _ = app.emit_to(window::LABEL, "summary-job", ());
    }
    Ok(())
}

/// The panel picks up the waiting job; returns false when there is none (it was taken)
#[tauri::command]
pub async fn summary_run(app: AppHandle, on_event: Channel<Event>) -> Result<bool, String> {
    let state = app.state::<Summaries>();
    let (generation, paths) = {
        let mut job = state.job.lock().unwrap();
        let Some(paths) = job.pending.take() else { return Ok(false) };
        job.docs = paths
            .iter()
            .map(|path| Doc {
                path: path.clone(),
                key: Key::of(path),
                text: None,
                notes: vec![],
                versions: vec![],
                current: None,
                document_language: None,
                answers: vec![],
            })
            .collect();
        (job.generation, paths)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let send = Sender::new(&app, generation, &on_event, paths.len());
        send.send(Event::Start { files: paths.iter().map(|p| file_name(p).to_string()).collect() });
        for file in 0..paths.len() {
            let live = if recall(&app, &send, file) { send.live() } else { summarize_file(&app, &send, file, None) };
            if !live {
                return;
            }
        }
        send.send(Event::Finished);
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// One file of the last job again: in another language (the finished summary is
/// translated; a language it was already written in comes back at once), or from
/// scratch after a failure, with `language` None.
#[tauri::command]
pub async fn summary_redo(app: AppHandle, file: usize, language: Option<String>, on_event: Channel<Event>) -> Result<(), String> {
    let state = app.state::<Summaries>();
    let generation = state.next();
    let files = state.job.lock().unwrap().docs.len();
    if file >= files {
        return Err("There is nothing to summarize".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let send = Sender::new(&app, generation, &on_event, files);
        send.send(Event::Restart { file, language: language.clone() });
        let state = app.state::<Summaries>();
        let known = language.as_ref().and_then(|wanted| {
            let mut job = state.job.lock().unwrap();
            let doc = job.docs.get_mut(file)?;
            let (_, summary) = doc.versions.iter().find(|(written, _)| written == wanted)?.clone();
            doc.current = Some(summary.clone());
            Some((summary, doc.document_language.clone()))
        });
        if language.is_none() {
            // From scratch: translations of the summary being replaced go with it
            if let Some(doc) = state.job.lock().unwrap().docs.get_mut(file) {
                doc.versions.clear();
                doc.current = None;
            }
        }
        if let (Some((data, document_language)), Some(language)) = (known, language.clone()) {
            state.remember(file);
            send.send(Event::Done { file, data, language, document_language, seconds: 0.0, cached: false });
        } else if !summarize_file(&app, &send, file, language.as_deref()) {
            return;
        }
        send.send(Event::Finished);
    })
    .await
    .map_err(|e| e.to_string())
}

/// A file summarised before and unchanged since: its summary, in the language it was
/// last shown in, is back at once. Returns whether it was.
fn recall(app: &AppHandle, send: &Sender, file: usize) -> bool {
    let state = app.state::<Summaries>();
    let Some(key) = state.job.lock().unwrap().docs.get(file).and_then(|doc| doc.key.clone()) else { return false };
    let Some(earlier) = state.recall(&key) else { return false };
    let (Some(text), Some(data)) = (earlier.text.clone(), earlier.current.clone()) else { return false };
    let language = earlier.versions.iter().find(|(_, version)| *version == data).map_or("en".into(), |(written, _)| written.clone());
    let document_language = earlier.document_language.clone();
    {
        let mut job = state.job.lock().unwrap();
        let Some(doc) = job.docs.get_mut(file) else { return false };
        *doc = Doc { path: doc.path.clone(), ..earlier };
    }
    send.send(Event::Read { file, kind: text.kind, pages: text.pages, words: text.words, ocr: text.ocr });
    send.send(Event::Done { file, data, language, document_language, seconds: 0.0, cached: true });
    true
}

/// Reads one file and has it summarised. Returns false once the run is out of date.
fn summarize_file(app: &AppHandle, send: &Sender, file: usize, language: Option<&str>) -> bool {
    let state = app.state::<Summaries>();
    let Some((path, cached, notes, original)) = state.job.lock().unwrap().docs.get(file).map(|doc| {
        // Another language translates the finished summary (written in the document's own
        // language); a retry starts from scratch
        let original = language.and(doc.versions.first()).map(|(_, summary)| summary.to_string());
        let notes = if language.is_some() { doc.notes.clone() } else { vec![] };
        (doc.path.clone(), doc.text.clone(), notes, original)
    }) else {
        return send.live();
    };
    let name = file_name(&path).to_string();
    // The model loads while the text is read
    helper::warm();

    let text = match cached {
        Some(text) => text,
        None => {
            let listening = crate::convert::transcribe::is_recording(&path);
            send.send(Event::Reading { file, page: 0, pages: 0, listening });
            let on_page = |page: usize, pages: usize| send.send(Event::Reading { file, page, pages, listening });
            // Reading runs third-party decoders: a bug there fails this file, not the app
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| text::extract(&path, &on_page)))
                .unwrap_or_else(|_| Err(format!("\"{name}\" could not be read (internal error)")));
            match read {
                Ok(text) => text,
                Err(message) => {
                    send.send(Event::Failed { file, code: "unreadable".into(), message });
                    return send.live();
                }
            }
        }
    };
    if !send.live() {
        return false;
    }
    send.send(Event::Read { file, kind: text.kind, pages: text.pages, words: text.words, ocr: text.ocr });
    if let Some(doc) = state.job.lock().unwrap().docs.get_mut(file) {
        doc.text = Some(text.clone());
    }

    let request = helper::Request {
        text: &text.text,
        name: &name,
        kind: text.kind,
        pages: text.pages,
        language: language.unwrap_or("auto"),
        notes: &notes,
        summary: original.as_deref(),
        question: None,
    };
    let mut document_language = None;
    let mut ended = false;
    let result = helper::summarize(&state.running, &request, |event| {
        let count = |key: &str| event[key].as_u64().unwrap_or(0) as usize;
        match event["event"].as_str().unwrap_or_default() {
            "language" => document_language = event["document"].as_str().map(String::from),
            "plan" if count("parts") > 1 => {
                send.send(Event::Plan { file, parts: count("parts"), kept: event["kept"].as_f64().unwrap_or(1.0) })
            }
            "peek" => {
                let sentences = event["sentences"].as_array().map(|all| all.iter().filter_map(|s| s.as_str().map(String::from)).collect());
                send.send(Event::Peek { file, sentences: sentences.unwrap_or_default() })
            }
            "part" => send.send(Event::Part { file, part: count("part"), parts: count("parts") }),
            "notes" => send.send(Event::Notes { file, part: count("part"), text: event["text"].as_str().unwrap_or_default().into() }),
            "compose" => send.send(Event::Writing { file }),
            "summary" => send.send(Event::Summary { file, data: event["data"].clone() }),
            "done" => {
                ended = true;
                let language: String = event["language"].as_str().unwrap_or("en").into();
                if let Some(doc) = state.job.lock().unwrap().docs.get_mut(file) {
                    let data = event["data"].clone();
                    doc.versions.retain(|(written, _)| *written != language);
                    doc.versions.push((language.clone(), data.clone()));
                    doc.current = Some(data);
                    if doc.document_language.is_none() {
                        doc.document_language = document_language.clone();
                    }
                    if let Some(notes) = event["notes"].as_array() {
                        doc.notes = notes.iter().filter_map(|n| n.as_str().map(String::from)).collect();
                    }
                }
                state.remember(file);
                send.send(Event::Done {
                    file,
                    data: event["data"].clone(),
                    language,
                    document_language: document_language.clone(),
                    seconds: event["seconds"].as_f64().unwrap_or(0.0),
                    cached: false,
                });
            }
            "error" => {
                ended = true;
                send.send(Event::Failed {
                    file,
                    code: event["code"].as_str().unwrap_or("other").into(),
                    message: event["message"].as_str().unwrap_or_default().into(),
                });
            }
            _ => {}
        }
    });
    match result {
        Err(message) => send.send(Event::Failed { file, code: "unsupported".into(), message }),
        Ok(true) if !ended => send.send(Event::Failed {
            file,
            code: "crashed".into(),
            message: "Apple Intelligence stopped before the summary was finished".into(),
        }),
        _ => {}
    }
    send.live()
}

/// A question about a summarised file, answered from its text by the model
#[tauri::command]
pub async fn summary_ask(app: AppHandle, file: usize, question: String, on_event: Channel<Event>) -> Result<(), String> {
    let question: String = question.trim().chars().take(500).collect();
    if question.is_empty() {
        return Ok(());
    }
    let state = app.state::<Summaries>();
    let (path, text, summary) = {
        let job = state.job.lock().unwrap();
        let doc = job.docs.get(file).ok_or("There is nothing to ask about")?;
        let text = doc.text.clone().ok_or("The document has not been read yet")?;
        (doc.path.clone(), text, doc.current.as_ref().map(Value::to_string))
    };
    let generation = state.next();
    let files = state.job.lock().unwrap().docs.len();
    tauri::async_runtime::spawn_blocking(move || {
        let send = Sender::new(&app, generation, &on_event, files);
        let name = file_name(&path).to_string();
        let request = helper::Request {
            text: &text.text,
            name: &name,
            kind: text.kind,
            pages: text.pages,
            language: "auto",
            notes: &[],
            summary: summary.as_deref(),
            question: Some(&question),
        };
        let state = app.state::<Summaries>();
        let mut ended = false;
        let result = helper::summarize(&state.running, &request, |event| match event["event"].as_str().unwrap_or_default() {
            "answer" => send.send(Event::Answer { file, text: event["text"].as_str().unwrap_or_default().into(), done: false, error: None }),
            "answered" => {
                ended = true;
                let answer = event["text"].as_str().unwrap_or_default().to_string();
                if let Some(doc) = state.job.lock().unwrap().docs.get_mut(file) {
                    doc.answers.push((question.clone(), answer.clone()));
                }
                send.send(Event::Answer { file, text: answer, done: true, error: None });
            }
            "error" => {
                ended = true;
                let message = event["message"].as_str().unwrap_or("The question could not be answered").to_string();
                send.send(Event::Answer { file, text: String::new(), done: true, error: Some(message) });
            }
            _ => {}
        });
        let failure = match result {
            Err(message) => Some(message),
            Ok(true) if !ended => Some("Apple Intelligence stopped before answering".to_string()),
            _ => None,
        };
        if let Some(message) = failure {
            send.send(Event::Answer { file, text: String::new(), done: true, error: Some(message) });
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// A file is being dragged over the circle: the model wakes up in the meantime
#[tauri::command]
pub fn summary_warm() {
    helper::warm();
}

/// The circle was clicked after a summary: shows the panel again
#[tauri::command]
pub async fn summary_show(app: AppHandle) -> Result<(), String> {
    window::show(&app).map(|_| ()).map_err(|e| e.to_string())
}

/// The panel was closed: anything still running stops, and the circle lets go of it
#[tauri::command]
pub fn summary_close(app: AppHandle) {
    app.state::<Summaries>().next();
    window::hide(&app);
    let idle = Progress { title: String::new(), hint: String::new(), tone: "idle", progress: None };
    let _ = app.emit_to("main", "summary-progress", idle);
}

fn markdown(app: &AppHandle, file: usize) -> Result<(String, PathBuf), String> {
    let state = app.state::<Summaries>();
    let job = state.job.lock().unwrap();
    let doc = job.docs.get(file).ok_or("There is no summary yet")?;
    let summary = doc.current.as_ref().ok_or("The summary is not finished yet")?;
    Ok((report::markdown(file_name(&doc.path), summary, &doc.answers), doc.path.clone()))
}

/// Copies a finished summary as Markdown
#[tauri::command]
pub fn summary_copy(app: AppHandle, file: usize) -> Result<(), String> {
    let (markdown, _) = markdown(&app, file)?;
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(markdown))
        .map_err(|e| format!("Could not copy: {e}"))
}

/// Saves a finished summary next to the original as `name_summary.md` (in Downloads
/// for a pasted image); it joins the history, so Undo removes it again.
#[tauri::command]
pub fn summary_save(app: AppHandle, file: usize) -> Result<String, String> {
    let (markdown, path) = markdown(&app, file)?;
    let out_dir = if app.state::<Summaries>().job.lock().unwrap().to_downloads {
        Some(app.path().download_dir().map_err(|_| "The Downloads folder could not be found".to_string())?)
    } else {
        None
    };
    let output = crate::convert::unique_output_path(&path, out_dir.as_deref(), "summary", "md");
    std::fs::write(&output, markdown).map_err(|e| format!("Could not save: {e}"))?;
    let output = output.to_string_lossy().into_owned();
    app.state::<Mutex<History>>().lock().unwrap().record(std::slice::from_ref(&output));
    crate::platform::tray::refresh(&app);
    Ok(output)
}

/// Opens System Settings at Apple Intelligence, to turn it on
#[tauri::command]
pub fn summary_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Siri-Settings.extension").spawn();
}

/// The app is quitting: a summary still being written stops with it
pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Summaries>() {
        state.running.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(key: Option<Key>, summary: Option<Value>) -> Doc {
        Doc {
            path: key.as_ref().map_or_else(|| PathBuf::from("a.txt"), |k| k.path.clone()),
            key,
            text: Some(Text { text: "Hello.".into(), pages: 1, words: 1, kind: "Text", ocr: false }),
            notes: vec![],
            versions: summary.iter().map(|s| ("en".to_string(), s.clone())).collect(),
            current: summary,
            document_language: Some("en".into()),
            answers: vec![("Why?".into(), "Because.".into())],
        }
    }

    fn key(name: &str, size: u64) -> Key {
        Key { path: PathBuf::from(name), size, modified: None }
    }

    #[test]
    fn finished_files_come_back_until_they_change() {
        let state = Summaries::default();
        state.job.lock().unwrap().docs = vec![doc(Some(key("a.txt", 5)), Some(json!({"title": "A"}))), doc(Some(key("b.txt", 5)), None)];
        state.remember(0);
        state.remember(1); // not finished: nothing to keep

        let earlier = state.recall(&key("a.txt", 5)).expect("kept");
        assert_eq!(earlier.current, Some(json!({"title": "A"})));
        assert!(earlier.answers.is_empty(), "questions start over");
        assert!(state.recall(&key("a.txt", 6)).is_none(), "a changed file is summarized again");
        assert!(state.recall(&key("b.txt", 5)).is_none());
    }

    #[test]
    fn the_least_recently_used_goes_first() {
        let state = Summaries::default();
        for i in 0..=CACHED {
            state.job.lock().unwrap().docs = vec![doc(Some(key(&format!("{i}.txt"), 1)), Some(json!({})))];
            state.remember(0);
            if i == 0 {
                continue;
            }
            // The first file stays in use
            assert!(state.recall(&key("0.txt", 1)).is_some());
        }
        assert_eq!(state.cache.lock().unwrap().len(), CACHED);
        assert!(state.recall(&key("0.txt", 1)).is_some());
        assert!(state.recall(&key("1.txt", 1)).is_none(), "the oldest unused one made room");
    }
}
