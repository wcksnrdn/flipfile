use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::catalog::{self, Catalog, File, Pack, SYSTEM};
use super::download::{self, Failure};
use super::store::{Presence, Store};
use super::{install, Active, Packs};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("flipfile-packs-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha(data: &[u8]) -> String {
    download::hex(&Sha256::digest(data))
}

fn file(path: &str, data: &[u8]) -> File {
    File { path: path.into(), sha256: sha(data), size: data.len() as u64 }
}

/// A pack of a Core ML-like model (a folder of two files) and a one-file model
fn fake_catalog(base: &str, weights: &[u8]) -> Catalog {
    let mut files = BTreeMap::new();
    files.insert(SYSTEM.to_string(), vec![file("fake.mlmodelc/coremldata.bin", b"header"), file("fake.mlmodelc/weights/weight.bin", weights)]);
    files.insert("all".to_string(), vec![file("fake.bin", b"single model")]);
    let pack = Pack { id: "fake".into(), name: "Fake".into(), targets: vec!["fake-2x".into()], files };
    Catalog { format: catalog::FORMAT, serial: 1, base: base.into(), packs: vec![pack] }
}

/// Puts each file of `catalog` in `downloads` as if downloaded
fn downloaded(store: &Store, contents: &[(&str, &[u8])]) {
    std::fs::create_dir_all(store.downloads()).unwrap();
    for (_, data) in contents {
        std::fs::write(store.downloads().join(sha(data)), data).unwrap();
    }
}

#[test]
fn the_built_in_catalog_is_valid_and_complete() {
    let catalog = Catalog::built_in();
    assert_eq!(catalog.format, catalog::FORMAT);
    let ids: Vec<&str> = catalog.packs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["upscale", "upscale-text", "colorize", "transcribe"]);
    let mut targets = vec![];
    for pack in &catalog.packs {
        assert!(pack.size() > 1_000_000, "{} has its models here", pack.id);
        for system in ["coreml", "onnx"] {
            let on = |s: &str| pack.files.get(s).into_iter().chain(pack.files.get("all")).flatten().count();
            assert!(on(system) > 0, "{} has files for {system}", pack.id);
        }
        targets.extend(pack.targets.iter().cloned());
    }
    let count = targets.len();
    targets.sort();
    targets.dedup();
    assert_eq!(targets.len(), count, "each target needs one pack");
    // Every model the engines load is in a pack, or ships with the app
    for model in ["upscale.mlmodelc", "upscale-photo.onnx", "yunet.onnx", "dejpeg.mlmodelc", "colorize.onnx", "whisper-small.bin", "whisper-vad.bin"] {
        let found = catalog.packs.iter().any(|p| p.files.values().flatten().any(|f| f.model() == model));
        assert!(found, "{model} is in a pack");
    }
    assert!(catalog.pack_of("fonts/Inter-Regular.ttf").is_none() && catalog.pack_of("words.txt").is_none());
}

#[test]
fn the_built_in_catalog_matches_the_fetched_models() {
    // The models scripts/fetch-models.mjs fetched and checked are the ones listed
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
    for file in Catalog::built_in().packs.iter().flat_map(Pack::files) {
        let Ok(data) = std::fs::read(dir.join(&file.path)) else { continue };
        assert_eq!(sha(&data), file.sha256, "{} is the catalog's", file.path);
    }
}

#[test]
fn a_catalog_is_refused_when_it_could_reach_outside_the_models() {
    let good = fake_catalog("https://example.com/packs", b"weights");
    let json = |c: &Catalog| serde_json::to_vec(c).unwrap();
    assert!(Catalog::parse(&json(&good)).is_ok());

    for path in ["../escape.bin", "/etc/passwd", "a/../../b", "C:/x", "a\\b", ".hidden/x", "a//b", ""] {
        let mut bad = good.clone();
        bad.packs[0].files.get_mut("all").unwrap()[0].path = path.into();
        assert!(Catalog::parse(&json(&bad)).is_err(), "{path:?} is refused");
    }
    let mut bad = good.clone();
    bad.packs[0].files.get_mut("all").unwrap()[0].sha256 = "ABC".into();
    assert!(Catalog::parse(&json(&bad)).is_err(), "a bad hash");
    let mut bad = good.clone();
    bad.base = "http://example.com".into();
    assert!(Catalog::parse(&json(&bad)).is_err(), "plain HTTP");
    let mut bad = good;
    bad.format = catalog::FORMAT + 1;
    assert!(Catalog::parse(&json(&bad)).is_err(), "another build's format");
}

// A throwaway key, made with `tauri signer generate`, and its signature of DATA
const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENCQUFEQUQzM0NCNzFCODEKUldTQkc3YzgwOXFxeTlmTzFYVm1pK3dvcGo5Zm9qdENVd2hMa1FMTUdLNFVYMUxjQm44QzBvS2gK";
const DATA: &[u8] = br#"{"hello":"packs"}"#;
const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTQkc3YzgwOXFxeTBpZTRXS3JoQXFZVGwzZFJRM2dLQWFIS3ZiUWhhYzVYYTlnTGVmSEQ5SmR3djF5OUt5MXUzM0lYbmdhT3hDMjlGR3JnN2xnSkVJMVU3QnZzTGg4NFFBPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk1NTgwODI2CWZpbGU6ZGF0YS5qc29uCjVMZUQ4VVdVamk2VFpVS1ViM2x3dDNLTmFYeEVIV3RzNk40S1A2a2lWa0tKMXZ5aGpZVDNCTzd6YkNzcDQrcEdMR1pRWjNjSkJja2plZUFtRnAxWEJBPT0K";

#[test]
fn only_a_catalog_signed_with_the_key_is_trusted() {
    assert_eq!(catalog::verify(DATA, SIGNATURE, KEY), Ok(()));
    assert!(catalog::verify(br#"{"hello":"pack"}"#, SIGNATURE, KEY).is_err(), "changed data");
    assert!(catalog::verify(DATA, "bm90IGEgc2lnbmF0dXJl", KEY).is_err(), "not a signature");
    // The app's own key didn't sign it
    let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let app_key = conf["plugins"]["updater"]["pubkey"].as_str().unwrap();
    assert!(catalog::verify(DATA, SIGNATURE, app_key).is_err(), "another key");
}

#[test]
fn a_pack_is_installed_updated_and_removed_as_whole_models() {
    let dir = temp_dir("store");
    let store = Store { dir: dir.join("models"), bundled: vec![] };
    let v1 = fake_catalog("https://example.com", b"weights v1");
    let pack = &v1.packs[0];

    assert_eq!(store.presence(pack, &v1, &store.installed()), Presence::Missing);
    assert_eq!(store.to_download(pack, &v1, &store.installed()).len(), 3);
    // Half downloaded: the model isn't put together from what is missing
    downloaded(&store, &[("", b"header")]);
    assert!(store.assemble(pack, &v1).is_err());
    assert!(!store.dir.join("fake.mlmodelc").exists(), "no half model in place");

    downloaded(&store, &[("", b"weights v1"), ("", b"single model")]);
    assert_eq!(store.to_download(pack, &v1, &store.installed()).len(), 0, "all downloaded");
    assert_eq!(store.assemble(pack, &v1), Ok(true));
    assert_eq!(store.presence(pack, &v1, &store.installed()), Presence::Current);
    assert_eq!(std::fs::read(store.dir.join("fake.mlmodelc/weights/weight.bin")).unwrap(), b"weights v1");
    assert!(std::fs::read_dir(store.downloads()).unwrap().next().is_none(), "downloads cleared once in place");

    // A new version changes one file: only that one is downloaded
    let v2 = fake_catalog("https://example.com", b"weights v2");
    let pack2 = &v2.packs[0];
    assert_eq!(store.presence(pack2, &v1, &store.installed()), Presence::Outdated);
    let needed = store.to_download(pack2, &v1, &store.installed());
    assert_eq!(needed.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["fake.mlmodelc/weights/weight.bin"]);
    downloaded(&store, &[("", b"weights v2")]);
    assert_eq!(store.assemble(pack2, &v1), Ok(true));
    assert_eq!(store.presence(pack2, &v1, &store.installed()), Presence::Current);
    assert_eq!(std::fs::read(store.dir.join("fake.mlmodelc/weights/weight.bin")).unwrap(), b"weights v2");
    assert_eq!(std::fs::read(store.dir.join("fake.mlmodelc/coremldata.bin")).unwrap(), b"header", "kept");

    store.remove(pack2, &v2).unwrap();
    assert_eq!(store.presence(pack2, &v1, &store.installed()), Presence::Missing);
    assert!(store.installed().is_empty());
    store.finish_pending();
    assert!(!store.dir.join(".trash").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bundled_pack_needs_nothing_and_stays() {
    let dir = temp_dir("bundled");
    let bundle = dir.join("bundle");
    let catalog = fake_catalog("https://example.com", b"weights v1");
    for (path, data) in [("fake.mlmodelc/coremldata.bin", &b"header"[..]), ("fake.mlmodelc/weights/weight.bin", b"weights v1"), ("fake.bin", b"single model")] {
        std::fs::create_dir_all(bundle.join(path).parent().unwrap()).unwrap();
        std::fs::write(bundle.join(path), data).unwrap();
    }
    let store = Store { dir: dir.join("models"), bundled: vec![bundle] };
    let pack = &catalog.packs[0];
    assert_eq!(store.presence(pack, &catalog, &store.installed()), Presence::Current);
    assert!(store.to_download(pack, &catalog, &store.installed()).is_empty());
    assert_eq!(store.assemble(pack, &catalog), Ok(true));
    assert!(!store.dir.join("fake.mlmodelc").exists(), "not copied out of the bundle");

    // A newer pack than the bundled one: the unchanged files are copied from the bundle
    let newer = fake_catalog("https://example.com", b"weights v2");
    let pack = &newer.packs[0];
    assert_eq!(store.presence(pack, &catalog, &store.installed()), Presence::Outdated);
    assert_eq!(store.to_download(pack, &catalog, &store.installed()).len(), 1);
    downloaded(&store, &[("", b"weights v2")]);
    assert_eq!(store.assemble(pack, &catalog), Ok(true));
    assert_eq!(store.presence(pack, &catalog, &store.installed()), Presence::Current);
    assert_eq!(std::fs::read(store.dir.join("fake.mlmodelc/coremldata.bin")).unwrap(), b"header");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn bundled_models_are_kept_for_an_update_without_them() {
    let dir = temp_dir("adopt");
    let bundle = dir.join("bundle");
    let catalog = fake_catalog("https://example.com", b"weights v1");
    for (path, data) in [("fake.mlmodelc/coremldata.bin", &b"header"[..]), ("fake.mlmodelc/weights/weight.bin", b"weights v1"), ("fake.bin", b"single model")] {
        std::fs::create_dir_all(bundle.join(path).parent().unwrap()).unwrap();
        std::fs::write(bundle.join(path), data).unwrap();
    }
    let store = Store { dir: dir.join("models"), bundled: vec![bundle.clone()] };
    let pack = &catalog.packs[0];
    // Half staged by an earlier start that was quit: cleared, then adopted whole
    std::fs::create_dir_all(store.dir.join(".staging/fake.mlmodelc")).unwrap();
    store.finish_pending();
    assert!(!store.dir.join(".staging/fake.mlmodelc").exists());
    store.adopt(pack, &catalog, &bundle).unwrap();
    assert_eq!(std::fs::read(store.dir.join("fake.mlmodelc/weights/weight.bin")).unwrap(), b"weights v1");
    assert!(store.bundles(pack), "nothing to remove while the app still has them");

    // The update without them: still there, nothing to download
    std::fs::remove_dir_all(&bundle).unwrap();
    let after = Store { dir: store.dir.clone(), bundled: vec![bundle.clone()] };
    assert_eq!(after.presence(pack, &catalog, &after.installed()), Presence::Current);
    assert!(after.to_download(pack, &catalog, &after.installed()).is_empty());
    assert!(!after.bundles(pack));

    // A model here already (in any version) is never replaced by the bundled one
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("fake.bin"), b"single model").unwrap();
    std::fs::write(store.dir.join("fake.bin"), b"newer model").unwrap();
    store.adopt(pack, &catalog, &bundle).unwrap();
    assert_eq!(std::fs::read(store.dir.join("fake.bin")).unwrap(), b"newer model");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_model_waiting_for_the_next_start_is_put_in_place_then() {
    let dir = temp_dir("pending");
    let store = Store { dir: dir.join("models"), bundled: vec![] };
    let catalog = fake_catalog("https://example.com", b"weights v1");
    // As `stage` leaves it when the installed model is locked
    let staging = store.dir.join(".staging");
    std::fs::create_dir_all(staging.join("fake.bin").parent().unwrap()).unwrap();
    std::fs::write(staging.join("fake.bin"), b"single model").unwrap();
    let files: Vec<&File> = catalog.packs[0].files().filter(|f| f.model() == "fake.bin").collect();
    std::fs::write(staging.join("fake.bin.ready"), serde_json::to_vec(&files).unwrap()).unwrap();
    assert!(store.waiting("fake.bin"));

    store.finish_pending();
    assert!(!store.waiting("fake.bin"));
    assert_eq!(std::fs::read(store.dir.join("fake.bin")).unwrap(), b"single model");
    assert_eq!(store.installed().get("fake.bin"), Some(&files[0].sha256));
    let _ = std::fs::remove_dir_all(dir);
}

/// A server for `body` that honours ranges; `cut` stops the first answer after so many
/// bytes, as a dropped connection does. Counts the requests.
fn serve(body: Vec<u8>, cut: Option<usize>) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let ranges = Arc::new(Mutex::new(vec![]));
    let (count, seen) = (requests.clone(), ranges.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut request = vec![];
            let mut byte = [0u8];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request).to_lowercase();
            let first = count.fetch_add(1, Ordering::SeqCst) == 0;
            let from = request
                .lines()
                .find_map(|l| l.strip_prefix("range: bytes="))
                .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
            seen.lock().unwrap().push(from.map(|f| f.to_string()).unwrap_or_default());
            let start = from.unwrap_or(0);
            let status = if from.is_some() { "206 Partial Content" } else { "200 OK" };
            let rest = &body[start..];
            let header = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", rest.len());
            let _ = stream.write_all(header.as_bytes());
            let send = match cut {
                Some(n) if first => &rest[..n.min(rest.len())],
                _ => rest,
            };
            let _ = stream.write_all(send);
        }
    });
    (url, requests, ranges)
}

fn fetch(url: &str, file: &File, part: &Path) -> Result<(), Failure> {
    let client = download::client().unwrap();
    let cancel = AtomicBool::new(false);
    tauri::async_runtime::block_on(download::fetch(&client, url, file, part, &cancel, &mut |_| {}))
}

#[test]
fn a_dropped_download_picks_up_where_it_stopped() {
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, requests, ranges) = serve(body.clone(), Some(70_000));
    let dir = temp_dir("resume");
    let part = dir.join("file.part");
    assert_eq!(fetch(&url, &file("file", &body), &part), Ok(()));
    assert_eq!(std::fs::read(&part).unwrap(), body);
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(ranges.lock().unwrap()[1], "70000", "the rest asked for, not the whole file again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_download_that_is_not_the_file_is_thrown_away() {
    // As long as the file, but not it
    let other = b"something else entirely";
    let (url, _, _) = serve(other.to_vec(), None);
    let dir = temp_dir("mismatch");
    let part = dir.join("file.part");
    let expected = file("file", &[b'x'; 23]);
    assert_eq!(other.len(), 23);
    assert!(matches!(fetch(&url, &expected, &part), Err(Failure::Failed(_))));
    assert!(!part.exists(), "nothing unchecked is kept");

    // Larger than the catalog says: stopped, not filled up
    let (url, _, _) = serve(vec![7u8; 100_000], None);
    let big = fetch(&url, &file("file", &[7u8; 1000]), &part);
    assert!(matches!(big, Err(Failure::Failed(_))), "{big:?}");
    assert!(!part.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn installing_downloads_what_is_missing_and_puts_it_in_place() {
    // Every file at <base>/<sha256>: one server answers them all by the hash asked for
    let contents: Vec<&[u8]> = vec![b"header", b"weights v1", b"single model"];
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let by_hash: BTreeMap<String, Vec<u8>> = contents.iter().map(|c| (sha(c), c.to_vec())).collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut request = vec![];
            let mut byte = [0u8];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request);
            let hash = request.split_whitespace().nth(1).unwrap_or("").trim_start_matches('/');
            let body = by_hash.get(hash).cloned().unwrap_or_default();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(&body);
        }
    });

    let dir = temp_dir("install");
    let catalog = fake_catalog(&base, b"weights v1");
    let packs = Packs {
        store: Store { dir: dir.join("models"), bundled: vec![] },
        built_in: catalog.clone(),
        catalog: Mutex::new(catalog),
        key: None,
        active: Mutex::default(),
        checked: Mutex::default(),
    };
    let active = Active::default();
    let last = Mutex::new((0, 0));
    let done = tauri::async_runtime::block_on(install(&packs, "fake", &active, &|done, total| *last.lock().unwrap() = (done, total)));
    assert_eq!(done, Ok(true));
    let total = (b"header".len() + b"weights v1".len() + b"single model".len()) as u64;
    assert_eq!(*last.lock().unwrap(), (total, total));
    let status = packs.status();
    assert_eq!((status[0].state, status[0].download, status[0].removable), ("installed", 0, true));
    assert_eq!(std::fs::read(packs.store.dir.join("fake.bin")).unwrap(), b"single model");

    // Cancelled: nothing is put in place
    let other = Active::default();
    other.cancel.store(true, Ordering::Relaxed);
    std::fs::remove_dir_all(packs.store.dir.join("fake.mlmodelc")).unwrap();
    let cancelled = tauri::async_runtime::block_on(install(&packs, "fake", &other, &|_, _| {}));
    assert_eq!(cancelled, Err(Failure::Cancelled));
    assert_eq!(packs.status()[0].state, "missing");
    let _ = std::fs::remove_dir_all(dir);
}
