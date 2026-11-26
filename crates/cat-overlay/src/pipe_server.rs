use crate::logging::{log_info, log_warn};
use pet_core::ipc::{decode_frame_payload, encode_frame, MAX_IPC_FRAME_BYTES};
use pet_core::{DesignerToOverlay, OverlayToDesigner};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    HLOCAL, LocalFree, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcessToken, SetEvent, WaitForMultipleObjects, INFINITE,
};

struct LocalFreeGuard(*mut c_void);
impl Drop for LocalFreeGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(HLOCAL(self.0));
            }
        }
    }
}

/// Build an explicit current-user-SID-only SDDL DACL: `D:P(A;;GA;;;<USER_SID>)`.
/// Never uses `OW` or broad group ACEs.
fn current_user_sddl() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| format!("OpenProcessToken: {e}"))?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len.max(64) as usize];
        let res = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut c_void),
            buf.len() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        res.map_err(|e| format!("GetTokenInformation: {e}"))?;

        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid_str)
            .map_err(|e| format!("ConvertSidToStringSidW: {e}"))?;
        let _guard = LocalFreeGuard(sid_str.0 as *mut c_void);
        let s = sid_str
            .to_string()
            .map_err(|e| format!("sid utf16: {e}"))?;
        Ok(format!("D:P(A;;GA;;;{s})"))
    }
}

#[allow(clippy::large_enum_variant)]
pub enum PipeEvent {
    Message(DesignerToOverlay),
    Disconnected,
}

pub struct PipeSession {
    inbound: Arc<Mutex<VecDeque<PipeEvent>>>,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    out_event_raw: isize,
    stop: Arc<AtomicBool>,
    stop_event_raw: isize,
    thread: Option<JoinHandle<()>>,
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl PipeSession {
    /// Create a fresh, single-instance, user-SID-locked Named Pipe server before spawning `cat-designer.exe`.
    pub fn start(
        pipe_name: String,
        expected_child_pid: Arc<Mutex<Option<u32>>>,
        ui_wake_event: HANDLE,
    ) -> Result<Self, String> {
        let sddl = current_user_sddl()?;
        let sddl_w: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let pipe_w: Vec<u16> = std::ffi::OsStr::new(&pipe_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let (pipe_handle, out_event, stop_event) = unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl_w.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
            .map_err(|e| format!("SDDL parse: {e}"))?;
            let _sd_guard = LocalFreeGuard(sd.0);

            let sa = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };

            let hpipe = CreateNamedPipeW(
                PCWSTR(pipe_w.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                Some(&sa),
            );
            if hpipe.is_invalid() {
                return Err(format!("CreateNamedPipeW failed: {:?}", GetLastError()));
            }
            let out_ev = CreateEventW(None, false, false, PCWSTR::null())
                .map_err(|e| format!("CreateEventW out: {e}"))?;
            let stop_ev = CreateEventW(None, true, false, PCWSTR::null())
                .map_err(|e| format!("CreateEventW stop: {e}"))?;
            (hpipe, out_ev, stop_ev)
        };

        let inbound = Arc::new(Mutex::new(VecDeque::new()));
        let outbound = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let last_error = Arc::new(Mutex::new(None));

        let in_bg = inbound.clone();
        let out_bg = outbound.clone();
        let stop_bg = stop.clone();
        let err_bg = last_error.clone();
        let pipe_raw = pipe_handle.0 as isize;
        let out_ev_raw = out_event.0 as isize;
        let stop_ev_raw = stop_event.0 as isize;
        let ui_wake_raw = ui_wake_event.0 as isize;

        let thread = thread::Builder::new()
            .name("ipc-pipe".to_string())
            .spawn(move || {
                run_pipe_server(
                    pipe_raw,
                    out_ev_raw,
                    stop_ev_raw,
                    ui_wake_raw,
                    expected_child_pid,
                    in_bg,
                    out_bg,
                    stop_bg,
                    err_bg,
                );
            })
            .map_err(|e| format!("spawn pipe thread: {e}"))?;

        Ok(Self {
            inbound,
            outbound,
            out_event_raw: out_ev_raw,
            stop,
            stop_event_raw: stop_ev_raw,
            thread: Some(thread),
            last_error,
        })
    }

    pub fn send(&self, msg: &OverlayToDesigner) {
        if let Ok(frame) = encode_frame(msg) {
            if let Ok(mut q) = self.outbound.lock() {
                q.push_back(frame);
            }
            unsafe {
                let _ = SetEvent(HANDLE(self.out_event_raw as *mut _));
            }
        }
    }

    pub fn drain_events(&self) -> Vec<PipeEvent> {
        self.inbound
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn shutdown(&mut self) {
        self.send(&OverlayToDesigner::OverlayShuttingDown);
        thread::sleep(std::time::Duration::from_millis(30));
        self.stop.store(true, Ordering::Release);
        unsafe {
            let _ = SetEvent(HANDLE(self.stop_event_raw as *mut _));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe {
            let _ = CloseHandle(HANDLE(self.out_event_raw as *mut _));
            let _ = CloseHandle(HANDLE(self.stop_event_raw as *mut _));
        }
    }
}

impl Drop for PipeSession {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.shutdown();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_pipe_server(
    pipe_raw: isize,
    out_ev_raw: isize,
    stop_ev_raw: isize,
    ui_wake_raw: isize,
    expected_pid: Arc<Mutex<Option<u32>>>,
    inbound: Arc<Mutex<VecDeque<PipeEvent>>>,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    let hpipe = HANDLE(pipe_raw as *mut _);
    let out_ev = HANDLE(out_ev_raw as *mut _);
    let stop_ev = HANDLE(stop_ev_raw as *mut _);
    let ui_wake = HANDLE(ui_wake_raw as *mut _);

    unsafe {
        let Ok(io_ev) = CreateEventW(None, true, false, PCWSTR::null()) else {
            let _ = CloseHandle(hpipe);
            return;
        };

        // 1. Overlapped ConnectNamedPipe (treating ERROR_PIPE_CONNECTED as success!)
        let mut ov = OVERLAPPED {
            hEvent: io_ev,
            ..Default::default()
        };
        let connected = match ConnectNamedPipe(hpipe, Some(&mut ov)) {
            Ok(()) => true,
            Err(_) => {
                let err = GetLastError();
                if err == ERROR_PIPE_CONNECTED {
                    true
                } else if err == ERROR_IO_PENDING {
                    let wait = WaitForMultipleObjects(&[io_ev, stop_ev], false, 15_000);
                    if wait == WAIT_OBJECT_0 {
                        let mut dummy = 0u32;
                        GetOverlappedResult(hpipe, &ov, &mut dummy, false).is_ok()
                    } else {
                        false
                    }
                } else {
                    if let Ok(mut e) = last_error.lock() {
                        *e = Some(format!("ConnectNamedPipe err {:?}", err));
                    }
                    false
                }
            }
        };

        if !connected || stop.load(Ordering::Acquire) {
            let _ = CloseHandle(io_ev);
            let _ = CloseHandle(hpipe);
            notify_disconnect(&inbound, ui_wake);
            return;
        }

        // 2. Verify connected client PID matches the spawned cat-designer.exe child!
        let mut client_pid = 0u32;
        if GetNamedPipeClientProcessId(hpipe, &mut client_pid).is_ok() {
            if let Ok(exp) = expected_pid.lock() {
                if let Some(expected) = *exp {
                    if client_pid != expected {
                        log_warn("ipc", "rejected unexpected Named Pipe client PID");
                        let _ = DisconnectNamedPipe(hpipe);
                        let _ = CloseHandle(io_ev);
                        let _ = CloseHandle(hpipe);
                        notify_disconnect(&inbound, ui_wake);
                        return;
                    }
                }
            }
        }
        log_info("ipc", "cat-designer connected over SID-locked Named Pipe");

        // 3. Overlapped duplex message loop
        let Ok(read_ev) = CreateEventW(None, true, false, PCWSTR::null()) else {
            let _ = CloseHandle(io_ev);
            let _ = CloseHandle(hpipe);
            return;
        };
        let mut read_buf = [0u8; 8192];
        let mut accum = Vec::<u8>::new();
        let mut read_ov = OVERLAPPED {
            hEvent: read_ev,
            ..Default::default()
        };
        let mut read_pending = start_overlapped_read(hpipe, &mut read_buf, &mut read_ov);

        while !stop.load(Ordering::Acquire) {
            // Flush outbound writes first
            loop {
                let next_frame = outbound.lock().ok().and_then(|mut q| q.pop_front());
                let Some(frame) = next_frame else { break };
                if !write_all_overlapped(hpipe, &frame, io_ev, stop_ev) {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
            if stop.load(Ordering::Acquire) {
                break;
            }

            if !read_pending {
                // Either immediate data or disconnect
                let mut n = 0u32;
                if GetOverlappedResult(hpipe, &read_ov, &mut n, false).is_err() || n == 0 {
                    break;
                }
                accum.extend_from_slice(&read_buf[..n as usize]);
                drain_frames(&mut accum, &inbound, ui_wake);
                read_ov = OVERLAPPED {
                    hEvent: read_ev,
                    ..Default::default()
                };
                read_pending = start_overlapped_read(hpipe, &mut read_buf, &mut read_ov);
                continue;
            }

            let handles = [read_ev, out_ev, stop_ev];
            let res = WaitForMultipleObjects(&handles, false, INFINITE);
            if res == WAIT_OBJECT_0 {
                let mut n = 0u32;
                match GetOverlappedResult(hpipe, &read_ov, &mut n, false) {
                    Ok(()) if n > 0 => {
                        accum.extend_from_slice(&read_buf[..n as usize]);
                        drain_frames(&mut accum, &inbound, ui_wake);
                        read_ov = OVERLAPPED {
                            hEvent: read_ev,
                            ..Default::default()
                        };
                        read_pending = start_overlapped_read(hpipe, &mut read_buf, &mut read_ov);
                    }
                    _ => {
                        let err = GetLastError();
                        if err != ERROR_BROKEN_PIPE {
                            if let Ok(mut e) = last_error.lock() {
                                *e = Some(format!("pipe read ended: {:?}", err));
                            }
                        }
                        break;
                    }
                }
            } else if res.0 == WAIT_OBJECT_0.0 + 1 {
                // Outbound write queued -> handled at top of loop
                continue;
            } else {
                break;
            }
        }

        let _ = DisconnectNamedPipe(hpipe);
        let _ = CloseHandle(read_ev);
        let _ = CloseHandle(io_ev);
        let _ = CloseHandle(hpipe);
        notify_disconnect(&inbound, ui_wake);
    }
}

unsafe fn start_overlapped_read(hpipe: HANDLE, buf: &mut [u8], ov: &mut OVERLAPPED) -> bool {
    match ReadFile(hpipe, Some(buf), None, Some(ov)) {
        Ok(()) => false,
        Err(_) => GetLastError() == ERROR_IO_PENDING,
    }
}

unsafe fn write_all_overlapped(hpipe: HANDLE, data: &[u8], io_ev: HANDLE, stop_ev: HANDLE) -> bool {
    let mut written = 0usize;
    while written < data.len() {
        let mut ov = OVERLAPPED {
            hEvent: io_ev,
            ..Default::default()
        };
        let slice = &data[written..];
        let ok = match WriteFile(hpipe, Some(slice), None, Some(&mut ov)) {
            Ok(()) => true,
            Err(_) if GetLastError() == ERROR_IO_PENDING => {
                WaitForMultipleObjects(&[io_ev, stop_ev], false, 2000) == WAIT_OBJECT_0
            }
            Err(_) => false,
        };
        if !ok {
            return false;
        }
        let mut n = 0u32;
        if GetOverlappedResult(hpipe, &ov, &mut n, false).is_err() || n == 0 {
            return false;
        }
        written += n as usize;
    }
    true
}

fn drain_frames(accum: &mut Vec<u8>, inbound: &Arc<Mutex<VecDeque<PipeEvent>>>, ui_wake: HANDLE) {
    loop {
        if accum.len() < 4 {
            break;
        }
        let len = u32::from_le_bytes(accum[0..4].try_into().unwrap()) as usize;
        if len > MAX_IPC_FRAME_BYTES {
            accum.clear();
            break;
        }
        if accum.len() < 4 + len {
            break;
        }
        let payload = &accum[4..4 + len];
        if let Ok(msg) = decode_frame_payload::<DesignerToOverlay>(payload) {
            if let Ok(mut q) = inbound.lock() {
                q.push_back(PipeEvent::Message(msg));
            }
            unsafe {
                let _ = SetEvent(ui_wake);
            }
        }
        accum.drain(..4 + len);
    }
}

fn notify_disconnect(inbound: &Arc<Mutex<VecDeque<PipeEvent>>>, ui_wake: HANDLE) {
    if let Ok(mut q) = inbound.lock() {
        q.push_back(PipeEvent::Disconnected);
    }
    unsafe {
        let _ = SetEvent(ui_wake);
    }
}
