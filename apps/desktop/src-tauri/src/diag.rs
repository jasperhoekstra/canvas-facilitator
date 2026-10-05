//! Local, redacted diagnostics log. Never contains keys or audio; shared only via explicit export.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX_BYTES: u64 = 1_000_000;

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    *PATH.lock().unwrap() = Some(dir.join("diagnostics.log"));
}

pub fn path() -> Option<PathBuf> {
    PATH.lock().unwrap().clone()
}

pub fn log(msg: impl AsRef<str>) {
    let Some(p) = path() else { return };
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {}", crate::db::now_ms(), crate::secrets::redact(msg.as_ref()));
    }
}
