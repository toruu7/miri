use pet_core::ipc::{decode_frame_payload, encode_frame, MAX_IPC_FRAME_BYTES};
use pet_core::{DesignerToOverlay, OverlayToDesigner, PROTOCOL_VERSION};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GetLastError, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_NONE,
    OPEN_EXISTING, ReadFile, WriteFile,
};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, SetEvent, WaitForMultipleObjects, INFINITE,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_APP_IPC_EVENT: u32 = WM_APP + 20;

#[allow(clippy::large_enum_variant)]
pub enum ClientEvent {
    Message(OverlayToDesigner),
    Disconnected,
}

pub struct PipeClient {
    inbound: Arc<Mutex<VecDeque<ClientEvent>>>,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    out_ev_raw: isize,
    stop: Arc<AtomicBool>,
    stop_ev_raw: isize,
    connected: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PipeClient {
    pub fn connect(pipe_name: &str, notify_hwnd: HWND) -> Result<Self, String> {
        let pipe_w: Vec<u16> = OsStr::new(pipe_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let (hpipe, out_ev, stop_ev) = unsafe {
            let h = CreateFileW(
                PCWSTR(pipe_w.as_ptr()),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(|e| format!("open pipe: {e}"))?;
            let oev = CreateEventW(None, false, false, PCWSTR::null())
                .map_err(|e| format!("out event: {e}"))?;
            let sev = CreateEventW(None, true, false, PCWSTR::null())
                .map_err(|e| format!("stop event: {e}"))?;
            (h, oev, sev)
        };

        let inbound = Arc::new(Mutex::new(VecDeque::new()));
        let outbound = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(true));

        let in_bg = inbound.clone();
        let out_bg = outbound.clone();
        let stop_bg = stop.clone();
        let conn_bg = connected.clone();
        let pipe_raw = hpipe.0 as isize;
        let out_raw = out_ev.0 as isize;
        let stop_raw = stop_ev.0 as isize;
        let hwnd_raw = notify_hwnd.0 as isize;

        let thread = thread::Builder::new()
            .name("designer-ipc".to_string())
            .spawn(move || {
                run_client_loop(
                    pipe_raw, out_raw, stop_raw, hwnd_raw, in_bg, out_bg, stop_bg, conn_bg,
                );
            })
            .map_err(|e| format!("spawn client: {e}"))?;

        let client = Self {
            inbound,
            outbound,
            out_ev_raw: out_raw,
            stop,
            stop_ev_raw: stop_raw,
            connected,
            thread: Some(thread),
        };
        client.send(&DesignerToOverlay::Hello {
            protocol_version: PROTOCOL_VERSION,
            designer_pid: unsafe { GetCurrentProcessId() },
        });
        Ok(client)
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn send(&self, msg: &DesignerToOverlay) {
        if !self.is_connected() {
            return;
        }
        if let Ok(frame) = encode_frame(msg) {
            if let Ok(mut q) = self.outbound.lock() {
                q.push_back(frame);
            }
            unsafe {
                let _ = SetEvent(HANDLE(self.out_ev_raw as *mut _));
            }
        }
    }

    pub fn drain(&self) -> Vec<ClientEvent> {
        self.inbound
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }
}

impl Drop for PipeClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            let _ = SetEvent(HANDLE(self.stop_ev_raw as *mut _));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe {
            let _ = CloseHandle(HANDLE(self.out_ev_raw as *mut _));
            let _ = CloseHandle(HANDLE(self.stop_ev_raw as *mut _));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_client_loop(
    pipe_raw: isize,
    out_raw: isize,
    stop_raw: isize,
    hwnd_raw: isize,
    inbound: Arc<Mutex<VecDeque<ClientEvent>>>,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
) {
    let hpipe = HANDLE(pipe_raw as *mut _);
    let out_ev = HANDLE(out_raw as *mut _);
    let stop_ev = HANDLE(stop_raw as *mut _);
    let hwnd = HWND(hwnd_raw as *mut _);

    unsafe {
        let (Ok(read_ev), Ok(write_ev)) = (
            CreateEventW(None, true, false, PCWSTR::null()),
            CreateEventW(None, true, false, PCWSTR::null()),
        ) else {
            let _ = CloseHandle(hpipe);
            connected.store(false, Ordering::Release);
            return;
        };

        let mut buf = [0u8; 8192];
        let mut accum = Vec::<u8>::new();
        let mut read_ov = OVERLAPPED {
            hEvent: read_ev,
            ..Default::default()
        };
        let mut read_pending = match ReadFile(hpipe, Some(&mut buf), None, Some(&mut read_ov)) {
            Ok(()) => false,
            Err(_) => GetLastError() == ERROR_IO_PENDING,
        };

        while !stop.load(Ordering::Acquire) {
            loop {
                let next = outbound.lock().ok().and_then(|mut q| q.pop_front());
                let Some(frame) = next else { break };
                if !write_all(hpipe, &frame, write_ev, stop_ev) {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
            if stop.load(Ordering::Acquire) {
                break;
            }

            if !read_pending {
                let mut n = 0u32;
                if GetOverlappedResult(hpipe, &read_ov, &mut n, false).is_err() || n == 0 {
                    break;
                }
                accum.extend_from_slice(&buf[..n as usize]);
                parse_frames(&mut accum, &inbound, hwnd);
                read_ov = OVERLAPPED {
                    hEvent: read_ev,
                    ..Default::default()
                };
                read_pending = match ReadFile(hpipe, Some(&mut buf), None, Some(&mut read_ov)) {
                    Ok(()) => false,
                    Err(_) => GetLastError() == ERROR_IO_PENDING,
                };
                continue;
            }

            let handles = [read_ev, out_ev, stop_ev];
            let r = WaitForMultipleObjects(&handles, false, INFINITE);
            if r == WAIT_OBJECT_0 {
                let mut n = 0u32;
                match GetOverlappedResult(hpipe, &read_ov, &mut n, false) {
                    Ok(()) if n > 0 => {
                        accum.extend_from_slice(&buf[..n as usize]);
                        parse_frames(&mut accum, &inbound, hwnd);
                        read_ov = OVERLAPPED {
                            hEvent: read_ev,
                            ..Default::default()
                        };
                        read_pending = match ReadFile(hpipe, Some(&mut buf), None, Some(&mut read_ov)) {
                            Ok(()) => false,
                            Err(_) => GetLastError() == ERROR_IO_PENDING,
                        };
                    }
                    _ => break,
                }
            } else if r.0 == WAIT_OBJECT_0.0 + 1 {
                continue;
            } else {
                break;
            }
        }

        let _ = CloseHandle(read_ev);
        let _ = CloseHandle(write_ev);
        let _ = CloseHandle(hpipe);
    }

    connected.store(false, Ordering::Release);
    if let Ok(mut q) = inbound.lock() {
        q.push_back(ClientEvent::Disconnected);
    }
    unsafe {
        let _ = PostMessageW(hwnd, WM_APP_IPC_EVENT, WPARAM(0), LPARAM(0));
    }
}

unsafe fn write_all(hpipe: HANDLE, data: &[u8], write_ev: HANDLE, stop_ev: HANDLE) -> bool {
    let mut off = 0usize;
    while off < data.len() {
        let mut ov = OVERLAPPED {
            hEvent: write_ev,
            ..Default::default()
        };
        let ok = match WriteFile(hpipe, Some(&data[off..]), None, Some(&mut ov)) {
            Ok(()) => true,
            Err(_) if GetLastError() == ERROR_IO_PENDING => {
                WaitForMultipleObjects(&[write_ev, stop_ev], false, 2000) == WAIT_OBJECT_0
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
        off += n as usize;
    }
    true
}

fn parse_frames(accum: &mut Vec<u8>, inbound: &Arc<Mutex<VecDeque<ClientEvent>>>, hwnd: HWND) {
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
        if let Ok(msg) = decode_frame_payload::<OverlayToDesigner>(&accum[4..4 + len]) {
            if let Ok(mut q) = inbound.lock() {
                q.push_back(ClientEvent::Message(msg));
            }
            unsafe {
                let _ = PostMessageW(hwnd, WM_APP_IPC_EVENT, WPARAM(0), LPARAM(0));
            }
        }
        accum.drain(..4 + len);
    }
}
