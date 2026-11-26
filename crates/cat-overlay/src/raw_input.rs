use crate::logging::{log_info, log_warn};
use pet_core::{from_windows_scancode, PhysicalKey};
use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::SetEvent;
use windows::Win32::UI::Input::{
    GetRawInputBuffer, GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
    RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE,
    MSG, PostMessageW, PostQuitMessage, RI_KEY_BREAK, RI_KEY_E0, RI_KEY_E1,
    RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, RI_MOUSE_RIGHT_BUTTON_DOWN,
    RI_MOUSE_RIGHT_BUTTON_UP, RI_MOUSE_WHEEL, RegisterClassExW, WM_APP, WM_INPUT, WNDCLASSEXW,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};

const WM_APP_SET_RAWINPUT: u32 = WM_APP + 10;
const WM_APP_STOP_THREAD: u32 = WM_APP + 11;

pub struct DrainedInput {
    pub mouse_dx: i32,
    pub mouse_dy: i32,
    pub wheel_delta: i16,
    pub left_btn: Option<bool>,
    pub left_clicked: bool,
    pub right_btn: Option<bool>,
    pub right_clicked: bool,
    pub keys: Vec<(PhysicalKey, bool)>,
    /// Anonymous key activity flag emitted ONLY in Manual Privacy Mode (zero `PhysicalKey` crosses boundary).
    pub privacy_activity: bool,
}

/// Lock-free mailbox between the `HWND_MESSAGE` Raw Input thread and the UI frame loop.
/// Handles 4-8 kHz gaming mice with zero `PostMessageW` storms while awake, preserves sub-frame
/// clicks via atomic edge-latching, enforces key-identity stripping at the input-thread boundary
/// during Privacy Mode, and wakes `PARKED_0` instantaneously using a `SeqCst` handshake.
pub struct InputMailbox {
    mouse_dx: AtomicI32,
    mouse_dy: AtomicI32,
    mouse_wheel: AtomicI32,
    // bit 0 (0b001) = DOWN edge latched, bit 1 (0b010) = UP edge latched, bit 2 (0b100) = latest held state
    mouse_left_bits: AtomicU32,
    mouse_right_bits: AtomicU32,
    key_ring: Mutex<VecDeque<(PhysicalKey, bool)>>,
    privacy_active: AtomicBool,
    privacy_allow_lease: AtomicBool,
    privacy_activity_flag: AtomicBool,
    pub input_seq: AtomicU64,
    pub overlay_parked: AtomicBool,
    pub events_counter: AtomicU32,
    pub registered: AtomicBool,
    wake_event_raw: isize,
}

impl InputMailbox {
    pub fn new(wake_event: HANDLE) -> Self {
        Self {
            mouse_dx: AtomicI32::new(0),
            mouse_dy: AtomicI32::new(0),
            mouse_wheel: AtomicI32::new(0),
            mouse_left_bits: AtomicU32::new(0),
            mouse_right_bits: AtomicU32::new(0),
            key_ring: Mutex::new(VecDeque::with_capacity(64)),
            privacy_active: AtomicBool::new(false),
            privacy_allow_lease: AtomicBool::new(false),
            privacy_activity_flag: AtomicBool::new(false),
            input_seq: AtomicU64::new(0),
            overlay_parked: AtomicBool::new(false),
            events_counter: AtomicU32::new(0),
            registered: AtomicBool::new(false),
            wake_event_raw: wake_event.0 as isize,
        }
    }

    /// Update input-thread privacy boundary enforcement. When `active` is true, key identities
    /// are dropped BEFORE scancode mapping on the Raw Input thread; `allow_lease` is true ONLY
    /// in Manual Privacy Mode (`CredentialHeuristic` auto-privacy stays strictly silent at 0 wakeups).
    pub fn set_privacy_state(&self, active: bool, allow_lease: bool) {
        self.privacy_active.store(active, Ordering::Release);
        self.privacy_allow_lease
            .store(active && allow_lease, Ordering::Release);
        if active {
            if let Ok(mut q) = self.key_ring.lock() {
                q.clear();
            }
            self.privacy_activity_flag.store(false, Ordering::Release);
        }
    }

    fn signal_if_parked(&self) {
        self.input_seq.fetch_add(1, Ordering::SeqCst);
        if self.overlay_parked.load(Ordering::SeqCst) {
            unsafe {
                let _ = SetEvent(HANDLE(self.wake_event_raw as *mut _));
            }
        }
    }

    fn push_key(&self, key: PhysicalKey, is_down: bool) {
        if let Ok(mut q) = self.key_ring.lock() {
            if q.len() >= 64 {
                q.pop_front();
            }
            q.push_back((key, is_down));
        }
        self.events_counter.fetch_add(1, Ordering::Relaxed);
        self.signal_if_parked();
    }

    fn push_privacy_activity(&self) {
        self.privacy_activity_flag.store(true, Ordering::Release);
        self.events_counter.fetch_add(1, Ordering::Relaxed);
        self.signal_if_parked();
    }

    /// Bug #2 fix: atomic edge-latching preserves sub-frame DOWN+UP taps (`< 1` frame duration)
    /// so fast mouse clicks never get overwritten by the subsequent UP flag before `drain()`.
    fn latch_button(slot: &AtomicU32, down: bool) {
        let _ = slot.try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
            if down {
                Some(old | 0b101)
            } else {
                Some((old | 0b010) & !0b100)
            }
        });
    }

    fn push_mouse(&self, dx: i32, dy: i32, flags: u16, wheel: i16) {
        if dx != 0 {
            self.mouse_dx.fetch_add(dx, Ordering::Relaxed);
        }
        if dy != 0 {
            self.mouse_dy.fetch_add(dy, Ordering::Relaxed);
        }
        if (flags & RI_MOUSE_WHEEL as u16) != 0 && wheel != 0 {
            self.mouse_wheel.fetch_add(wheel as i32, Ordering::Relaxed);
        }
        if (flags & RI_MOUSE_LEFT_BUTTON_DOWN as u16) != 0 {
            Self::latch_button(&self.mouse_left_bits, true);
        }
        if (flags & RI_MOUSE_LEFT_BUTTON_UP as u16) != 0 {
            Self::latch_button(&self.mouse_left_bits, false);
        }
        if (flags & RI_MOUSE_RIGHT_BUTTON_DOWN as u16) != 0 {
            Self::latch_button(&self.mouse_right_bits, true);
        }
        if (flags & RI_MOUSE_RIGHT_BUTTON_UP as u16) != 0 {
            Self::latch_button(&self.mouse_right_bits, false);
        }
        self.events_counter.fetch_add(1, Ordering::Relaxed);
        self.signal_if_parked();
    }

    pub fn drain(&self) -> DrainedInput {
        let mouse_dx = self.mouse_dx.swap(0, Ordering::Relaxed);
        let mouse_dy = self.mouse_dy.swap(0, Ordering::Relaxed);
        let wheel = self
            .mouse_wheel
            .swap(0, Ordering::Relaxed)
            .clamp(-32000, 32000) as i16;

        let lbits = self.mouse_left_bits.swap(0, Ordering::AcqRel);
        let left_btn = if lbits == 0 {
            None
        } else {
            Some((lbits & 0b100) != 0)
        };
        let left_clicked = (lbits & 0b001) != 0;

        let rbits = self.mouse_right_bits.swap(0, Ordering::AcqRel);
        let right_btn = if rbits == 0 {
            None
        } else {
            Some((rbits & 0b100) != 0)
        };
        let right_clicked = (rbits & 0b001) != 0;

        let privacy_activity = self.privacy_activity_flag.swap(false, Ordering::AcqRel);

        let mut keys = Vec::new();
        if let Ok(mut q) = self.key_ring.lock() {
            keys.extend(q.drain(..));
        }

        DrainedInput {
            mouse_dx,
            mouse_dy,
            wheel_delta: wheel,
            left_btn,
            left_clicked,
            right_btn,
            right_clicked,
            keys,
            privacy_activity,
        }
    }
}

pub struct RawInputWorker {
    hwnd_raw: Arc<AtomicIsize>,
    thread: Option<JoinHandle<()>>,
}

static WORKER_MAILBOX: Mutex<Option<Arc<InputMailbox>>> = Mutex::new(None);

impl RawInputWorker {
    pub fn spawn(mailbox: Arc<InputMailbox>) -> Self {
        if let Ok(mut g) = WORKER_MAILBOX.lock() {
            *g = Some(mailbox.clone());
        }
        let hwnd_raw = Arc::new(AtomicIsize::new(0));
        let hwnd_ref = hwnd_raw.clone();

        let thread = thread::Builder::new()
            .name("raw-input".to_string())
            .spawn(move || {
                run_raw_input_loop(mailbox, hwnd_ref);
            })
            .expect("spawn raw-input thread");

        Self {
            hwnd_raw,
            thread: Some(thread),
        }
    }

    /// Thread-affined toggle of `RIDEV_INPUTSINK` vs `RIDEV_REMOVE` (mandatory NULL `hwndTarget`).
    pub fn set_enabled(&self, enabled: bool) {
        let raw = self.hwnd_raw.load(Ordering::Acquire);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(raw as *mut _),
                    WM_APP_SET_RAWINPUT,
                    WPARAM(if enabled { 1 } else { 0 }),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn shutdown(&mut self) {
        let raw = self.hwnd_raw.load(Ordering::Acquire);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(HWND(raw as *mut _), WM_APP_STOP_THREAD, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RawInputWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn register_devices(hwnd: HWND, enable: bool, mailbox: &InputMailbox) {
    let flags = if enable { RIDEV_INPUTSINK } else { RIDEV_REMOVE };
    // CRITICAL Win32 invariant: `RIDEV_REMOVE` REQUIRES `hwndTarget == HWND(0)`!
    let target = if enable { hwnd } else { HWND::default() };
    let devices = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02, // Generic Desktop Mouse
            dwFlags: flags,
            hwndTarget: target,
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06, // Generic Desktop Keyboard
            dwFlags: flags,
            hwndTarget: target,
        },
    ];
    unsafe {
        match RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) {
            Ok(()) => {
                mailbox.registered.store(enable, Ordering::Release);
            }
            Err(e) => {
                log_warn(
                    "raw_input",
                    &format!("RegisterRawInputDevices(enable={enable}) failed: {e}"),
                );
            }
        }
    }
}

fn run_raw_input_loop(mailbox: Arc<InputMailbox>, hwnd_out: Arc<AtomicIsize>) {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();
        let class_name = w!("DesktopCatRawInputSink");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(raw_input_wndproc),
            hInstance: hinst,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            hinst,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log_warn(
                    "raw_input",
                    &format!("CreateWindowExW HWND_MESSAGE failed: {e}"),
                );
                return;
            }
        };

        hwnd_out.store(hwnd.0 as isize, Ordering::Release);
        register_devices(hwnd, true, &mailbox);
        log_info("raw_input", "HWND_MESSAGE Raw Input worker running");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = DispatchMessageW(&msg);
        }

        register_devices(hwnd, false, &mailbox);
        let _ = DestroyWindow(hwnd);
        hwnd_out.store(0, Ordering::Release);
    }
}

fn process_raw_packet(raw: &RAWINPUT, mailbox: &InputMailbox) {
    let dw_type = raw.header.dwType;
    let in_privacy = mailbox.privacy_active.load(Ordering::Acquire);

    if dw_type == RIM_TYPEKEYBOARD.0 {
        let kb = unsafe { raw.data.keyboard };
        let flags = kb.Flags as u32;
        let is_break = (flags & RI_KEY_BREAK) != 0;

        if in_privacy {
            // Strip key identity immediately at thread boundary (scancode never mapped or queued!).
            if !is_break && mailbox.privacy_allow_lease.load(Ordering::Acquire) {
                mailbox.push_privacy_activity();
            }
            return;
        }

        let e0 = (flags & RI_KEY_E0) != 0;
        let e1 = (flags & RI_KEY_E1) != 0;
        if let Some(pk) = from_windows_scancode(kb.MakeCode, e0, e1) {
            mailbox.push_key(pk, !is_break);
        }
    } else if dw_type == RIM_TYPEMOUSE.0 {
        if in_privacy {
            return;
        }
        let m = unsafe { raw.data.mouse };
        let (btn_flags, btn_data) = unsafe {
            (
                m.Anonymous.Anonymous.usButtonFlags,
                m.Anonymous.Anonymous.usButtonData as i16,
            )
        };
        mailbox.push_mouse(m.lLastX, m.lLastY, btn_flags, btn_data);
    }
}

unsafe extern "system" fn raw_input_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            let mailbox = WORKER_MAILBOX.lock().ok().and_then(|g| g.clone());
            if let Some(mb) = mailbox {
                let mut raw = RAWINPUT::default();
                let mut size = size_of::<RAWINPUT>() as u32;
                let res = GetRawInputData(
                    HRAWINPUT(lparam.0 as *mut _),
                    RID_INPUT,
                    Some(&mut raw as *mut RAWINPUT as *mut _),
                    &mut size,
                    size_of::<RAWINPUTHEADER>() as u32,
                );
                if res != u32::MAX && res > 0 {
                    process_raw_packet(&raw, &mb);
                }

                // Batch-drain queued 4-8 kHz packets via 8-byte aligned GetRawInputBuffer
                let mut buf = [0u64; 512];
                let mut cb = (buf.len() * size_of::<u64>()) as u32;
                let count = GetRawInputBuffer(
                    Some(buf.as_mut_ptr() as *mut RAWINPUT),
                    &mut cb,
                    size_of::<RAWINPUTHEADER>() as u32,
                );
                if count != u32::MAX && count > 0 {
                    let base = buf.as_ptr() as *const u8;
                    let mut offset = 0usize;
                    let max_bytes = buf.len() * size_of::<u64>();
                    for _ in 0..count {
                        if offset + size_of::<RAWINPUTHEADER>() > max_bytes {
                            break;
                        }
                        let pkt = &*(base.add(offset) as *const RAWINPUT);
                        let pkt_size = pkt.header.dwSize as usize;
                        if pkt_size == 0 || offset + pkt_size > max_bytes {
                            break;
                        }
                        process_raw_packet(pkt, &mb);
                        offset = (offset + pkt_size + 7) & !7;
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_SET_RAWINPUT => {
            if let Some(mb) = WORKER_MAILBOX.lock().ok().and_then(|g| g.clone()) {
                register_devices(hwnd, wparam.0 != 0, &mb);
            }
            LRESULT(0)
        }
        WM_APP_STOP_THREAD => {
            PostQuitMessage(0);
            let _ = GetLastError();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
