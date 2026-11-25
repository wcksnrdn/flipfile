// Downloading one file of a pack: streamed to `<name>.part`, picked up where it stopped
// when the connection drops or the app quits, and checked against the catalog's size and
// SHA-256 before it is anything but a part.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::catalog::File;

#[derive(Debug, PartialEq)]
pub enum Failure {
    Cancelled,
    Failed(String),
    /// The connection, not the file: worth trying again
    Dropped(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Failed(message)
    }
}

/// How often a dropped connection is tried again, picking up where it stopped
const TRIES: u32 = 4;

pub fn client() -> Result<reqwest::Client, String> {
    // reqwest is built with rustls and no crypto provider of its own (as the updater
    // uses it): ring, unless one is installed already
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .user_agent(concat!("flipfile/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// Downloads `file` from `url` to `part`, then checks it; `progress` hears every new
/// byte count. On success `part` holds exactly the file; on a mismatch it is deleted.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    file: &File,
    part: &Path,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), Failure> {
    let mut tries = 0;
    loop {
        match fetch_once(client, url, file, part, cancel, progress).await {
            Err(Failure::Dropped(_)) if tries + 1 < TRIES && !cancel.load(Ordering::Relaxed) => {
                tries += 1;
                sleep(if cfg!(test) { Duration::from_millis(10) } else { Duration::from_secs(2 << tries) }).await;
            }
            Err(Failure::Dropped(message)) => return Err(Failure::Failed(message)),
            other => return other,
        }
    }
}

async fn fetch_once(
    client: &reqwest::Client,
    url: &str,
    file: &File,
    part: &Path,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), Failure> {
    // What is there already, and its hash so far
    let mut hasher = Sha256::new();
    let mut have = hash_part(part, file.size, &mut hasher)?;
    if have == file.size {
        return check(part, file, hasher);
    }

    let mut request = client.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().await.map_err(|e| Failure::Dropped(format!("could not download ({})", cause(&e))))?;
    if !(response.url().scheme() == "https" || cfg!(test) && response.url().host_str() == Some("127.0.0.1")) {
        return Err(format!("the download was sent elsewhere ({})", response.url()).into());
    }
    let status = response.status();
    let append = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !status.is_success() {
        let message = format!("could not download (HTTP {})", status.as_u16());
        let busy = status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
        return Err(if busy { Failure::Dropped(message) } else { Failure::Failed(message) });
    }
    if !append {
        // The whole file again: the server ignored the range
        have = 0;
        hasher = Sha256::new();
    }
    progress(have);

    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part)
        .map_err(|e| write_error(&e))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Failure::Cancelled);
        }
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                out.flush().map_err(|e| write_error(&e))?;
                return Err(Failure::Dropped(format!("the download stopped ({})", cause(&e))));
            }
        };
        have += chunk.len() as u64;
        if have > file.size {
            drop(out);
            let _ = std::fs::remove_file(part);
            return Err(format!("{} is larger than it should be", file.path).into());
        }
        out.write_all(&chunk).map_err(|e| write_error(&e))?;
        hasher.update(&chunk);
        progress(have);
    }
    out.sync_all().map_err(|e| write_error(&e))?;
    drop(out);
    if have < file.size {
        return Err(Failure::Dropped("the download stopped early".into()));
    }
    check(part, file, hasher)
}

/// The bytes of `part` there already (none when there is more than the file), hashed
fn hash_part(part: &Path, size: u64, hasher: &mut Sha256) -> Result<u64, Failure> {
    let Ok(mut existing) = std::fs::File::open(part) else { return Ok(0) };
    let length = existing.metadata().map(|m| m.len()).unwrap_or(0);
    if length > size {
        drop(existing);
        let _ = std::fs::remove_file(part);
        return Ok(0);
    }
    let mut buffer = vec![0u8; 1 << 20];
    let mut read = 0;
    loop {
        let n = existing.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(read);
        }
        hasher.update(&buffer[..n]);
        read += n as u64;
    }
}

fn check(part: &Path, file: &File, hasher: Sha256) -> Result<(), Failure> {
    let hash = hex(&hasher.finalize());
    if hash != file.sha256 {
        let _ = std::fs::remove_file(part);
        return Err(format!("{} did not match its checksum, so it was not used", file.path).into());
    }
    Ok(())
}

/// What went wrong underneath: reqwest's own message only names the URL
pub fn cause(e: &reqwest::Error) -> String {
    let mut deepest: &dyn std::error::Error = e;
    while let Some(source) = deepest.source() {
        deepest = source;
    }
    deepest.to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_error(e: &std::io::Error) -> Failure {
    Failure::Failed(save_error(e))
}

pub fn save_error(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::StorageFull => "there is not enough free disk space".into(),
        _ => format!("could not save the download ({e})"),
    }
}

async fn sleep(duration: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await;
}
