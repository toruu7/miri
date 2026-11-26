use crate::logging::{log_info, log_warn};
use pet_core::config::fnv1a_64_bytes;
use pet_core::{CatProfile, Config, CURRENT_SCHEMA_VERSION, MAX_USER_PROFILES};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::SetEvent;

const AUTOSTART_VALUE_NAME: &str = "DesktopCatCompanion";

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("DesktopCat")
}

pub fn config_path() -> PathBuf {
    app_data_dir().join("config.json")
}

pub fn profiles_dir() -> PathBuf {
    app_data_dir().join("profiles")
}

fn wide_null(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub struct ConfigStore {
    pub config: Config,
    pub user_profiles: Vec<CatProfile>,
    pub last_applied_hash: u64,
    pub read_only_future_schema: bool,
    pub last_error: Option<String>,
    write_seq: u64,
}

impl ConfigStore {
    pub fn load_or_create() -> Self {
        let dir = app_data_dir();
        let _ = fs::create_dir_all(&dir);
        let _ = fs::create_dir_all(profiles_dir());

        let mut store = Self {
            config: Config::default(),
            user_profiles: Vec::new(),
            last_applied_hash: 0,
            read_only_future_schema: false,
            last_error: None,
            write_seq: 0,
        };

        let cfg_path = config_path();
        if cfg_path.exists() {
            store.reload_config_with_recovery();
        } else {
            let _ = store.save_config();
        }
        store.reload_user_profiles();
        store
    }

    pub fn reload_config_with_recovery(&mut self) -> bool {
        let path = config_path();
        let mut last_err = None;
        // 3x retry loop against Notepad in-place truncation & Defender locks
        for delay_ms in [0u64, 50, 100, 200] {
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            match fs::read(&path) {
                Ok(bytes) => {
                    let h = fnv1a_64_bytes(&bytes);
                    if h == self.last_applied_hash {
                        return false; // Suppress self-write reload loop
                    }
                    match serde_json::from_slice::<Config>(&bytes) {
                        Ok(mut parsed) => {
                            if parsed.schema_version > CURRENT_SCHEMA_VERSION {
                                self.read_only_future_schema = true;
                                log_warn("config", "newer schema_version loaded in read-only mode");
                            } else {
                                if parsed.schema_version < CURRENT_SCHEMA_VERSION {
                                    let bak = app_data_dir()
                                        .join(format!("config.v{}.bak", parsed.schema_version));
                                    let _ = fs::copy(&path, bak);
                                    parsed.schema_version = CURRENT_SCHEMA_VERSION;
                                }
                                self.read_only_future_schema = false;
                            }
                            parsed.sanitize();
                            self.config = parsed;
                            self.last_applied_hash = h;
                            self.last_error = None;
                            return true;
                        }
                        Err(e) => {
                            last_err = Some(format!(
                                "json parse error at line {} col {}",
                                e.line(),
                                e.column()
                            ));
                        }
                    }
                }
                Err(e) => {
                    last_err = Some(format!("config read io error: {:?}", e.kind()));
                }
            }
        }

        // Preserve corrupt file untouched (if first boot, fall back to .bak without clobbering)
        if let Some(err) = last_err {
            log_warn("config", &err);
            self.last_error = Some(err);
            if self.last_applied_hash == 0 {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let corrupt = app_data_dir().join(format!("config.corrupt.{ts}.json"));
                let _ = fs::rename(&path, corrupt);
                let bak = app_data_dir().join("config.json.bak");
                if let Ok(bytes) = fs::read(&bak) {
                    if let Ok(mut parsed) = serde_json::from_slice::<Config>(&bytes) {
                        parsed.sanitize();
                        self.last_applied_hash = fnv1a_64_bytes(&bytes);
                        self.config = parsed;
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn save_config(&mut self) -> Result<(), String> {
        if self.read_only_future_schema {
            return Ok(());
        }
        self.config.sanitize();
        let bytes = serde_json::to_vec_pretty(&self.config).map_err(|e| format!("encode: {e}"))?;
        let new_hash = fnv1a_64_bytes(&bytes);
        let path = config_path();
        self.write_seq += 1;
        atomic_write_file(&path, &bytes, self.write_seq)?;
        self.last_applied_hash = new_hash;
        Ok(())
    }

    pub fn reload_user_profiles(&mut self) {
        self.user_profiles.clear();
        let dir = profiles_dir();
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if self.user_profiles.len() >= MAX_USER_PROFILES {
                break;
            }
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = fs::read(&p) {
                if let Ok(mut prof) = serde_json::from_slice::<CatProfile>(&bytes) {
                    prof.sanitize();
                    if !prof.is_builtin() {
                        self.user_profiles.push(prof);
                    }
                }
            }
        }
        self.user_profiles.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn save_user_profile(&mut self, mut profile: CatProfile) -> Result<CatProfile, String> {
        profile.sanitize();
        if profile.is_builtin() {
            return Err("built-in presets are read-only; clone into a user profile".to_string());
        }
        if self.user_profiles.len() >= MAX_USER_PROFILES
            && !self.user_profiles.iter().any(|p| p.id == profile.id)
        {
            return Err(format!("profile limit ({MAX_USER_PROFILES}) reached"));
        }
        let slug = profile
            .id
            .strip_prefix("user:")
            .unwrap_or(&profile.id)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(64)
            .collect::<String>();
        let path = profiles_dir().join(format!("{slug}.json"));
        let bytes = serde_json::to_vec_pretty(&profile).map_err(|e| format!("encode: {e}"))?;
        self.write_seq += 1;
        atomic_write_file(&path, &bytes, self.write_seq)?;
        if let Some(slot) = self.user_profiles.iter_mut().find(|p| p.id == profile.id) {
            *slot = profile.clone();
        } else {
            self.user_profiles.push(profile.clone());
        }
        self.user_profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profile)
    }

    pub fn delete_user_profile(&mut self, id: &str) -> Result<(), String> {
        if id.starts_with("builtin:") {
            return Err("cannot delete built-in preset".to_string());
        }
        let slug = id
            .strip_prefix("user:")
            .unwrap_or(id)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(64)
            .collect::<String>();
        let path = profiles_dir().join(format!("{slug}.json"));
        let _ = fs::remove_file(path);
        self.user_profiles.retain(|p| p.id != id);
        if self.config.active_profile_id == id {
            self.config.active_profile_id = "builtin:midnight".to_string();
            let _ = self.save_config();
        }
        Ok(())
    }
}

/// Atomic temp -> `sync_all` -> `.bak` rotate -> `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`
/// with 3× retry against transient Windows Defender sharing locks.
pub fn atomic_write_file(dest: &Path, bytes: &[u8], seq: u64) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let pid = std::process::id();
    let tmp = dest.with_extension(format!("json.{pid}.{seq}.tmp"));
    let mut file = File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
    file.write_all(bytes).map_err(|e| format!("write tmp: {e}"))?;
    file.sync_all().map_err(|e| format!("sync tmp: {e}"))?;
    drop(file);

    if dest.exists() {
        let bak = dest.with_extension("json.bak");
        let _ = fs::copy(dest, bak);
    }

    let tmp_w = wide_null(tmp.as_os_str());
    let dest_w = wide_null(dest.as_os_str());
    for attempt in 0..4 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(50));
        }
        unsafe {
            if MoveFileExW(
                PCWSTR(tmp_w.as_ptr()),
                PCWSTR(dest_w.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .is_ok()
            {
                return Ok(());
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err("atomic MoveFileExW failed after retries".to_string())
}

/// Parent-directory `ReadDirectoryChangesW` watcher signaling UI event with 150 ms trailing debounce.
pub struct ConfigWatcher {
    stop: Arc<AtomicBool>,
    pending_since_ms: Arc<AtomicU64>,
    _thread: Option<JoinHandle<()>>,
}

impl ConfigWatcher {
    pub fn spawn(wake_event: HANDLE) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pending_since_ms = Arc::new(AtomicU64::new(0));
        let stop_bg = stop.clone();
        let pending_bg = pending_since_ms.clone();
        let wake_raw = wake_event.0 as isize;

        let handle = thread::Builder::new()
            .name("config-watch".to_string())
            .spawn(move || {
                watch_config_dir(stop_bg, pending_bg, wake_raw);
            })
            .ok();

        Self {
            stop,
            pending_since_ms,
            _thread: handle,
        }
    }

    /// Returns true once the 150 ms debounce window has elapsed since last directory notification.
    pub fn poll_debounced_ready(&self) -> bool {
        let ts = self.pending_since_ms.load(Ordering::Acquire);
        if ts == 0 {
            return false;
        }
        let now = now_ms();
        if now.saturating_sub(ts) >= 150 {
            self.pending_since_ms.store(0, Ordering::Release);
            true
        } else {
            false
        }
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn watch_config_dir(stop: Arc<AtomicBool>, pending: Arc<AtomicU64>, wake_raw: isize) {
    let dir = app_data_dir();
    let _ = fs::create_dir_all(&dir);
    let dir_w = wide_null(dir.as_os_str());
    unsafe {
        let hdir = match CreateFileW(
            PCWSTR(dir_w.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log_warn("config_watch", &format!("CreateFileW dir failed: {e}"));
                return;
            }
        };

        let mut buf = [0u32; 512];
        let mut bytes_returned = 0u32;
        while !stop.load(Ordering::Acquire) {
            if ReadDirectoryChangesW(
                hdir,
                buf.as_mut_ptr() as *mut _,
                (buf.len() * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
                Some(&mut bytes_returned),
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            if stop.load(Ordering::Acquire) {
                break;
            }
            pending.store(now_ms(), Ordering::Release);
            thread::sleep(Duration::from_millis(160));
            let _ = SetEvent(HANDLE(wake_raw as *mut _));
        }
        let _ = CloseHandle(hdir);
    }
}

/// Reconcile `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DesktopCatCompanion`
/// while respecting user Task Manager overrides in `StartupApproved\Run`.
pub fn reconcile_autostart(enabled: bool) {
    unsafe {
        let run_subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let val_w = wide_null(OsStr::new(AUTOSTART_VALUE_NAME));

        if !enabled {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, run_subkey, 0, KEY_WRITE, &mut hkey).is_ok() {
                let _ = RegDeleteValueW(hkey, PCWSTR(val_w.as_ptr()));
                let _ = RegCloseKey(hkey);
            }
            return;
        }

        // Respect Task Manager disabled status in StartupApproved\Run
        let approved_subkey =
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
        let mut h_app = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, approved_subkey, 0, KEY_READ, &mut h_app).is_ok() {
            let mut data = [0u8; 16];
            let mut len = data.len() as u32;
            let mut kind = REG_BINARY;
            if RegQueryValueExW(
                h_app,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut len),
            )
            .is_ok()
                && len >= 1
                && (data[0] & 0x01) != 0
            {
                // Odd first byte (e.g. 0x03) means user disabled startup via Task Manager!
                let _ = RegCloseKey(h_app);
                log_info("autostart", "disabled in Task Manager StartupApproved; skipping write");
                return;
            }
            let _ = RegCloseKey(h_app);
        }

        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let cmd = format!("\"{}\" --autostart", exe.display());
        let cmd_w: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();

        let mut hkey = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            run_subkey,
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .is_ok()
        {
            let bytes = std::slice::from_raw_parts(cmd_w.as_ptr() as *const u8, cmd_w.len() * 2);
            let _ = RegSetValueExW(hkey, PCWSTR(val_w.as_ptr()), 0, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hkey);
        }
    }
}
