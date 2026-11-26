use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 1024 * 1024; // 1 MB per file, max 3 files (<= 3 MB total)

struct Logger {
    path: PathBuf,
    file: Option<File>,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

pub fn local_log_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("DesktopCat").join("logs")
}

pub fn init_logging() {
    let dir = local_log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("overlay.log");
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let _ = LOGGER.set(Mutex::new(Logger { path, file }));
    log_info("startup", "cat-overlay logging initialized");
}

fn rotate_if_needed(logger: &mut Logger) {
    let needs_rotate = logger
        .file
        .as_ref()
        .and_then(|f| f.metadata().ok())
        .map(|m| m.len() >= MAX_LOG_BYTES)
        .unwrap_or(false);
    if !needs_rotate {
        return;
    }
    logger.file = None;
    let p2 = logger.path.with_extension("log.2");
    let p1 = logger.path.with_extension("log.1");
    let _ = fs::remove_file(&p2);
    let _ = fs::rename(&p1, &p2);
    let _ = fs::rename(&logger.path, &p1);
    logger.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logger.path)
        .ok();
}

/// Strictly redacted structured logger.
/// INVARIANT: Never pass raw keycodes, characters, window titles, or user file paths.
pub fn log_line(level: &str, scope: &str, message: &str) {
    if let Some(lock) = LOGGER.get() {
        if let Ok(mut guard) = lock.lock() {
            rotate_if_needed(&mut guard);
            if let Some(f) = &mut guard.file {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(f, "{ts} [{level}] {scope}: {message}");
                let _ = f.flush();
            }
        }
    }
}

pub fn log_info(scope: &str, message: &str) {
    log_line("INFO", scope, message);
}

pub fn log_warn(scope: &str, message: &str) {
    log_line("WARN", scope, message);
}

pub fn log_error(scope: &str, message: &str) {
    log_line("ERROR", scope, message);
}
